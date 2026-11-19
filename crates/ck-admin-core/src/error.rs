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
    #[error("the stick key window cannot be used: {0}")]
    InvalidStickKeyWindow(&'static str),
    #[error("make a new stick image to change the key's contest or validity window")]
    StickRekeyRequired,
    #[error("stick {0} is already in the registry")]
    DuplicateStick(StickId),
    #[error("{0}")]
    Image(#[from] ck_image::Error),
    #[error("{0}")]
    Writer(#[from] ck_writer::Error),
    #[error("{0}")]
    BaseImage(#[from] ck_base_image::Error),
    #[error("{0}")]
    Hub(String),
    #[error(
        "there is no Wi-Fi network to give a password to: set one in the profile or give --wifi-ssid"
    )]
    NoWifiNetwork,
    #[error("the wallpaper cannot be used: {0}")]
    Wallpaper(String),
    #[error("the image has no settings yet (no ck/config.json on its EFI partition)")]
    ImageNotConfigured,
    #[error("the settings on the image are not valid: {0}")]
    ImageSettingsUnreadable(serde_json::Error),
    #[error("this base image does not support first-boot enrolment")]
    EnrolmentUnsupported,
    #[error("this image uses first-boot enrolment. Build a fresh image to change its settings")]
    EnrolmentImage,
    #[error("the contest has no more enrolment machine numbers")]
    EnrolmentNumbersExhausted,
    #[error("the image has conflicting enrolment settings")]
    AmbiguousImageConfig,
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
