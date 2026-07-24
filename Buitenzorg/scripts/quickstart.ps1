# Buitenzorg OS - one-command quick start (Windows).
#
# Installs every dependency needed to build and boot Buitenzorg OS, then builds
# the disk image and launches it in QEMU. Safe to re-run: each step is skipped
# if the tool is already present.
#
#   Dependencies handled: Rust (rustup, nightly toolchain + bare-metal target),
#   .NET SDK, QEMU, and bflat (the C#->native compiler, downloaded into tools/).
#
# Usage:
#   .\scripts\quickstart.ps1              # install deps, build, boot in QEMU
#   .\scripts\quickstart.ps1 -NoRun       # install + build only
#   .\scripts\quickstart.ps1 -SmokeTest   # install + build + headless self-test
param(
    [switch]$NoRun,
    [switch]$SmokeTest
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root

function Info($m) { Write-Host "==> $m" -ForegroundColor Cyan }
function Ok($m)   { Write-Host "  ok: $m" -ForegroundColor Green }
function Warn($m) { Write-Host "  ! $m" -ForegroundColor Yellow }
function Have($cmd) { return [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }

function Ensure-Winget {
    if (-not (Have winget)) {
        throw "winget (App Installer) not found. Install it from the Microsoft Store, or install Rust/.NET/QEMU manually - see docs/getting-started.md."
    }
}

# --- 1. Rust (rustup) --------------------------------------------------------
Info "Checking Rust toolchain (rustup)"
if (-not (Have rustup)) {
    Ensure-Winget
    Info "Installing Rust via winget..."
    winget install --id Rustlang.Rustup -e --accept-source-agreements --accept-package-agreements
    $env:Path += ";$env:USERPROFILE\.cargo\bin"
}
if (-not (Have rustup)) { throw "rustup still not on PATH; open a new terminal and re-run." }
Ok "rustup present"
# The kernel pins its toolchain via kernel/rust-toolchain.toml (nightly + the
# x86_64-unknown-none target + rust-src); rustup installs it on first build.
# Pre-install so the first build doesn't stall on a big toolchain download.
Info "Ensuring nightly toolchain + bare-metal target (from rust-toolchain.toml)"
rustup show 2>&1 | Out-Null
Ok "Rust ready"

# --- 2. .NET SDK -------------------------------------------------------------
Info "Checking .NET SDK"
if (-not (Have dotnet)) {
    Ensure-Winget
    Info "Installing .NET SDK via winget..."
    winget install --id Microsoft.DotNet.SDK.Preview -e --accept-source-agreements --accept-package-agreements
}
if (Have dotnet) { Ok ".NET SDK: $(dotnet --version)" } else { Warn ".NET SDK not detected; the C# ABI tests will be skipped (kernel still builds)." }

# --- 3. QEMU -----------------------------------------------------------------
Info "Checking QEMU"
$qemuExe = "C:\Program Files\qemu\qemu-system-x86_64.exe"
if (-not (Have qemu-system-x86_64) -and -not (Test-Path $qemuExe)) {
    Ensure-Winget
    Info "Installing QEMU via winget..."
    winget install --id SoftwareFreedomConservancy.QEMU -e --accept-source-agreements --accept-package-agreements
}
if (Have qemu-system-x86_64) { Ok "QEMU on PATH" }
elseif (Test-Path $qemuExe) { Ok "QEMU at $qemuExe (auto-detected by the build)" }
else { Warn "QEMU not detected; set the QEMU env var to qemu-system-x86_64.exe before running." }

# --- 4. bflat (C# -> native) -------------------------------------------------
Info "Checking bflat (tools/bflat)"
$bflat = Join-Path $root "tools\bflat\bflat.exe"
if (-not (Test-Path $bflat)) {
    Info "Downloading the latest bflat release for windows-x64..."
    $rel = Invoke-RestMethod "https://api.github.com/repos/bflattened/bflat/releases/latest" -Headers @{ "User-Agent" = "buitenzorg" }
    $asset = $rel.assets | Where-Object { $_.name -match "windows" -and $_.name -match "x64" -and $_.name -notmatch "debugsymbols" -and $_.name -like "*.zip" } | Select-Object -First 1
    if (-not $asset) { throw "Could not find a windows-x64 bflat asset in the latest release. Download bflat manually into tools/bflat/ (see docs/getting-started.md)." }
    $zip = Join-Path $env:TEMP $asset.name
    Invoke-WebRequest $asset.browser_download_url -OutFile $zip
    $dest = Join-Path $root "tools\bflat"
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    Expand-Archive -Path $zip -DestinationPath $dest -Force
    Remove-Item $zip -Force
    # Some archives nest under a top folder; flatten so tools/bflat/bflat.exe exists.
    if (-not (Test-Path $bflat)) {
        $found = Get-ChildItem -Path $dest -Recurse -Filter bflat.exe | Select-Object -First 1
        if ($found) { Copy-Item -Recurse -Force "$($found.Directory.FullName)\*" $dest }
    }
}
if (Test-Path $bflat) { Ok "bflat present" } else { throw "bflat.exe still missing under tools/bflat/." }

# --- 5. Build + run ----------------------------------------------------------
Info "Building the C# userland apps"
& (Join-Path $root "scripts\build-hello-csharp.ps1")
Info "Building the disk image (kernel + bootloader)"
Push-Location (Join-Path $root "kernel")
try { cargo run --release -p bzimage -- --out ..\dist } finally { Pop-Location }
Ok "Images built: dist\buitenzorg-bios.img, dist\buitenzorg-uefi.img"

if ($SmokeTest) {
    Info "Running the headless smoke test (all 4 boot media)"
    & (Join-Path $root "scripts\smoke-test.ps1")
}
elseif (-not $NoRun) {
    Info "Booting Buitenzorg OS in QEMU (close the window or Ctrl+C to stop)"
    & (Join-Path $root "scripts\run-qemu.ps1")
}
else {
    Ok "Done (build only). Boot it with:  .\scripts\run-qemu.ps1"
}
Pop-Location
