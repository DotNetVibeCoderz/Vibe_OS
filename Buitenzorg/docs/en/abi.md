# Syscall ABI v1 — Contract (English)

Source of truth: kernel/abi/src/lib.rs (bz-abi). The runtime mirrors the ABI at runtime/Buitenzorg.Runtime/Sys/. Tests run on both sides to ensure compatibility.

Principles

- C ABI only (extern "C") with minimal marshaling: primitive types, pointers, and C-compatible structs.
- Stable numbering: syscalls are append-only. Changes that break existing layout require a new ABI version.
- Zero-copy for large data (framebuffer, files, tensors) via shared memory.
- Validate user pointers in the kernel: user ranges must be under USER_ADDR_MAX and mapped/writable when required.

Syscall table (summary)

This ABI exposes a compact set of syscalls for debug, process management, memory mapping, windowing & drawing, filesystem, audio, packages, networking (UDP), threading primitives (futex), and time. See the kernel/abi source for the authoritative list and types.

Security hardening

All user pointers are validated before use. The kernel verifies range, page presence, and access rights per-page. Output buffers are checked for writability. The ABI is frozen for v1: sizes, alignments and syscall numbers are enforced by tests on both Rust and C# sides.

Shared structs

Key shared structs include FramebufferInfo, DrawCmd, ProcInfo, SysStat, AudioInfo, PkgInfo and FsEntry. All are defined as C-layout structs with fixed sizes. Check kernel/abi and runtime/Buitenzorg.Runtime/Sys for exact definitions.

Extending the ABI

To add a syscall: append a constant in the bz-abi sysno table, increase the COUNT, implement the dispatcher in the kernel and the bridge in the runtime, and update tests on both sides. Breaking changes must be a new ABI version.
