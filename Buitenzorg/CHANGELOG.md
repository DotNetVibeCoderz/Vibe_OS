# Changelog — Buitenzorg OS

Release history by version codename (following plant growth stages, an homage to
the Bogor Botanical Gardens). Every milestone is verified automatically in QEMU
on 4 media (IDE/AHCI/NVMe/USB) via the `MILESTONE: … OK` markers the smoke test
checks.

The format loosely follows [Keep a Changelog](https://keepachangelog.com);
versioning follows the codenames in [PLAN.md](PLAN.md). Full technical detail is
in [Progress.md](Progress.md) and [requirements.md](requirements.md) §17.

---

## [Unreleased] — the v1.0 "Buitenzorg" track (stabilization)

Hardening toward a stable x86-64 release.

### Added
- **Security hardening — syscall pointer validation.** Every pointer from ring 3
  is validated (`memory::validate_user_range`/`validate_user_cstr`): no wrap,
  entirely below `USER_ADDR_MAX`, every page *present* & `USER_ACCESSIBLE`
  (writable for output buffers). Closes real holes (a kernel-memory leak via
  `DEBUG_WRITE`, arbitrary kernel writes via `SYS_STAT`/`PROC_LIST`/`FS_READ`/
  `NET_RECV`, a kernel page-fault from an unmapped pointer). Checked only on the
  ring-3 path (`dispatch_from_user`). 14 hostile probes → `MILESTONE: SECURITY OK`.
- **ABI v1 freeze.** `abi_v1_is_frozen` (Rust) + `AbiV1IsFrozen` (C#) pin the
  version, `COUNT`, the size+alignment of all 10 boundary structs, and the error
  codes.
- **Debugger** — `scripts/debug-kernel.ps1`/`.sh`/`.gdb`: attach GDB to a paused
  QEMU with the kernel symbols + break/fault/register helpers.
- **Profiler** — `profile.rs`: an instrumented TSC zone profiler, inert when off,
  with the syscall/compositor hot paths instrumented, shell `prof` →
  `MILESTONE: PROFILER OK`.
- **CI regression benchmark** — `scripts/bench.sh` (boot-to-READY + async-I/O
  throughput vs a budget) as a CI job.
- **USB hardware boot** — `scripts/flash-usb.ps1`/`.sh` (write the image to USB,
  layered safeguards + read-back verification) + `docs/install-hardware.md`.
  *(Physical-machine validation to follow — marked experimental.)*
- **Hyper-V VHDX image** — `make-vm-images` emits `dist/buitenzorg.vhdx` (64 MiB
  whole-MiB, Hyper-V friendly) + `scripts/make-hyperv-vm.ps1` (creates a
  Generation 1 / BIOS VM, guarded). Documented in `docs/run-in-vm.md`.
- **MIT license** — the `LICENSE` file.

### Fixed
- **CI never ran on push** — the trigger was `branches: [main]` while the repo's
  branch is `master`; now `[master, main]` + `workflow_dispatch`.
- **`build-hello-csharp.sh` (Linux/CI) was missing `imgview`+`jpgtest`** (only in
  the `.ps1`) → Linux builds lacked those ELFs; both scripts now build 27 programs.
- The long-standing address-validation TODOs in `sys_win_create`/`sys_debug_write`
  are honored.

---

## v0.16 "Panen" — Preloaded Suite, Audio & Optimization

A complete preloaded app suite on top of the v0.15 BCL, an audio subsystem, and
an optimization pass.

### Added
- **OS audio subsystem** — an **AC'97** driver (`audio.rs`): PCI enumeration,
  codec cold-reset, mixer (master + PCM-out), 16-bit stereo 48 kHz PCM playback
  via bus-master DMA + a BDL. The `AUDIO_STAT`/`SET_VOLUME`/`TONE`/`PLAY`
  syscalls + the `Buitenzorg.Audio` library + an audio settings panel
  (`MILESTONE: AUDIO OK`).
- **`Buitenzorg.Drawing`** as a client-side software renderer (`bzgfx.cs`):
  Graphics/Color/Bitmap, transforms (Matrix), GraphicsPath, clipping, hatch, an
  8×8 `DrawString`/`Font`, rounded/gradient/shadow/anti-aliasing, BMP load/save,
  and a **baseline JPEG decoder** (`Jpeg.Load`, integer IDCT). One-syscall blit
  (`draw_op::BLIT`) — the WPF/Avalonia compositor model (`MILESTONE: DRAW OK`).
- **`Buitenzorg.UI`** (`bzui.cs`) — a WPF/Avalonia-style retained toolkit: a
  `UIElement` tree, Measure/Arrange layout (Stack/Grid/Canvas), a full control
  set (Button/CheckBox/Slider/ListBox/TextBox/Menu/ComboBox/TabControl/TreeView/
  ScrollViewer/DataGrid), a popup/overlay layer, mouse routing (`MILESTONE: UI OK`).
- **Preloaded suite (8 apps)**: Calculator, Text Editor (productivity), 2048
  (game), Clock, File Manager (utility), Piano, Image Viewer (multimedia), App
  Store (wired to `pkg.rs` via `PKG_LIST`/`PKG_SET`).
- **Desktop UX shell** — taskbar + Start button + start menu + desktop icons + a
  live RTC tray clock (blending macOS/Ubuntu/Win XP) in `wm.rs`
  (`MILESTONE: DESKTOP SHELL OK`).
- **Interactive Editor & File Manager** when `run` from the shell (keyboard
  routing via `KEY_READ` + the `IS_INTERACTIVE` syscall).
- New syscalls: `PKG_LIST`/`PKG_SET`, `FS_LIST`, `FS_READ`.
- **MagicAppGen** (`tools/MagicAppGen`) — an Avalonia code editor with the AI
  assistant "Jack - The Code Bender" (Semantic Kernel, multi-LLM) that generates
  Buitenzorg apps; 8 project templates (5 C# ones verified to compile with bflat).
- **BCL completion** — `bzbcl2.cs`: System.IO/Text/RegularExpressions/
  Globalization/Diagnostics/Management/Net(+Sockets)/Threading.Tasks/Timers/GC/
  Pkg. New syscalls `FS_WRITE`, `CLOCK_RTC`, `NET_*` (real UDP) (`MILESTONE: BCL2 OK`).
- Onboarding: `quickstart.ps1`/`.sh`, `docs/getting-started.md`,
  `docs/first-app.md`, `docs/run-in-vm.md` + `make-vm-images`.

### Fixed (Heisenbugs — all binary-layout-sensitive)
- **`from_raw_parts` over user memory** in string syscalls → boot corruption;
  fixed with `copy_user_bytes`/`read_volatile`.
- **Syscall argument registers (`rdi`/`rsi`/`rdx`) must be `inlateout`** in the
  userland shim — the kernel only restores rcx/r11/rsp; `in` corrupted `HEAP_CAP`.
- **A 20 KiB `PRIVILEGE_STACK` overflow** in `compose_into` via `WIN_PRESENT` →
  fixed with a 64 KiB `PRIV_STACK_SIZE` + `PRESENT_BUF` reuse + a 32 MiB kernel heap.

---

## v0.15 "Matang" — Managed C# Runtime (heap + hand-written BCL)

Raises ring-3 apps from pure zerolib to a working heap + a BCL.

### Added
- **Memory PAL** — the `MMAP`/`MPROTECT`/`MUNMAP` syscalls; a user arena; lazy
  reserve (PROT_NONE) + commit-on-demand (the .NET GC heap pattern).
- **Cooperative ring-3 threads** — `THREAD_CREATE`/`JOIN`/`EXIT` (a per-thread
  SYSCALL stack).
- **Sync/TLS/clock** — `FUTEX_WAIT`/`WAKE` (a scheduler `Blocked` state),
  `THREAD_SELF`, `CLOCK_MONO` (TSC).
- **A working managed heap** — `new`/arrays/objects/generics work (a bump heap
  that grows via mmap) + libc `malloc`/`free`/`calloc`/`realloc`.
- **`Buitenzorg.Bcl`** (`bzbcl.cs`) — `BzList/Stack/Queue/IntMap/StrMap/IntSet/
  RefList`, LINQ (function pointers), `BzStringBuilder`, `BzMath`/`BzRandom`/
  `BzConvert`/`BzStr`/`BzHex`/`BzBase64`/`BzBitConverter` (`MILESTONE: BCL OK`).

---

## v0.14 "Babel" — Polyglot Runtime

- **`script.rs`** — one tree-walking interpreter with 3 front-ends:
  **JavaScript**, **TypeScript** (a real transpile that strips annotations),
  **Python** (INDENT/DEDENT indentation). Functions + recursion, if/while/for,
  operators, a step budget. Shell `script <lang>` + `js`/`ts`/`py`
  (`MILESTONE: BABEL OK`).

## v0.13 "Lapis" — Virtualization

- **`vmx.rs`** detects VT-x/AMD-V; **`vmm.rs`** is a software VMM (the BZVM
  virtual CPU, virtio, a RAM disk, full snapshot/restore). A tiny guest "NanoOS"
  (assembled by an in-kernel two-pass assembler) boots and runs. A VM manager +
  the `vm` shell command (`MILESTONE: LAPIS OK`).

## v0.12 "Nalar" — AI Subsystem & Power

- **`ai.rs`** — a char-level bigram LLM, Sobel edge-detect CV, procedural
  text-to-image (CPU-local); **`model.rs`** is a Hugging Face-style model gallery;
  the `ask` shell command. **`power.rs`** — ACPI shutdown/restart + light sleep
  (`MILESTONE: NALAR OK`).

## v0.11 "Cahaya" — GPU Compute API & Desktop Polish

- **`compute.rs`** (SAXPY/blend, a CPU backend, a reserved GPU backend); the
  screensaver (6 classic styles); wallpaper (a user BMP); window controls
  (min/max/close + rounded); micro-interactions. **A latent bugfix:** re-enable
  interrupts after a ring-3 app exits (`MILESTONE: CAHAYA OK`).

## v0.10 "Buah" — Theme Engine & Package Manager

- **`theme.rs`** design tokens + 8 built-in themes + dark/light; **`pkg.rs`**
  registry + `bz install/remove/list/search` (`MILESTONE: BUAH OK`).

## v0.9 "Serbuk" — 4 App Variants, Drawing, Task Manager

- 4 app variants (console/desktop/web/widget); `Buitenzorg.Drawing`
  (System.Drawing-style); Task Manager + the `process.rs` process registry
  (`PROC_LIST`/`PROC_KILL`/`SYS_STAT`) (`MILESTONE: SERBUK OK`).

## v0.8 "Kembang" — App Framework

- The window syscall ABI (`WIN_CREATE`/`WIN_CMD`/`WIN_PRESENT`/`KEY_READ`) +
  `DrawCmd`; the `app.rs` launcher; the XOX app; the desktop SDK template
  (`MILESTONE: KEMBANG OK`).

## v0.7 "Kanopi" — Desktop Environment

- `theme.rs` (dark/light), 4 workspaces, `terminal.rs` + `shell.rs`,
  `keyboard.rs` (`MILESTONE: KANOPI OK`).

## v0.6 "Daun" — Compositor & Window Manager

- `gfx.rs` + `wm.rs` (a double-buffered compositor, floating windows, taskbar,
  cursor) (`MILESTONE: WINDOWS OK`).

## v0.5 "Dahan" — VFS, Services, Async I/O, Networking

- VFS + FAT12 write; a parallel service/init manager; io_uring-style async I/O;
  a loopback Ethernet/ARP/IPv4/ICMP stack; a C# service as a process
  (`MILESTONE: DAHAN OK`).

## v0.4 "Tunas" — C# in Ring 3

- SYSCALL/SYSRET, an ELF64 loader, ring-3 user segments + TSS; `hello.cs` via
  bflat (zerolib) + the `bzstart.rs` shim → "Hello from C#!" (`MILESTONE: TUNAS OK`).

## v0.3 "Batang" — Drivers, Storage, Boot on 4 Media

- PCI, a block-device registry, IDE/ATA PIO, FAT12/16/32 read, a PS/2 mouse, a
  pixel demo, boot on 4 media (the `MILESTONE: BATANG` group).

## v0.2 "Akar" — Kernel Core

- Memory management + paging + heap, the scheduler + context switch, the syscall
  ABI v1, kernel IPC (`MILESTONE: MEMORY/SYSCALL/SCHEDULER/IPC OK`).

## v0.1 "Benih" — Bootloader + Minimal Kernel

- Boot on BIOS + UEFI in QEMU, a framebuffer console, GDT/IDT/PIC (timer +
  keyboard), the ASCII boot logo (`MILESTONE: HELLO KERNEL OK`).

---

## License

Released under the **MIT License** — see the [LICENSE](LICENSE) file.
© 2026 Gravicode Studios (led by Kang Fadhil).
