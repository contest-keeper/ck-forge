# Signs the given files with the organization's code signing certificate (Authenticode).
# Needs CK_CODESIGN_PFX_BASE64 (the .pfx file in base64) and CK_CODESIGN_PASSWORD in the environment.
# The develop workflow runs it only when the certificate secret exists.
param([Parameter(Mandatory, ValueFromRemainingArguments)][string[]]$Files)

$ErrorActionPreference = 'Stop'
if (-not $env:CK_CODESIGN_PFX_BASE64) { throw 'CK_CODESIGN_PFX_BASE64 is not set' }

$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
    Sort-Object FullName | Select-Object -Last 1
if (-not $signtool) { throw 'signtool.exe was not found' }

$pfx = Join-Path $env:RUNNER_TEMP 'codesign.pfx'
[IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:CK_CODESIGN_PFX_BASE64))
try {
    & $signtool.FullName sign /f $pfx /p $env:CK_CODESIGN_PASSWORD /fd SHA256 `
        /tr http://timestamp.digicert.com /td SHA256 $Files
    if ($LASTEXITCODE -ne 0) { throw "signtool failed with $LASTEXITCODE" }
} finally {
    Remove-Item $pfx -Force -ErrorAction SilentlyContinue
}
