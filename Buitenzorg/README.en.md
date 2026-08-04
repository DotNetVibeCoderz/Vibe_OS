# Buitenzorg OS

Codename: Buitenzorg — a hybrid, AI-native research operating system combining a Rust-based kernel and a managed C# runtime. This repository contains the OS kernel, runtime, SDK and documentation developed by Gravicode Studios (lead: Kang Fadhil).

Summary

- Kernel: Rust (no_std, ring 0) — boot, scheduler, drivers, syscall ABI, VFS, networking, virtualization.
- Runtime & userland: C#/.NET (ring 3) — managed runtime, system services, desktop environment, app framework, AI subsystem.
- Tooling: bx CLI (sdk/bz), bflat toolchain for compiling managed apps into ELF for the OS, QEMU for emulation.

Status

Current milestone: v0.16 "Panen" → moving toward v1.0 stabilization. Features implemented include: multitasking kernel, FAT VFS with write support, GUI desktop shell with themes and windowing, polyglot runtime support (JS/TS/Python), managed C# runtime (Buitenzorg.Bcl), local AI subsystem (LLM + CV + GenAI), AC'97 audio, virtualization (software VMM + snapshot), and an 8-app preloaded suite.

Important links

- Design & specs: requirements.md
- Quickstart & tutorial: docs/getting-started.md and docs/tutorial.md
- Development guide: docs/debugging.md
- Release notes: CHANGELOG.md
- Roadmap: PLAN.md

Requirements

- Rust (nightly) — kernel target: x86_64-unknown-none (configured via kernel/rust-toolchain.toml)
- .NET SDK 10+
- QEMU (qemu-system-x86_64)
- bflat (tools/bflat) for building managed userland apps

Quickstart (recommended)

1. Run the provided automated installer & builder:
   - Windows PowerShell: .\scripts\quickstart.ps1
   - Linux/macOS: ./scripts/quickstart.sh

2. Build only (no run):
   - Windows: .\scripts\build.ps1
   - Linux/macOS: ./scripts/build.sh

3. Run QEMU with the prepared image:
   - Windows: .\scripts\run-qemu.ps1
   - Linux/macOS: ./scripts/run-qemu.sh

Daily development flow

- Full build: .\scripts\build.ps1 (or ./scripts/build.sh)
- Run in QEMU: .\scripts\run-qemu.ps1
- Kernel iterative build & run: cd kernel; cargo run --release -p bzimage -- --run --media nvme
- Tests: cargo test -p bz-abi (kernel ABI); dotnet test Buitenzorg.slnx

Repository layout (high level)

- kernel/: Rust workspace (abi/, bzkernel/, bzimage/)
- runtime/: C# runtime and tests (Buitenzorg.Runtime)
- userland/: managed apps compiled to ELF (hello-csharp/)
- sdk/: bz CLI, templates and VS Code extension
- tools/: third-party tooling (bflat and binaries)
- docs/: user & developer documentation
- scripts/: build, quickstart, run, smoke test

Documentation

Please start from docs/README.md. Key documents:
- docs/tutorial.md — step-by-step developer tutorial
- docs/getting-started.md — prerequisites, quickstart, troubleshooting
- docs/first-app.md — creating your first app using the SDK and native APIs
- docs/abi.md — stable syscall ABI between kernel (Rust) and runtime (C#)

Credits & License

Buitenzorg OS by Gravicode Studios — lead: Kang Fadhil.
Released under the MIT License (see LICENSE).
© 2026 Gravicode Studios.
