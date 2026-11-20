# Windows wrapper: runs scripts/build-doom.sh with Git Bash.
#   .\scripts\build-doom.ps1 [x86_64|aarch64|riscv64|all]
param([string]$Arch = 'all')
$ErrorActionPreference = 'Stop'
$bash = (Get-Command bash -ErrorAction SilentlyContinue).Source
if (-not $bash) { $bash = 'C:\Program Files\Git\bin\bash.exe' }
& $bash (Join-Path $PSScriptRoot 'build-doom.sh') $Arch
exit $LASTEXITCODE
