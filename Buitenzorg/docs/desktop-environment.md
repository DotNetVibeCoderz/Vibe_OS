# Desktop Environment (v0.7 "Kanopi")

The v0.7 milestone: **"switch between virtual desktops, toggle dark/light, run
ls/dir in the terminal"**. Built on the v0.6 window manager.

**English** · [Bahasa Indonesia](desktop-environment.id.md) · ← [Documentation index](README.md)

![The v0.7 "Kanopi" desktop](img/desktop-kanopi.png)

## System theme (`theme.rs`)

Color design tokens with two variants: **dark** (default, deep leaf-green) and
**light**. The compositor and window manager read colors from `theme::current()`
rather than hardcoding them. Toggle via `theme::toggle()` or the `theme
dark|light|toggle` command. This is the foundation of the full theme engine
(§15) that arrives in v0.10.

## Virtual desktops / workspaces (`wm.rs`)

4 workspaces. Each window has a `workspace` field; the compositor only draws the
windows on the active workspace. `switch_workspace(n)` moves between desktops.
The wallpaper shifts slightly per desktop (the `shift` function). The taskbar
shows a `[1][2][3][4]` indicator with the active one highlighted, plus that
workspace's window buttons.

## Terminal + shell

| Module | Role |
|---|---|
| `terminal.rs` | A terminal bound to a window: scrollback + input line, line editing, Enter runs the shell, the window body = scrollback tail + prompt. |
| `shell.rs` | A command interpreter over the VFS/theme/wm. `run(line) -> (output, Effect)`. |
| `keyboard.rs` | An input queue; the keyboard IRQ pushes chars, the desktop loop drains them → terminal. |

**Commands**: `help`, `ls`/`dir`, `cat`/`type`, `cd`, `pwd`, `echo`, `mounts`,
`clear`/`cls`, `ver`/`uname`, `theme [dark|light|toggle]`, `ws [1-4]`,
`bz <sub>`, and more. Windows aliases (`dir`, `type`, `cls`) map to the
equivalent behavior (§14.2). Relative paths resolve against the cwd (default
`/disk`).

## Input routing

`desktop_loop` (the final loop after boot):
1. Drain `keyboard::pop()` → `terminal::feed_char` (line editing + shell).
2. Poll `mouse::state()` → `wm::handle_mouse` (move/resize a window).
3. Recompose the desktop when anything changes.

## Verification

`kanopi_demo` scripts it: it runs `ver`/`mounts`/`ls`/`dir`/`cat` in the terminal
(asserting the listing contains DAHAN.TXT → `TERMINAL OK`), toggles the theme
dark→light (`THEME OK`), and switches from workspace 1→2 (`WORKSPACE OK`). The
smoke test requires those markers; visual verification in
`docs/img/desktop-kanopi.png`.

## The v0.16 desktop shell

The v0.7 environment grew into a full desktop shell in v0.16 — a Start button
and menu, desktop icons, and a taskbar tray with a live RTC clock:

![The v0.16 desktop shell](img/desktop-shell.png)

## Scheduler note

Timer preemption is **off** except for `scheduler_demo` (v0.2). Everything else
is cooperative (`yield_now`). Enabling global preemption while the boot task does
heavy rendering/heap work triggers a memory-corruption race — see CLAUDE.md.

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
