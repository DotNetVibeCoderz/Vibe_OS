#!/usr/bin/env bash
# Performance regression check (requirements.md design principle: "benchmark
# regression checks in CI").
#
# Boots the built image headlessly and extracts the two numbers the kernel
# already reports, then fails if either has regressed past its budget:
#
#   * boot-to-READY, in timer ticks (the kernel prints it at the end of boot)
#   * async I/O throughput, in ops/sec (the v0.5 io_uring-style benchmark)
#
# Budgets are deliberately loose — this catches a real regression (a demo that
# doubles boot time, an I/O path that falls off a cliff), not normal jitter
# between CI runners. Override with BOOT_BUDGET_S / AIO_MIN_OPS.
#
# Note: the tick-based boot number under-reports wall clock, because roughly
# half of boot runs with interrupts off (ELF loading over IDE PIO and the
# full-screen compositor passes). It is still the right thing to trend: it is
# stable across machines in a way wall-clock is not.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
img="$root/dist/buitenzorg-bios.img"
log="$root/dist/bench-boot.log"

BOOT_BUDGET_S="${BOOT_BUDGET_S:-90}"
AIO_MIN_OPS="${AIO_MIN_OPS:-10000}"
WAIT_S="${WAIT_S:-180}"

if [ ! -f "$img" ]; then
  echo "bench: $img not found — run scripts/build.sh first" >&2
  exit 1
fi

qemu="${QEMU:-qemu-system-x86_64}"
rm -f "$log"

echo "bench: booting $img (headless, up to ${WAIT_S}s)"
"$qemu" -drive "file=$img,format=raw,if=ide" -m 512M \
        -serial "file:$log" -display none \
        -audiodev none,id=snd0 -device AC97,audiodev=snd0 &
qemu_pid=$!

# shellcheck disable=SC2317
cleanup() { kill "$qemu_pid" 2>/dev/null || true; }
trap cleanup EXIT

for _ in $(seq 1 "$WAIT_S"); do
  sleep 1
  if [ -f "$log" ] && grep -q "BUITENZORG READY" "$log" 2>/dev/null; then break; fi
done
kill "$qemu_pid" 2>/dev/null || true

if ! grep -q "BUITENZORG READY" "$log" 2>/dev/null; then
  echo "bench: FAILED — never reached READY within ${WAIT_S}s" >&2
  exit 1
fi

# "[kernel] boot to READY in ~42s (timer ticks)"
boot_s="$(tr -d '\000' < "$log" | sed -n 's/.*boot to READY in ~\([0-9]\+\)s.*/\1/p' | head -1)"
# "[aio] 2001 ops in 1 ticks (~36418 ops/sec)" — the kernel prints '~' or '>'
# depending on whether the run fitted inside a single timer tick, so accept both.
aio_ops="$(tr -d '\000' < "$log" | sed -n 's/.*[(][>~]\([0-9]\+\) ops\/sec[)].*/\1/p' | head -1)"

status=0
if [ -z "$boot_s" ]; then
  echo "bench: FAILED — no boot-time line in the log" >&2
  status=1
else
  echo "bench: boot to READY = ${boot_s}s (budget ${BOOT_BUDGET_S}s)"
  if [ "$boot_s" -gt "$BOOT_BUDGET_S" ]; then
    echo "bench: FAILED — boot time regressed past its budget" >&2
    status=1
  fi
fi

if [ -z "$aio_ops" ]; then
  echo "bench: FAILED — no async-I/O benchmark line in the log" >&2
  status=1
else
  echo "bench: async I/O = ${aio_ops} ops/sec (minimum ${AIO_MIN_OPS})"
  if [ "$aio_ops" -lt "$AIO_MIN_OPS" ]; then
    echo "bench: FAILED — async I/O throughput regressed" >&2
    status=1
  fi
fi

[ "$status" -eq 0 ] && echo "bench: PASSED"
exit "$status"
