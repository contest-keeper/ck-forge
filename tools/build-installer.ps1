<#
.SYNOPSIS
  Builds the Keeper Forge installer on this PC, the same way the release workflow does.
.DESCRIPTION
  Needs Rust (the pinned GNU toolchain), Node and pnpm (corepack is fine). The installer lands in
  app\target\x86_64-pc-windows-gnu\release\bundle\nsis\. It is not signed, so Windows SmartScreen
  warns when it is run.
#>
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

Push-Location $root
try {
    cargo build --release --locked -p ck-forge
    if ($LASTEXITCODE) { throw 'building ck-forge failed' }

    # The installer ships the command line tool next to the window: the window starts it with
    # administrator rights to write sticks.
    New-Item -ItemType Directory -Force app\binaries | Out-Null
    Copy-Item target\release\ck-forge.exe app\binaries\ck-forge-x86_64-pc-windows-gnu.exe -Force

    Push-Location ui
    try {
        corepack pnpm install --frozen-lockfile
        if ($LASTEXITCODE) { throw 'pnpm install failed' }
    } finally { Pop-Location }

    # Tauri runs "pnpm --dir ui build" from the repository root, so pnpm must be on PATH.
    if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
        $shim = Join-Path $env:TEMP 'ck-pnpm-shim'
        New-Item -ItemType Directory -Force $shim | Out-Null
        Set-Content -Path (Join-Path $shim 'pnpm.cmd') -Value "@echo off`r`ncd /d `"$root\ui`"`r`ncorepack pnpm build"
        $env:PATH = "$shim;$env:PATH"
    }

    Push-Location app
    try {
        node ..\ui\node_modules\@tauri-apps\cli\tauri.js build --target x86_64-pc-windows-gnu
        if ($LASTEXITCODE) { throw 'building the installer failed' }
    } finally { Pop-Location }
} finally { Pop-Location }

Get-ChildItem "$root\app\target\x86_64-pc-windows-gnu\release\bundle\nsis\*-setup.exe" |
    Select-Object Name, Length, FullName
