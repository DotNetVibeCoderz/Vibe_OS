#!/usr/bin/env bash
# Build DOOM (doomgeneric, GPL-2.0) for Buitenzorg with zig cc: the engine's
# own sources and its SDL2 backend (unchanged), against Buitenzorg's C runtime
# and SDL2 subset (userland/c). Needs scripts/fetch-doom.sh first (zig, the
# pinned doomgeneric source, Freedoom). Output: kernel/target/doom/<arch>/DOOM.ELF,
# which bzimage puts on /disk (with FREEDOOM.WAD) when present.
#   scripts/build-doom.sh [x86_64|aarch64|riscv64|all]   (default all)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
base="$root/kernel/target/doom"
zig="$base/zig/zig"
[ -x "$zig" ] || zig="$base/zig/zig.exe"
src="$(ls -d "$base"/src/doomgeneric-*/doomgeneric 2>/dev/null | head -1)"
if [ ! -x "$zig" ] || [ -z "$src" ]; then
    echo "build-doom: run scripts/fetch-doom.sh first (zig + doomgeneric not found)"; exit 1
fi
rt="$root/userland/c"
arches="${1:-all}"
[ "$arches" = all ] && arches="x86_64 aarch64 riscv64"

# the engine without its SDL_mixer sound / music modules: FEATURE_SOUND is on,
# and userland/c/doom/bzsound.c provides DG_sound_module / DG_music_module
doom_src="dummy am_map doomdef doomstat dstrings d_event d_items d_iwad d_loop d_main d_mode d_net f_finale f_wipe g_game hu_lib hu_stuff info i_cdmus i_endoom i_joystick i_scale i_sound i_system i_timer memio m_argv m_bbox m_cheat m_config m_controls m_fixed m_menu m_misc m_random p_ceilng p_doors p_enemy p_floor p_inter p_lights p_map p_maputl p_mobj p_plats p_pspr p_saveg p_setup p_sight p_spec p_switch p_telept p_tick p_user r_bsp r_data r_draw r_main r_plane r_segs r_sky r_things sha1 sounds statdump st_lib st_stuff s_sound tables v_video wi_stuff w_checksum w_file w_main w_wad z_zone w_file_stdc i_input i_video doomgeneric doomgeneric_sdl mus2mid"

for arch in $arches; do
    case "$arch" in
        x86_64) target="x86_64-freestanding-none"; cpu="-mcpu=x86_64" ;;
        aarch64) target="aarch64-freestanding-none"; cpu="-mcpu=generic" ;;
        riscv64) target="riscv64-freestanding-none"; cpu="-mcpu=baseline_rv64" ;;
        *) echo "build-doom: unknown architecture $arch"; exit 1 ;;
    esac
    out="$base/$arch"
    mkdir -p "$out/obj"
    cflags=(-target "$target" "$cpu" -O2 -ffreestanding -fno-stack-protector -fno-pic -fno-pie -fno-builtin-printf
            -fno-sanitize=all -fno-omit-frame-pointer -nostdinc -isystem "$rt/include"
            -Wno-everything -D__BUZORG__=1 -DNORMALUNIX -DFEATURE_SOUND)
    objs=()
    for f in "$rt/src/crt0.c" "$rt/src/libc.c" "$rt/src/sdl.c"; do
        o="$out/obj/rt_$(basename "${f%.c}").o"
        "$zig" cc "${cflags[@]}" -fno-builtin -c "$f" -o "$o"
        objs+=("$o")
    done
    for n in $doom_src; do
        o="$out/obj/$n.o"
        "$zig" cc "${cflags[@]}" -I "$src" -c "$src/$n.c" -o "$o"
        objs+=("$o")
    done
    o="$out/obj/bzsound.o"
    "$zig" cc "${cflags[@]}" -I "$src" -c "$rt/doom/bzsound.c" -o "$o"
    objs+=("$o")
    "$zig" cc -target "$target" -nostdlib -static -fno-pie -Wl,-T,"$rt/user.ld" -Wl,--gc-sections -o "$out/DOOM.ELF" "${objs[@]}"
    echo "build-doom: $out/DOOM.ELF ($(wc -c < "$out/DOOM.ELF") bytes)"
done
