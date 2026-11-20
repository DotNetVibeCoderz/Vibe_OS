#!/usr/bin/env bash
# Fetch what building DOOM for Buitenzorg needs into kernel/target/doom (not
# committed: GPL source, a 29 MB game file, a C toolchain), every download
# checked against a pinned SHA-256:
#   - zig 0.13.0 (its `zig cc` cross-compiles freestanding C for x86-64,
#     ARM64 and RISC-V)
#   - doomgeneric (GPL-2.0) at a fixed commit: DOOM with a small platform layer
#   - Freedoom phase 1 0.13.0 (BSD licence): a free IWAD -> FREEDOOM.WAD
# Then scripts/build-doom.sh builds kernel/target/doom/<arch>/DOOM.ELF, and
# bzimage puts DOOM.ELF + FREEDOOM.WAD on /disk.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
base="$root/kernel/target/doom"
mkdir -p "$base"
cd "$base"

DOOMGENERIC_COMMIT=e257a09527062bfb986b6e5e0780839b6cbd5b71
DOOMGENERIC_SHA256=e10d8506e52462a015c4bbbefe2acb68414a548aef65361adf68f50c2d216c34
FREEDOOM_SHA256=3f9b264f3e3ce503b4fb7f6bdcb1f419d93c7b546f4df3e874dd878db9688f59
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) ZIG_NAME=zig-windows-x86_64-0.13.0; ZIG_EXT=zip; ZIG_SHA256=d859994725ef9402381e557c60bb57497215682e355204d754ee3df75ee3c158 ;;
    Darwin) ZIG_NAME=zig-macos-aarch64-0.13.0; ZIG_EXT=tar.xz; ZIG_SHA256=46fae219656545dfaf4dce12fb4e8685cec5b51d721beee9389ab4194d43394c ;;
    *) ZIG_NAME=zig-linux-x86_64-0.13.0; ZIG_EXT=tar.xz; ZIG_SHA256=d45312e61ebcc48032b77bc4cf7fd6915c11fa16e4aad116b66c9468211230ea ;;
esac

fetch() { # url file sha256
    if [ -f "$2" ] && echo "$3  $2" | sha256sum -c --status 2>/dev/null; then return 0; fi
    echo "fetch-doom: downloading $1"
    curl -fsSL -o "$2.part" "$1"
    if ! echo "$3  $2.part" | sha256sum -c --status; then
        echo "fetch-doom: checksum mismatch for $2"; rm -f "$2.part"; exit 1
    fi
    mv "$2.part" "$2"
}

fetch "https://ziglang.org/download/0.13.0/$ZIG_NAME.$ZIG_EXT" "zig.$ZIG_EXT" "$ZIG_SHA256"
if [ ! -d zig ]; then
    rm -rf "$ZIG_NAME"
    if [ "$ZIG_EXT" = zip ]; then unzip -q "zig.$ZIG_EXT"; else tar -xJf "zig.$ZIG_EXT"; fi
    mv "$ZIG_NAME" zig
fi

fetch "https://github.com/ozkl/doomgeneric/archive/$DOOMGENERIC_COMMIT.zip" doomgeneric.zip "$DOOMGENERIC_SHA256"
if [ ! -d "src/doomgeneric-$DOOMGENERIC_COMMIT" ]; then
    mkdir -p src
    (cd src && unzip -q ../doomgeneric.zip)
fi

fetch "https://github.com/freedoom/freedoom/releases/download/v0.13.0/freedoom-0.13.0.zip" freedoom.zip "$FREEDOOM_SHA256"
if [ ! -f FREEDOOM.WAD ]; then
    rm -rf fd && mkdir fd
    (cd fd && unzip -q ../freedoom.zip)
    cp fd/freedoom-0.13.0/freedoom1.wad FREEDOOM.WAD
    cp fd/freedoom-0.13.0/COPYING.txt FREEDOOM-COPYING.txt
fi
echo "fetch-doom: ready in $base (zig $(./zig/zig* version 2>/dev/null | head -1), doomgeneric $DOOMGENERIC_COMMIT, FREEDOOM.WAD)"
