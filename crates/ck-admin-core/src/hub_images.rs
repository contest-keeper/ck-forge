//! Downloads the published Keeper OS images through the signed-in Hub.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use ureq::http::header::{ACCEPT_ENCODING, AUTHORIZATION, CONTENT_ENCODING, LOCATION};
use ureq::http::{Response, StatusCode, Uri};

use crate::hub::{HubSession, agent, checked_base};
use crate::{Error, folder_of, import_image};

const MAX_CATALOGUE_BYTES: u64 = 1024 * 1024;
const MAX_IMAGES: usize = 200;
// The importer caps the raw image at 32 GiB. Allow for archive overhead.
const MAX_ARCHIVE_BYTES: u64 = 33 * 1024 * 1024 * 1024;

#[derive(Debug, Deserialize, Serialize)]
pub struct HubImage {
    pub id: String,
    pub name: String,
    pub version: String,
    pub profile: String,
    pub size: u64,
    pub filename: Option<String>,
    #[serde(skip_serializing)]
    url: String,
    #[serde(skip_serializing)]
    sha256: String,
}

#[derive(Deserialize)]
struct Catalogue {
    images: Vec<HubImage>,
}

/// A verified catalogue entry and a short-lived storage link. Keep the signed URL in Rust.
pub struct HubImageDownload {
    image: HubImage,
    location: Uri,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubImageStep {
    Download,
    Import,
}

pub fn list_hub_images(hub: &str, session: &HubSession) -> Result<Vec<HubImage>, Error> {
    let answer = hub_get(hub, session, "/api/v1/images")?;
    match answer.status() {
        StatusCode::OK => {}
        StatusCode::NOT_FOUND | StatusCode::SERVICE_UNAVAILABLE => {
            return Err(Error::Hub("the Hub image catalogue is unavailable".into()));
        }
        status => return Err(hub_status(status)),
    }
    let unreadable = || Error::Hub("the Hub returned an invalid image catalogue".into());
    let mut bytes = Vec::new();
    answer
        .into_body()
        .into_reader()
        .take(MAX_CATALOGUE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    if bytes.len() as u64 > MAX_CATALOGUE_BYTES {
        return Err(unreadable());
    }
    let catalogue: Catalogue = serde_json::from_slice(&bytes).map_err(|_| unreadable())?;
    if catalogue.images.len() > MAX_IMAGES {
        return Err(unreadable());
    }
    let mut seen = HashSet::new();
    for item in &catalogue.images {
        if !valid_image(item) || !seen.insert(&item.id) {
            return Err(unreadable());
        }
    }
    Ok(catalogue.images)
}

fn valid_image(image: &HubImage) -> bool {
    let id = &image.id;
    (1..=64).contains(&id.len())
        && !matches!(id.as_str(), "." | "..")
        && id.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.' | b'_')
        })
        && image.url == format!("/download/image/{id}")
        && (1..=MAX_ARCHIVE_BYTES).contains(&image.size)
        && image.sha256.len() == 64
        && image.sha256.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn prepare_hub_image_download(
    hub: &str,
    session: &HubSession,
    id: &str,
) -> Result<HubImageDownload, Error> {
    // Use a fresh catalogue so the WebView never supplies a URL, size or checksum.
    let image = list_hub_images(hub, session)?
        .into_iter()
        .find(|image| image.id == id)
        .ok_or_else(|| Error::Hub("the selected image is no longer listed by the Hub".into()))?;
    let answer = hub_get(hub, session, &image.url)?;
    if answer.status() != StatusCode::FOUND {
        return Err(hub_status(answer.status()));
    }
    let invalid = || Error::Hub("the Hub returned an invalid image download link".into());
    let location = answer
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .filter(|url| !url.contains('#'))
        .and_then(|url| url.parse::<Uri>().ok())
        .filter(|url| valid_download_location(hub, url))
        .ok_or_else(invalid)?;
    Ok(HubImageDownload { image, location })
}

fn valid_download_location(hub: &str, location: &Uri) -> bool {
    let Some(authority) = location.authority() else {
        return false;
    };
    if authority.host().is_empty() || authority.as_str().contains('@') {
        return false;
    }
    // Local developer Hubs can use a second loopback server to exercise the handoff.
    location.scheme_str() == Some("https")
        || (hub.starts_with("http://")
            && location.scheme_str() == Some("http")
            && matches!(authority.host(), "127.0.0.1" | "localhost"))
}

fn hub_get(hub: &str, session: &HubSession, path: &str) -> Result<Response<ureq::Body>, Error> {
    let hub = checked_base(hub)?;
    let bearer = session.authorization()?;
    agent()
        .get(format!("{hub}{path}"))
        .header(AUTHORIZATION, bearer)
        .call()
        .map_err(|_| Error::Hub("cannot reach the Hub image catalogue".into()))
}

fn hub_status(status: StatusCode) -> Error {
    let message = match status {
        StatusCode::UNAUTHORIZED => {
            return Error::SignedOut("sign in to the Hub again".into());
        }
        StatusCode::FORBIDDEN => "this account cannot download Keeper OS images".into(),
        _ => format!("the Hub image request returned HTTP {}", status.as_u16()),
    };
    Error::Hub(message)
}

/// Streams the archive into a temporary file and reuses the checked archive importer. The output
/// is published only after verification and expansion succeed.
pub fn download_hub_image(
    download: HubImageDownload,
    out: &Path,
    mut progress: impl FnMut(HubImageStep, u64, u64),
) -> Result<(), Error> {
    if out.exists() {
        return Err(ck_base_image::Error::Exists(out.display().to_string()).into());
    }
    let mut archive = NamedTempFile::new_in(folder_of(out))
        .map_err(|error| Error::io("creating a temporary Keeper OS download", error))?;
    download_archive(
        download.location,
        download.image.size,
        &mut archive,
        |done, total| {
            progress(HubImageStep::Download, done, total);
        },
    )?;
    import_image(
        archive.path(),
        out,
        &download.image.sha256,
        |done, total| {
            progress(HubImageStep::Import, done, total);
        },
    )?;
    Ok(())
}

fn download_archive(
    location: Uri,
    size: u64,
    archive: &mut NamedTempFile,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), Error> {
    // The download can be several GiB even though requesting its short-lived link is quick.
    let storage: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_per_read(Some(Duration::from_secs(60)))
        .timeout_global(Some(Duration::from_secs(4 * 60 * 60)))
        .build()
        .into();
    let response = storage
        .get(location)
        .header(ACCEPT_ENCODING, "identity")
        .call()
        .map_err(|_| Error::Hub("cannot reach Keeper OS download storage".into()))?;
    if response.status() != StatusCode::OK {
        return Err(Error::Hub(format!(
            "Keeper OS download storage returned HTTP {}. Try downloading again",
            response.status().as_u16()
        )));
    }
    if response
        .headers()
        .get(CONTENT_ENCODING)
        .is_some_and(|encoding| encoding != "identity")
    {
        return Err(Error::Hub(
            "the Keeper OS download used an unexpected encoding".into(),
        ));
    }
    let wrong_size = || Error::Hub("the Keeper OS download does not match its listed size".into());
    if response
        .body()
        .content_length()
        .is_some_and(|length| length != size)
    {
        return Err(wrong_size());
    }
    let mut reader = response.into_body().into_reader().take(size + 1);
    let mut buffer = vec![0; 1024 * 1024];
    let mut done = 0;
    progress(done, size);
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| Error::Hub("the Keeper OS download was interrupted".into()))?;
        if count == 0 {
            break;
        }
        done += count as u64;
        if done > size {
            return Err(wrong_size());
        }
        archive
            .write_all(&buffer[..count])
            .map_err(|error| Error::io("saving the Keeper OS download", error))?;
        progress(done, size);
    }
    if done != size {
        return Err(wrong_size());
    }
    Ok(())
}
