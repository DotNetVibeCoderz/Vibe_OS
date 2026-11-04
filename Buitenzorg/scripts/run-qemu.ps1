# Boot Buitenzorg OS in QEMU with a display window + serial on the console.
# usage: .\scripts\run-qemu.ps1 [-Uefi] [-SelfTest]
#   -SelfTest: the self-test boot (every demo, self-test and milestone, as the
#              smoke tests run it) instead of the normal boot straight to the
#              desktop -- QEMU's fw_cfg file opt/buitenzorg/selftest.
param([switch]$Uefi, [switch]$SelfTest)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

if ($SelfTest) { $env:QEMU_EXTRA = (($env:QEMU_EXTRA, "-fw_cfg name=opt/buitenzorg/selftest,string=1") -join " ").Trim() }
Push-Location (Join-Path $root "kernel")
try {
    if ($Uefi) { cargo run --release -p bzimage -- --run --uefi }
    else       { cargo run --release -p bzimage -- --run }
} finally { Pop-Location }
