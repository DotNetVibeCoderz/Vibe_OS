#!/usr/bin/env bash
# Buitenzorg OS - full build (Linux/macOS/CI).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"

if [ -x "$root/tools/bflat/bflat" ]; then
    echo "==> user program (C# -> ELF)"
    "$root/scripts/build-hello-csharp.sh"
    # The same C# programs for ARM64 (packed into dist/buitenzorg-arm64.img).
    "$root/scripts/build-hello-csharp-arm64.sh"
else
    echo "==> skipping C# user program (tools/bflat not installed)"
fi

# Multi-arch M1: Limine binaries (pinned + SHA-256) so bzimage also builds the
# Limine-protocol UEFI image. Optional: a download failure only skips it.
"$root/scripts/fetch-limine.sh" || echo "==> Limine not fetched; no Limine image"
"$root/scripts/fetch-rpi4-firmware.sh" || echo "==> Raspberry Pi firmware not fetched; no SD image"

echo "==> kernel (Rust)"
# Multi-arch M3: the ARM64 kernel first (bzimage packs it into
# dist/buitenzorg-arm64.img when it exists). Non-fatal.
(cd "$root/kernel" && cargo build --release -p bzkernel --target aarch64-unknown-none-softfloat) \
  || echo "==> ARM64 kernel build failed; no ARM64 image"
# Multi-arch M4: the RISC-V kernel (-> dist/buitenzorg-riscv64.img). Non-fatal.
(cd "$root/kernel" && cargo build --release -p bzkernel --target riscv64imac-unknown-none-elf) \
  || echo "==> RISC-V kernel build failed; no RISC-V image"
# The managed C# runtime (RustCLR host, /disk/CLRHOST.ELF) for every
# architecture. Non-fatal: the images build without it.
"$root/scripts/build-clrhost.sh" || echo "==> managed runtime host build failed; no CLRHOST.ELF"
(cd "$root/kernel" && cargo build --release && cargo run --release -p bzimage -- --out "$root/dist")

echo "==> runtime + sdk (.NET)"
dotnet build "$root/Buitenzorg.slnx" -c Release

echo "==> done. images in dist/"
