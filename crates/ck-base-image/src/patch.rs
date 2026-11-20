//! Applies the Keeper OS patch to the finished image, through WSL like `tools/patch-image.ps1`.

use std::path::Path;
use std::process::Command;

use crate::Error;
use crate::tools::run;

// Releases wrap the patch in ck-os-patch. Accept flat archives made by local development tools.
const SCRIPT: &str = r#"set -euo pipefail
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
tar -xzf "$1" -C "$work"
patch_root=$work
if [[ -d "$work/ck-os-patch" ]]; then
  patch_root="$work/ck-os-patch"
fi
bash "$patch_root/apply-to-image.sh" --in-place "$2" --agent "$patch_root/ck-agent"
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
    let mut command = Command::new("wsl");
    command.args(["-e", "wslpath", "-a"]).arg(path);
    let output = ck_writer::without_console_window(&mut command)
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

#[cfg(all(test, unix))]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn runs_the_patch_from_flat_and_release_archives() {
        for release in [false, true] {
            let dir = tempfile::tempdir().expect("test folder");
            let source = dir.path().join("source");
            let files = if release {
                source.join("ck-os-patch")
            } else {
                source.clone()
            };
            fs::create_dir_all(&files).expect("patch folder");
            fs::write(
                files.join("apply-to-image.sh"),
                "set -eu\n[ \"$1\" = --in-place ] && [ \"$3\" = --agent ] && [ -f \"$4\" ]\ncat \"$2\"\n",
            )
            .expect("patch script");
            fs::write(files.join("ck-agent"), b"fixture agent").expect("fixture agent");
            let archive = dir.path().join("patch.tar.gz");
            let member = if release { "ck-os-patch" } else { "." };
            assert!(
                Command::new("tar")
                    .arg("-czf")
                    .arg(&archive)
                    .arg("-C")
                    .arg(&source)
                    .arg(member)
                    .status()
                    .expect("make patch archive")
                    .success()
            );
            let image = dir.path().join("image.img");
            fs::write(&image, b"fixture image").expect("fake image");
            let result = Command::new("bash")
                .args(["-c", SCRIPT, "bash"])
                .arg(&archive)
                .arg(&image)
                .output()
                .expect("run patch wrapper");
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(result.stdout, b"fixture image");
        }
    }
}
