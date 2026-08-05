# Getting Started

This guide takes you from nothing to **Buitenzorg OS running in QEMU**, even if
you have never built an operating system before.

> Buitenzorg OS is a hybrid, AI-native OS: a **Rust** kernel (ring 0) and
> **C#/.NET** apps and services (ring 3). It runs inside the **QEMU** emulator —
> you never touch your real machine.

**English** · [Bahasa Indonesia](getting-started.id.md) · ← [Documentation index](README.md)

---

## 1. Fastest path (one command)

The **quickstart** script installs everything you need (Rust, .NET, QEMU,
bflat), builds the OS, and boots it in QEMU. Open a terminal in the repo folder:

**Windows** (PowerShell):
```powershell
.\scripts\quickstart.ps1
```

**Linux / macOS** (bash):
```bash
./scripts/quickstart.sh
```

That's it — a QEMU window opens and boots Buitenzorg. To build without running,
add `-NoRun` (Windows) / `--no-run` (Linux); for a headless self-test use
`-SmokeTest` / `--smoke`.

> The script is safe to re-run: steps that are already done are skipped. On
> Windows it uses **winget**; on Linux, **apt / dnf / pacman / brew** + rustup.
> If a step fails, follow the printed message or use the manual setup below.

## 2. Dependencies

| Tool | Used for | Manual install |
|---|---|---|
| **Rust** (rustup) | building the kernel (Rust `no_std`) | [rustup.rs](https://rustup.rs) — the nightly toolchain + `x86_64-unknown-none` target are pinned automatically by `kernel/rust-toolchain.toml` |
| **.NET SDK 10** | building the C# runtime/SDK + running tests | [dotnet.microsoft.com](https://dotnet.microsoft.com/download) |
| **QEMU** | the emulator the OS boots in | [qemu.org/download](https://www.qemu.org/download/) — on Windows it is auto-detected at `C:\Program Files\qemu\` |
| **bflat** | compiling C# apps → native ELF (ring 3) | download a release from [bflattened/bflat](https://github.com/bflattened/bflat/releases) and extract it into `tools/bflat/` (quickstart does this for you) |

> `bflat` and `tools/` are gitignored. Without bflat the kernel still boots, but
> the ring-3 C# apps are not built.

## 3. Manual setup (without quickstart)

1. Install **Rust**, **.NET SDK 10**, and **QEMU** (see the table above).
2. Download **bflat** (windows-x64 / linux-glibc-x64) and extract it into
   `tools/bflat/`, so you have `tools/bflat/bflat.exe` (Windows) or
   `tools/bflat/bflat` (Linux).
3. Build and run (see the daily workflow below).

## 4. Daily workflow

**Windows:**
```powershell
.\scripts\build.ps1        # build everything  → dist\*.img
.\scripts\run-qemu.ps1     # boot + watch serial; -Uefi for the UEFI/OVMF path
.\scripts\smoke-test.ps1   # verify boot milestones automatically (4 media)
```

**Linux / macOS:**
```bash
./scripts/build.sh
./scripts/smoke-test.sh
```

**Kernel-only iteration (fastest):**
```powershell
cd kernel
cargo run --release -p bzimage -- --run    # rebuild + boot in one step
```

`bzimage` is the boot pipeline: it compiles `bzkernel` (an artifact dependency,
target `x86_64-unknown-none`), wraps it with the `bootloader` 0.11 crate into a
GPT/FAT image (UEFI) plus an MBR image (BIOS), then optionally launches QEMU. The
OVMF firmware for UEFI is downloaded automatically to `kernel/target/ovmf/`.

> ⚠️ Never run `cargo build` for `bzkernel` without `--target
> x86_64-unknown-none` — the kernel cannot (and need not) be built for the host,
> which is why it is excluded from the workspace `default-members`.

## 5. The C# side (host)

```powershell
dotnet build Buitenzorg.slnx     # note: .slnx, not .sln
dotnet test  Buitenzorg.slnx     # the Rust ↔ C# ABI contract
dotnet run --project runtime\samples\HelloBuitenzorg
```

On the host, C# apps run against a **simulation backend** (`HostSyscalls`) whose
API is identical to the real target — so the same code runs on bare metal. Want
to build your own app? See **[Your First App](first-app.md)**.

## 6. Debug the kernel (GDB)

The turnkey way is `scripts/debug-kernel.ps1` / `.sh` (see
[Debugging & Profiling](debugging.md)). The manual way:

```powershell
cd kernel
$env:QEMU_EXTRA = "-s -S"                   # QEMU: GDB server, boot paused
cargo run --release -p bzimage -- --run
# from another terminal:  gdb → target remote :1234
```

Kernel symbols: `kernel/target/x86_64-unknown-none/release/bzkernel`.

## Beyond QEMU

- **Run in a VM** (VMware / VirtualBox / Hyper-V): [run-in-vm.md](run-in-vm.md).
- **Boot on real hardware** from USB: [install-hardware.md](install-hardware.md)
  *(experimental — not yet validated on a physical machine; see the doc for the
  honest capability matrix).*

## Troubleshooting

- **`rustup` / `dotnet` / `qemu` not found after quickstart** — open a new
  terminal (PATH was just refreshed) and try again.
- **QEMU not on PATH (Windows)** — the build still detects
  `C:\Program Files\qemu\`. If yours is elsewhere, set the `QEMU` env var to the
  full path of `qemu-system-x86_64.exe`.
- **"offset is not a multiple of 16" when building the kernel** — you built
  `bzkernel` for the host. Always go through `bzimage` (`cargo run -p bzimage`),
  which uses the bare-metal target.
- **C# apps don't appear at boot** — make sure `tools/bflat/bflat.exe` exists,
  then run `scripts/build-hello-csharp.ps1` and check for bflat errors.
- **Boot feels slow (~1 minute to READY)** — normal: the boot runs many demo
  apps. The full log always streams to serial.
- **QEMU screen is black but serial is running** — the kernel renders to the
  framebuffer; give it a few seconds for the desktop, or read the serial output.

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
