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
| 40 | `PROC_SPAWN` | ptr path (NUL-terminated app name or VFS path) | ptr args (NUL-terminated, 0 = none) | pid of the new process |
| 41 | `PROC_WAIT` | pid | — | exit code (blocks until exit); `INVAL` if unknown |
| 42 | `PROC_ARGS` | ptr buffer | max bytes | full length of the caller's argument string |
| 43 | `FUTEX_WAIT_TIMEOUT` | ptr u32 word | expected value | 0 woken / word changed, 1 timed out (a2 = timeout in ns, 0 = none) |
| 44 | `CLOCK_NANOS` | — | — | monotonic nanoseconds since boot |
| 45 | `WIN_EVENT` | window id | — | next pointer event, 0 = none (packed: x bits 0-15, y 16-31, buttons 32-39, wheel notches 40-55 (signed, kind 5 only), kind 56-63: 1 move, 2 down, 3 up, 4 leave, 5 wheel -- positive = scroll up) |
| 46 | `TEXT_METRICS` | style (0 body, 1 strong, 2 heading) | — | `(glyph width << 32) \| line height` |
| 47 | `HW_BOARD` | `hw_query` code | argument (clock / voltage id) | the value; `NOSYS` without board I/O (Raspberry Pi 4 only) |
| 48 | `HW_GPIO` | `hw_gpio` operation | pin | 0, the level (READ) or the mode (GET_MODE); a2 = value |
| 49 | `HW_I2C` | ptr `HwTransfer` | — | `hw_status` (0 ok, 1 no acknowledge, 2 clock stretch, 3 timeout) |
| 50 | `HW_SPI` | ptr `HwTransfer` | — | `hw_status` |
| 51 | `HW_UART` | `hw_uart` operation (0 open, 1 write, 2 read, 3 close) | ptr `HwTransfer` | bytes written / read (read does not block), 0 for open / close |
| 52 | `HW_LED` | 0 activity (green) / 1 power (red) | 1 on / 0 off | 0 |
| 53 | `SETTING` | `setting_op` (0 get, 1 set, 2 count, 3 name, 4 description) | `setting_key` (\| index << 16 for name / description) | value / count / text length; 0 after set (a2 = value or 64-byte buffer) |
| 54 | `FS_OP` | `fs_op` (0 mkdir, 1 remove, 2 rename/move, 3 stat) | path (NUL-terminated, absolute, folders allowed) | 0 or `INVAL`; a2 = second path (rename) or, for stat, a pointer to 4 u64: size, is-folder, FAT date << 16 \| time, 0 |
| 55 | `USER_OP` | `user_op` (0 list, 1 verify, 2 add, 3 remove, 4 passwd, 5 reset, 6 avatar) | request: NUL-terminated `\n`-separated fields (see `user_op`) | 0 or `INVAL`; list writes `name\tadmin\tavatar\n` lines into a2 (2048 bytes) and returns their length. Changing accounts needs an administrator's password (own password: the current one); passwords are PBKDF2-HMAC-SHA256 with salt in `/disk/USERS.CFG` |
| 56 | `GFX3D` | pointer to a `Gfx3dCmd` | — | new resource id (CREATE / MESH / TEXTURE), `pixels << 32 \| triangles` (DRAW), a pixel (READ_PIXEL), 0, or `INVAL`. The 3D API (G3): see **3D (GFX3D)** below |

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

**SMP:** processes *and* the threads of one process run in parallel on all
CPUs. Memory shared between threads must be accessed with the shim's atomics
(`bz_atomic_add`, `bz_atomic_add32`, `bz_atomic_cas32`) or under the futex
mutex (`bz_mutex_lock` / `BzMutex`); the runtime heap (`new`, arrays) is
thread-safe. `TICKS` and
`CLOCK_MONO` are answered without taking any kernel lock. Every other syscall
runs on the CPU that issued it: memory (`MMAP`/`MPROTECT`/`MUNMAP`), scheduler
(`YIELD`, `THREAD_SELF`, `FUTEX_*`), console (`DEBUG_WRITE`), window system
(`WIN_*`, `FB_INFO`, `KEY_READ`), files (`FS_*`), process registry and control
(`PROC_LIST`, `PROC_ARGS`, `PROC_SPAWN`, `PROC_WAIT`, `SYS_STAT`), clock
(`CLOCK_RTC`), network (`NET_*`), packages (`PKG_*`) and audio (`AUDIO_*`)
syscalls take only their own subsystem's lock and run in parallel across CPUs;
`EXIT`, `THREAD_EXIT`, `PROC_KILL`, `THREAD_CREATE` and `THREAD_JOIN` run under
the kernel's Big Kernel Lock.

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
`5 = fill_ellipse`, `6 = rect` (v0.9), `7 = blit` (v0.16), and the toolkit ops:
`8 = fill_round_rect` (radius in `_pad`), `9 = round_rect` (radius in `_pad`),
`10 = gradient_v` (`color` top, `_pad` bottom), `11 = fill_round_gradient_v`
(`color` top, `_pad` bottom, radius in `text_len`, `text_ptr` = 0),
`12 = draw_text_styled` (`_pad`: 0 body, 1 strong, 2 heading),
`13 = blend_round_rect` (`_pad` = alpha << 16 | radius). Color is `0x00RRGGBB`.
`WIN_CREATE`'s `a2` (dims) takes the flag `win_flags::CLIENT_SIZE` (bit 63): the
width and height are then the client area, and the window grows by its title
bar and borders.
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

`HwTransfer` (v1.x, board I/O) — `#[repr(C)]`, **48 bytes**: `bus:u32, addr:u32,
speed_hz:u32, mode:u32, tx_ptr:u64, tx_len:u64, rx_ptr:u64, rx_len:u64`. `bus` =
I2C/SPI bus or serial port; `addr` = I2C 7-bit address or SPI chip select;
`speed_hz` 0 = the bus default (I2C 100 kHz, SPI 1 MHz) or the baud rate for
UART OPEN; `mode` = SPI mode 0-3. I2C writes `tx` then reads `rx`; SPI clocks
`max(tx_len, rx_len)` bytes full-duplex.

**Settings keys** (`SETTING`): `0 THEME` (index into the theme list), `1 WALLPAPER`
(index of an official wallpaper), `2 SCREENSAVER` (index; the last choice is
"off"), `3 SCREENSAVER_DELAY` (idle seconds, 5-3600), `4 ANIMATIONS` (0/1),
`5 LARGE_CURSOR` (0/1), `6 TIMEZONE` (index; NAME = "WIB", DESCRIPTION =
places + UTC offset), `7 LANGUAGE` (0 Bahasa Indonesia, 1 English; NAME =
language name, DESCRIPTION = code), `8 CLOCK_24H` (0/1), `9 CLOCK` (local
date-time as seconds since 1970-01-01 local midnight; GET reads, SET sets the
clock, 2000-2099). A SET recomposes the desktop at once. Region values persist
in `/disk/REGION.CFG`; the RTC is read as UTC on every architecture.

**Board I/O codes.** `hw_query`: `0 BOARD` (1 = Raspberry Pi 4), `1 TEMPERATURE`
(m°C), `2 TEMPERATURE_MAX` (m°C), `3 CLOCK` (Hz; a1 = firmware clock id: 3 ARM,
4 core), `4 VOLTAGE` (µV; a1 = 1 core, 2-4 SDRAM), `5 THROTTLED` (bit 0
under-voltage, 1 frequency capped, 2 throttled, 3 soft temperature limit; +16 =
"has occurred"), `6 REVISION`, `7 SERIAL`, `8 MEMORY` (bytes), `9 GPIO_COUNT`.
`hw_gpio`: `0 MODE` (0 input, 1 output, 2+n ALTn), `1 WRITE`, `2 READ`, `3 PULL`
(0 none, 1 up, 2 down), `4 GET_MODE`. Applications may use the header pins
GPIO 0-27 except 14/15 (the serial console) and GPIO 42 (the activity LED); others
are refused with `INVAL`. Serial ports 2-5 are the Pi 4's extra PL011 UARTs (TX/RX
on GPIO 0/1, 4/5, 8/9, 12/13).

**3D (GFX3D, G3).** One syscall, `a0` = a `Gfx3dCmd` — `#[repr(C)]`, **56
bytes**: `op:u32, target:u32, a:u32, b:u32, c:u32, d:u32, ptr:u64, len:u64,
ptr2:u64, len2:u64`. `gfx3d_op`:

| op | name | parameters | result |
|---|---|---|---|
| 0 | `CREATE` | `a` width, `b` height (1..=1024) | context id (color + 24-bit depth buffer) |
| 1 | `FREE` | `target` = context, mesh or texture | 0 |
| 2 | `CLEAR` | `target`, `a` = 0xRRGGBB, `b` = `gfx3d_clear` (1 color, 2 depth) | 0 |
| 3 | `MESH` | `a` vertex count, `ptr` = `Vertex3D[a]`, `b` index count (multiple of 3), `ptr2` = `u32[b]` | mesh id |
| 4 | `TEXTURE` | `a` width, `b` height (1..=1024), `ptr` = 0xAARRGGBB `u32[a*b]`, top row first | texture id |
| 5 | `LIGHTS` | `target`, `ptr` = `f32` block (`gfx3d_light`), `len` bytes | 0 |
| 6 | `DRAW` | `target`, `a` mesh, `b` texture (0 none), `c` = `gfx3d_flags`, `d` = material 0xAARRGGBB, `ptr` = 32 `f32`: model then view-projection, row-major, column vectors | `pixels << 32 \| triangles` |
| 7 | `PRESENT` | `target`, `a` window, `b`/`c` = client x/y (`i32`) | 0 (WIN_PRESENT then shows it) |
| 8 | `READ_PIXEL` | `target`, `a` x, `b` y | 0x00RRGGBB |
| 9 | `READ` | `target`, `ptr` = `u32[w*h]` out, `len` bytes | 0 |

`Vertex3D` — `#[repr(C)]`, **40 bytes**, 4-byte aligned (an array element):
`x, y, z, nx, ny, nz, u, v: f32, color: u32` (0xAARRGGBB, multiplied with the
material color), `_reserved: u32`. Texture coordinates repeat outside 0..1; `v = 0` is
the top row. Clip space is OpenGL's (z in -w..w); counter-clockwise triangles
face the viewer and back faces are culled unless `DOUBLE_SIDED`.

`gfx3d_flags`: shading `0 UNLIT`, `1 LAMBERT`, `2 PHONG` (mask 3; per-vertex
lighting, Gouraud), `4 DOUBLE_SIDED`, `8 WIREFRAME`, `16 NO_DEPTH_TEST`,
`32 NO_DEPTH_WRITE`, `64 BLEND` (material alpha), bits 8-15 specular exponent
(0 = 32), bits 16-23 specular strength (0 = 128).

`gfx3d_light` block (`f32` indices): `0..3` ambient rgb (0..1), `3..6` eye
position, `6..9` fog color, `9` fog start, `10` fog end (off unless end > start;
distances from the eye), `11` light count (<= 8), then from `12` one block of 8
per light: kind (0 directional, 1 point), x, y, z (directional: the direction
the light travels; point: its position), r, g, b (color x intensity), range
(point: fade-out distance, 0 = never).

Every resource belongs to the calling process and is freed when it exits; all of
them together are capped at 8 MiB (`gfx3d::BUDGET`). The kernel converts the
`f32`s to 16.16 fixed point without floating point (the kernels are soft-float)
and rasterizes in integers.

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
