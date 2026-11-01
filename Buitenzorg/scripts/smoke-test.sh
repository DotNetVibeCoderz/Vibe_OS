#!/usr/bin/env bash
# CI boot smoke test (requirements.md Â§17 + Â§18): build the images, boot the
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
    # Multi-arch M2: the boot disk on virtio-blk (2 CPUs).
    # Multi-arch M2: the boot disk on virtio-blk, a virtio keyboard + tablet,
    # and a QEMU monitor to type/move through them once the desktop is up.
    virtio) drive=(-smp 2 -drive "id=bzdisk,format=raw,file=$img,if=none"
                 -device virtio-blk-pci,drive=bzdisk,bootindex=0,disable-legacy=on
                 -device virtio-keyboard-pci -device virtio-tablet-pci
                 -netdev user,id=n0 -device virtio-net-pci,netdev=n0,disable-legacy=on
                 -device virtio-gpu-pci -audiodev none,id=vsnd -device virtio-sound-pci,audiodev=vsnd
                 -monitor tcp:127.0.0.1:4481,server,nowait) ;;
  esac
  timeout --foreground 190 "$qemu" "${drive[@]}" \
    -m 512M -audiodev none,id=snd0 -device AC97,audiodev=snd0 \
    -display none -serial "file:$log" -no-reboot &
  local pid=$!
  # Give the kernel time to reach READY, then stop QEMU ourselves.
  for _ in $(seq 130); do
    sleep 1
    grep -qF "BUITENZORG READY" "$log" 2>/dev/null && break
  done
  if [ "$media" = virtio ]; then virtio_input "$log" 4481; fi
  kill "$pid" 2>/dev/null
  wait "$pid" 2>/dev/null
}

# Type a few keys and move/click the pointer through the QEMU monitor; the
# events reach the guest through the virtio keyboard/tablet.
virtio_input() { # virtio_input <log> <monitor port>
  local log="$1" port="$2"
  if exec 3<>/dev/tcp/127.0.0.1/"$port"; then
    for cmd in "sendkey l" "sendkey s" "sendkey ret" "mouse_move 200 150" "mouse_button 1" "mouse_button 0"; do
      printf '%s\n' "$cmd" >&3; sleep 0.7
    done
    for _ in $(seq 20); do
      sleep 1
      grep -qF "VIRTIO INPUT OK" "$log" && break
    done
    exec 3>&-
  fi
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

# virtio-blk boot disk (multi-arch M2): the kernel's own virtio driver must
# carry /disk -- every app, the process tests, and a write that reaches the
# image file on the host.
boot virtio "$root/dist/boot-virtio.log"
require "$root/dist/boot-virtio.log" "${basic[@]}" "MILESTONE: VIRTIO BLK OK" "via virtio-blk" "MILESTONE: DISK WRITE OK" "MILESTONE: VIRTIO INPUT OK" "MILESTONE: VIRTIO NET OK" "MILESTONE: VIRTIO GPU OK" "MILESTONE: VIRTIO SND OK"
if grep -qF "Hello from C#" "$root/dist/boot-virtio.log"; then
  require "$root/dist/boot-virtio.log" "MILESTONE: MULTIPROC OK" "MILESTONE: PROCCTL OK" "MILESTONE: EDITOR OK" "MILESTONE: SMP FINELOCK OK"
fi
needle="$(grep -a -o 'BOOTCNT.TXT = BZPERSIST[^[:cntrl:]]*' "$root/dist/boot-virtio.log" | head -1 | sed 's/^BOOTCNT.TXT = //')"
if [ -z "$needle" ] || ! grep -a -q -F "$needle" "$img"; then
  echo "SMOKE TEST FAILED: virtio /disk write not found in the image ('$needle')"; exit 1
fi
echo "smoke [virtio]: /disk on virtio-blk, apps + processes, write persisted ($needle)"

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

# UEFI boot (OVMF, 2 CPUs). UEFI firmware leaves the 8259 PIC fully masked â€”
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

# ARM64 (multi-arch M3): the ARM64 kernel through Limine on QEMU `virt`
# (GICv3, Cortex-A72, edk2 firmware, Device Tree: acpi=off -- with ACPI on,
# edk2 hides the DTB).
qemu_arm="$(command -v qemu-system-aarch64 || true)"
arm_fw=""
for f in /usr/share/qemu-efi-aarch64/QEMU_EFI.fd /usr/share/AAVMF/AAVMF_CODE.fd /usr/share/qemu/edk2-aarch64-code.fd; do
  [ -f "$f" ] && { arm_fw="$f"; break; }
done
if [ -n "$qemu_arm" ] && [ -n "$arm_fw" ] && [ -f "$root/dist/buitenzorg-arm64.img" ]; then
  log="$root/dist/boot-arm64.log"
  rm -f "$log"
  timeout --foreground 150 "$qemu_arm" -M virt,gic-version=3,acpi=off -cpu cortex-a72 -smp 2 -m 512M \
    -bios "$arm_fw" -global virtio-mmio.force-legacy=false -drive "id=d0,format=raw,file=$root/dist/buitenzorg-arm64.img,if=none" \
    -device virtio-blk-device,drive=d0 -device virtio-gpu-device -device ramfb     -netdev user,id=n0 -device virtio-net-device,netdev=n0 \
    -device virtio-keyboard-device -device virtio-tablet-device \
    -monitor tcp:127.0.0.1:4482,server,nowait -display none -serial "file:$log" -no-reboot &
  qpid=$!
  for _ in $(seq 90); do
    sleep 1
    grep -q "ARM64 READY" "$log" 2>/dev/null && break
  done
  # The interactive desktop: type + move/click through the virtio devices.
  sleep 3
  virtio_input "$log" 4482
  kill "$qpid" 2>/dev/null; wait "$qpid" 2>/dev/null
  require "$log" "MILESTONE: HELLO KERNEL OK" "loader=limine" "loader DTB: parsed" "MILESTONE: DTB OK" \
    "MILESTONE: ARM64 MEMORY OK" "MILESTONE: MEMORY OK" "MILESTONE: ARM64 ASPACE OK" "MILESTONE: ARM64 CONTEXT OK" "MILESTONE: ARM64 EL0 OK" "Hello from EL0" "MILESTONE: ARM64 RTC OK" "MILESTONE: VIRTIO BLK OK (virtio-mmio" "MILESTONE: ARM64 DISK OK" "MILESTONE: VIRTIO GPU OK"     "MILESTONE: SECURITY OK" "MILESTONE: ARM64 PSCI OK" "MILESTONE: NETWORK OK" "MILESTONE: VIRTIO NET OK" \
    "[desktop] tidied" "MILESTONE: VIRTIO INPUT OK" "MILESTONE: ARM64 SMP OK (2 CPUs" "MILESTONE: ARM64 TIMER OK" "MILESTONE: ARM64 BOOT OK" "BUITENZORG ARM64 READY"
  if grep -qF "program 'hello'" "$log"; then
    require "$log" "Hello from C#!" "MILESTONE: ARM64 CSHARP OK" "MILESTONE: MATANG OK" "MILESTONE: HEAP OK" "MILESTONE: GCMEM OK" "MILESTONE: ARM64 MULTIPROC OK" "MILESTONE: THREAD OK" "MILESTONE: SYNC OK" "frames leaked: 0" "from /disk"       "MILESTONE: BCL OK" "MILESTONE: PROCCTL OK" "MILESTONE: CALC OK" "MILESTONE: GAME OK" "MILESTONE: UI OK"       "MILESTONE: DRAW OK" "MILESTONE: ARM64 DESKTOP FRAME OK" "MILESTONE: ARM64 SMP SCHED OK"
  fi
  echo "smoke [arm64]: ARM64 kernel via Limine: live DTB, PL011, GICv3 + timer, paging + heap + address spaces, threads + EL0 programs, C# apps, preemptive processes + threads, virtio-mmio disk + GPU + net, PL031, PSCI, shared syscalls, C# GUI apps + interactive desktop, 2 CPUs (PSCI)"
else
  echo "smoke [arm64]: skipped (needs qemu-system-aarch64, AAVMF/edk2 firmware and dist/buitenzorg-arm64.img)"
fi

# RISC-V (multi-arch M4): the RISC-V kernel through Limine on QEMU `virt`
# (OpenSBI -> edk2 in pflash -> Limine; acpi=off so the DTB is handed over).
qemu_rv="$(command -v qemu-system-riscv64 || true)"
rv_code=""; rv_vars=""
for d in /usr/share/qemu-efi-riscv64 /usr/share/qemu; do
  if [ -f "$d/RISCV_VIRT_CODE.fd" ] && [ -f "$d/RISCV_VIRT_VARS.fd" ]; then rv_code="$d/RISCV_VIRT_CODE.fd"; rv_vars="$d/RISCV_VIRT_VARS.fd"; break; fi
  if [ -f "$d/edk2-riscv-code.fd" ] && [ -f "$d/edk2-riscv-vars.fd" ]; then rv_code="$d/edk2-riscv-code.fd"; rv_vars="$d/edk2-riscv-vars.fd"; break; fi
done
if [ -n "$qemu_rv" ] && [ -n "$rv_code" ] && [ -f "$root/dist/buitenzorg-riscv64.img" ]; then
  log="$root/dist/boot-riscv64.log"
  rm -f "$log"
  cp "$rv_vars" "$root/dist/riscv64-vars.fd"
  timeout --foreground 120 "$qemu_rv" -M virt,acpi=off -cpu rv64 -m 512M \
    -drive "if=pflash,format=raw,unit=0,file=$rv_code,readonly=on" -drive "if=pflash,format=raw,unit=1,file=$root/dist/riscv64-vars.fd" \
    -global virtio-mmio.force-legacy=false -drive "id=d0,format=raw,file=$root/dist/buitenzorg-riscv64.img,if=none" \
    -device virtio-blk-device,drive=d0 -device ramfb -display none -serial "file:$log" -no-reboot &
  qpid=$!
  for _ in $(seq 90); do
    sleep 1
    grep -q "RISCV READY" "$log" 2>/dev/null && break
  done
  kill "$qpid" 2>/dev/null; wait "$qpid" 2>/dev/null
  require "$log" "MILESTONE: HELLO KERNEL OK" "loader=limine" "loader DTB: parsed" "MILESTONE: DTB OK" "MILESTONE: RISCV MEMORY OK" "MILESTONE: MEMORY OK" "MILESTONE: RISCV ASPACE OK" "MILESTONE: RISCV CONTEXT OK" "MILESTONE: RISCV TIMER OK" "Hello from U-mode" "MILESTONE: RISCV UMODE OK" "MILESTONE: RISCV BOOT OK" "BUITENZORG RISCV READY"
  echo "smoke [riscv64]: RISC-V kernel via Limine: S-mode under OpenSBI, live DTB, NS16550, Sv48 paging + heap + address spaces, SBI timer, context switch, U-mode programs + ECALL"
else
  echo "smoke [riscv64]: skipped (needs qemu-system-riscv64, RISC-V edk2 firmware (qemu-efi-riscv64) and dist/buitenzorg-riscv64.img)"
fi

echo "SMOKE TEST PASSED: ide/ahci/nvme/usb"
