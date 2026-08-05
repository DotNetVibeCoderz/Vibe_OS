# Buitenzorg OS

**English** · [Bahasa Indonesia](README.id.md)

> A hybrid, **AI-native operating system**: a **Rust** kernel and drivers (the
> "unsafe world"), a **C#/.NET** application, UI, and AI layer (the "managed
> world"). Written from scratch for x86-64.
>
> *Codename **Buitenzorg** — the old Dutch name for Bogor, "without worries"
> (zonder zorg). Made by [Gravicode Studios](#credits), led by Kang Fadhil.*

**`Status: v0.1–v0.16 complete` · `v1.0 stabilization in progress` · `License: MIT`**

New here? Start with the **[Tutorial](docs/tutorial.md)**. Full documentation
index: **[docs/](docs/README.md)**. Release history: **[CHANGELOG](CHANGELOG.md)**.

> 🌐 The user-facing docs (README, `docs/`) are written in **English**. The
> authoritative technical spec **[requirements.md](requirements.md)** and the
> living trackers **[PLAN.md](PLAN.md)** / **[Progress.md](Progress.md)** are
> maintained in **Bahasa Indonesia**.

![Buitenzorg OS desktop — start menu, taskbar with a live clock, and app windows](docs/img/desktop-shell.png)

---

## Contents

- [What is Buitenzorg?](#what-is-buitenzorg)
- [Status at a glance](#status-at-a-glance)
- [Screenshots](#screenshots)
- [Quickstart](#quickstart)
- [Documentation](#documentation)
- [Architecture](#architecture)
- [Repository layout](#repository-layout)
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [Credits & License](#credits)

---

## What is Buitenzorg?

Buitenzorg is a **microkernel-leaning, AI-native OS** built on a clear split of
responsibilities:

- **Rust (`no_std`, ring 0)** — bootloader, kernel, memory manager, scheduler,
  interrupts, and hardware drivers.
- **C#/.NET (ring 3)** — system services, the window manager and desktop, the AI
  subsystem, the SDK, and applications. The runtime strategy is NativeAOT first,
  CoreCLR/JIT later.
- **AI-native** — a local LLM, computer vision, and generative AI are OS-level
  services (not add-on apps), with a Hugging Face-style Model Manager.
- **Polyglot apps** — one app model, four variants (console, desktop, web,
  widget), writable in C#, JavaScript, TypeScript, or Python.

Unlike Cosmos (a C# kernel via IL2CPU), Buitenzorg keeps the kernel in Rust and
gives C# the full .NET runtime in user-space. The make-or-break seam is the
**Rust ↔ C# ABI** (`kernel/abi` ↔ `runtime/Buitenzorg.Runtime/Sys`) — a stable,
numbered syscall table guarded by identical contract tests on both sides.

## Status at a glance

Every milestone is verified in QEMU on **four boot media (IDE / AHCI / NVMe /
USB)** via `MILESTONE: … OK` markers that the smoke test greps for.

| Version | Codename | Highlights |
|---|---|---|
| v0.1–0.4 | Benih → Tunas | Boot (BIOS+UEFI) · memory/paging/heap · scheduler · syscall ABI · IPC · PCI · IDE+FAT · **C# in ring 3** |
| v0.5–0.7 | Dahan → Kanopi | VFS + FAT write · service manager · async I/O · networking · compositor + window manager · desktop, terminal, themes, workspaces |
| v0.8–0.10 | Kembang → Buah | App framework + window syscalls · 4 app variants · `Buitenzorg.Drawing` · Task Manager · theme engine (8 themes) · package manager |
| v0.11–0.12 | Cahaya → Nalar | Compute API · screensaver · window controls · **AI subsystem** (LLM + CV + GenAI + Model Manager) · power management |
| v0.13–0.14 | Lapis → Babel | **Virtualization** (software VMM runs a guest OS + snapshots) · **polyglot runtime** (JS / TS / Python) |
| v0.15 | Matang | **Managed C# runtime** — working heap + `Buitenzorg.Bcl` (collections, LINQ, System.IO/Text/Regex/Net/Tasks, …) |
| v0.16 | Panen | **AC'97 audio** · `Buitenzorg.UI` toolkit · **8 preloaded apps** · desktop shell (start menu + tray clock) · JPEG decoder |
| **v1.0** | Buitenzorg | *In progress* — security hardening, ABI freeze, GDB debugger + profiler, CI benchmark, USB/VM images, MIT license |

<details>
<summary><b>Sample boot log</b> (serial output, click to expand)</summary>

```
[kernel] Hello Kernel -- Buitenzorg OS v0.1 'Benih'
[kernel] MILESTONE: HELLO KERNEL OK
[kernel] MILESTONE: MEMORY OK
[kernel] MILESTONE: SYSCALL ABI V1 OK
[kernel] MILESTONE: SCHEDULER OK (two tasks alternated preemptively)
[kernel] MILESTONE: IPC OK (3 messages, checksum verified)
[kernel] MILESTONE: PCI OK (6 devices enumerated)
[kernel] MILESTONE: STORAGE OK (file read from disk via IDE PIO + FAT)
[kernel] MILESTONE: TUNAS OK (C# ran in ring 3 -> 'Hello from C#!')
[kernel] MILESTONE: VFS OK (FAT write + read-back verified on /ram)
[kernel] MILESTONE: SERVICES OK (dependency-ordered parallel init)
[aio] 2001 ops in <1 tick (>36418 ops/sec)
[kernel] MILESTONE: ASYNC IO OK (io_uring-style SQ/CQ, benchmark-able)
[kernel] MILESTONE: NETWORK OK (Ethernet/ARP/IPv4/ICMP stack)
[kernel] MILESTONE: WINDOWS OK (two windows moved & resized)
[kernel] MILESTONE: KANOPI OK (desktop environment: terminal, theme, multi-desktop)
[kernel] MILESTONE: SERBUK OK (System.Drawing library + Task Manager + 4 app variants)
[kernel] MILESTONE: BUAH OK (theme engine + 8 styles + package manager)
[kernel] MILESTONE: CAHAYA OK (GPU compute + window controls + screensaver + personalization)
[kernel] MILESTONE: NALAR OK (AI subsystem + power management)
[kernel] MILESTONE: LAPIS OK (virtualization: software VMM + guest OS)
[kernel] MILESTONE: BABEL OK (polyglot runtime: JS/TS/Python)
[kernel] MILESTONE: BCL OK (Buitenzorg.Bcl) / BCL2 OK (System.IO/Text/Regex/Net/...)
[kernel] MILESTONE: AUDIO OK (AC'97 mixer + PCM) / UI OK / DRAW OK / JPEG OK
[kernel] MILESTONE: SUITE OK (8 preloaded apps) / DESKTOP SHELL OK
[kernel] MILESTONE: SECURITY OK (syscall pointer validation) / PROFILER OK
[kernel] BUITENZORG READY -- terminal ('run calc', 'prof self', 'ask ...', 'vm start nanovm').
```
</details>

## Screenshots

| | |
|---|---|
| **Desktop shell** — start menu, taskbar, live tray clock, app windows | **UI toolkit** — `Buitenzorg.UI` controls |
| [![Desktop shell](docs/img/desktop-shell.png)](docs/img/desktop-shell.png) | [![UI toolkit](docs/img/desktop-ui.png)](docs/img/desktop-ui.png) |
| **Clock** — analog + digital, real CMOS time | **2048** — a preloaded game |
| [![Clock](docs/img/desktop-clock.png)](docs/img/desktop-clock.png) | [![2048](docs/img/desktop-2048.png)](docs/img/desktop-2048.png) |
| **AI subsystem** — local LLM + model gallery (v0.12 "Nalar") | **Screensaver** — Mystify, Win 3.1/98 style |
| [![AI subsystem](docs/img/desktop-nalar.png)](docs/img/desktop-nalar.png) | [![Screensaver](docs/img/screensaver-mystify.png)](docs/img/screensaver-mystify.png) |

**MagicAppGen** — the host-side AI app generator (`tools/MagicAppGen`) that
writes Buitenzorg apps from a prompt:

[![MagicAppGen](docs/img/magicappgen.png)](docs/img/magicappgen.png)

More screenshots are embedded throughout the [documentation](docs/README.md).

## Quickstart

**Prerequisites**

| Tool | Version | Notes |
|---|---|---|
| Rust (rustup) | nightly (pinned by `kernel/rust-toolchain.toml`) | target `x86_64-unknown-none` |
| .NET SDK | 10.0+ | runtime, SDK, `bz` CLI |
| QEMU | `qemu-system-x86_64` | primary emulator |

**Fastest path (zero setup):** one script installs everything (Rust, .NET, QEMU,
bflat), builds, and boots in QEMU:

```powershell
.\scripts\quickstart.ps1     # Linux/macOS: ./scripts/quickstart.sh
```

**Daily workflow** (dependencies already installed):

```powershell
.\scripts\build.ps1          # build kernel + boot images + .NET  → dist/
.\scripts\run-qemu.ps1       # boot in QEMU (graphical + serial); add -Uefi for UEFI
.\scripts\smoke-test.ps1     # headless boot, assert milestone markers

cd kernel; cargo test -p bz-abi     # Rust-side ABI contract tests
dotnet test Buitenzorg.slnx         # C#-side ABI contract + manifest tests
dotnet run --project sdk\bz -- new console-csharp MyApp   # scaffold an app
```

Boot a specific medium with `cargo run --release -p bzimage -- --run --media nvme`
(`ide` / `ahci` / `nvme` / `usb`). Full setup and troubleshooting:
**[Getting Started](docs/getting-started.md)**.

## Documentation

The complete, organized index lives in **[docs/README.md](docs/README.md)**.
Highlights:

| Doc | For |
|---|---|
| **[Tutorial](docs/tutorial.md)** | Zero-to-app walkthrough — **start here** |
| [Getting Started](docs/getting-started.md) | Setup, dependencies, daily workflow, troubleshooting |
| [Your First App](docs/first-app.md) | Build an app + the built-in library catalog |
| [Run in a VM](docs/run-in-vm.md) | VMware, VirtualBox, Hyper-V |
| [Install on Hardware](docs/install-hardware.md) | Flash to USB and boot a physical machine |
| [Debugging & Profiling](docs/debugging.md) | GDB attach + the TSC zone profiler |
| [Syscall ABI](docs/abi.md) | The v1 ABI table and evolution rules |
| [CHANGELOG](CHANGELOG.md) · [CONTRIBUTING](CONTRIBUTING.md) | Release history · how to contribute |

## Architecture

Ten layers, bottom-up (see [requirements.md](requirements.md) §3):

```
Hardware → Bootloader (Rust) → Kernel (Rust, ring 0) → Drivers →
Managed Runtime (.NET — the critical bridge) → System Services (C#) →
AI Subsystem (C#) → Desktop Environment (C#) → App Framework (polyglot) → Apps
```

The **interop rules** (§4) are the heart of the design: all cross-language calls
go through the C ABI; only primitives, pointers, and `#[repr(C)]` structs cross
the boundary; syscall numbers form a stable, append-only table; large data
(framebuffers, files, tensors) uses zero-copy shared memory; and managed objects
are GC-pinned while Rust holds pointers to them. The kernel side
(`kernel/abi`) and the C# mirror (`runtime/Buitenzorg.Runtime/Sys`) are held in
lockstep by identical contract tests — change one side without the other and the
tests go red.

## Repository layout

```
kernel/            Rust workspace (nightly, no_std)
  abi/               bz-abi — the syscall ABI v1 contract (source of truth)
  bzkernel/          ring-0 kernel: boot, console, GDT/IDT, memory, heap, syscalls, drivers
  bzimage/           boot-image builder (UEFI + BIOS) + QEMU runner
runtime/           C# managed world
  Buitenzorg.Runtime/        ABI mirror, syscall backends, app manifest
  Buitenzorg.Runtime.Tests/  ABI + manifest contract tests
  samples/                   HelloBuitenzorg (host-sim sample)
userland/          ring-3 programs (bflat/zerolib C# + bzstart.rs shim → *.elf)
sdk/               bz CLI + app templates + VS Code extension
tools/             third-party toolchains (bflat) — gitignored
ai/  apps/         AI subsystem (v0.12) · preloaded suite (v0.16)
docs/  scripts/  dist/   documentation · build & run scripts · image output (gitignored)
```

## Roadmap

- **[PLAN.md](PLAN.md)** — product roadmap, version by version (v0.1 → v1.x). *(ID)*
- **[Progress.md](Progress.md)** — per-feature checklist tracker. *(ID)*
- **[requirements.md](requirements.md)** — full technical spec; §16 roadmap, §17 checklist. *(ID)*

## Contributing

See **[CONTRIBUTING.md](CONTRIBUTING.md)** for coding standards and the PR flow.
The golden rule: **any ABI change must touch both sides plus both contract test
suites plus `docs/abi.md` in the same change**, and syscall numbers are
append-only.

## Credits

**Buitenzorg OS** is made by **Gravicode Studios**, led by **Kang Fadhil**. This
attribution also appears inside the OS itself — the boot logo, the desktop
**Welcome** window, and the shell `ver` / `about` commands.

## License

Released under the **MIT License** — see [LICENSE](LICENSE).
© 2026 Gravicode Studios.
