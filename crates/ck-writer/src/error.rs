use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
    #[error("could not list the disks: {0}")]
    Listing(String),
    #[error("disk {0} was not found")]
    NoSuchDisk(u32),
    #[error("disk {number} is {reason}, so it is never written")]
    Refused { number: u32, reason: String },
    #[error("disk {number} holds {disk} bytes but the image is {image} bytes; use a larger stick")]
    TooSmall { number: u32, disk: u64, image: u64 },
    #[error("could not get disk {number} ready: {detail}")]
    Prepare { number: u32, detail: String },
    #[error("the image is empty")]
    EmptyImage,
    #[error(
        "what was read back from the target is not what was written (written {written}, read back {read_back})"
    )]
    Mismatch { written: String, read_back: String },
}

impl Error {
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}
