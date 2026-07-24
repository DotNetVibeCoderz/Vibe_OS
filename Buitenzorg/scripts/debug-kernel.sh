#!/usr/bin/env bash
# Buitenzorg OS - debug the kernel with GDB (Linux / macOS).
#
# Boots the BIOS image in QEMU paused with a GDB stub on tcp:1234, then attaches
# GDB with the kernel symbols and the helpers in scripts/debug-kernel.gdb. Set a
# breakpoint (e.g. bz-break-main), continue, and step through ring-0 code.
# See docs/debugging.md.
#
# Usage:
#   ./scripts/debug-kernel.sh              # BIOS image, attach GDB
#   ./scripts/debug-kernel.sh --uefi       # UEFI image
#   ./scripts/debug-kernel.sh --no-attach  # start QEMU paused; attach GDB yourself
#
# Requires a built kernel (./scripts/build.sh) and gdb (or gdb-multiarch) on
# PATH. QEMU is taken from the QEMU env var or PATH.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
uefi=0
attach=1
port=1234

while [ $# -gt 0 ]; do
  case "$1" in
    --uefi) uefi=1 ;;
    --no-attach) attach=0 ;;
    --port) shift; port="$1" ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

# Kernel ELF with symbols: prefer release, fall back to debug.
elf="$root/kernel/target/x86_64-unknown-none/release/bzkernel"
[ -f "$elf" ] || elf="$root/kernel/target/x86_64-unknown-none/debug/bzkernel"
if [ ! -f "$elf" ]; then
  echo "Kernel ELF not found. Build first: ./scripts/build.sh" >&2
  exit 1
fi

qemu="${QEMU:-qemu-system-x86_64}"
if ! command -v "$qemu" >/dev/null 2>&1; then
  echo "QEMU not found ($qemu); install it or set the QEMU env var." >&2
  exit 1
fi

if [ "$uefi" = "1" ]; then
  img="$root/dist/buitenzorg-uefi.img"
else
  img="$root/dist/buitenzorg-bios.img"
fi
[ -f "$img" ] || { echo "Disk image not found: $img (run ./scripts/build.sh)." >&2; exit 1; }

echo "==> Kernel symbols: $elf"
echo "==> Starting QEMU paused with a GDB stub on :$port ..."
"$qemu" -drive "file=$img,format=raw,if=ide" -m 512M -serial mon:stdio \
        -audiodev none,id=snd0 -device AC97,audiodev=snd0 \
        -gdb "tcp::$port" -S &
qemu_pid=$!
# shellcheck disable=SC2317
cleanup() { kill "$qemu_pid" 2>/dev/null || true; }
trap cleanup EXIT

gdb_script="$root/scripts/debug-kernel.gdb"

if [ "$attach" = "0" ]; then
  echo
  echo "QEMU is paused. Attach with:"
  echo "  gdb -x scripts/debug-kernel.gdb \"$elf\""
  echo "  (gdb) target remote :$port"
  echo
  echo "Press Enter to stop QEMU..."
  read -r _
  exit 0
fi

# Find a usable gdb (gdb-multiarch is common on Linux for cross targets).
gdb_bin="${GDB:-}"
if [ -z "$gdb_bin" ]; then
  if command -v gdb >/dev/null 2>&1; then gdb_bin="gdb"
  elif command -v gdb-multiarch >/dev/null 2>&1; then gdb_bin="gdb-multiarch"
  fi
fi

if [ -z "$gdb_bin" ]; then
  echo
  echo "gdb not found. QEMU is paused on :$port - attach manually:"
  echo "  gdb -x scripts/debug-kernel.gdb \"$elf\""
  echo "  (gdb) target remote :$port"
  echo
  echo "Install gdb / gdb-multiarch, or set the GDB env var. Press Enter to stop QEMU..."
  read -r _
  exit 0
fi

echo "==> Attaching $gdb_bin ..."
"$gdb_bin" -x "$gdb_script" -ex "target remote :$port" "$elf"
