# Buitenzorg OS - full build (Windows).
# Builds the Rust kernel + boot images and the .NET solution, outputs to dist/.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

# v0.4 "Tunas": build the C# user program first (if bflat is present) so the
# kernel image can embed it. Optional — the kernel boots fine without it.
if (Test-Path (Join-Path $root "tools\bflat\bflat.exe")) {
    Write-Host "==> user program (C# -> ELF)" -ForegroundColor Green
    & (Join-Path $PSScriptRoot "build-hello-csharp.ps1")
    if ($LASTEXITCODE -ne 0) { throw "hello-csharp build failed" }
    # The same C# programs for ARM64 (packed into dist/buitenzorg-arm64.img).
    & (Join-Path $PSScriptRoot "build-hello-csharp-arm64.ps1")
    if ($LASTEXITCODE -ne 0) { throw "ARM64 C# build failed" }
} else {
    Write-Host "==> skipping C# user program (tools/bflat not installed)" -ForegroundColor Yellow
}

# Multi-arch M1: Limine binaries (pinned + SHA-256) so bzimage also builds the
# Limine-protocol UEFI image. Optional: a download failure only skips it.
try { & (Join-Path $PSScriptRoot "fetch-limine.ps1") } catch { Write-Host "==> Limine not fetched ($_); no Limine image" -ForegroundColor Yellow }

Write-Host "==> kernel (Rust)" -ForegroundColor Green
Push-Location (Join-Path $root "kernel")
try {
    # Multi-arch M3: the ARM64 kernel (bzimage packs it into
    # dist/buitenzorg-arm64.img when it exists). Non-fatal.
    cargo build --release -p bzkernel --target aarch64-unknown-none-softfloat
    if ($LASTEXITCODE -ne 0) { Write-Host "==> ARM64 kernel build failed; no ARM64 image" -ForegroundColor Yellow }
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "kernel build failed" }
    cargo run --release -p bzimage -- --out (Join-Path $root "dist")
    if ($LASTEXITCODE -ne 0) { throw "image build failed" }
} finally { Pop-Location }

Write-Host "==> runtime + sdk (.NET)" -ForegroundColor Green
dotnet build (Join-Path $root "Buitenzorg.slnx") -c Release
if ($LASTEXITCODE -ne 0) { throw "dotnet build failed" }

Write-Host "==> done. images in dist\" -ForegroundColor Green
