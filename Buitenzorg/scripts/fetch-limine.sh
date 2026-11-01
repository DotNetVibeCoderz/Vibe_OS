#!/usr/bin/env bash
# Download the Limine bootloader binaries (pinned release + SHA-256) into
# kernel/target/limine/limine-binary. bzimage then also builds
# dist/buitenzorg-limine-uefi.img: the same kernel booted through the Limine
# protocol (multi-arch M1 -- the loader the ARM64 / RISC-V ports use).
set -euo pipefail
VERSION="v12.9.3"
SHA256="9f42fe9ea2e84d71056529969b1d5c24ba9a2287ffcc01bcce58b0ef8aa98b1d"
root="$(cd "$(dirname "$0")/.." && pwd)"
dir="$root/kernel/target/limine"
if [ -f "$dir/limine-binary/BOOTX64.EFI" ] && [ "$(cat "$dir/VERSION" 2>/dev/null)" = "$VERSION" ]; then
  echo "limine $VERSION already present"; exit 0
fi
rm -rf "$dir"; mkdir -p "$dir"
curl -fsSL -o "$dir/limine-binary.tar.gz" \
  "https://github.com/Limine-Bootloader/Limine/releases/download/$VERSION/limine-binary.tar.gz"
echo "$SHA256  $dir/limine-binary.tar.gz" | sha256sum -c -
tar -xzf "$dir/limine-binary.tar.gz" -C "$dir"
echo "$VERSION" > "$dir/VERSION"
echo "limine $VERSION -> $dir/limine-binary"
