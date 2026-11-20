# Windows wrapper: runs scripts/fetch-doom.sh with Git Bash (curl, sha256sum,
# unzip). See that script for what is fetched and why it is not committed.
$ErrorActionPreference = 'Stop'
$bash = (Get-Command bash -ErrorAction SilentlyContinue).Source
if (-not $bash) { $bash = 'C:\Program Files\Git\bin\bash.exe' }
& $bash (Join-Path $PSScriptRoot 'fetch-doom.sh')
exit $LASTEXITCODE
