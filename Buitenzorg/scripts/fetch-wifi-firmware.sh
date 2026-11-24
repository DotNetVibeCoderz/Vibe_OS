#!/usr/bin/env bash
# Fetch the Raspberry Pi 4's Wi-Fi firmware (Cypress CYW43455) into
# kernel/target/wifi, each file checked against a pinned SHA-256. The files
# come from Raspberry Pi OS's firmware-nonfree package (Cypress licence:
# redistributable in binary form, not committed here). bzimage puts them on
# the ARM64 / Pi images as /disk/WIFI/FW.BIN, FW.CLM, FW.TXT, where
# platform/virt/cyw43.rs loads them.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
dir="$root/kernel/target/wifi"
mkdir -p "$dir"
base=https://raw.githubusercontent.com/RPi-Distro/firmware-nonfree/c91cd2804cf7463aab913e7247c176049f16bbd6/debian/config/brcm80211

fetch() { # url file sha256
    if [ -f "$dir/$2" ] && echo "$3  $dir/$2" | sha256sum -c --status 2>/dev/null; then return 0; fi
    echo "fetch-wifi-firmware: downloading $1"
    curl -fsSL -o "$dir/$2.part" "$1"
    if ! echo "$3  $dir/$2.part" | sha256sum -c --status; then
        echo "fetch-wifi-firmware: checksum mismatch for $2"; rm -f "$dir/$2.part"; exit 1
    fi
    mv "$dir/$2.part" "$dir/$2"
}

fetch "$base/cypress/cyfmac43455-sdio-standard.bin" FW.BIN d608f866582519c0a28d86db43040f4f1b98dd1d153e72e9752586546b4a36c3
fetch "$base/cypress/cyfmac43455-sdio.clm_blob" FW.CLM 9823842cae9fb9a5dd1e5fb31f595516ec7deee341354bef30bb3026eee29cc1
fetch "$base/brcm/brcmfmac43455-sdio.txt" FW.TXT ca709be81a78bdb6932936374f39943acbd7af07fae6151011127599a3ce9e3d
echo "fetch-wifi-firmware: $dir ready"
