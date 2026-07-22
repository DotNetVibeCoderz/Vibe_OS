#!/usr/bin/env bash
# CI boot smoke test (requirements.md §17 + §18): build the images, boot the
# BIOS image in QEMU on all four storage controllers (IDE/AHCI/NVMe/USB), and
# require the milestone markers on the serial console.
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
img="$root/dist/buitenzorg-bios.img"
qemu="${QEMU:-qemu-system-x86_64}"

(cd "$root/kernel" && cargo run --release -p bzimage -- --out "$root/dist") || exit 1

boot() { # boot <media> <log>; QEMU is killed by timeout after the grace period
  local media="$1" log="$2"
  local -a drive
  case "$media" in
    ide)  drive=(-drive "format=raw,file=$img") ;;
    ahci) drive=(-drive "id=bzdisk,format=raw,file=$img,if=none"
                 -device ahci,id=ahci0 -device ide-hd,drive=bzdisk,bus=ahci0.0) ;;
    nvme) drive=(-drive "id=bzdisk,format=raw,file=$img,if=none"
                 -device nvme,drive=bzdisk,serial=bz0001) ;;
    usb)  drive=(-drive "id=bzdisk,format=raw,file=$img,if=none"
                 -usb -device usb-storage,drive=bzdisk) ;;
  esac
  timeout --foreground 130 "$qemu" "${drive[@]}" \
    -m 512M -display none -serial "file:$log" -no-reboot &
  local pid=$!
  # Give the kernel time to reach READY, then stop QEMU ourselves.
  for _ in $(seq 110); do
    sleep 1
    grep -qF "BUITENZORG READY" "$log" 2>/dev/null && break
  done
  kill "$pid" 2>/dev/null
  wait "$pid" 2>/dev/null
}

require() { # require <log> <marker>...
  local log="$1"; shift
  for marker in "$@"; do
    if ! grep -qF "$marker" "$log"; then
      echo "SMOKE TEST FAILED (${log##*/}): missing '$marker'" >&2
      exit 1
    fi
  done
}

basic=("MILESTONE: HELLO KERNEL OK" "MILESTONE: SCHEDULER OK" "MILESTONE: IPC OK" "MILESTONE: WINDOWS OK" "MILESTONE: THEMES OK" "MILESTONE: BUAH OK" "MILESTONE: COMPUTE OK" "MILESTONE: WINDOWCTL OK" "MILESTONE: SAVER OK" "MILESTONE: CAHAYA OK" "MILESTONE: AI OK" "MILESTONE: POWER OK" "MILESTONE: NALAR OK" "BUITENZORG READY")

# Full run on IDE: storage milestones must pass (file read via own driver).
boot ide "$root/dist/boot-ide.log"
require "$root/dist/boot-ide.log" "${basic[@]}" \
  "MILESTONE: MEMORY OK" "MILESTONE: SYSCALL ABI V1 OK" "MILESTONE: PCI OK" \
  "MILESTONE: STORAGE OK" "MILESTONE: MOUSE OK" "MILESTONE: PIXELS OK" \
  "MILESTONE: VFS OK" "MILESTONE: SERVICES OK" "MILESTONE: ASYNC IO OK" \
  "MILESTONE: NETWORK OK" "MILESTONE: TERMINAL OK" "MILESTONE: THEME OK" \
  "MILESTONE: WORKSPACE OK" "MILESTONE: KANOPI OK"
# C# ring-3 milestones only when the ELFs were embedded (bflat present).
if grep -qF "Hello from C#" "$root/dist/boot-ide.log"; then
  require "$root/dist/boot-ide.log" "MILESTONE: TUNAS OK" "MILESTONE: DAHAN OK" \
    "MILESTONE: KEMBANG OK" "MILESTONE: DRAWING OK" "MILESTONE: TASKMGR OK" \
    "MILESTONE: APPVARIANTS OK" "MILESTONE: SERBUK OK" "MILESTONE: PACKAGE OK" \
    "MILESTONE: PERSONALIZE OK"
fi
echo "smoke [ide]: all milestones present"

# Boot-media matrix: kernel must come up when the disk hangs off other
# controllers (their native drivers are later roadmap work).
for media in ahci nvme usb; do
  boot "$media" "$root/dist/boot-$media.log"
  require "$root/dist/boot-$media.log" "${basic[@]}"
  echo "smoke [$media]: boots to READY"
done

echo "SMOKE TEST PASSED: ide/ahci/nvme/usb"
