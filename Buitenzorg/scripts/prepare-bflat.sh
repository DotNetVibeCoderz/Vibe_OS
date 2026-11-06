#!/usr/bin/env bash
# Prepare the private bflat copy the C# build scripts use (see
# prepare-bflat.ps1): kernel/target/bflat-patched = tools/bflat with zerolib
# (linux x64 + arm64) run through tools/ZerolibPatch. Prints the bflat path.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
bflat_home="$root/tools/bflat"
patched="$root/kernel/target/bflat-patched"
patch_version="2"   # bump whenever tools/ZerolibPatch changes what it does
marker="$patched/ZEROLIB_PATCH"
current="$(cat "$marker" 2>/dev/null || true)"
if [ ! -x "$patched/bflat" ] || [ "$current" != "$patch_version" ]; then
    echo "==> preparing a bflat copy with the patched zerolib (x64 + arm64, patch v$patch_version)" >&2
    rm -rf "$patched"
    mkdir -p "$(dirname "$patched")"
    cp -r "$bflat_home" "$patched"
    for a in x64 arm64; do
        dotnet run --project "$root/tools/ZerolibPatch" -c Release -- \
            "$bflat_home/lib/linux/$a/zerolib.dll" "$patched/lib/linux/$a/zerolib.dll" >&2 \
            || { rm -rf "$patched"; echo "ZerolibPatch failed ($a)" >&2; exit 1; }
    done
    echo "$patch_version" > "$marker"
fi
echo "$patched/bflat"
