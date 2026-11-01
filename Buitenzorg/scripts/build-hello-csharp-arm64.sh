#!/usr/bin/env bash
# Build the ARM64 user programs (multi-arch M3): the same C# sources, compiled
# by bflat for linux-arm64 (zerolib) and linked with the aarch64 build of the
# bzstart.rs shim (SVC #0 syscalls) into static ELFs at 0x400000 under
# userland/hello-csharp/arm64/. bzimage puts them on dist/buitenzorg-arm64.img
# and the ARM64 kernel loads them from /disk (virtio-blk).
#
# arm64 code needs ThrowHelpers.ThrowOverflowException (division checks),
# which bflat's zerolib lacks: tools/ZerolibPatch adds it to a copy of
# zerolib, used by a private bflat copy in kernel/target/bflat-arm64 (the
# installed bflat is not modified).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
dir="$root/userland/hello-csharp"
bflat_home="$root/tools/bflat"
patched="$root/kernel/target/bflat-arm64"
bflat="$patched/bflat"
if [ ! -x "$bflat" ]; then
    echo "==> preparing a bflat copy with the patched arm64 zerolib"
    mkdir -p "$(dirname "$patched")"
    cp -r "$bflat_home" "$patched"
    dotnet run --project "$root/tools/ZerolibPatch" -c Release -- \
        "$bflat_home/lib/linux/arm64/zerolib.dll" "$patched/lib/linux/arm64/zerolib.dll" \
        || { rm -rf "$patched"; echo "ZerolibPatch failed"; exit 1; }
fi

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
build_prog bcl bcl.cs bzbcl.cs
build_prog proctest proctest.cs bzbcl.cs bzbcl2.cs
build_prog calc calc.cs bzui.cs bzgfx.cs
build_prog g2048 game2048.cs bzui.cs bzgfx.cs
build_prog ui ui.cs bzui.cs bzgfx.cs
build_prog draw draw.cs bzgfx.cs
build_prog xox xox.cs
