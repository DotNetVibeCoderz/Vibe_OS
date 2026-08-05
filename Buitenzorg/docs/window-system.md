# Graphics & Window System (v0.6 "Daun")

The v0.6 milestone: **"two windows can be moved and resized"**. The kernel
renders a windowed desktop to the framebuffer and routes mouse events to windows.

**English** · [Bahasa Indonesia](window-system.id.md) · ← [Documentation index](README.md)

## Layers

| Module | Role |
|---|---|
| `gfx.rs` | Rendering primitives into a `0x00RRGGBB` buffer: `fill_rect`, `rect_outline`, `fill_gradient`, alpha `blend`, text (the Noto font). |
| `wm.rs` | The window manager + compositor: floating windows, z-order, hit-testing, move/resize, taskbar, cursor. |
| `framebuffer::present` | Blit the full-screen back buffer to the hardware framebuffer (converting to the BGR/RGB pixel format). |

## Compositor

Double-buffered: each frame is rendered in full into a back buffer (a `Vec<u32>`
the size of the screen), then `present` copies it to the framebuffer once (no
flicker). Draw order: gradient wallpaper → windows (back-to-front, with drop
shadow, title bar, text, border, resize grip) → taskbar → cursor.

## Window manager

- **Floating windows**: `create_window(title, x, y, w, h, lines)`.
- **Z-order**: a `Vec<Window>` back-to-front; a click raises a window to the top.
- **Hit-testing**: `window_at(x, y)` returns the topmost window at that point.
- **Move**: press on the title bar → drag → the window follows the cursor (with a
  fixed offset).
- **Resize**: press on the bottom-right grip (`RESIZE_GRIP` px) → drag → the size
  changes (clamped by `MIN_W`/`MIN_H` and the screen edges).

## Event routing

`handle_mouse(x, y, left)` takes one mouse sample (an absolute position + the
left button), detects press/release edges, and starts/continues/ends a drag. The
source can be:
- **The real PS/2 mouse** (`desktop_loop`): poll `mouse::state()`, recompose when
  the pointer moves or a button changes.
- **Scripted events** (`daun_demo`): a sequence of `handle_mouse` calls that move
  one window and resize another, then verify the geometry changed
  (`window_rect`) — this is what makes the milestone headless-testable in CI.

## Verification

`daun_demo` prints the geometry changes, then `MILESTONE: WINDOWS OK`, and the
smoke test requires that marker on every boot medium. Visual verification via a
QEMU screenshot:

![The v0.6 "Daun" windowed desktop](img/desktop-daun.png)

## v0.11 "Cahaya": window controls, screensaver, personalization, micro-interactions

- **Window controls** (`wm.rs`): **minimize/maximize/close** buttons on each
  title bar, normal/minimized/maximized states (restore/focus from the taskbar),
  and **rounded corners** (per-theme; beveled themes stay square).
- **Screensaver** (`screensaver.rs`): 6 Win 3.1/98-style savers (Starfield,
  Mystify, 3D Pipes, Marquee, Bouncing, Blank), activated after ~12 s of idle
  (`desktop_loop`), dismissed on input. Choose with `saver <name|list|off>`.

  ![The Mystify screensaver](img/screensaver-mystify.png)

- **Wallpaper** (`wallpaper.rs`): built-in (theme/waves/grid/dots/aurora) + a
  **user image** (a 24-bit BMP from the VFS). Choose with `bg <name|/path.bmp|list>`.
- **Micro-interactions**: hover highlight on control buttons, an animated **click
  ripple**, a continuous desktop loop. Turn off with `anim off`.
- **Personalization** via the shell: `settings`, `bg`, `saver`, `cursor`, `anim`,
  `theme`.
- **Compute API** (`compute.rs`): a CPU backend with a GPU-ready interface.

> Important note: `desktop_loop` needs the timer alive. `usermode::enter_user`
> re-enables interrupts after a ring-3 app exits (IF used to stay off → the timer
> stopped → the interactive desktop/screensaver/animation was broken from v0.4).

## What's next

Tiling layout, window open/close animations, theme/workspace transitions, a
Personalization GUI app, and a real GPU driver (a GPU-accelerated compositor).

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
