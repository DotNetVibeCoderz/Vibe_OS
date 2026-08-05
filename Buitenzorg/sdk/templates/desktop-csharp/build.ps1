# Build & deploy a Buitenzorg desktop app template (Windows / PowerShell).
#
# Compiles app.cs against the Buitenzorg.UI / .Drawing library sources with
# bflat (--stdlib:zero), links it with the bzstart shim into a static ELF, and
# deploys it as userland/hello-csharp/userapp.elf — which the kernel image
# embeds as /disk/USERAPP.ELF, launchable in the OS with `run myapp`.
#
#   .\build.ps1              # build + deploy the ELF
#   .\build.ps1 -Run         # build + deploy, then rebuild the image and boot QEMU
#   .\build.ps1 -Libs bzgfx.cs,bzui.cs,bzbcl.cs,bzbcl2.cs   # extra library sources
#
# -RepoRoot is auto-detected (walks up for tools/bflat); pass it if the app
# folder lives outside the repo tree.
param(
    [string]$RepoRoot,
    [string[]]$Libs = @("bzgfx.cs", "bzui.cs"),
    [switch]$Run
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot

# Locate the repo root (the folder that holds tools/bflat + userland).
if (-not $RepoRoot) {
    $d = $here
    while ($d -and -not (Test-Path (Join-Path $d "tools\bflat"))) { $d = Split-Path -Parent $d }
    if (-not $d) { throw "Could not find the Buitenzorg repo root (tools/bflat). Pass -RepoRoot." }
    $RepoRoot = $d
}
$ul = Join-Path $RepoRoot "userland\hello-csharp"
$bflat = Join-Path $RepoRoot "tools\bflat\bflat.exe"
if (-not (Test-Path $bflat)) { throw "bflat not found at $bflat (run scripts/quickstart.ps1 first)." }
$lld = Get-ChildItem "$env:USERPROFILE\.rustup\toolchains\nightly-*\lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe" |
    Select-Object -First 1 -ExpandProperty FullName
if (-not $lld) { throw "rust-lld not found (install the rust nightly toolchain)." }

# The freestanding startup/PAL shim (shared with every C# app).
$bzstart = Join-Path $ul "bzstart.o"
if (-not (Test-Path $bzstart) -or (Get-Item (Join-Path $ul "bzstart.rs")).LastWriteTime -gt (Get-Item $bzstart).LastWriteTime) {
    Write-Host "==> rustc: bzstart.rs -> bzstart.o"
    Push-Location $ul
    & rustc +nightly --edition 2021 --crate-type staticlib --emit obj `
        --target x86_64-unknown-none -C panic=abort -C opt-level=2 -o bzstart.o bzstart.rs
    Pop-Location
    if ($LASTEXITCODE -ne 0) { throw "rustc failed" }
}

# Full paths to the library sources this app pulls in.
$libPaths = $Libs | ForEach-Object { Join-Path $ul $_ }
$appCs = Join-Path $here "app.cs"

Write-Host "==> bflat: app.cs + [$($Libs -join ', ')] -> userapp.o"
& $bflat build $appCs @libPaths --stdlib:zero --os:linux --arch:x64 -c -Os `
    --no-debug-info --no-reflection --no-stacktrace-data -o (Join-Path $here "userapp.o")
if ($LASTEXITCODE -ne 0) { throw "bflat failed" }

Write-Host "==> rust-lld: link -> userapp.elf"
& $lld -flavor gnu -o (Join-Path $here "userapp.elf") -T (Join-Path $ul "user.ld") `
    --static --no-dynamic-linker -e _start (Join-Path $here "userapp.o") $bzstart
if ($LASTEXITCODE -ne 0) { throw "link failed" }

# Deploy: drop the ELF where build.rs embeds it as /disk/USERAPP.ELF.
Copy-Item (Join-Path $here "userapp.elf") (Join-Path $ul "userapp.elf") -Force
Write-Host "==> deployed: $ul\userapp.elf ($((Get-Item (Join-Path $here 'userapp.elf')).Length) bytes)" -ForegroundColor Green
Write-Host "    In the OS terminal, launch it with:  run myapp"

if ($Run) {
    Write-Host "==> rebuilding image + booting QEMU (run 'run myapp' at the prompt)"
    Push-Location (Join-Path $RepoRoot "kernel")
    & cargo run --release -p bzimage -- --run
    Pop-Location
}
