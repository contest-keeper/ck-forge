<#
.SYNOPSIS
  Builds the Keeper Forge installer on this PC, with the same steps as the release workflow.
.DESCRIPTION
  Needs Rust (the pinned GNU toolchain), cargo-about, NSIS and Git for Windows. The installer lands
  in egui-out\. It is not signed, so Windows SmartScreen warns when it is run. It uses the develop Hub
  and does not look for updates.
#>
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$env:RUSTUP_TOOLCHAIN = '1.99.1-x86_64-pc-windows-gnu'
$env:CK_FORGE_VERSION = (Get-Content "$root\version.txt" -Raw).Trim()
$env:PATH = "$env:PATH;${env:ProgramFiles(x86)}\NSIS"
# By full path: a plain "bash" can be the one of WSL.
$bash = "$env:ProgramFiles\Git\bin\bash.exe"

Push-Location $root
try {
    cargo build --release --locked -p ck-forge
    if ($LASTEXITCODE) { throw 'building ck-forge failed' }
    cargo build --release --locked --manifest-path app-egui\Cargo.toml
    if ($LASTEXITCODE) { throw 'building the window failed' }

    New-Item -ItemType Directory -Force egui-out | Out-Null
    & $bash tools/make-egui-licences.sh egui-out/THIRD-PARTY-LICENSES.txt
    if ($LASTEXITCODE) { throw 'writing the licences failed' }
    & $bash tools/make-egui-installer.sh $env:CK_FORGE_VERSION egui-out/THIRD-PARTY-LICENSES.txt egui-out/keeper-forge-setup-x64.exe
    if ($LASTEXITCODE) { throw 'building the installer failed' }
} finally { Pop-Location }

Get-Item "$root\egui-out\keeper-forge-setup-x64.exe" | Select-Object Name, Length, FullName
