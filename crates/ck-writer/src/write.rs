use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, channel, sync_channel};
use std::thread::{self, ScopedJoinHandle};

use sha2::{Digest, Sha256};

use crate::Error;
use crate::thin::{self, Parts};

/// Disks only take whole sectors, so the last block is padded with zeros to this size.
const SECTOR: usize = 512;
/// Large blocks keep a slow stick busy. Not larger: reading a hard disk in 16 MiB blocks measured
/// 112 MB/s where 1, 2 and 4 MiB all gave 163 MB/s. A whole number of sectors.
const BLOCK: usize = 4 * 1024 * 1024;
/// Blocks that may wait between the thread that reads and a thread that writes.
const IN_FLIGHT: usize = 3;

/// What came of writing one stick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    /// SHA-256 of the image, which the stick was read back and checked against.
    pub sha256: String,
    /// Bytes of the image that the stick already held, so they were not written again.
    pub unchanged: u64,
}

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
/// written and checked. `image` can be a thin image, see [`crate::thin`].
pub fn write_image(
    image: &Path,
    target: &Path,
    mut progress: impl FnMut(Phase, u64, u64),
) -> Result<String, Error> {
    let mut results =
        write_image_to_all(image, &[target.to_path_buf()], |_, phase, done, total| {
            progress(phase, done, total);
        })?;
    results.remove(0).map(|written| written.sha256)
}

/// Writes `image` to every target at once. See [`write_images`]; this is the same image for all.
pub fn write_image_to_all(
    image: &Path,
    targets: &[PathBuf],
    progress: impl FnMut(usize, Phase, u64, u64),
) -> Result<Vec<Result<Written, Error>>, Error> {
    let jobs: Vec<(PathBuf, PathBuf)> = targets
        .iter()
        .map(|target| (image.to_path_buf(), target.clone()))
        .collect();
    write_images(&jobs, progress)
}

/// Writes every image of `jobs`, a pair of an image and its target, at once, then reads every
/// target back and compares.
///
/// The images must be the same size and made of the same base, which is the case for thin images
/// of one base, or the same plain image for every target. What they share is read once. Each target
/// is written by a thread of its own: its own image's start, then the shared rest. So a batch of
/// sticks, each with its own identity, takes about as long as the slowest stick and not as long as
/// all of them in a row. The check starts only after every write is flushed, so it reads what a
/// target holds and not what was just handed to it, and all targets are checked together.
///
/// A target that already holds the image, such as a stick that was written before with another
/// stick's image of the same base, is written only where it differs: a few blocks are compared
/// first to tell, and then every block is compared with what is there before it is written. Reading
/// is faster than writing on a stick, so such a stick takes much less time. The check after the
/// write is the same either way and reads all of the target, so a block that was left alone is
/// checked too. This relies on reads of a device coming from the device and not from a cache.
///
/// Returns one result per job, in order: the SHA-256 of that target's image as hex, or what went
/// wrong with that target. One failing target does not stop the others. An image that cannot be
/// opened, or images that do not belong together, fail the whole call before anything is written.
/// `progress` gets the index of the job, the phase, the bytes done and the total, always on the
/// calling thread.
pub fn write_images(
    jobs: &[(PathBuf, PathBuf)],
    mut progress: impl FnMut(usize, Phase, u64, u64),
) -> Result<Vec<Result<Written, Error>>, Error> {
    let images: Vec<&Path> = jobs.iter().map(|(image, _)| image.as_path()).collect();
    let parts = open_together(&images)?;
    let Some(first) = parts.first() else {
        return Ok(Vec::new());
    };
    let (total, head_len) = (first.len(), first.head_len);
    let tail = first.tail.clone();

    let mut results: Vec<Option<Result<Written, Error>>> = jobs.iter().map(|_| None).collect();
    let mut writers = Vec::new();
    for (index, parts) in parts.into_iter().enumerate() {
        let (image, target) = (&jobs[index].0, &jobs[index].1);
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(target)
        {
            Ok(mut out) => {
                let compare = holds_the_image(&mut out, &tail, head_len, total);
                // The probe moved the file; writing starts at its beginning.
                match out.rewind() {
                    Ok(()) => writers.push(Writer {
                        index,
                        out,
                        head: parts.head,
                        compare,
                        image,
                        target,
                    }),
                    Err(e) => {
                        let context = format!("opening {} for writing", target.display());
                        results[index] = Some(Err(Error::io(context, e)));
                    }
                }
            }
            Err(e) => {
                let context = format!("opening {} for writing", target.display());
                results[index] = Some(Err(Error::io(context, e)));
            }
        }
    }

    let shared = Shared {
        tail: &tail,
        head_len,
        total,
    };
    let written = write_all_targets(&shared, writers, &mut progress)?;
    let mut to_check = Vec::new();
    for (index, outcome) in written {
        match outcome {
            Ok(done) => to_check.push((index, done)),
            Err(error) => results[index] = Some(Err(error)),
        }
    }

    let indexes: Vec<usize> = to_check.iter().map(|(index, _)| *index).collect();
    let targets: Vec<&Path> = jobs.iter().map(|(_, target)| target.as_path()).collect();
    let read_backs = check_all_targets(&indexes, &targets, total, &mut progress);
    for ((index, done), (_, read_back)) in to_check.into_iter().zip(read_backs) {
        let Written { sha256, unchanged } = done;
        results[index] = Some(match read_back {
            Ok(read_back) if read_back == sha256 => Ok(Written { sha256, unchanged }),
            Ok(read_back) => Err(Error::Mismatch {
                written: sha256,
                read_back,
            }),
            Err(error) => Err(error),
        });
    }
    Ok(results.into_iter().flatten().collect())
}

/// Checks that `images` can be written together, and returns the size of each, without writing
/// anything. Run it before a disk is erased, so that a set of images that does not fit together
/// leaves the disks as they are. Fails for an image that cannot be opened, for thin images whose
/// base is not what they were made from, and for images of different bases or sizes.
pub fn check_images(images: &[PathBuf]) -> Result<u64, Error> {
    let images: Vec<&Path> = images.iter().map(PathBuf::as_path).collect();
    Ok(open_together(&images)?.first().map_or(0, Parts::len))
}

fn open_together(images: &[&Path]) -> Result<Vec<Parts>, Error> {
    let mut parts = Vec::new();
    for image in images {
        parts.push(thin::open(image)?);
    }
    if let Some(first) = parts.first() {
        if first.len() == 0 {
            return Err(Error::EmptyImage);
        }
        if parts.iter().any(|other| !first.shares_tail_with(other)) {
            return Err(Error::Thin(
                "the images are not made from the same base image, so they cannot be written                  together"
                    .to_owned(),
            ));
        }
    }
    Ok(parts)
}

/// What one target is written from and to.
struct Writer<'a> {
    index: usize,
    out: File,
    /// The start of this target's own image, if it has one.
    head: Option<File>,
    /// Whether the target already holds the image, so each block is compared with what is there
    /// and only written when it differs.
    compare: bool,
    image: &'a Path,
    target: &'a Path,
}

/// What every target is written from.
struct Shared<'a> {
    /// The file the rest of the images is read from, after the first `head_len` bytes.
    tail: &'a Path,
    head_len: u64,
    /// The size of an image: its own start and the rest.
    total: u64,
}

type Outcomes = Vec<(usize, Result<Written, Error>)>;

/// The write pass: one thread reads the shared rest of the image and hands every block to one
/// writer thread per target, which first writes its own start. Returns how each writer fared: the
/// SHA-256 of what it wrote.
fn write_all_targets(
    shared: &Shared<'_>,
    writers: Vec<Writer<'_>>,
    progress: &mut impl FnMut(usize, Phase, u64, u64),
) -> Result<Outcomes, Error> {
    let (head_len, total) = (shared.head_len, shared.total);
    let mut tail = File::open(shared.tail)
        .map_err(|e| Error::io(format!("opening {}", shared.tail.display()), e))?;
    tail.seek(SeekFrom::Start(head_len))
        .map_err(|e| Error::io(format!("reading {}", shared.tail.display()), e))?;
    let tail_len = total - head_len;
    let read_context = format!("reading {}", shared.tail.display());

    thread::scope(|scope| {
        let (events, events_in) = channel::<(usize, u64)>();
        let mut senders = Vec::new();
        let mut handles = Vec::new();
        for Writer {
            index,
            mut out,
            head,
            compare,
            image,
            target,
        } in writers
        {
            let (sender, receiver) = sync_channel::<Arc<Vec<u8>>>(IN_FLIGHT);
            senders.push(sender);
            let events = events.clone();
            handles.push((
                index,
                scope.spawn(move || {
                    let write_error = |e| Error::io(format!("writing to {}", target.display()), e);
                    let mut hash = Sha256::new();
                    let mut put = Put::new(compare);
                    if let Some(mut head) = head {
                        let mut buffer = vec![0_u8; BLOCK];
                        let mut done = 0_u64;
                        while done < head_len {
                            let want = block_len(head_len - done);
                            head.read_exact(&mut buffer[..want]).map_err(|e| {
                                Error::io(format!("reading {}", image.display()), e)
                            })?;
                            hash.update(&buffer[..want]);
                            put.block(&mut out, done, &buffer[..want])
                                .map_err(write_error)?;
                            done += want as u64;
                            let _ = events.send((index, done));
                        }
                    }
                    let mut before = 0_u64;
                    let (done, hash) = consume_all(receiver, tail_len, hash, |block, done| {
                        put.block(&mut out, head_len + before, block)
                            .map_err(write_error)?;
                        before = done;
                        let _ = events.send((index, head_len + done));
                        Ok(())
                    })?;
                    if done != tail_len {
                        return Err(write_error(io::ErrorKind::UnexpectedEof.into()));
                    }
                    out.sync_all()
                        .map_err(|e| Error::io(format!("flushing {}", target.display()), e))?;
                    Ok(Written {
                        sha256: hex(&hash.finalize()),
                        unchanged: put.unchanged,
                    })
                }),
            ));
        }
        drop(events);
        let reader =
            scope.spawn(move || read_blocks(tail, tail_len, false, &read_context, senders));

        // Ends when every writer is done, because each holds a sender of the events.
        for (index, done) in events_in {
            progress(index, Phase::Writing, done, total);
        }
        let read = join(reader);
        let written = handles
            .into_iter()
            .map(|(index, writer)| (index, join(writer)))
            .collect();
        read.map(|()| written)
    })
}

/// The check pass: reads every target back, all at once. Returns the SHA-256 of what each holds.
fn check_all_targets(
    indexes: &[usize],
    targets: &[&Path],
    total: u64,
    progress: &mut impl FnMut(usize, Phase, u64, u64),
) -> Vec<(usize, Result<String, Error>)> {
    thread::scope(|scope| {
        let (events, events_in) = channel::<(usize, u64)>();
        let mut checks = Vec::new();
        for &index in indexes {
            let events = events.clone();
            let target = targets[index];
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

/// How many blocks of the shared rest a target is asked about to tell whether it already holds
/// the image.
const PROBES: u64 = 8;

/// Whether `out` already holds the image: a few blocks spread over the rest of the image, which is
/// what a stick that was written with another stick's image of the same base looks like, are read
/// from `out` and from the shared rest and found equal. The start is not asked about, as it is the
/// part that differs. When `out` is new, short or something else, the answer is no and every block
/// is written. Any error is a no, too.
fn holds_the_image(out: &mut File, tail: &Path, head_len: u64, total: u64) -> bool {
    let tail_blocks = (total - head_len).div_ceil(BLOCK as u64);
    if tail_blocks < 2 {
        return false;
    }
    let asked = PROBES.min(tail_blocks - 1);
    let Ok(mut tail) = File::open(tail) else {
        return false;
    };
    let mut wanted = vec![0_u8; BLOCK];
    let mut held = vec![0_u8; BLOCK];
    (0..asked).all(|n| {
        // The middle of each of `asked` equal parts of the blocks that are not the last.
        let block = (2 * n + 1) * (tail_blocks - 1) / (2 * asked);
        let offset = head_len + block * BLOCK as u64;
        tail.seek(SeekFrom::Start(offset)).is_ok()
            && tail.read_exact(&mut wanted).is_ok()
            && out.seek(SeekFrom::Start(offset)).is_ok()
            && out.read_exact(&mut held).is_ok()
            && wanted == held
    })
}

/// Puts blocks on a target, sparing the ones it already holds.
struct Put {
    /// A buffer to read what the target holds into, when blocks are compared.
    held: Option<Vec<u8>>,
    /// Bytes that were already there.
    unchanged: u64,
}

impl Put {
    fn new(compare: bool) -> Self {
        Self {
            held: compare.then(Vec::new),
            unchanged: 0,
        }
    }

    /// Writes `data` at `offset` of `out`. When the target is compared with the image and holds
    /// exactly these bytes there, writes nothing. A target that ends before the block, or holds
    /// something else, is written.
    fn block(&mut self, out: &mut File, offset: u64, data: &[u8]) -> io::Result<()> {
        let Some(held) = self.held.as_mut() else {
            // Every block follows the one before, so the file is at `offset` already.
            return out.write_all(data);
        };
        held.resize(data.len(), 0);
        out.seek(SeekFrom::Start(offset))?;
        if read_up_to(out, held)? == data.len() && held[..] == *data {
            self.unchanged += data.len() as u64;
            return Ok(());
        }
        out.seek(SeekFrom::Start(offset))?;
        out.write_all(data)
    }
}

/// Reads until `buffer` is full or the file ends; returns how many bytes were read.
fn read_up_to(file: &mut File, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

fn join<T>(handle: ScopedJoinHandle<'_, T>) -> T {
    match handle.join() {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Reads `total` bytes from `source` in blocks and sends every block to every sender. A sender
/// whose receiver is gone is dropped, and reading stops when none is left.
///
/// Each block is padded with zeros to a whole sector. With `read_padded` the padding is read from
/// `source` too (a target that was written padded); without it the padding is made here.
fn read_blocks<R: Read>(
    mut source: R,
    total: u64,
    read_padded: bool,
    read_context: &str,
    mut senders: Vec<SyncSender<Arc<Vec<u8>>>>,
) -> Result<(), Error> {
    let mut done = 0_u64;
    while done < total && !senders.is_empty() {
        let want = block_len(total - done);
        let padded = want.next_multiple_of(SECTOR);
        let mut block = vec![0_u8; padded];
        let read_len = if read_padded { padded } else { want };
        source
            .read_exact(&mut block[..read_len])
            .map_err(|e| Error::io(read_context, e))?;
        done += want as u64;
        let block = Arc::new(block);
        senders.retain(|sender| sender.send(Arc::clone(&block)).is_ok());
    }
    Ok(())
}

/// Gives `consume` every block the reader sends, with the bytes done so far, and adds the bytes of
/// the image in each block, without the padding, to `hash`. Returns the number of bytes and the
/// hash. Stops at the first error of `consume`; `receiver` is dropped then, which lets a reader
/// that waits to send go on without this one.
fn consume_all(
    receiver: Receiver<Arc<Vec<u8>>>,
    total: u64,
    mut hash: Sha256,
    mut consume: impl FnMut(&[u8], u64) -> Result<(), Error>,
) -> Result<(u64, Sha256), Error> {
    let mut done = 0_u64;
    for block in receiver {
        let want = block_len(total - done);
        hash.update(&block[..want]);
        done += want as u64;
        consume(&block, done)?;
    }
    Ok((done, hash))
}

/// Reads `total` bytes from `source` on a second thread while `consume` works on the previous block
/// on the calling thread. Returns the SHA-256 of the bytes as hex, and only when every byte went
/// through: when `consume` fails its error is returned, and when the reading fails or stops short,
/// that is.
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
        let consumed = consume_all(receiver, total, Sha256::new(), consume);
        let read = join(reader);
        let (done, hash) = consumed?;
        read?;
        if done == total {
            Ok(hex(&hash.finalize()))
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
            let (done, hash) =
                consume_all(kept_receiver, total, Sha256::new(), |_, _| Ok(())).unwrap();
            assert_eq!(done, total);
            join(reader).unwrap();
            hex(&hash.finalize())
        });
        assert_eq!(hash, sha(5, total));
    }
}
