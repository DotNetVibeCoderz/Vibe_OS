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
    # The full IDE run uses 2 CPUs (ring-3 processes on the AP, SMP SCHED);
    # "ide1" re-runs the process tests on a single CPU.
    ide)  drive=(-smp 2 -drive "format=raw,file=$img") ;;
    ide1) drive=(-drive "format=raw,file=$img") ;;
    # AHCI also boots with 2 CPUs: SMP bring-up must start the second one.
    ahci) drive=(-smp 2 -drive "id=bzdisk,format=raw,file=$img,if=none"
                 -device ahci,id=ahci0 -device ide-hd,drive=bzdisk,bus=ahci0.0) ;;
    nvme) drive=(-drive "id=bzdisk,format=raw,file=$img,if=none"
                 -device nvme,drive=bzdisk,serial=bz0001) ;;
    usb)  drive=(-drive "id=bzdisk,format=raw,file=$img,if=none"
                 -usb -device usb-storage,drive=bzdisk) ;;
  esac
  timeout --foreground 150 "$qemu" "${drive[@]}" \
    -m 512M -audiodev none,id=snd0 -device AC97,audiodev=snd0 \
    -display none -serial "file:$log" -no-reboot &
  local pid=$!
  # Give the kernel time to reach READY, then stop QEMU ourselves.
  for _ in $(seq 130); do
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

basic=("MILESTONE: HELLO KERNEL OK" "MILESTONE: SCHEDULER OK" "MILESTONE: IPC OK" "MILESTONE: WINDOWS OK" "MILESTONE: THEMES OK" "MILESTONE: BUAH OK" "MILESTONE: COMPUTE OK" "MILESTONE: WINDOWCTL OK" "MILESTONE: SAVER OK" "MILESTONE: CAHAYA OK" "MILESTONE: AI OK" "MILESTONE: POWER OK" "MILESTONE: NALAR OK" "MILESTONE: VMX OK" "MILESTONE: VM OK" "MILESTONE: VIRTIO OK" "MILESTONE: SNAPSHOT OK" "MILESTONE: LAPIS OK" "MILESTONE: SCRIPT JS OK" "MILESTONE: SCRIPT TS OK" "MILESTONE: SCRIPT PY OK" "MILESTONE: POLYGLOT OK" "MILESTONE: BABEL OK" "MILESTONE: PAL MEM OK" "MILESTONE: THREADS OK" "MILESTONE: SYNC PAL OK" "MILESTONE: HEAP PAL OK" "MILESTONE: GCMEM PAL OK" "MILESTONE: BCL PAL OK" "MILESTONE: BCL2 PAL OK" "MILESTONE: DRAWING2 OK" "MILESTONE: UI TOOLKIT OK" "MILESTONE: AUDIO OK" "MILESTONE: AUDIO PANEL OK" "MILESTONE: SUITE OK" "MILESTONE: DESKTOP SHELL OK" "MILESTONE: SECURITY OK" "MILESTONE: PROFILER OK" "MILESTONE: IOAPIC OK" "MILESTONE: DTB OK" "BUITENZORG READY")

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
    "MILESTONE: PERSONALIZE OK" "MILESTONE: MMAP OK" "MILESTONE: MATANG OK" \
    "MILESTONE: THREAD OK" "MILESTONE: SYNC OK" "MILESTONE: HEAP OK" \
    "MILESTONE: GCMEM OK" "MILESTONE: BCL OK" "MILESTONE: BCL2 OK" "MILESTONE: DRAW OK" \
    "MILESTONE: UI OK" "MILESTONE: CALC OK" "MILESTONE: GAME OK" "MILESTONE: CLOCK OK" "MILESTONE: PIANO OK" "MILESTONE: STORE OK" "MILESTONE: FILES OK" "MILESTONE: EDITOR OK" "MILESTONE: IMGVIEW OK" "MILESTONE: JPEG OK" \
    "MILESTONE: MULTIPROC OK" "MILESTONE: DISK WRITE OK" "MILESTONE: PROCCTL OK" "MILESTONE: WALLPAPERS OK" "MILESTONE: ICONS OK" \
    "MILESTONE: SMP SCHED OK" "MILESTONE: SMP KERNEL OK" "MILESTONE: SMP FINELOCK OK" "MILESTONE: SMP THREADS OK"
  # Persistent /disk: the exact bytes the kernel wrote to /disk/BOOTCNT.TXT
  # must now be inside the image file on the host (they reached the medium).
  needle="$(grep -a -o 'BOOTCNT.TXT = BZPERSIST[^[:cntrl:]]*' "$root/dist/boot-ide.log" | head -1 | sed 's/^BOOTCNT.TXT = //')"
  if [ -z "$needle" ] || ! grep -a -q -F "$needle" "$img"; then
    echo "SMOKE TEST FAILED: /disk write not found in the image ('$needle')"; exit 1
  fi
  echo "smoke [ide]: /disk write persisted to the image ($needle)"
fi
require "$root/dist/boot-ide.log" "MILESTONE: SMP OK (2 CPU(s): 1 AP(s)" "MILESTONE: SMP PERCPU OK"
echo "smoke [ide]: all milestones present (2 CPUs, ring-3 on the AP)"

# Uniprocessor regression: the same process/thread tests on one CPU.
boot ide1 "$root/dist/boot-ide1.log"
require "$root/dist/boot-ide1.log" "${basic[@]}" "MILESTONE: SMP OK (1 CPU(s)"
if grep -qF "Hello from C#" "$root/dist/boot-ide1.log"; then
  require "$root/dist/boot-ide1.log" "MILESTONE: THREAD OK" "MILESTONE: SYNC OK" "MILESTONE: MULTIPROC OK" "MILESTONE: PROCCTL OK"
fi
echo "smoke [ide1]: process tests pass on a single CPU"

# Boot-media matrix: kernel must come up when the disk hangs off other
# controllers (their native drivers are later roadmap work).
for media in ahci nvme usb; do
  boot "$media" "$root/dist/boot-$media.log"
  require "$root/dist/boot-$media.log" "${basic[@]}"
  if [ "$media" = ahci ]; then
    require "$root/dist/boot-ahci.log" "MILESTONE: SMP OK (2 CPU(s): 1 AP(s)" "MILESTONE: SMP PERCPU OK"
    echo "smoke [ahci]: second CPU started (SMP)"
  fi
  echo "smoke [$media]: boots to READY"
done

# UEFI boot (OVMF, 2 CPUs). UEFI firmware leaves the 8259 PIC fully masked —
# that once hung every UEFI boot at the first timer wait while the BIOS-only
# smoke test stayed green. Needs the OVMF files `bzimage --run --uefi` downloads.
ovmf="$root/kernel/target/ovmf/x64"
if [ -f "$ovmf/code.fd" ] && [ -f "$root/dist/buitenzorg-uefi.img" ]; then
  log="$root/dist/boot-uefi.log"
  rm -f "$log"
  timeout --foreground 150 "$qemu" \
    -drive "if=pflash,format=raw,readonly=on,file=$ovmf/code.fd" \
    -drive "if=pflash,format=raw,readonly=on,file=$ovmf/vars.fd" \
    -drive "format=raw,file=$root/dist/buitenzorg-uefi.img" -smp 2 \
    -m 512M -audiodev none,id=snd0 -device AC97,audiodev=snd0 \
    -display none -serial "file:$log" -no-reboot &
  qpid=$!
  for _ in $(seq 130); do
    sleep 1
    grep -q "BUITENZORG READY" "$log" 2>/dev/null && break
  done
  kill "$qpid" 2>/dev/null; wait "$qpid" 2>/dev/null
  require "$log" "${basic[@]}" "MILESTONE: SMP OK (2 CPU(s): 1 AP(s)" "MILESTONE: SMP PERCPU OK"
  echo "smoke [uefi]: boots to READY on 2 CPUs"
else
  echo "smoke [uefi]: skipped (OVMF not downloaded; run 'cargo run -p bzimage -- --run --uefi' once)"
fi

# Limine boot (multi-arch M1): the same kernel through the Limine protocol,
# UEFI, 2 CPUs, /disk on an MBR FAT partition. Needs OVMF + the Limine
# binaries (scripts/fetch-limine.sh) so bzimage built the image.
if [ -f "$ovmf/code.fd" ] && [ -f "$root/dist/buitenzorg-limine-uefi.img" ]; then
  log="$root/dist/boot-limine.log"
  rm -f "$log"
  timeout --foreground 170 "$qemu" \
    -drive "if=pflash,format=raw,readonly=on,file=$ovmf/code.fd" \
    -drive "if=pflash,format=raw,readonly=on,file=$ovmf/vars.fd" \
    -drive "format=raw,file=$root/dist/buitenzorg-limine-uefi.img" -smp 2 \
    -m 512M -audiodev none,id=snd0 -device AC97,audiodev=snd0 \
    -display none -serial "file:$log" -no-reboot &
  qpid=$!
  for _ in $(seq 150); do
    sleep 1
    grep -q "BUITENZORG READY" "$log" 2>/dev/null && break
  done
  kill "$qpid" 2>/dev/null; wait "$qpid" 2>/dev/null
  require "$log" "${basic[@]}" "[boot] loader=limine" "MILESTONE: SMP OK (2 CPU(s): 1 AP(s)" "MILESTONE: DISK WRITE OK" "MILESTONE: MULTIPROC OK"
  echo "smoke [limine]: Limine protocol boot (UEFI) to READY on 2 CPUs, /disk + processes"
else
  echo "smoke [limine]: skipped (needs OVMF and scripts/fetch-limine.sh before the build)"
fi

echo "SMOKE TEST PASSED: ide/ahci/nvme/usb"
