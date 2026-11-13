# ck-forge

Keeper Forge prepares Keeper OS sticks for a contest: settings, branding and writing the image
to USB drives. It is a command line (`ck-forge`) and a window (`app/` and `ui/`) that do the same
work through the same code (`crates/ck-admin-core`, `crates/ck-writer`).

Work in progress.

## The window

The window has four steps: edit and save a contest **profile**, make a stick **image** from the base
image, **write** it to a USB stick, and tell the **Hub** about the sticks. Writing a stick opens the
Windows permission prompt; the window itself runs without administrator rights.

Build and run it on Windows (needs the toolchain in `ck-os/docs/dev-setup.md`, and Node with pnpm):

```powershell
cd ui;  pnpm install; pnpm build; cd ..
cargo build -p ck-forge                      # the helper that writes the stick
cd app; cargo run --features custom-protocol
```

For work on the page itself, run `pnpm dev` in `ui/` and `cargo run` in `app/`.

## The command line

```sh
ck-forge profile new p.json                  # then edit it, and: ck-forge profile check p.json
ck-forge image build --base base.img --out stick.img --profile p.json
ck-forge stick list                          # disks, and which ones may be written
ck-forge stick write stick.img --disk 3      # in an administrator terminal; erases the stick
ck-forge registry push --hub https://hub.example.org --user ana
```

A stick is written, read back and compared by SHA-256. Only USB disks are written, never the disk
Windows runs from. To try the writer without a stick, see [docs/testing-the-writer.md](docs/testing-the-writer.md).
