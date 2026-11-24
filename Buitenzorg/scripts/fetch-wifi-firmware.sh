#!/usr/bin/env bash
# Fetch Wi-Fi firmware: the Raspberry Pi 4's (Cypress CYW43455) into
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
# Intel Wireless 8265 (laptops, platform/pc/iwlwifi.rs): linux-firmware tag
# 20241110, Intel's redistributable licence; /disk/WIFI/IWL8265.UCO on the PC images.
fetch "https://git.kernel.org/pub/scm/linux/kernel/git/firmware/linux-firmware.git/plain/iwlwifi-8265-36.ucode?h=20241110" IWL8265.UCO d7d84c9b6a3b1976acb97a2bec7102e24a6b5d794110c0dcbd107ab5d9877e5f
echo "fetch-wifi-firmware: $dir ready"
