#!/usr/bin/env bash
# Build the ARM64 user programs (multi-arch M3): the same C# sources, compiled
# by bflat for linux-arm64 (zerolib) and linked with the aarch64 build of the
# bzstart.rs shim (SVC #0 syscalls) into static ELFs at 0x400000 under
# userland/hello-csharp/arm64/. bzimage puts them on dist/buitenzorg-arm64.img
# and Limine loads them as boot modules (the ARM64 kernel has no disk driver
# yet). Only programs whose syscalls the ARM64 kernel serves are listed.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
dir="$root/userland/hello-csharp"
bflat="$root/tools/bflat/bflat"

lld="$(ls "$HOME"/.rustup/toolchains/nightly-*/lib/rustlib/*/bin/rust-lld 2>/dev/null | head -1)"
[ -n "$lld" ] || { echo "rust-lld not found (install rust nightly)"; exit 1; }

cd "$dir"
mkdir -p arm64
echo "==> rustc: bzstart.rs -> arm64/bzstart.o (aarch64 shim)"
rustc +nightly --edition 2021 --crate-type staticlib --emit obj \
    --target aarch64-unknown-none -C panic=abort -C opt-level=2 \
    -o arm64/bzstart.o bzstart.rs

build_prog() {
    local elf="$1"; shift
    echo "==> bflat (linux-arm64): $* -> arm64/$elf.o"
    "$bflat" build "$@" --stdlib:zero --os:linux --arch:arm64 -c -Os \
        --no-debug-info --no-reflection --no-stacktrace-data -o "arm64/$elf.o"
    "$lld" -flavor gnu -o "arm64/$elf.elf" -T user.ld --static --no-dynamic-linker \
        -e _start "arm64/$elf.o" arm64/bzstart.o
    echo "==> arm64/$elf.elf: $(stat -c%s "arm64/$elf.elf" 2>/dev/null || stat -f%z "arm64/$elf.elf") bytes"
}
build_prog hello hello.cs
build_prog matang matang.cs
build_prog heap heap.cs
build_prog gcmem gcmem.cs
build_prog mproc mproc.cs
build_prog thread thread.cs
build_prog sync sync.cs
# bcl.cs: not yet -- bflat's arm64 codegen needs
# ThrowHelpers.ThrowOverflowException, which zerolib lacks.
