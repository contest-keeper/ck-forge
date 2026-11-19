use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
    #[error("there is no image called {0} in the catalog (see `ck-forge image catalog`)")]
    UnknownDistro(String),
    #[error("downloading {url}: {detail}")]
    Download { url: String, detail: String },
    #[error("the download's SHA-256 does not match: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error("enter the 64-character SHA-256 from the image download page")]
    InvalidChecksum,
    #[error("the archive contains an empty image")]
    EmptyArchive,
    #[error("the expanded image exceeds the 32 GiB limit")]
    ImageTooLarge,
    #[error("{tool} failed: {detail}")]
    Tool { tool: String, detail: String },
    #[error("the provisioning machine failed: {0}")]
    Provision(String),
    /// QEMU itself died (on Windows the accelerator sometimes does): the disk is in an unknown
    /// state, but a new attempt from the download may work.
    #[error("the provisioning machine crashed: {0}")]
    MachineStopped(String),
    #[error("{0} already exists; it is never replaced")]
    Exists(String),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

impl Error {
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}
