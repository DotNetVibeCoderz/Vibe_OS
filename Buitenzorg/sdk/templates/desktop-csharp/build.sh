#!/usr/bin/env bash
# Build & deploy a Buitenzorg desktop app template (Linux / macOS).
#
# Compiles app.cs against the Buitenzorg.UI / .Drawing library sources with
# bflat (--stdlib:zero), links it with the bzstart shim into a static ELF, and
# deploys it as userland/hello-csharp/userapp.elf — which the kernel image
# embeds as /disk/USERAPP.ELF, launchable in the OS with `run myapp`.
#
#   ./build.sh                 # build + deploy the ELF
#   ./build.sh --run           # build + deploy, then rebuild the image and boot QEMU
#   LIBS="bzgfx.cs bzui.cs bzbcl.cs bzbcl2.cs" ./build.sh   # extra library sources
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Locate the repo root (holds tools/bflat + userland).
root="${REPO_ROOT:-}"
if [ -z "$root" ]; then
    d="$here"
    while [ -n "$d" ] && [ ! -d "$d/tools/bflat" ]; do d="$(dirname "$d")"; [ "$d" = "/" ] && break; done
    [ -d "$d/tools/bflat" ] || { echo "Could not find repo root (tools/bflat). Set REPO_ROOT." >&2; exit 1; }
    root="$d"
fi
ul="$root/userland/hello-csharp"
bflat="$root/tools/bflat/bflat"
[ -x "$bflat" ] || { echo "bflat not found at $bflat (run scripts/quickstart.sh first)." >&2; exit 1; }
lld="$(command -v rust-lld || true)"
[ -n "$lld" ] || lld="$(find "$HOME/.rustup/toolchains" -name rust-lld -type f 2>/dev/null | head -n1)"
[ -n "$lld" ] || { echo "rust-lld not found (install the rust nightly toolchain)." >&2; exit 1; }

libs="${LIBS:-bzgfx.cs bzui.cs}"

# The freestanding startup/PAL shim (shared with every C# app).
if [ ! -f "$ul/bzstart.o" ] || [ "$ul/bzstart.rs" -nt "$ul/bzstart.o" ]; then
    echo "==> rustc: bzstart.rs -> bzstart.o"
    ( cd "$ul" && rustc +nightly --edition 2021 --crate-type staticlib --emit obj \
        --target x86_64-unknown-none -C panic=abort -C opt-level=2 -o bzstart.o bzstart.rs )
fi

libpaths=""; for l in $libs; do libpaths="$libpaths $ul/$l"; done

echo "==> bflat: app.cs + [$libs] -> userapp.o"
"$bflat" build "$here/app.cs" $libpaths --stdlib:zero --os:linux --arch:x64 -c -Os \
    --no-debug-info --no-reflection --no-stacktrace-data -o "$here/userapp.o"

echo "==> rust-lld: link -> userapp.elf"
"$lld" -flavor gnu -o "$here/userapp.elf" -T "$ul/user.ld" \
    --static --no-dynamic-linker -e _start "$here/userapp.o" "$ul/bzstart.o"

# Deploy: drop the ELF where build.rs embeds it as /disk/USERAPP.ELF.
cp -f "$here/userapp.elf" "$ul/userapp.elf"
echo "==> deployed: $ul/userapp.elf ($(wc -c < "$here/userapp.elf") bytes)"
echo "    In the OS terminal, launch it with:  run myapp"

if [ "${1:-}" = "--run" ]; then
    echo "==> rebuilding image + booting QEMU (run 'run myapp' at the prompt)"
    ( cd "$root/kernel" && cargo run --release -p bzimage -- --run )
fi
