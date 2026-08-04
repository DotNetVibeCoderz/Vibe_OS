# Getting Started — Run Buitenzorg OS (English)

This guide helps you build and run Buitenzorg OS in QEMU from a clean machine.

Overview

Buitenzorg is a hybrid OS: the kernel is written in Rust (no_std, ring 0), while userland and system services are implemented in managed C# (.NET) and executed in ring 3. QEMU is used for emulation—no need to touch your host system bootloader.

Prerequisites

- Rust (rustup) — nightly toolchain; the kernel target is x86_64-unknown-none (configured by kernel/rust-toolchain.toml)
- .NET SDK 10+
- QEMU (qemu-system-x86_64)
- bflat tool (optional for building managed userland apps)

Quickstart (automated)

Run the provided quickstart script to install dependencies, build the project and boot Buitenzorg in QEMU:

- Windows (PowerShell): .\scripts\quickstart.ps1
- Linux/macOS (bash): ./scripts/quickstart.sh

If you prefer to build only, use:
- Windows: .\scripts\build.ps1
- Linux/macOS: ./scripts/build.sh

Daily workflow

- Full build: .\scripts\build.ps1 or ./scripts/build.sh
- Boot in QEMU: .\scripts\run-qemu.ps1
- Iterative kernel build & run: cd kernel; cargo run --release -p bzimage -- --run
- Tests: cargo test -p bz-abi; dotnet test Buitenzorg.slnx

Further reading

- docs/tutorial.md — guided developer tutorial
- docs/first-app.md — creating your first app
- docs/run-in-vm.md — running images in VMware / VirtualBox
- docs/install-hardware.md — writing images to USB

Troubleshooting

See the troubleshooting section in the local docs/getting-started.md and the scripts output for common issues (missing PATH entries, bflat missing, build target mistakes, etc.).