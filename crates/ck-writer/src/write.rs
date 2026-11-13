use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, channel, sync_channel};
use std::thread::{self, ScopedJoinHandle};

use sha2::{Digest, Sha256};

use crate::Error;
use crate::thin::Source;

/// Disks only take whole sectors, so the last block is padded with zeros to this size.
const SECTOR: usize = 512;
/// Large blocks keep a slow stick busy. Not larger: reading a hard disk in 16 MiB blocks measured
/// 112 MB/s where 1, 2 and 4 MiB all gave 163 MB/s. A whole number of sectors.
const BLOCK: usize = 4 * 1024 * 1024;
/// Blocks that may wait between the thread that reads and a thread that writes.
const IN_FLIGHT: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Writing,
    Verifying,
}

/// The path that opens disk `number` as a raw device. The two leading backslashes matter: with one,
/// Windows takes it for a file named `PhysicalDriveN` on the current drive.
#[must_use]
pub fn disk_path(number: u32) -> PathBuf {
    PathBuf::from(format!(r"\\.\PhysicalDrive{number}"))
}

/// Copies `image` to `target`, then reads `target` back and compares. Returns the image's SHA-256
/// as hex. `progress` gets the phase, the bytes done and the total.
///
/// `target` can be a device (see [`disk_path`]), which must already be prepared by
/// [`crate::prepare_disk`], or a file, which is created if it does not exist. The last block is
/// padded to a whole sector. A device may be larger than the image; only the image's bytes are
/// written and checked.
pub fn write_image(
    image: &Path,
    target: &Path,
    mut progress: impl FnMut(Phase, u64, u64),
) -> Result<String, Error> {
    let mut results =
        write_image_to_all(image, &[target.to_path_buf()], |_, phase, done, total| {
            progress(phase, done, total);
        })?;
    results.remove(0)
}

/// Writes `image` to every target at once, then reads every target back and compares.
///
/// The image is read and hashed once, whatever the number of targets, and each target is written
/// by a thread of its own, so a batch of sticks takes about as long as the slowest one and not as
/// long as all of them in a row. The same goes for the check: it starts only after every write is
/// flushed, so it reads what a target holds and not what was just handed to it, and all targets are
/// checked together.
///
/// Returns one result per target, in the order of `targets`: the image's SHA-256 as hex, or what
/// went wrong with that target. One failing target does not stop the others. An image that cannot
/// be read fails the whole call. `progress` gets the index of the target, the phase, the bytes done
/// and the total, always on the calling thread.
pub fn write_image_to_all(
    image: &Path,
    targets: &[PathBuf],
    mut progress: impl FnMut(usize, Phase, u64, u64),
) -> Result<Vec<Result<String, Error>>, Error> {
    let Source {
        reader: source,
        len: total,
    } = crate::thin::open(image)?;
    if total == 0 {
        return Err(Error::EmptyImage);
    }

    let mut results: Vec<Option<Result<String, Error>>> = targets.iter().map(|_| None).collect();
    let mut outs = Vec::new();
    for (index, target) in targets.iter().enumerate() {
        match OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(target)
        {
            Ok(out) => outs.push((index, out)),
            Err(e) => {
                let context = format!("opening {} for writing", target.display());
                results[index] = Some(Err(Error::io(context, e)));
            }
        }
    }

    let (hash, written) = write_all_targets(source, outs, targets, total, &mut progress);
    let hash = hash?;
    let mut to_check = Vec::new();
    for (index, outcome) in written {
        match outcome {
            Ok(()) => to_check.push(index),
            Err(error) => results[index] = Some(Err(error)),
        }
    }

    for (index, read_back) in check_all_targets(&to_check, targets, total, &mut progress) {
        results[index] = Some(match read_back {
            Ok(read_back) if read_back == hash => Ok(hash.clone()),
            Ok(read_back) => Err(Error::Mismatch {
                written: hash.clone(),
                read_back,
            }),
            Err(error) => Err(error),
        });
    }
    Ok(results.into_iter().flatten().collect())
}

type Written = Vec<(usize, Result<(), Error>)>;

/// The write pass: one thread reads and hashes the image and hands every block to one writer thread
/// per target. Returns the SHA-256 of the image and how each writer fared.
fn write_all_targets(
    source: Box<dyn Read + Send>,
    outs: Vec<(usize, File)>,
    targets: &[PathBuf],
    total: u64,
    progress: &mut impl FnMut(usize, Phase, u64, u64),
) -> (Result<String, Error>, Written) {
    thread::scope(|scope| {
        let (events, events_in) = channel::<(usize, u64)>();
        let mut senders = Vec::new();
        let mut writers = Vec::new();
        for (index, mut out) in outs {
            let (sender, receiver) = sync_channel::<Arc<Vec<u8>>>(IN_FLIGHT);
            senders.push(sender);
            let events = events.clone();
            let target = &targets[index];
            writers.push((
                index,
                scope.spawn(move || {
                    let done = consume_all(receiver, total, |block, done| {
                        out.write_all(block).map_err(|e| {
                            Error::io(format!("writing to {}", target.display()), e)
                        })?;
                        let _ = events.send((index, done));
                        Ok(())
                    })?;
                    if done != total {
                        return Err(Error::io(
                            format!("writing to {}", target.display()),
                            io::ErrorKind::UnexpectedEof.into(),
                        ));
                    }
                    out.sync_all()
                        .map_err(|e| Error::io(format!("flushing {}", target.display()), e))
                }),
            ));
        }
        drop(events);
        let reader =
            scope.spawn(move || read_blocks(source, total, false, "reading the image", senders));

        // Ends when every writer is done, because each holds a sender of the events.
        for (index, done) in events_in {
            progress(index, Phase::Writing, done, total);
        }
        let hash = join(reader);
        let written = writers
            .into_iter()
            .map(|(index, writer)| (index, join(writer)))
            .collect();
        (hash, written)
    })
}

/// The check pass: reads every target back, all at once. Returns the SHA-256 of what each holds.
fn check_all_targets(
    indexes: &[usize],
    targets: &[PathBuf],
    total: u64,
    progress: &mut impl FnMut(usize, Phase, u64, u64),
) -> Vec<(usize, Result<String, Error>)> {
    thread::scope(|scope| {
        let (events, events_in) = channel::<(usize, u64)>();
        let mut checks = Vec::new();
        for &index in indexes {
            let events = events.clone();
            let target = &targets[index];
            checks.push((
                index,
                scope.spawn(move || {
                    let reopened = File::open(target)
                        .map_err(|e| Error::io(format!("reopening {}", target.display()), e))?;
                    let context = format!("reading back {}", target.display());
                    stream(reopened, total, true, &context, |_, done| {
                        let _ = events.send((index, done));
                        Ok(())
                    })
                }),
            ));
        }
        drop(events);
        for (index, done) in events_in {
            progress(index, Phase::Verifying, done, total);
        }
        checks
            .into_iter()
            .map(|(index, check)| (index, join(check)))
            .collect()
    })
}

fn join<T>(handle: ScopedJoinHandle<'_, T>) -> T {
    match handle.join() {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Reads `total` bytes from `source` in blocks, hashes them and sends every block to every sender.
/// A sender whose receiver is gone is dropped, and reading stops when none is left. Returns the
/// SHA-256 as hex, which is the hash of the whole image only if a sender stayed to the end.
///
/// Each block is padded with zeros to a whole sector. With `read_padded` the padding is read from
/// `source` too (a target that was written padded); without it the padding is made here. Only the
/// image's bytes go into the hash.
fn read_blocks<R: Read>(
    mut source: R,
    total: u64,
    read_padded: bool,
    read_context: &str,
    mut senders: Vec<SyncSender<Arc<Vec<u8>>>>,
) -> Result<String, Error> {
    let mut hash = Sha256::new();
    let mut done = 0_u64;
    while done < total && !senders.is_empty() {
        let want = block_len(total - done);
        let padded = want.next_multiple_of(SECTOR);
        let mut block = vec![0_u8; padded];
        let read_len = if read_padded { padded } else { want };
        source
            .read_exact(&mut block[..read_len])
            .map_err(|e| Error::io(read_context, e))?;
        hash.update(&block[..want]);
        done += want as u64;
        let block = Arc::new(block);
        senders.retain(|sender| sender.send(Arc::clone(&block)).is_ok());
    }
    Ok(hex(&hash.finalize()))
}

/// Gives `consume` every block the reader sends, with the bytes of the image done so far, and
/// returns that number. Stops at the first error of `consume`; `receiver` is dropped then, which
/// lets a reader that waits to send go on without this one.
fn consume_all(
    receiver: Receiver<Arc<Vec<u8>>>,
    total: u64,
    mut consume: impl FnMut(&[u8], u64) -> Result<(), Error>,
) -> Result<u64, Error> {
    let mut done = 0_u64;
    for block in receiver {
        done += block_len(total - done) as u64;
        consume(&block, done)?;
    }
    Ok(done)
}

/// Reads `total` bytes from `source` on a second thread while `consume` works on the previous block
/// on the calling thread. Returns the SHA-256 as hex, and only when every byte went through: when
/// `consume` fails its error is returned, and when the reading fails or stops short, that is.
fn stream<R: Read + Send>(
    source: R,
    total: u64,
    read_padded: bool,
    read_context: &str,
    consume: impl FnMut(&[u8], u64) -> Result<(), Error>,
) -> Result<String, Error> {
    thread::scope(|scope| {
        let (sender, receiver) = sync_channel(IN_FLIGHT);
        let reader = scope
            .spawn(move || read_blocks(source, total, read_padded, read_context, vec![sender]));
        let consumed = consume_all(receiver, total, consume);
        let hashed = join(reader);
        let done = consumed?;
        let hash = hashed?;
        if done == total {
            Ok(hash)
        } else {
            Err(Error::io(read_context, io::ErrorKind::UnexpectedEof.into()))
        }
    })
}

/// How many bytes of the image the next block holds, with `left` bytes still to go.
fn block_len(left: u64) -> usize {
    usize::try_from(left).map_or(BLOCK, |left| left.min(BLOCK))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader of `length` bytes that all have the value `value`.
    fn bytes_of(value: u8, length: u64) -> impl Read + Send {
        io::repeat(value).take(length)
    }

    fn sha(value: u8, length: u64) -> String {
        let mut hash = Sha256::new();
        let mut left = length;
        while left > 0 {
            let part = block_len(left);
            hash.update(vec![value; part]);
            left -= part as u64;
        }
        hex(&hash.finalize())
    }

    #[test]
    fn hashes_every_byte_and_hands_over_whole_sectors() {
        let total = 2 * BLOCK as u64 + 1000;
        let mut seen = 0_u64;
        let mut blocks = Vec::new();
        let hash = stream(
            bytes_of(7, total),
            total,
            false,
            "reading",
            |block, done| {
                assert_eq!(block.len() % SECTOR, 0);
                blocks.push(block.len());
                seen = done;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(hash, sha(7, total));
        assert_eq!(seen, total);
        assert_eq!(blocks, vec![BLOCK, BLOCK, 1024]);
    }

    #[test]
    fn a_failing_consumer_stops_the_reading_and_its_error_is_returned() {
        // Many more blocks than the channel holds, so the reader is blocked when the error comes.
        let total = 40 * BLOCK as u64;
        let mut calls = 0;
        let error = stream(bytes_of(1, total), total, false, "reading", |_, _| {
            calls += 1;
            if calls == 2 {
                Err(Error::io("writing", io::ErrorKind::StorageFull.into()))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(matches!(&error, Error::Io { context, .. } if context == "writing"));
        assert_eq!(calls, 2);
    }

    #[test]
    fn a_source_that_ends_early_is_an_error_and_never_a_hash() {
        let total = 5 * BLOCK as u64;
        let mut seen = 0;
        let result = stream(
            bytes_of(1, total - 3),
            total,
            false,
            "reading",
            |_, done| {
                seen = done;
                Ok(())
            },
        );
        assert!(matches!(&result, Err(Error::Io { context, .. }) if context == "reading"));
        assert!(seen < total);
    }

    #[test]
    fn padding_is_read_from_a_padded_source() {
        // A target written padded holds 1024 bytes for an image of 1000.
        let hash = stream(bytes_of(9, 1024), 1000, true, "reading", |block, done| {
            assert_eq!((block.len(), done), (1024, 1000));
            Ok(())
        })
        .unwrap();
        assert_eq!(hash, sha(9, 1000));
    }

    #[test]
    fn a_receiver_that_goes_away_does_not_stop_the_others() {
        let total = 10 * BLOCK as u64;
        let (gone, gone_receiver) = sync_channel(IN_FLIGHT);
        let (kept, kept_receiver) = sync_channel(IN_FLIGHT);
        drop(gone_receiver);
        let hash = thread::scope(|scope| {
            let reader = scope.spawn(move || {
                read_blocks(bytes_of(5, total), total, false, "r", vec![gone, kept])
            });
            let done = consume_all(kept_receiver, total, |_, _| Ok(())).unwrap();
            assert_eq!(done, total);
            join(reader).unwrap()
        });
        assert_eq!(hash, sha(5, total));
    }
}
