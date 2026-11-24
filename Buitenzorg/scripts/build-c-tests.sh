#!/usr/bin/env bash
# Build the C runtime's test programs (userland/c/tests/*.c) with zig cc
# against userland/c: one ELF per program and architecture in
# kernel/target/c/<arch>/<NAME>.ELF, which bzimage puts on /disk when present.
# Needs zig (scripts/fetch-doom.sh downloads it, pinned).
#   scripts/build-c-tests.sh [x86_64|aarch64|riscv64|all]   (default all)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
zig="$root/kernel/target/doom/zig/zig"
[ -x "$zig" ] || zig="$root/kernel/target/doom/zig/zig.exe"
if [ ! -x "$zig" ]; then
    echo "build-c-tests: zig not found (run scripts/fetch-doom.sh first)"; exit 1
fi
rt="$root/userland/c"
arches="${1:-all}"
[ "$arches" = all ] && arches="x86_64 aarch64 riscv64"
for arch in $arches; do
    case "$arch" in
        x86_64) target="x86_64-freestanding-none"; cpu="-mcpu=x86_64" ;;
        aarch64) target="aarch64-freestanding-none"; cpu="-mcpu=generic" ;;
        riscv64) target="riscv64-freestanding-none"; cpu="-mcpu=baseline_rv64" ;;
        *) echo "build-c-tests: unknown architecture $arch"; exit 1 ;;
    esac
    out="$root/kernel/target/c/$arch"
    mkdir -p "$out/obj"
    cflags=(-target "$target" "$cpu" -O2 -ffreestanding -fno-stack-protector -fno-pic -fno-pie -fno-builtin
            -fno-sanitize=all -fno-omit-frame-pointer -nostdinc -isystem "$rt/include" -Wall -Wno-unused-function -Wno-empty-body)
    rtobjs=()
    for f in crt0 libc pthread socket; do
        o="$out/obj/rt_$f.o"
        "$zig" cc "${cflags[@]}" -c "$rt/src/$f.c" -o "$o"
        rtobjs+=("$o")
    done
    for t in "$rt"/tests/*.c; do
        name="$(basename "${t%.c}")"
        upper="$(echo "$name" | tr '[:lower:]' '[:upper:]')"
        "$zig" cc "${cflags[@]}" -c "$t" -o "$out/obj/$name.o"
        "$zig" cc -target "$target" -nostdlib -static -fno-pie -Wl,-T,"$rt/user.ld" -Wl,--gc-sections -o "$out/$upper.ELF" "$out/obj/$name.o" "${rtobjs[@]}"
        echo "build-c-tests: $out/$upper.ELF ($(wc -c < "$out/$upper.ELF") bytes)"
    done
done
