# Boot Buitenzorg OS in QEMU with a display window + serial on the console.
# usage: .\scripts\run-qemu.ps1 [-Uefi]
param([switch]$Uefi)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

Push-Location (Join-Path $root "kernel")
try {
    if ($Uefi) { cargo run --release -p bzimage -- --run --uefi }
    else       { cargo run --release -p bzimage -- --run }
} finally { Pop-Location }
