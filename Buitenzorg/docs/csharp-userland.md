# Running C# in Ring 3 (v0.4 "Tunas")

The v0.4 milestone: an ahead-of-time-compiled **C#** program runs in user-space
(ring 3) on top of the Rust kernel and calls into the kernel via syscalls. This
is "Layer 4 — the Managed Runtime" (requirements.md §3, §5.1) in its lightest
form: the **NativeAOT** path, no JIT, no GC.

**English** · [Bahasa Indonesia](csharp-userland.id.md) · ← [Documentation index](README.md)

## Build pipeline

```
hello.cs ──bflat(ILC/NativeAOT, zerolib)──► hello.o ─┐
                                                     ├─rust-lld─► hello.elf (static ELF)
bzstart.rs ──rustc(x86_64-unknown-none)──► bzstart.o ┘
```

- **bflat** (`--stdlib:zero`) compiles C# to a freestanding object: no full .NET
  runtime, no GC. `Console.Write` calls `SystemNative_Log`.
- **bzstart.rs** is the startup shim + PAL: it provides `_start` (which calls the
  NativeAOT entry `__managed__Main`) and `SystemNative_Log/Malloc/Abort` that
  translate to Buitenzorg syscalls. This replaces glibc + libSystem.Native.
- **rust-lld** links both into a static ELF (no interpreter) at `0x400000`.

Build: `scripts/build-hello-csharp.ps1` (or `.sh`). Needs `tools/bflat`
(downloaded from github.com/bflattened/bflat) + Rust nightly.

## Execution path in the kernel

1. `bzimage/build.rs` embeds `hello.elf` in the image (as `HELLO.ELF`).
2. At boot, `tunas_demo` reads `HELLO.ELF` from disk via the IDE + FAT driver.
3. `elf::load` maps the PT_LOAD segments as **user-accessible** pages.
4. `usermode::enter_user` does a `sysretq` into ring 3 at the entry point.
5. C# prints via `SystemNative_Log` → the `DEBUG_WRITE` syscall → the kernel.
6. `Main` returns → the `exit` syscall → `usermode::exit_user` (longjmp) →
   returns to the kernel with the exit code.

## Ring-3 ABI

`syscall` with `rax` = the syscall number, `rdi/rsi/rdx` = the arguments, result
in `rax` (see [Syscall ABI](abi.md)). SFMASK clears IF on entry so a syscall is
uninterrupted; the timer still preempts *user* code via the ring-0 TSS stack.

## Two page-table gotchas (important)

To execute a user page, **every** level of the page-table walk must:
- have the `USER_ACCESSIBLE` bit (permission is the AND of all levels), and
- **not** have the `NX` bit (an instruction fetch faults if NX appears at any level).

The upper-level entries are shared with kernel mappings (created without USER,
and the bootloader's low PML4 entry has NX). `memory::make_user_path` ORs in USER
and clears NX on the intermediate entries along the path to each user page. The
leaves keep their own bits, so kernel memory stays protected and non-executable.

## What's next (v0.8+)

The CoreCLR + JIT path for full C# features (reflection, dynamic loading), GC
integration with the memory manager, and a full BCL — all built on this ring-3
foundation.

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
