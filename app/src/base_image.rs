//! Commands that make a base image from the catalog, for an admin with no contest image.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ck_admin_core::{CreateOptions, CreateStep, catalog, create_base_image, find_catalog_entry};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::profiles::message;

#[derive(Serialize)]
pub struct CatalogItem {
    id: String,
    label: String,
    disk_gb: u32,
}

#[derive(Clone, Serialize)]
struct BaseProgress {
    /// `download`, `convert`, `provision`, `patch` or `hash`.
    step: &'static str,
    done: u64,
    total: u64,
}

#[tauri::command]
pub fn list_catalog() -> Result<Vec<CatalogItem>, String> {
    let mut items = Vec::new();
    for entry in catalog().map_err(|e| message(&e.into()))? {
        items.push(CatalogItem {
            id: entry.id,
            label: entry.label,
            disk_gb: entry.disk_gb,
        });
    }
    Ok(items)
}

/// Makes the base image `out`; it takes 15 to 40 minutes, so it runs off the window's thread and
/// reports through the `base-progress` event. Returns the new image's SHA-256.
#[tauri::command]
pub async fn make_base_image(
    app: AppHandle,
    distro: String,
    out: PathBuf,
    patch: Option<PathBuf>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let entry = find_catalog_entry(&distro).map_err(|e| message(&e.into()))?;
        let options = CreateOptions {
            entry,
            out,
            cache: cache_folder(),
            qemu_dir: None,
            accel: "whpx".to_owned(),
            memory_mb: 4096,
            timeout: Duration::from_secs(90 * 60),
            patch,
        };
        let mut last = Instant::now();
        let record = create_base_image(&options, |step, done, total| {
            // Progress is sent four times a second at most, but a new step is always sent.
            if last.elapsed() < Duration::from_millis(250) && done != total {
                return;
            }
            last = Instant::now();
            let step = match step {
                CreateStep::Download => "download",
                CreateStep::Convert => "convert",
                CreateStep::Provision => "provision",
                CreateStep::Patch => "patch",
                CreateStep::Hash => "hash",
            };
            let _ = app.emit("base-progress", BaseProgress { step, done, total });
        })
        .map_err(|e| message(&e.into()))?;
        Ok(record.image_sha256)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Downloads are kept between builds, so a second image does not download again.
fn cache_folder() -> PathBuf {
    match std::env::var_os("LOCALAPPDATA") {
        Some(base) => PathBuf::from(base).join("ck-forge").join("cache"),
        None => PathBuf::from("ck-forge-cache"),
    }
}
