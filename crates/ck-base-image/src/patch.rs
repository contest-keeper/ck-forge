//! Applies the Keeper OS patch to the finished image, through WSL like `tools/patch-image.ps1`.

use std::path::Path;
use std::process::Command;

use crate::Error;
use crate::tools::run;

// Unpack the release's patch (apply-to-image.sh and ck-agent at its top) and run it on the image.
const SCRIPT: &str = r#"set -euo pipefail
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
tar -xzf "$1" -C "$work"
bash "$work/apply-to-image.sh" --in-place "$2" --agent "$work/ck-agent"
"#;

pub fn apply(image: &Path, patch: &Path) -> Result<(), Error> {
    let image = wsl_path(image)?;
    let patch = wsl_path(patch)?;
    run(
        Command::new("wsl").args(["-e", "bash", "-c", SCRIPT, "bash", &patch, &image]),
        "the Keeper OS patch (needs WSL with e2fsprogs and fdisk)",
    )
}

fn wsl_path(path: &Path) -> Result<String, Error> {
    let output = Command::new("wsl")
        .args(["-e", "wslpath", "-a"])
        .arg(path)
        .output()
        .map_err(|e| Error::io("starting WSL", e))?;
    if !output.status.success() {
        return Err(Error::Tool {
            tool: "wslpath".to_owned(),
            detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
