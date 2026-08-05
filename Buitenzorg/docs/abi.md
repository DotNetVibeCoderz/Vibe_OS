# Syscall ABI v1 — the Rust ↔ C# Contract

Source of truth: [`kernel/abi/src/lib.rs`](../kernel/abi/src/lib.rs) (`bz-abi`).
C# mirror: [`runtime/Buitenzorg.Runtime/Sys/`](../runtime/Buitenzorg.Runtime/Sys/).
Both are held in lockstep by identical contract tests (`cargo test -p bz-abi` ↔
`AbiContractTests.cs`).

**English** · [Bahasa Indonesia](abi.id.md) · ← [Documentation index](README.md)

## Rules (requirements.md §4)

1. **C ABI only** — `extern "C"` + P/Invoke (`[LibraryImport("bzsys")]`).
2. **Minimal marshalling** — only primitives, pointers, and `#[repr(C)]` /
   `[StructLayout(LayoutKind.Sequential)]` structs.
3. **Stable numbers** — append-only; never renumbered after release.
4. **Zero-copy** — large data (framebuffers, files, tensors) travels via shared memory.
5. **GC-aware pinning** — managed objects are `fixed`/pinned while Rust holds a pointer.

## The v1 syscall table

| # | Name | a0 | a1 | Result |
|---|---|---|---|---|
| 0 | `ABI_VERSION` | — | — | ABI version (currently `1`) |
| 1 | `DEBUG_WRITE` | ptr (u64) | len (u64) | bytes written |
| 2 | `EXIT` | exit code | — | does not return |
| 3 | `YIELD` | — | — | 0 |
| 4 | `TICKS` | — | — | timer ticks since boot (PIT ~18.2 Hz) |
| 5 | `FB_INFO` | ptr → `FramebufferInfo` | — | 0 = success |
| 6 | `WIN_CREATE` | title ptr | title len | window id (a2 = (w≪32)\|h) |
| 7 | `WIN_CMD` | window id | ptr → `DrawCmd` | 0 = success |
| 8 | `WIN_PRESENT` | window id | — | 0 (recompose the desktop) |
| 9 | `KEY_READ` | — | — | 1 char (0 if empty) |
| 10 | `PROC_LIST` | buffer ptr (ProcInfo[]) | max count | entries written |
| 11 | `PROC_KILL` | pid | — | 0 = success |
| 12 | `SYS_STAT` | ptr → `SysStat` | — | 0 = success |
| 13 | `MMAP` | size (u64) | prot (u64) | base VA (syserr in the high range on failure) |
| 14 | `MPROTECT` | addr (u64) | size (u64) | 0 = success (a2 = prot) |
| 15 | `MUNMAP` | addr (u64) | size (u64) | 0 = success |
| 16 | `THREAD_CREATE` | entry rip (u64) | arg (u64) | thread id (a2 = user stack top); syserr on failure |
| 17 | `THREAD_JOIN` | thread id (u64) | — | 0 once the thread finishes |
| 18 | `THREAD_EXIT` | exit code (u64) | — | does not return |
| 19 | `FUTEX_WAIT` | addr (u64) | expected (u64) | 0 (blocks while *addr==expected until woken) |
| 20 | `FUTEX_WAKE` | addr (u64) | count (u64) | threads woken |
| 21 | `THREAD_SELF` | — | — | the calling thread's id |
| 22 | `CLOCK_MONO` | — | — | monotonic counter (TSC cycles) |
| 23 | `AUDIO_STAT` | ptr → `AudioInfo` | — | 0 = success |
| 24 | `AUDIO_SET_VOLUME` | volume 0..=100 (u64) | — | 0 = success (non-zero un-mutes) |
| 25 | `AUDIO_TONE` | frequency Hz (u64) | duration ms (u64) | 0 = success (DMA, non-blocking) |
| 26 | `AUDIO_PLAY` | ptr to i16 stereo PCM | length in bytes (u64) | 0 = success |
| 27 | `PKG_LIST` | ptr `PkgInfo[]` | max count (u64) | entries written |
| 28 | `PKG_SET` | name ptr | name len (u64) | 0 = success (a2 = 1 install / 0 remove) |
| 29 | `FS_LIST` | path ptr (NUL-term) | ptr `FsEntry[]` | entries written (a2 = max; empty path = list mounts) |
| 30 | `FS_READ` | path ptr (NUL-term) | out buffer ptr | bytes read (a2 = max bytes; 0 = none) |
| 31 | `IS_INTERACTIVE` | — | — | 1 in an interactive session (desktop up), 0 during headless boot demos |
| 32 | `FS_WRITE` | path ptr (NUL-term) | source buffer ptr | bytes written (a2 = byte count; 0 = failed/read-only) |
| 33 | `CLOCK_RTC` | ptr `RtcTime` | — | 0 = success (year/month/day/hour/minute/second from the CMOS RTC) |
| 34 | `NET_SOCKET` | kind (0 = UDP) | — | socket handle (≥ 1), 0 = failure |
| 35 | `NET_BIND` | handle | port | 0 = success |
| 36 | `NET_SEND` | handle | ptr `NetDatagram` + payload | payload bytes sent (a2 = payload length) |
| 37 | `NET_RECV` | handle | ptr `NetDatagram` + room for payload | payload length; 0 = none (non-blocking, a2 = max) |
| 38 | `NET_CLOSE` | handle | — | 0 = success |
| 39 | `NET_INFO` | ptr `NetInfo` | — | 0 = success (address + state + counters) |

Errors are returned in the high `u64` range: `NOSYS = u64::MAX`,
`INVAL = u64::MAX - 1`.

## 🔒 Pointer security model (v1.0 hardening)

**Every pointer from ring 3 is untrusted.** Before this hardening, syscalls
copied through raw pointers as-is, so an unprivileged app could:

- `DEBUG_WRITE(kernel_addr, len)` → the kernel prints its own memory to serial
  (an **information leak**);
- `SYS_STAT` / `PROC_LIST` / `PKG_LIST` / `FS_READ` / `NET_RECV` with
  `out_ptr = kernel_addr` → the kernel **writes results into kernel memory**
  (an **arbitrary kernel write = full privilege escalation**);
- an unmapped pointer → the kernel **page-faults inside the syscall** and dies.

Now `memory::validate_user_range(ptr, len, need_write)` requires:

1. the range does not wrap and lies entirely **below `USER_ADDR_MAX` = 0x8000_0000**
   (every kernel mapping — the heap at `0x4444_4444_0000`, the physical-memory
   window, the kernel image — is above it);
2. **every page** in the range is present and `USER_ACCESSIBLE`;
3. for output buffers, the pages are also **writable** (`user_write`).

`validate_user_cstr` does the same for NUL-terminated paths (re-checking at each
page boundary). Validation applies only on the **ring-3** path
(`dispatch_from_user`, called from the SYSCALL entry); kernel-internal callers of
`dispatch` legitimately pass their own addresses and stay trusted.

Verified headlessly by `syscall::security_self_test()` — 14 hostile probes
(kernel addresses, unmapped pages, overflowing ranges, wrapping lengths, null)
must all be refused with `INVAL` → `MILESTONE: SECURITY OK`.

## 🧊 ABI v1 freeze (v1.0)

The v1 table is **frozen**: syscall numbers are append-only and struct layouts
may never change. The guard is mechanical — `abi_v1_is_frozen` (Rust) +
`AbiV1IsFrozen` (C#) pin `ABI_VERSION`, `COUNT`, the **size and alignment of
every struct**, and the error codes. Adding a syscall = append a constant, bump
`COUNT`, extend both tests. Changing an existing syscall/struct = a **new ABI
major version**, not an edit.

## Syscall groups

**BCL completion (pre-v1.0):** `FS_WRITE` completes `System.IO` (file write —
needs a writable mount, e.g. the FAT12 RAM disk `/ram`); `CLOCK_RTC` gives
`System.Globalization` / `BzDateTime` a real wall clock (`rtc.rs`, CMOS,
BCD/binary + 12/24 h, re-read until two samples agree so it can't tear); `NET_*`
gives `System.Net.Sockets` a **real UDP** socket over the loopback stack
(`net.rs`: Ethernet/ARP/IPv4/ICMP + UDP with a pseudo-header checksum). Shim
wrappers: `bz_fs_write`, `bz_clock_rtc`, `bz_net_socket`/`bind`/`send`/`recv`/`close`/`info`.

> **Honest limits:** only **UDP** exists. `sock_kind::STREAM` (TCP) is rejected
> with `INVAL`, so `System.Net.Http` (`BzHttp`) is only a message layer (build a
> request / parse a response), not a client — it becomes one the moment TCP
> lands. The device is also still **loopback** (no NIC driver yet, e1000 to
> follow), so datagrams reach this machine only.

**Package manager (v0.16 App Store):** `PKG_LIST` returns the registry catalog
(`pkg.rs`) + install state; `PKG_SET` installs/removes a package by name (gating
`run`). Shim: `bz_pkg_list` / `bz_pkg_set`.

**File I/O (v0.16):** `FS_LIST` browses a VFS directory; `FS_READ` reads a file's
bytes into a client buffer (e.g. the Image Viewer loads `PHOTO.BMP`, the editor
opens a file). Shim: `bz_fs_list` / `bz_fs_read`.

**Audio (v0.16):** the AC'97 driver (`audio.rs`) — PCI enumeration (class
0x04/0x01), codec cold-reset, the mixer (master + PCM-out volume), and 16-bit
stereo 48 kHz PCM playback over bus-master DMA (a buffer-descriptor list).
`AUDIO_TONE` generates a sine in the kernel; `AUDIO_PLAY` copies client PCM into
the DMA buffer. Shim: `bz_audio_stat`/`set_volume`/`tone`/`play`; the
`Buitenzorg.Audio` library (`Mixer`/`Tone`) sits on top.

**Sync/TLS/clock (v0.15 increment 3):** `FUTEX_WAIT`/`FUTEX_WAKE` add a scheduler
**Blocked** state (a thread truly blocks rather than busy-yielding) — the
foundation for mutexes/condvars. The shim builds `bz_mutex_lock`/`unlock` on top.
`THREAD_SELF` is the `pthread_self`/TLS foundation. `CLOCK_MONO` is the TSC (the
PAL pairs it with a frequency for `Stopwatch`/`GetTimestamp`).

**Threading (v0.15 increment 2, cooperative):** `THREAD_CREATE` runs `entry(arg)`
in ring 3 on stack `a2`, sharing the address space; threads are scheduled
cooperatively (they yield via `YIELD`). Each thread has its own SYSCALL kernel
stack (separate from the TSS interrupt stack). Shim:
`bz_thread_create`/`join`/`exit`/`bz_yield`.

> **Register note (important):** a syscall clobbers `rcx` + `r11` (the `syscall`
> instruction) **and** `r8`/`r9`/`r10` plus other caller-saved registers (kernel
> entry marshalling + the C dispatcher). A user-side inline-asm syscall must
> declare `r8`/`r9`/`r10` (and the argument registers) clobbered, or a value
> kept there can be corrupted across the call.

**Memory PAL (v0.15).** `prot` (`mmap_prot` flags, OR them): `NONE=0`, `READ=1`,
`WRITE=2`, `EXEC=4`. `MMAP` maps `ceil(size/4096)` anonymous pages in the user
mmap arena (0x2000_0000..0x6000_0000), reset per process. This is the memory
foundation the .NET runtime/GC uses for the managed heap.

**Reserve/commit (increment 5):** `MMAP` with `prot=NONE` only **reserves**
address space (no frames) — the pattern the .NET GC uses to pre-book a large
heap. `MPROTECT` with access (READ/WRITE) **commits on demand**: an unmapped page
gets a fresh zeroed frame; an already-mapped page is only re-flagged. So a 256
MiB reservation does not consume physical RAM until it is actually touched.

## Shared structs

`FramebufferInfo` — `#[repr(C)]`, 7 × u64 = **56 bytes**:
`address, size, width, height, stride, bytes_per_pixel, pixel_format`.
Pixel format: `0 = RGB`, `1 = BGR`, `2 = GRAY`, `255 = UNKNOWN`.

`DrawCmd` (v0.8) — `#[repr(C)]`, **48 bytes**:
`op:u64, x,y,w,h:i32, color:u32, _pad:u32, text_ptr:u64, text_len:u64`.
Op: `0 = fill_rect`, `1 = draw_text`, `2 = clear`, `3 = line`, `4 = ellipse`,
`5 = fill_ellipse`, `6 = rect` (v0.9), `7 = blit` (v0.16). Color is `0x00RRGGBB`.
**`blit`**: `text_ptr` is the client's ARGB pixel buffer (`w`×`h` `u32`,
`text_len` bytes), copied to the window canvas at (x,y). This backs the
client-side `Buitenzorg.Drawing` software renderer (the WPF/Avalonia compositor
model).

`ProcInfo` (v0.9) — `#[repr(C)]`, **64 bytes**:
`pid:u64, state:u64, cpu_ticks:u64, kind:u64, name:[u8;32]`.
State: `0=runnable, 1=running, 2=finished`. Kind: `0=kernel task, 1=user app`.

`SysStat` (v0.9) — `#[repr(C)]`, **48 bytes**:
`uptime_ticks, tick_hz, heap_used, heap_total, task_count, mem_total_mib` (all u64).

`AudioInfo` (v0.16) — `#[repr(C)]`, **48 bytes**:
`present, sample_rate, channels, bits, volume, muted` (all u64). `present`/`muted`
= 0/1; `volume` = 0..=100.

`PkgInfo` (v0.16) — `#[repr(C)]`, **48 bytes**:
`name:[u8;24], category:[u8;16], installed:u64`. Name/category are null-padded ASCII.

`FsEntry` (v0.16) — `#[repr(C)]`, **32 bytes**:
`name:[u8;24], is_dir:u64`. Name is null-padded ASCII; `is_dir=1` for a mount/directory.

`RtcTime` (pre-v1.0) — `#[repr(C)]`, **48 bytes**: `year, month, day, hour, minute, second` (all u64).

`NetDatagram` (pre-v1.0) — `#[repr(C)]`, **16 bytes**: `addr:[u8;4], port:u32, length:u64`; the payload follows immediately.

`NetInfo` (pre-v1.0) — `#[repr(C)]`, **48 bytes**: `addr:[u8;8], up, tx_datagrams, rx_datagrams, icmp_replies, arp_replies`.

## Implementation status

- **Kernel** (`bzkernel/src/syscall.rs`): a complete dispatcher for the v1 table,
  reached from ring 3 via the SYSCALL/SYSRET entry (`usermode.rs`) and, for boot
  self-tests, directly from kernel context. Ring-3 pointer arguments are
  validated (see the security model above).
- **C#** (`BzSys`): a uniform facade — `NativeSyscalls` (P/Invoke `bzsys`, used
  when running on Buitenzorg) or `HostSyscalls` (a host simulation for dev/test).

## Adding a new syscall

1. Add a constant to `bz-abi` (`sysno`) **at the end of the table**, bump `COUNT`.
2. Mirror it in `SyscallNumbers.cs`.
3. Implement it in `bzkernel/src/syscall.rs` + (if needed) `HostSyscalls`.
4. Update the contract test **on both sides**, the freeze test, and the table
   above.

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
