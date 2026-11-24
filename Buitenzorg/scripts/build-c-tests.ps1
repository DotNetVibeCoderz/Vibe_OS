# Windows wrapper: runs scripts/build-c-tests.sh with Git Bash.
#   .\scripts\build-c-tests.ps1 [x86_64|aarch64|riscv64|all]
param([string]$Arch = 'all')
$ErrorActionPreference = 'Stop'
$bash = (Get-Command bash -ErrorAction SilentlyContinue).Source
if (-not $bash) { $bash = 'C:\Program Files\Git\bin\bash.exe' }
& $bash (Join-Path $PSScriptRoot 'build-c-tests.sh') $Arch
exit $LASTEXITCODE
