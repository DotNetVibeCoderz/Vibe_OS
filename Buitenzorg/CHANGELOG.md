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
- **Preemptive multi-process ring 3 (increment 1).** Several ring-3 programs
  run at once, timer-preempted, each in its own address space (`aspace.rs`:
  two private page directories per process covering the 2 GiB user half,
  swapped in the shared PDPT on a switch). Per-task SYSCALL stack + TSS rsp0 +
  FXSAVE state; spawn/kill; a ring-3 #PF/#GP ends only the faulting program;
  the desktop and the shell's `run` launch apps as processes; `KEY_READ`
  follows window focus. `mproc.cs` + `multiproc_demo` → `MILESTONE: MULTIPROC
  OK`, with a negative control (fails with ring-3 preemption disabled).
- **Process control (increment 2).** Closing a process's last window ends it;
  shell `ps` / `kill <pid>`; an idle `KEY_READ` yields the CPU for spawned
  processes instead of spinning.

- **Multi-arch M0 (increment 1): HAL boundary.** x86 CPU code moved to
  `src/arch/x86/`, PC port-I/O devices to `src/platform/pc/`; generic code uses
  the `arch::cpu` facade; `scripts/check-arch-boundary.sh` enforces it in CI
  (paging, context switch and boot glue are the remaining allowlisted files).
- **Multi-arch M0 (increment 2): context-switch HAL.** `arch::context`
  (`FpuState`, `prepare_stack`, `switch`) holds the x86 FXSAVE + stack-switch
  assembly; the scheduler (`task.rs`) is now architecture-neutral.
- **Multi-arch M4 (increments 1-2): the RISC-V port starts.** The same
  kernel source now also builds for 64-bit RISC-V and boots on QEMU `virt`
  (OpenSBI, edk2, Limine): serial console, Device Tree, paging with an address
  space per process, the timer, kernel-thread switching, and user-mode
  programs calling the kernel with `ECALL` -- a faulting program is ended
  without taking the kernel down.
- **Multi-arch M3 (increment 9): SMP on ARM64 — the ARM64 port is complete.**
  Secondary CPUs start through PSCI and join the shared scheduler; processes
  run in parallel on all cores (4 copies of a CPU-bound program on 4 CPUs at
  once), with per-CPU interrupt controllers and timers.
- **Multi-arch M3 (increment 8): the interactive desktop on ARM64.** The
  desktop session loop is now shared code; on ARM64 the terminal takes typing
  from a virtio keyboard, the pointer comes from a virtio tablet, and the live
  desktop is shown through virtio-gpu.
- **Multi-arch M3 (increment 7): the whole kernel on ARM64.** Every generic
  subsystem of the PC kernel — syscalls, app launcher, window manager, shell,
  network, packages, VMM, scripting — now builds and runs on ARM64 unchanged,
  with PSCI power, virtio input/net and a pointer driver as the ARM platform
  parts. C# GUI apps (Calculator, 2048, the UI toolkit, Drawing, XOX) run on
  ARM64 and are composited into a desktop frame. A small tool patches the
  arm64 zerolib with the division-overflow helper bflat's code generator needs.
- **Multi-arch M3 (increment 6): disk, clock and display on ARM64.** A
  virtio-mmio transport lets the existing virtio drivers run on ARM64: `/disk`
  is mounted from a virtio disk through the shared FAT and VFS code (read and
  write), programs are loaded from it, the PL031 clock dates files, and
  virtio-gpu shows frames (`ARM64 DISK OK`, `ARM64 RTC OK`, `VIRTIO GPU OK`).
- **Multi-arch M3 (increment 5): processes and threads on ARM64.** The
  scheduler and process registry are shared with x86 unchanged; programs run
  as preemptively scheduled processes, two CPU-bound C# programs share the CPU
  through timer preemption with their own FP state and memory, and ring-3
  threads and futex mutexes work (`ARM64 MULTIPROC OK`, `THREAD OK`,
  `SYNC OK`).
- **Multi-arch M3 (increment 4): C# apps on ARM64.** The C# programs are
  compiled by bflat for linux-arm64 and linked with an ARM64 build of the
  runtime shim; Limine hands them to the kernel as boot modules and the
  generic ELF loader runs them in user mode. `Hello from C#!` and the C#
  runtime tests (mmap, managed heap growth, reserve/commit) pass on ARM64.
- **Multi-arch M3 (increment 3): threads and user mode on ARM64.** Kernel
  threads switch with their own FP/SIMD state, and programs run in EL0 in
  their own address space, calling the kernel with `SVC` and exiting back to
  it; a program that touches kernel memory is terminated on its own
  (`ARM64 CONTEXT OK`, `ARM64 EL0 OK`).
- **Multi-arch M3 (increment 2): memory on ARM64.** The generic frame
  allocator, paging context and kernel heap run on ARM64 over a new AArch64
  paging layer (TTBR1 kernel half, TTBR0 user half), and every process gets its
  own TTBR0 address space (`ARM64 MEMORY OK`, `ARM64 ASPACE OK`).
- **Multi-arch M3 (increment 1): the ARM64 kernel boots.** The kernel crate
  builds for `aarch64-unknown-none-softfloat`; Limine loads it on QEMU `virt`
  (UEFI). It installs its exception vectors, reads the live Device Tree,
  maps the PL011 console and the GICv3 into the MMU, and takes timer
  interrupts at 100 Hz (`MILESTONE: ARM64 BOOT OK`, smoke step `[arm64]`).
  The x86 kernel moved to `pc_main.rs`; `main.rs` picks the architecture.
- **Desktop: `tile` / `tutup` shell commands; console no longer scrolls the
  desktop.** `tile` arranges the current workspace's windows side by side,
  `tutup` closes them (except the terminal). Kernel log lines printed after the
  desktop came up used to scroll the framebuffer, shifting the desktop up and
  leaving log text under the taskbar; they now go to serial only. The
  wallpaper screenshots were retaken with different apps open on each.
- **Multi-arch M2: virtio drivers — blk, input, net, gpu, sound.** A generic
  virtio core (split virtqueues, feature negotiation) over the modern
  virtio-PCI transport. The whole OS boots from a virtio-blk disk; virtio
  keyboard and tablet drive the desktop; virtio-net replaces the loopback
  device (ARP + ping of QEMU's gateway, local traffic still works); virtio-gpu
  mirrors the desktop pixel for pixel; virtio-snd plays a test tone. New smoke
  step `[virtio]` with all five devices. Fixed on the way: the AC'97 driver
  claimed a virtio sound card (same PCI class).
- **Multi-arch M1 (increment 2): Limine boot — M1 complete.** The same
  kernel ELF boots through the Limine protocol (base revision 6, requests
  declared in `arch/x86/limine.rs`): higher-half load, no identity map,
  HHDM-relative RSDP. `bzimage` builds `buitenzorg-limine-uefi.img` (MBR + FAT
  partition with Limine, the kernel and all /disk files) when
  `scripts/fetch-limine.*` has fetched the pinned Limine v12.9.3 binaries; a
  new `[limine]` smoke step boots it to READY on 2 CPUs with /disk and
  processes, reaching the same milestones as the IDE boot.
- **Multi-arch M1 (increment 1): boot-protocol HAL + Device Tree.** The
  `bootloader` crate is confined to `arch/x86/boot.rs`, which converts its
  hand-off into a neutral `boot::BootInfo` (memory map, framebuffer,
  ACPI RSDP / DTB, physical-memory offset); the boundary check forbids
  `bootloader_api` elsewhere. New `dtb.rs` parses flattened Device Trees and
  discovers RAM, CPUs, UART, interrupt controller, timer, virtio-mmio slots and
  PSCI — verified against real QEMU `virt` trees for aarch64 and riscv64, with
  malformed blobs rejected (`MILESTONE: DTB OK`).
- **Multi-arch M0 (increment 3): paging HAL — M0 complete.** `arch::paging`
  maps/unmaps/protects pages with neutral `Flags { write, user, exec }` and
  `u64` frames, and answers the ring-3 access check used by syscall pointer
  validation. `memory.rs` is generic policy (frame allocator `alloc`/`free`,
  mmap arena, validation); the ELF loader, kernel heap and boot path go
  through `MemCtx`, which is now installed before the heap; `aspace` (the
  PDPT-swap model) moved to `arch/x86`. The boundary checker's allowlist is
  empty. The kernel heap is now mapped non-executable.
- **Desktop UI/UX overhaul.** New default theme "kebun" (from the lotus-pond
  wallpaper: moss glass panels, mist text, a single lotus-pink accent, pollen
  time). The start menu is organised by category (Kantor, Multimedia,
  Permainan, Utilitas, Pengaturan) with a leaf-colored rail, icon cards, the
  date, and Sleep/Restart/Shut down; Wallpaper and Tema are system actions.
  Glass taskbar with a leaf Start button, quiet window labels and a date +
  time tray; Bold/24 px type for hierarchy; legible desktop icon labels. The
  session starts tidy: frozen boot-demo windows are closed and the terminal
  opens fresh next to a Tips card.
- **App icons.** Ten AI-generated icons in one style (files, editor, calc,
  store, image viewer, clock, piano, 2048, task manager, paint) replace the
  identical placeholder tiles on the desktop and in the start menu. `bzimage`
  packs them (44 px + 18 px, anti-aliased rounded alpha) into
  `/disk/ICONS.BIN`; without `/disk` each app gets its own colored tile.
  `MILESTONE: ICONS OK`.
- **Eight official wallpapers.** AI-generated artwork (nature — the default,
  Bogor Botanical Gardens at dawn — futuristic, city, art, surreal, imaginary,
  space, minimal) in `assets/wallpapers/`; `bzimage` converts them to 800×450
  BMPs on `/disk`, the kernel resamples once (bilinear) and caches. `bg list` /
  `bg <name>`; `MILESTONE: WALLPAPERS OK`; gallery in the README and docs.
- **Processes, network, packages and audio without the BKL.** PROC_SPAWN,
  PROC_WAIT, NET_*, PKG_* and AUDIO_* run under their subsystems' own locks.
  PROC_WAIT decides "child still alive?" and blocks in one step under the
  scheduler lock, so a child exiting on another CPU cannot be missed (SMPPROC
  workload; a deliberately racy variant hangs and fails the test).
- **Fixed: rare UEFI hang/fault in the scheduler demo.** New tasks started with
  interrupts enabled while still finishing the context switch; a timer tick
  there (kernel preemption, demo only) could switch away holding the scheduler
  lock or corrupt the BKL ticket order. Tasks now start with interrupts off.
- **Syscalls with interrupts on; files without the BKL.** A 4-CPU livelock
  (CPU 0 spinning with interrupts off froze the PIT tick count) led to serving
  every syscall with interrupts enabled and to coalescing presents. FS_LIST /
  FS_READ / FS_WRITE run without the BKL under the VFS mount lock (SMPFS
  workload). Process arguments are recorded before the child can be
  scheduled (on SMP it could start reading them on another CPU).
- **Parallel threads.** Threads of one process run on several CPUs at once.
  The ring-3 runtime became SMP-safe: the heap allocator is locked,
  `bz_mutex_lock` uses a compare-and-swap (it was a plain read-then-write),
  and new `bz_atomic_add` / `bz_atomic_add32` / `bz_atomic_cas32` exports
  replace racy shared-counter updates. `PTHREAD.ELF`: `MILESTONE: SMP THREADS
  OK` (4 threads on 4 CPUs at once; disabling the heap lock makes it detect
  overlapping allocations).
- **I/O APIC.** Device interrupts are routed through the I/O APIC (MADT +
  ISA overrides) instead of the 8259, which is now masked; handlers EOI the
  Local APIC. The PIT stays on CPU 0, keyboard and mouse go to another CPU.
  Self-test raises real keyboard/mouse IRQs through the 8042 (0xD2/0xD3) and
  checks the CPU that handled them: `MILESTONE: IOAPIC OK`.
- **SMP increment 5: fine-grained locking.** Memory-management, scheduler
  and console syscalls run without the Big Kernel Lock, in parallel on all
  CPUs. The "from ring 3" pointer-validation flag became per-task (it was
  global); TLB shootdowns no longer happen under a lock and frames are freed
  only after every CPU acknowledged; futex check-and-block is atomic under the
  scheduler lock. The window system (WIN_*, KEY_READ, FB_INFO), process registry (PROC_LIST,
  PROC_ARGS, SYS_STAT) and CLOCK_RTC followed (the CMOS port pair got its own
  lock). The WM lock was then split: each window canvas has its own lock and a
  compose works on a snapshot, so apps keep drawing while another CPU
  composes. New test programs SMPSYS and SMPGUI: `MILESTONE: SMP FINELOCK OK`
  (4 syscalls in the kernel at once with `-smp 4`, BKL waits ~4000 -> ~8,
  throughput x2.25).
- **SMP increment 4: symmetric SMP kernel.** Every CPU runs the same
  scheduler and executes kernel code: syscalls are served on the CPU that made
  them and kernel tasks run on any core, serialized by a Big Kernel Lock (FIFO
  ticket lock, dropped in ring 3, while idle and across context switches).
  Each CPU has an idle task; user-space mappings go through the running CPU's
  page tables; the exit context lives in the task. `MILESTONE: SMP KERNEL OK`
  (kernel tasks on all 4 CPUs with `-smp 4`, syscalls served on the APs).
  Replaces the asymmetric design of increment 3.
- **SMP increment 3: ring-3 processes run on every core.** Asymmetric
  multiprocessing: the kernel stays on the boot CPU, user programs run in
  parallel on the APs. Tasks move to an idle AP when they return to ring 3 and
  come back for syscalls, faults and kills; hand-offs only complete after the
  task's context is saved. Each AP has its own page-table root, SYSCALL entry
  stub and TSS stack; TLB shootdowns follow unmap/mprotect; a dead process's
  address space is freed only once none of its threads is on an AP. Threads of
  one process are still serialized. `MILESTONE: SMP SCHED OK` (3 APs busy at
  once with `-smp 4`); the IDE smoke boot now uses 2 CPUs, plus a 1-CPU run.
- **SMP increment 2: per-CPU state, AP interrupts, IPIs.** Each AP gets its
  own GDT + TSS (private double-fault and ring-0 stacks), loads the IDT, and
  runs a 100 Hz Local APIC timer calibrated against the PIT; per-CPU state is
  found by LAPIC id (`arch::cpu::id()`). IPIs: ping and a broadcast TLB
  shootdown with acknowledgement. `MILESTONE: SMP PERCPU OK` (required on the
  AHCI and UEFI smoke boots); shell `cpu` lists per-CPU counters. The APs still
  run no tasks.
- **SMP increment 1.** CPUs discovered from the ACPI MADT; the BSP's Local
  APIC enabled; every AP started with INIT-SIPI-SIPI through a real-mode ->
  long-mode trampoline at 0x8000 (temporary page tables with an identity map;
  memory below 1 MiB is never allocated) and parked → `MILESTONE: SMP OK`
  (3 APs online with `-smp 4`; smoke boots AHCI and UEFI with 2 CPUs). The
  scheduler is still single-core.
- **Every app is a process.** `app::run_named` (the boot demos) is now spawn +
  blocking wait instead of running the app on the launching task, so no app
  shares the boot task's privilege stack any more and every app can be killed;
  the "attached" mode is gone.
- **Frame reclamation.** The frame allocator has an intrusive free list; a dead
  process's pages, page tables and directories go back to it (`aspace::destroy`),
  and `MUNMAP` frees frames too. `multiproc_demo` requires 0 frames leaked and 0
  growth over 20 spawn/exit cycles.
- **Spawn + argv + wait from ring 3.** New syscalls `PROC_SPAWN` (40),
  `PROC_WAIT` (41, a real blocking wait woken on exit) and `PROC_ARGS` (42);
  `COUNT` = 43, both ABI mirrors + contract tests + `docs/abi.md` updated.
  `BzProcess.Start/WaitForExit/GetArgs`; shell `run <app> <args>`.
  `proctest.cs` → `MILESTONE: PROCCTL OK`.
- **Persistent `/disk`.** ATA PIO sector writes + cache flush; generic FAT12/16/32
  writing (cluster allocation, every FAT copy, timestamps, old chain freed after
  the new entry is written, delete, free space, FAT32 FSInfo invalidated);
  `/disk` mounted read-write; shell `echo > / >>`, `rm`, `df`; the Editor saves
  to `/disk/NOTE.TXT`. A boot counter survives reboots; the smoke test finds the
  written bytes inside the host image → `MILESTONE: DISK WRITE OK`.

### Fixed
- **UEFI boots hung at the first timer wait** (pre-existing; the BIOS-only
  smoke test never saw it). `pic8259::initialize` restores the firmware's IRQ
  masks, and UEFI firmware (OVMF) leaves the 8259 fully masked (0xff/0xff), so
  no timer interrupt ever arrived. The kernel now unmasks the lines it uses
  (and puts the BSP LAPIC in virtual-wire mode); the smoke tests boot UEFI too.
- **FAT12 writer corrupted the `/ram` root directory** — it located the second
  FAT copy at `(data_start - fat_start) / 2`, which includes the root directory
  sectors, so FAT entries were written into directory entries 64+. Overwriting a
  file also leaked its old cluster chain.
- **Recurring layout-sensitive boot crash — root cause found.** The static
  kernel stacks (`PRIVILEGE_STACK`, `DOUBLE_FAULT_STACK`) were `[u8; N]` with
  alignment 1; in some layouts the privilege stack landed on an odd address,
  so every ring-3 syscall ran on a misaligned stack and the boot died with a
  rodata dump at the first syscall. Now 16-byte aligned; proven causally by
  forcing an odd stack top on a working layout (identical crash).
- **Mouse position was not clamped** to the screen, leaving a dead zone after
  overshooting an edge.
- **Frame allocator was O(n) per frame** (`usable_frames().nth(next)` re-walked
  the memory map every allocation) — now a cursor; boot-to-READY ~35 s → ~20 s
  (tick estimate), ~83 s → ~52 s wall on IDE.
- **Keyboard queue deadlock risk** — `keyboard::pop` now holds interrupts off
  while taking the lock the keyboard IRQ also takes.

### Added (earlier on this track)
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
