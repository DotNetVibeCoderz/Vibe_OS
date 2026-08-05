# System Services (v0.5 "Dahan")

The v0.5 milestone: **"a C# service runs as a process; benchmark-able async
I/O"** (requirements.md §16). Everything runs in the kernel and is verified on
each boot via `MILESTONE: …` markers the smoke test also checks.

**English** · [Bahasa Indonesia](system-services.id.md) · ← [Documentation index](README.md)

## VFS + FAT read/write (`vfs.rs`, `fat.rs`, `ramdisk.rs`)

- **Mount table**: `vfs::mount(name, device, read_only)`. Paths are
  `/<mount>/<FILE>`. At boot: `/disk` (the boot disk, read-only) and `/ram` (a
  ramdisk, read-write).
- **FAT write** (`fat::write_file`, FAT12): allocate a chain of free clusters,
  write the data, link the FAT (both copies), and create/overwrite the 8.3 root
  directory entry.
- **RAM disk** (`ramdisk.rs`) is formatted in-kernel (`fat::format_fat12`) then
  mounted at `/ram`. The demo writes `/ram/DAHAN.TXT` and reads it back (verified).

## Service / init manager (`service.rs`)

`register(name, deps, entry)`, then `start_all()`: a **parallel,
dependency-aware** startup on top of the scheduler — each service becomes a task
and only runs once all its dependencies are at least `Running`. Demo:
`logger → {netd, storaged} → app`, with the startup order verified.

> Implementation note: `spin::Mutex` is not reentrant. `start_all` takes a
> *snapshot* of the state under one lock, then decides readiness without locking
> again (avoiding a lock-inside-a-lock deadlock).

## Async I/O, io_uring-style (`aio.rs`)

A submission queue (SQ) + completion queue (CQ). The submitter pushes an SQE
(`Nop`, `ReadBlock`); a worker task drains the SQ, performs the I/O against the
block device, and pushes a CQE with the matching `user_data`. `benchmark(count)`
measures ops per timer tick (PIT ~18.2 Hz) → **ops/second**. "Benchmark-able" as
the milestone requires.

## Early networking (`net.rs`)

A minimal **Ethernet + ARP + IPv4 + ICMP** stack over a `NetDevice` trait, driven
via **loopback**. Demo: send an ICMP echo request to our own IP → the stack
processes it (ARP self, IP, ICMP) → builds an echo reply → the round-trip is
verified (the reply counter increases). The same trait will later back an e1000
driver (a hardware NIC — later roadmap work).

## A C# service as a process (ring 3)

The init manager launches `SVC.ELF` (a second C# program,
`userland/hello-csharp/svc.cs`) as a ring-3 process via `run_user_elf` (load the
ELF → `enter_user` → unmap on exit). It prints via syscalls and exits cleanly.

> **SSE is required**: NativeAOT-generated code uses the xmm registers (e.g.
> `xorps` in `Console.WriteLine(int)`). The kernel enables SSE/SSE2
> (`gdt::enable_sse`: CR0.EM=0, CR0.MP=1, CR4.OSFXSR+OSXMMEXCPT) before running
> managed code — without it an xmm instruction triggers #UD → a double fault.

## Re-running user programs

`run_user_elf` may only be called when the **only runnable task is the boot
task** (so preemption is inert while in ring 3, per the v0.4 model). That's why
`dahan_demo` drains any finishing tasks (`yield_now`) before launching the C#
service. The user address is fixed (`0x400000`, stack `0x7000_0000`), so the
previous program must be unmapped first — `run_user_elf` does that.

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
