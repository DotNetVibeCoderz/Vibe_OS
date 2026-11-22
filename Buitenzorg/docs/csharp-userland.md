# Running C# in Ring 3 (v0.4 "Tunas")

The v0.4 milestone: an ahead-of-time-compiled **C#** program runs in user-space
(ring 3) on top of the Rust kernel and calls into the kernel via syscalls. This
is "Layer 4 — the Managed Runtime" (requirements.md §3, §5.1) in its lightest
form: the **NativeAOT** path, no JIT -- now with a reclaiming garbage
collector and exceptions.

**English** · [Bahasa Indonesia](csharp-userland.id.md) · ← [Documentation index](README.md)

## Build pipeline

```
hello.cs + bzexc.cs ──bflat build-il (zerolib)──► il/hello.dll
il/hello.dll + zerolib.dll ──ILCompiler 8.0.26 (NativeAOT)──► hello.o ─┐
                                                                       ├─rust-lld─► hello.elf
bzstart.rs ──rustc (x86_64-unknown-none / aarch64-unknown-none)──► bzstart.o ┘
```

- **`bflat build-il`** (`--stdlib:zero`) compiles C# to IL against zerolib, the
  tiny core library (no full .NET runtime). It runs the private bflat copy in
  `kernel/target/bflat-patched`, whose zerolib `tools/ZerolibPatch` extends
  (`ThrowOverflowException`, `System.IDisposable`, the shim's exports renamed).
- **The upstream ILCompiler 8.0.26** (`scripts/prepare-ilc.*`: the NuGet cache,
  else nuget.org into `kernel/target/ilc`, SHA-256 pinned) turns the IL into
  machine code. bflat bundles the same compiler but switches the unwind and
  exception-clause tables off for every standard library other than dotnet's,
  so `throw` could never work; the upstream one emits `.eh_frame`, an LSDA per
  method and the clause tables. Flags: `--systemmodule:zerolib --targetos:linux
  --Os --reflectiondata:none --directpinvoke:libSystem.Native` (without the last
  one zerolib's own imports are bound lazily and recurse).
- **bzstart.rs** is the startup shim + PAL: `_start` (calls `__managed__Main`),
  `SystemNative_Log/Malloc/Abort` over Buitenzorg syscalls, casts / interface
  dispatch / covariant array stores, threads, and now **`bzgc.rs`** (the
  collector behind `RhpNewFast` / `RhpNewArray`) and **`bzeh.rs`** (the
  exception dispatcher behind `RhpThrowEx` / `RhpRethrow`).
- **rust-lld** links them into a static ELF at `0x400000` (`user.ld` keeps
  `.eh_frame` and marks the data/bss range the collector scans).

Every program is compiled with **`bzexc.cs`** (System.Exception and the usual
exception types). Build: `scripts/build-hello-csharp.ps1` (or `.sh`; ARM64:
`build-hello-csharp-arm64.*`), the SDK's `desktop-csharp` template and
MagicAppGen use the same steps. Needs `tools/bflat`, Rust nightly and
(downloaded on first use) ILCompiler 8.0.26.

## Garbage collector

`bzgc.rs` is a **non-moving mark-sweep collector**. It reserves address space
(`MMAP` with no access) and commits 64 KiB blocks on first use; a small block
holds slots of one size class (16 B .. 8 KiB) with an allocated and a mark
bitmap, a larger object takes a run of whole blocks. A collection starts once
as much has been allocated as survived the previous one (at least 8 MiB), or
when blocks run out, or on `BzGC.Collect()`.

- **Roots, conservatively:** the program's data + bss (the GC static bases
  live there) and the stack of the collecting thread, with its callee-saved
  registers spilled first. Any word pointing into an allocated slot keeps it.
- **Objects:** a class instance is scanned word by word, an array only if its
  MethodTable says it holds references (`byte[]` / `int[]` never are).
- **Sweep:** unmarked slots are freed and zeroed when handed out again; empty
  blocks return to the pool.

**A bare address is not a reference.** An object whose address is stored
only outside the roots (an mmap page, kernel memory) is collected, with or
without threads: keep a managed reference to it for as long as it is used.

**Threads: stop the world at safepoints.** Every thread has a slot (stack
range, saved stack pointer, state). A thread parks itself at a *safepoint* --
every allocation (`RhpNewFast` / `RhpNewArray`) and every blocking call
(`bz_yield`, `bz_thread_join`, `bz_futex_wait`, `bz_proc_wait`,
`bz_mutex_lock`) -- through `bz_gc_safe`, which spills its callee-saved
registers onto its own stack first, so the collector can scan every parked
thread's stack from its saved pointer to its top. The collecting thread raises
a flag and waits (up to 50 ms) until every other running thread has parked;
threads that are blocked are parked already. **Limit:** a thread that
computes for longer than that without allocating or blocking makes the
collection give up (counted in `SkippedCount()`); the heap then grows until
the next try. The libc-style `malloc` family (used by the shim itself) is a
separate, never-collected heap.

`BzGC` (bzbcl2.cs): `Collect()`, `CollectionCount()`, `LiveBytes()`,
`FreedBytes()`, `SkippedCount()`, `LastPauseMicroseconds()`, `SetEnabled()`,
plus the old allocation figures. `gc.cs` allocates 128 MiB of garbage while
keeping linked lists and a large array alive, then lets two worker threads
allocate 24 MiB each while the main thread allocates too: collections run
with all three threads alive (`bz_gc_stopped_world()` >= 1) and every live
list is intact -> `MILESTONE: GC OK` (x86-64 and ARM64).

## Exceptions

`try` / `catch (T e)` / `catch ... when (filter)` / `finally` / `using` /
`throw;` work, including exceptions thrown inside catch and finally blocks,
in generic code, through delegates and on worker threads. `bzeh.rs` does what
the .NET runtime does in two passes: pass 1 walks up the stack (DWARF CFI from
`.eh_frame`) to the first clause that takes the exception -- a typed clause
whose type the object is an instance of, or a filter funclet that answers
true; pass 2 walks the same frames again, calls every finally / fault funclet
on the way and then the catch funclet, and resumes the method at the address
the funclet returns. Funclets run on their method's frame pointer (x86-64:
`rbp`, exception in `rsi`; ARM64: `x29`, exception in `x0`); they are called
through `bz_call_funclet`, which leaves a record above its return address so
an exception thrown inside a funclet continues in the method that owns it.
Clauses are matched against the call instruction (return address - 1): on
ARM64 a call can be the last instruction of a `try` block.

No handler: `Unhandled exception: <Message>` and the program ends (finally
blocks do not run then, as in .NET). An exception thrown inside a `when`
filter is not supported.

**Runtime checks throw.** `tools/ZerolibPatch` (patch version 8) moves
`System.Exception` and the runtime's exception types (`NullReferenceException`,
`IndexOutOfRangeException`, `DivideByZeroException`, `OverflowException`,
`InvalidCastException`, `ArrayTypeMismatchException`) into zerolib and makes
its throw helpers really throw, so a failed array bounds check, a failed cast
(`(T)x`), a bad covariant array store and checked overflow raise catchable
exceptions. Hardware faults do too: `_start` registers handlers with the
`FAULT_HANDLER` syscall (75), and the kernel redirects a load or store below
64 KiB (a null dereference) and an x86 integer division by zero into the
program as if the faulting instruction had called the handler, which throws
`NullReferenceException` / `DivideByZeroException` (ARM64 has no division
trap: ILC emits an explicit check there). Limit: on ARM64 a null fault in a
leaf function that keeps its return address only in the link register
unwinds from that function's caller. `exc.cs` checks 18 cases (5 of them
these) -> `MILESTONE: EXCEPTIONS OK` (x86-64 and ARM64).

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

## async / await (native)

Add `bzasync.cs` (with `bzbcl3.cs`) to a program and `async` / `await` work
under zerolib:

```csharp
static async Task<int> Fetch(int n) { await Task.Delay(10); return n * 2; }

static int Main()
{
    BzAsync.Run(async () =>
    {
        int a = await Fetch(1);
        await Task.WhenAll(Task.Delay(30), Task.Delay(10));
        BzDebug.WriteLine(a == 2 ? "ok" : "wrong");
    });
    return 0;
}
```

Everything runs on one thread, on the event loop `BzAsync.Run` pumps (the
JavaScript model): `Task.Delay` is a timer on it, `Task.Yield` lets other
work run, `Task.Run` queues work on it, `.Result` / `Wait()` pump it until the
task is done. An exception escaping an async method faults its task, and
`await`, `.Result` and `Wait()` throw it again (the exception itself, not an
AggregateException); an `async void` method's exception is dropped. For the
full .NET library
(LINQ, reflection), use the managed path (RustCLR,
[managed-apps.md](managed-apps.md)).

## What's next

On the native path: exceptions thrown from `when` filters, safepoints inside
long compute-only loops, and a
RISC-V build (bflat and ILC have no RISC-V target -- C# reaches RISC-V through
RustCLR). Full .NET (reflection, dynamic loading, the whole BCL) is the
managed RustCLR path.

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
