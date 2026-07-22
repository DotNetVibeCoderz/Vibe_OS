# Desktop Environment (v0.7 "Kanopi")

Milestone v0.7: **"pindah antar virtual desktop, ganti dark/light, jalankan
ls/dir di terminal"**. Dibangun di atas window manager v0.6.

## Tema sistem (`theme.rs`)

Design token warna dengan dua varian: **dark** (default, hijau-daun gelap) dan
**light**. Compositor & window manager membaca warna dari `theme::current()`,
bukan hardcode. Toggle via `theme::toggle()` atau command `theme dark|light|
toggle`. Fondasi theme engine penuh (§15) yang menyusul di v0.10.

## Virtual desktops / workspaces (`wm.rs`)

4 workspace. Tiap window punya field `workspace`; compositor hanya menggambar
window pada workspace aktif. `switch_workspace(n)` berpindah desktop. Wallpaper
sedikit bergeser per-desktop (fungsi `shift`). Taskbar menampilkan indikator
`[1][2][3][4]` dengan yang aktif disorot, plus tombol window workspace itu.

## Terminal + shell

| Modul | Peran |
|---|---|
| `terminal.rs` | Terminal terikat ke sebuah window: scrollback + input line, line editing, Enter menjalankan shell, body window = ekor scrollback + prompt. |
| `shell.rs` | Interpreter command di atas VFS/tema/wm. `run(line) -> (output, Effect)`. |
| `keyboard.rs` | Antrean input; IRQ keyboard push char, desktop loop drain → terminal. |

**Command**: `help`, `ls`/`dir`, `cat`/`type`, `cd`, `pwd`, `echo`, `mounts`,
`clear`/`cls`, `ver`/`uname`, `theme [dark|light|toggle]`, `ws [1-4]`,
`bz <sub>`. Alias Windows (`dir`, `type`, `cls`) dipetakan ke perilaku setara
(§14.2). Path relatif diselesaikan terhadap cwd (default `/disk`).

## Input routing

`desktop_loop` (loop akhir setelah boot):
1. Drain `keyboard::pop()` → `terminal::feed_char` (line editing + shell).
2. Poll `mouse::state()` → `wm::handle_mouse` (move/resize window).
3. Recompose desktop bila ada perubahan.

## Verifikasi

`kanopi_demo` men-*script*: menjalankan `ver`/`mounts`/`ls`/`dir`/`cat` di
terminal (assert listing memuat DAHAN.TXT → `TERMINAL OK`), toggle tema
dark→light (`THEME OK`), dan switch workspace 1→2 (`WORKSPACE OK`). Smoke test
mewajibkan marker itu; verifikasi visual di `docs/img/desktop-kanopi.png`.

## Catatan scheduler

Preemption timer **dimatikan** kecuali untuk `scheduler_demo` (v0.2). Sisanya
kooperatif (`yield_now`). Mengaktifkan preemption global saat boot task
mengerjakan render/heap berat memicu race korupsi memori — lihat CLAUDE.md.
