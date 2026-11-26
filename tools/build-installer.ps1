<#
.SYNOPSIS
  Builds the Keeper Forge installer on this PC, with the same steps as the release workflow.
.DESCRIPTION
  Needs Rust (the pinned GNU toolchain), cargo-about, NSIS and Git for Windows. The installer lands
  in egui-out\. It is not signed, so Windows SmartScreen warns when it is run. It uses the develop Hub
  and does not look for updates.
.PARAMETER Bash
  Path of Git Bash (bash.exe). Found from git when left out.
#>
param([string]$Bash)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
# By full path: a plain "bash" can be the one of WSL.
if (-not $Bash) {
    $git = Get-Command git -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $git) { throw 'git was not found: install Git for Windows or pass -Bash <path of bash.exe>' }
    # git names <its folder>\mingw64\libexec\git-core, wherever it is installed.
    $Bash = Join-Path (& $git.Source --exec-path) '..\..\..\bin\bash.exe'
}
if (-not (Test-Path -LiteralPath $Bash -PathType Leaf)) {
    throw "Git Bash was not found at ${Bash}: pass -Bash <path of bash.exe>"
}
$Bash = (Resolve-Path -LiteralPath $Bash).Path
$version = (Get-Content "$root\version.txt" -Raw).Trim()
# Put back at the end: they would otherwise stay set in the caller's shell.
$before = $env:RUSTUP_TOOLCHAIN, $env:CK_FORGE_VERSION, $env:PATH

Push-Location $root
try {
    $env:RUSTUP_TOOLCHAIN = '1.99.1-x86_64-pc-windows-gnu'
    $env:CK_FORGE_VERSION = $version
    $env:PATH = "$env:PATH;${env:ProgramFiles(x86)}\NSIS"

    cargo build --release --locked -p ck-forge --target-dir target
    if ($LASTEXITCODE) { throw 'building ck-forge failed' }
    cargo build --release --locked --manifest-path app-egui\Cargo.toml --target-dir app-egui\target
    if ($LASTEXITCODE) { throw 'building the window failed' }

    New-Item -ItemType Directory -Force egui-out | Out-Null
    & $Bash tools/make-egui-licences.sh egui-out/THIRD-PARTY-LICENSES.txt
    if ($LASTEXITCODE) { throw 'writing the licences failed' }
    & $Bash tools/make-egui-installer.sh $env:CK_FORGE_VERSION egui-out/THIRD-PARTY-LICENSES.txt egui-out/keeper-forge-setup-x64.exe
    if ($LASTEXITCODE) { throw 'building the installer failed' }
} finally {
    Pop-Location
    $env:RUSTUP_TOOLCHAIN, $env:CK_FORGE_VERSION, $env:PATH = $before
}

Get-Item "$root\egui-out\keeper-forge-setup-x64.exe" | Select-Object Name, Length, FullName
