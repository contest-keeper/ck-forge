use std::io;

use ck_protocol::{Problem, StickId};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
    #[error("{path} is not a valid file: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("there is no profile at {0}")]
    ProfileMissing(String),
    #[error("the profile has problems: {}", join(.0))]
    InvalidProfile(Vec<Problem>),
    #[error("stick {0} is already in the registry")]
    DuplicateStick(StickId),
    #[error("{0}")]
    Image(#[from] ck_image::Error),
    #[error("{0}")]
    Writer(#[from] ck_writer::Error),
    #[error("{0}")]
    Hub(String),
    #[error("the wallpaper cannot be used: {0}")]
    Wallpaper(String),
    #[error("the image has no settings yet (no ck/config.json on its EFI partition)")]
    ImageNotConfigured,
    #[error("the settings on the image are not valid: {0}")]
    ImageSettingsUnreadable(serde_json::Error),
    #[error(
        "this image is the stick {0}: written to several sticks they would all share its id and          secret and the Hub could not tell them apart. Make one image per stick, or write a plain          base image to several sticks"
    )]
    ImageForOneStick(String),
}

impl Error {
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

fn join(problems: &[Problem]) -> String {
    problems
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}
