# ck-forge

Keeper Forge prepares Keeper OS sticks for a contest: settings, branding and writing the image
to USB drives. It is a command line (`ck-forge`) and a window (`app-egui/`, built with egui) that do
the same work through the same code (`crates/ck-admin-core`, `crates/ck-writer`).

Work in progress.

## Download and install

Every release has a Windows installer and the command line tool on the
[releases page](https://github.com/contest-keeper/ck-forge/releases/latest). The file names never
change, so these links always give the newest release:

- `.../releases/latest/download/keeper-forge-setup-x64.exe`: the installer (needs Windows 10 or
  newer; it installs for all users, because the window starts the writer with administrator rights
  and that program must sit where only administrators can change it)
- `.../releases/latest/download/ck-forge-windows-x64.exe`: the command line tool alone
- `.../releases/latest/download/keeper-forge-macos-universal.dmg`: the window for macOS (Intel and
  Apple silicon), when the macOS build is on; open it and drag the app to Applications. It is signed
  ad hoc and not notarized, so macOS blocks the first start: then allow Keeper Forge in System
  Settings > Privacy & Security. Check the download against the checksums first
- `.../releases/latest/download/SHA256SUMS.txt`: the checksum of every file

Preview builds of `develop` have the same file names under `.../releases/download/dev/`.

The files are not code-signed yet, so Windows SmartScreen may warn ("More info", then "Run anyway").
Check what you downloaded before running it:

```powershell
(Get-FileHash .\keeper-forge-setup-x64.exe -Algorithm SHA256).Hash   # compare with SHA256SUMS.txt
gh attestation verify .\keeper-forge-setup-x64.exe --repo contest-keeper/ck-forge
```

The second command proves the file was built by this repository's release workflow.

Installed copies look for a newer build when they start and offer to install it. On Windows the window
downloads the installer, checks its signature and runs it. On macOS it downloads the disk image,
checks it and opens it, and you drag the app to Applications. Every merge to `develop` publishes a
preview build to the develop feed; releases publish their own feed.

To make an update mandatory, set `min-version.txt` to the oldest version that may still run (for
example the version being released). A copy older than that must update before it can be used.

Updates are signed with `tools/update-signer`. Each environment has its own key in its Azure Key Vault,
made by the pipeline on its first run and read from there afterwards, so no key is stored in GitHub.
The key must never be replaced: installed copies would refuse every later update. A release therefore
needs the production Hub (and its Key Vault) to be deployed.

The installer replaces an older Keeper Forge on the same PC and keeps the profiles and the Hub
sign-in. It always installs under Program Files.

To build the installer yourself: `tools/build-installer.ps1`.

## The window

The window has five pages: sign in to the **Hub**, edit and save a contest **Profile**, pick or make
base **Images**, see and retire **Sticks**, and **Write** them to USB. Writing a stick opens the
Windows permission prompt; the window itself runs without administrator rights.

Build and run it (on Windows with the toolchain in `ck-os/docs/dev-setup.md`):

```powershell
cargo build -p ck-forge                      # the helper that writes the stick
cargo run --manifest-path app-egui/Cargo.toml
```

## The command line

```sh
ck-forge profile new p.json                  # then edit it, and: ck-forge profile check p.json
ck-forge image catalog                       # the base images Keeper can make for you
ck-forge image create --distro ubuntu-24.04 --out base.img --patch ck-os-patch.tar.gz
ck-forge image build --base base.img --out stick.img --profile p.json
ck-forge stick list                          # disks, and which ones may be written
ck-forge stick write stick.img --disk 3      # in an administrator terminal (sudo on macOS); erases the stick
ck-forge registry push --hub https://hub.example.org --user ana
```

`image create` is for an admin with no contest image: it downloads the official Ubuntu cloud image
(checked against the hash in the catalog), boots it once in QEMU to install a desktop and the contest
tools, and applies the Keeper OS patch (needs WSL). It takes 15 to 40 minutes, needs QEMU and 30 GB
free, and writes `base.img.base.json` with what the image was made from. The window does the same in
the image step ("No base image? Make one"). Use the result as `--base` below.

`image build` takes about a second. The settings of a stick sit in the EFI partition at the start of
the image, so `stick.img` holds only that start (about 130 MB) and a manifest, `stick.img.thin.json`,
that names the base image. `stick write` reads the start from `stick.img` and the rest from the base,
and refuses to write when the base is not the image `stick.img` was made from. Keep the base where
it was. `--full-copy` makes a whole 30 GB image instead, which runs without the base. Several sticks
are written at the same time, which takes about as long as the slowest one: give one image per stick,
each made with `image build` (so each has its own identity), in the order of the disks:

```sh
ck-forge stick write a.img b.img c.img --disk 3 --disk 4 --disk 5
```

The base is read once for all of them. A stick that already holds the image, such as one written for
the last contest from the same base, is written only where it differs; every stick is read back and
checked completely either way. Only a plain base image, with no stick identity, may be written
to several sticks as one image.

A stick is written, read back and compared by SHA-256. Only USB disks are written, never the disk
the computer runs from.

On macOS build the command line tool with `cargo build --release -p ck-forge` and run it with `sudo`
for the write (`sudo ./target/release/ck-forge stick write stick.img --disk 3`); `stick list` needs no `sudo`. Only
external removable USB sticks are offered. The window runs on macOS too; its Write page lists the disks and prints the `sudo` command. To try the writer without a stick, see [docs/testing-the-writer.md](docs/testing-the-writer.md).
