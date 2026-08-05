# App Framework (v0.8 "Kembang")

The v0.8 milestone: **"a third-party desktop app runs"** — a third-party-style C#
app creates its own window and draws its UI via syscalls.

**English** · [Bahasa Indonesia](app-framework.id.md) · ← [Documentation index](README.md)

## Window syscall ABI (append-only, v0.8)

Added to the `bz-abi` ↔ C# contract (see [Syscall ABI](abi.md)):

| # | Name | Arguments | Result |
|---|---|---|---|
| 6 | `WIN_CREATE` | a0=title ptr, a1=len, a2=(w≪32)\|h | window id |
| 7 | `WIN_CMD` | a0=window id, a1=ptr to `DrawCmd` | 0 = success |
| 8 | `WIN_PRESENT` | a0=window id | 0 (recompose the desktop) |
| 9 | `KEY_READ` | — | 1 char (0 if empty) |

`DrawCmd` (`#[repr(C)]`, 48 bytes) holds an op (fill_rect / draw_text / clear),
coordinates, a `0x00RRGGBB` color, and a text pointer + length. The size contract
test exists on both sides (`cargo test -p bz-abi`, `AbiContractTests.cs`).

## Flow

1. A C# app (e.g. `userland/hello-csharp/xox.cs`) calls `bz_win_create`,
   `bz_win_cmd`, `bz_win_present` (provided by `bzstart.rs` as `#[no_mangle]`
   functions that wrap the Buitenzorg syscalls).
2. The kernel's `create_app_window` makes a `Window` with an `AppCanvas` (the
   client-area pixel buffer). `draw_on_window` applies a `DrawCmd` to the canvas.
3. The compositor blits the `AppCanvas` to the framebuffer when the window is
   composited.
4. The shell's `run <app>` (`app::run_named`) reads `<APP>.ELF` from `/disk`,
   loads it via the ELF loader, and runs it in ring 3 (load → run → unmap).

## SDK & tooling

- Templates: `sdk/templates/console-csharp`, `sdk/templates/desktop-csharp` (with
  a `bzui.cs` helper, `app.manifest`, `.vscode/launch.json`).
- `bz new desktop-csharp <name>` scaffolds a new app.
- `sdk/vscode-extension` — a skeleton VS Code extension (§13.1): New Project,
  Build & Run in QEMU, Validate Manifest, plus a `buitenzorg` debug type.

## Constraint: no GC (see CLAUDE.md)

Freestanding apps (zerolib, no GC). **As of v0.15 the heap works** (`new`,
arrays, generics), but the zerolib rules still apply — no static reference
fields, no method-group→delegate, no `object[]` element stores, no
`ToString()`/concat. See [Your First App](first-app.md) for the full rules.

## v0.9 "Serbuk": Drawing, Task Manager, 4 app variants

- **`Buitenzorg.Drawing`** (`userland/hello-csharp/bzdraw.cs`) — a managed,
  System.Drawing-style graphics library: `Graphics`, `Pen`, `Brush`, `Color`,
  `Point`, `Rectangle`, `Size`; `FillRectangle`/`DrawRectangle`, `DrawLine`,
  `DrawEllipse`/`FillEllipse`, `DrawString`/`DrawChars`. It translates to the new
  window-ABI draw ops (LINE=3, ELLIPSE=4, FILL_ELLIPSE=5, RECT=6). Demo: `paint.cs`.
  *(The v0.16 client-side renderer `bzgfx.cs` supersedes this — see
  [Your First App](first-app.md).)*
- **Task Manager** (`taskmgr.cs`) — a process list (kernel tasks + the active
  app), uptime/heap/RAM, and kill. Backed by the kernel process registry
  (`process.rs`) with per-tick CPU-time accounting + the `PROC_LIST`/`PROC_KILL`/
  `SYS_STAT` syscalls.
- **App variants**: a **widget** (`widget.cs`, docked on the widget board via a
  `widget:` title prefix) and a **web view** (`webview.cs`, a subset-HTML
  renderer) — completing console/desktop, so all four app variants run.

![The v0.9 "Serbuk" desktop — Drawing, Task Manager, and the app variants](img/desktop-serbuk.png)

## What's next

An XAML-based UI toolkit (binding/MVVM) — since realized as the retained-mode
`Buitenzorg.UI` in v0.16 — a full HTML/CSS/JS web engine, tabs + Details in the
Task Manager, web/widget SDK templates, DAP debugging from VS Code + a debug
bridge, and CoreCLR/JIT + GC.

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
