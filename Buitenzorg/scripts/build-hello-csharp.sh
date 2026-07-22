#!/usr/bin/env bash
# Build the v0.4 "Tunas" milestone program: compile hello.cs (C#) to a
# freestanding static ELF that runs in ring 3 on Buitenzorg.
# Pipeline: bflat (C# -> object, zerolib) + rustc (shim) then rust-lld links.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
dir="$root/userland/hello-csharp"
bflat="$root/tools/bflat/bflat"

lld="$(ls "$HOME"/.rustup/toolchains/nightly-*/lib/rustlib/*/bin/rust-lld 2>/dev/null | head -1)"
[ -n "$lld" ] || { echo "rust-lld not found (install rust nightly)"; exit 1; }

cd "$dir"
echo "==> rustc: bzstart.rs -> bzstart.o (freestanding shim)"
rustc +nightly --edition 2021 --crate-type staticlib --emit obj \
    --target x86_64-unknown-none -C panic=abort -C opt-level=2 \
    -o bzstart.o bzstart.rs

# name:sources (space-separated); apps that use Buitenzorg.Drawing add bzdraw.cs
build_prog() {
    local elf="$1"; shift
    echo "==> bflat: $* -> $elf.o"
    "$bflat" build "$@" --stdlib:zero --os:linux --arch:x64 -c -Os \
        --no-debug-info --no-reflection --no-stacktrace-data -o "$elf.o"
    echo "==> rust-lld: link -> $elf.elf"
    "$lld" -flavor gnu -o "$elf.elf" -T user.ld --static --no-dynamic-linker \
        -e _start "$elf.o" bzstart.o
    echo "==> $elf.elf: $(stat -c%s "$elf.elf" 2>/dev/null || stat -f%z "$elf.elf") bytes"
}
build_prog hello hello.cs
build_prog svc svc.cs
build_prog xox xox.cs
build_prog paint paint.cs bzdraw.cs
build_prog taskmgr taskmgr.cs bzdraw.cs
build_prog widget widget.cs bzdraw.cs
build_prog webview webview.cs bzdraw.cs
