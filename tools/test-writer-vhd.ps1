<#
Tests the stick writer without a stick: writes a stick image to a virtual disk (a VHD), which Windows
treats like a real disk, then detaches it so it can be booted in the VM test.

Run it in an administrator PowerShell. It only ever writes to the VHD it creates itself.

  pwsh tools/test-writer-vhd.ps1 -Image C:\rcpc-test\stick2.img -Vhd C:\rcpc-test\writer-test.vhd
#>
param(
    [Parameter(Mandatory)] [string] $Image,
    [Parameter(Mandatory)] [string] $Vhd,
    [string] $Forge = "cargo run -q -p ck-forge --"
)
$ErrorActionPreference = 'Stop'

$admin = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $admin.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run this in an administrator PowerShell."
}
if (Test-Path $Vhd) { throw "$Vhd already exists; choose a new name or delete it." }

# A little more than the image, rounded up to whole MiB, like a real stick that is a bit bigger.
$megabytes = [math]::Ceiling((Get-Item $Image).Length / 1MB) + 16
$before = (Get-Disk).Number

$create = "create vdisk file=`"$Vhd`" maximum=$megabytes type=fixed`nattach vdisk"
$create | diskpart | Out-Null
try {
    $new = @((Get-Disk).Number | Where-Object { $_ -notin $before })
    if ($new.Count -ne 1) { throw "Expected exactly one new disk, found $($new.Count). Nothing was written." }
    $number = $new[0]
    Write-Host "The virtual disk is disk $number."
    Invoke-Expression "$Forge stick write `"$Image`" --disk $number --allow-virtual --yes"
    if ($LASTEXITCODE -ne 0) { throw "The write failed." }
}
finally {
    "select vdisk file=`"$Vhd`"`ndetach vdisk" | diskpart | Out-Null
}
Write-Host "Written to $Vhd. Boot it with: cargo run -p ck-vmtest -- --image $Vhd --out <folder>   (in ck-os)"
