#!/usr/bin/env bash
# Multi-arch boundary check (PLAN.md §v1.x, milestone M0).
#
# Architecture-specific code belongs in kernel/bzkernel/src/arch/<arch>/ and
# platform-specific device code in kernel/bzkernel/src/platform/<platform>/.
# Generic kernel modules must reach the hardware through the HAL facades
# (crate::arch::cpu, …), not through the `x86_64` crate, inline assembly or
# port I/O — otherwise an ARM64 / RISC-V port has to touch them.
#
# ALLOWED lists the generic files that still need an architecture detail, with
# the M0 step that will remove it. The check fails when a file outside the list
# crosses the boundary, AND when a listed file no longer needs to be there — so
# the list can only shrink.
set -uo pipefail
src="$(cd "$(dirname "$0")/.." && pwd)/kernel/bzkernel/src"
pattern='x86_64::|asm!|core::arch::x86_64|\bPort(::|<)|\bMsr\b'

declare -A ALLOWED=(
  [memory.rs]="paging HAL (page-table walk, mapper, frame allocator types)"
  [aspace.rs]="paging HAL (per-process user roots)"
  [elf.rs]="paging HAL (mapping program segments)"
  [allocator.rs]="paging HAL (mapping the kernel heap)"
  [main.rs]="boot HAL (BootInfo + mapper set-up)"
)

fail=0
for f in "$src"/*.rs; do
  name="$(basename "$f")"
  if grep -Eq "$pattern" "$f"; then
    if [ -z "${ALLOWED[$name]+x}" ]; then
      echo "boundary: $name uses architecture details outside arch/ or platform/:"
      grep -En "$pattern" "$f" | head -5 | sed 's/^/    /'
      fail=1
    fi
  elif [ -n "${ALLOWED[$name]+x}" ]; then
    echo "boundary: $name is clean now -- remove it from ALLOWED in $(basename "$0")"
    fail=1
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "ARCH BOUNDARY CHECK FAILED"
  exit 1
fi
echo "arch boundary: ok (${#ALLOWED[@]} generic files still pending:" "${!ALLOWED[@]}" ")"
