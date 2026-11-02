#!/usr/bin/env bash
# Download the Raspberry Pi 4 boot files (pinned release + SHA-256) into
# kernel/target/rpi4-uefi: the Pi firmware (start4.elf, fixup4.dat), its
# Device Trees and overlays, and the community UEFI firmware (RPI_EFI.fd).
# bzimage then also builds dist/buitenzorg-rpi4.img, the SD card image.
# Nothing here is committed: these are third-party binaries, see Readme.md in
# that directory.
set -euo pipefail
VERSION="v1.53"
SHA256="ca9973e2a7a546b3df871cfb7382e656829114b6dfa424f40dc67cc90a217d88"
root="$(cd "$(dirname "$0")/.." && pwd)"
dir="$root/kernel/target/rpi4-uefi"
if [ -f "$dir/start4.elf" ] && [ "$(cat "$dir/VERSION" 2>/dev/null)" = "$VERSION" ]; then
  echo "rpi4 firmware $VERSION already present"; exit 0
fi
rm -rf "$dir"; mkdir -p "$dir"
zip="$dir/RPi4_UEFI_Firmware_$VERSION.zip"
curl -fsSL -o "$zip" "https://github.com/pftf/RPi4/releases/download/$VERSION/RPi4_UEFI_Firmware_$VERSION.zip"
echo "$SHA256  $zip" | sha256sum -c -
unzip -q -o "$zip" -d "$dir"
echo "$VERSION" > "$dir/VERSION"
echo "rpi4 firmware $VERSION -> $dir"
