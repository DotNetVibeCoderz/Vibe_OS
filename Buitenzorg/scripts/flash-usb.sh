#!/usr/bin/env bash
# Buitenzorg OS - write a bootable image to a USB drive (Linux / macOS).
#
# The build produces two raw, directly-writable disk images:
#   dist/buitenzorg-bios.img   MBR, for legacy-BIOS / CSM boot
#   dist/buitenzorg-uefi.img   GPT + FAT ESP, for UEFI boot
# This writes one of them byte-for-byte onto a USB disk with dd, so the machine
# can boot Buitenzorg from that stick. See docs/install-hardware.md.
#
# *** THIS ERASES THE TARGET DEVICE. *** Guardrails:
#   - candidate devices are listed (removable/USB where the OS reports it)
#   - the target device must be passed explicitly; it is never guessed
#   - the root/system device is refused
#   - size + model are shown and typed confirmation is required
#   - after dd the image is read back and compared
#
# Usage:
#   ./scripts/flash-usb.sh --list                 # list candidate devices
#   sudo ./scripts/flash-usb.sh /dev/sdX          # write BIOS image to /dev/sdX
#   sudo ./scripts/flash-usb.sh /dev/sdX --uefi   # write the UEFI image
#   sudo ./scripts/flash-usb.sh /dev/rdiskN       # macOS (use the RAW rdisk node)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
dist="$root/dist"
firmware="bios"
device=""
image=""
assume_yes=0
do_list=0

while [ $# -gt 0 ]; do
  case "$1" in
    --list) do_list=1 ;;
    --uefi) firmware="uefi" ;;
    --bios) firmware="bios" ;;
    --image) shift; image="$1" ;;
    --yes|-y) assume_yes=1 ;;
    -*) echo "unknown option: $1" >&2; exit 2 ;;
    *) device="$1" ;;
  esac
  shift
done

os="$(uname -s)"

list_devices() {
  echo "Candidate removable/USB devices:"
  if [ "$os" = "Darwin" ]; then
    # External physical disks on macOS.
    diskutil list external physical 2>/dev/null || diskutil list
    echo
    echo "Use the RAW node for speed: /dev/rdiskN (not /dev/diskN)."
  else
    # Linux: show removable block devices (RM=1) with model and size.
    lsblk -d -o NAME,SIZE,MODEL,TRAN,RM,TYPE | awk 'NR==1 || $5==1 || $4=="usb"'
    echo
    echo "Target the whole disk (e.g. /dev/sdb), not a partition (/dev/sdb1)."
  fi
}

if [ "$do_list" = "1" ]; then
  list_devices
  exit 0
fi

# Resolve image path.
if [ -z "$image" ]; then
  image="$dist/buitenzorg-$firmware.img"
fi
if [ ! -f "$image" ]; then
  echo "Image not found: $image" >&2
  echo "Build it first (./scripts/build.sh or quickstart), or pass --image." >&2
  exit 1
fi

if [ -z "$device" ]; then
  list_devices
  echo
  echo "Re-run with the target device, e.g.:  sudo $0 /dev/sdX" >&2
  exit 2
fi

if [ ! -e "$device" ]; then
  echo "Device not found: $device" >&2
  exit 1
fi

# Refuse the root/system device.
if [ "$os" = "Linux" ]; then
  root_src="$(findmnt -n -o SOURCE / 2>/dev/null || true)"
  # Strip partition suffix to get the base disk (sda1 -> sda, nvme0n1p2 -> nvme0n1).
  root_disk="$(lsblk -no pkname "$root_src" 2>/dev/null | head -1 || true)"
  base="$(basename "$device")"
  if [ -n "$root_disk" ] && [ "$base" = "$root_disk" ]; then
    echo "Refusing: $device is the system/root disk." >&2
    exit 1
  fi
  # Warn if it doesn't look removable.
  rm_flag="$(lsblk -dno RM "$device" 2>/dev/null | head -1 || echo 0)"
  if [ "$rm_flag" != "1" ] && [ "$assume_yes" != "1" ]; then
    echo "WARNING: $device is not reported as removable. Double-check this is your USB stick." >&2
  fi
fi

img_bytes="$(wc -c < "$image")"
img_mb="$(awk "BEGIN{printf \"%.2f\", $img_bytes/1048576}")"

echo
echo "About to ERASE and write:"
echo "  Target : $device"
if [ "$os" = "Linux" ]; then
  lsblk -d -o NAME,SIZE,MODEL,TRAN "$device" 2>/dev/null | sed 's/^/           /' || true
fi
echo "  Image  : $image  (${img_mb} MB, ${firmware} firmware)"
echo

if [ "$assume_yes" != "1" ]; then
  printf "Type ERASE to continue: "
  read -r confirm
  [ "$confirm" = "ERASE" ] || { echo "Cancelled."; exit 0; }
fi

# Unmount any mounted partitions of the target first.
if [ "$os" = "Darwin" ]; then
  diskutil unmountDisk "$device" || true
else
  for part in "${device}"?*; do
    [ -b "$part" ] && umount "$part" 2>/dev/null || true
  done
fi

echo "==> Writing (dd)..."
# bs=4M is a good balance; conv=fsync makes dd wait for the write to hit media.
if dd if="$image" of="$device" bs=4M conv=fsync 2>&1 | tail -1; then
  sync
else
  echo "dd failed" >&2
  exit 1
fi

echo "==> Verifying..."
# Read back exactly the image length and compare.
if cmp -n "$img_bytes" "$image" "$device"; then
  echo
  echo "DONE - $device now boots Buitenzorg OS (${firmware})."
  echo "Eject it, then boot the target machine from USB. See docs/install-hardware.md."
else
  echo "Verification FAILED - data read back does not match. Do not boot this disk; re-flash." >&2
  exit 1
fi
