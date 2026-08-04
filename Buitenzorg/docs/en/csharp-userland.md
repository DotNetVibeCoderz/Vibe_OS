# C# Userland & Interop (English)

This document explains how the managed C# runtime interacts with the kernel, the ABI expectations and host-side testing options.

Runtime bridging

- The kernel exposes a C ABI. The managed runtime provides a P/Invoke bridge (NativeSyscalls) which calls into the kernel when running on target.
- For development on the host, a HostSyscalls backend simulates syscall behavior and enables fast iteration and tests.

ELF loader & bzstart

- Userland ELF images include a small shim (bzstart) that prepares the managed runtime entry point and performs required relocation and initialization.
- bflat compiles managed artifacts into ELF suitable for the loader.

Testing & contracts

- The ABI contract is enforced by tests on both sides: cargo test -p bz-abi and dotnet test Buitenzorg.slnx. Keep both sides synchronized when changing shared structs or syscall numbers.