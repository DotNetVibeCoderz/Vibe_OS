//! # bz-abi — Buitenzorg OS syscall ABI v1
//!
//! This crate is the **single source of truth** for the Rust ↔ C# boundary
//! (requirements.md §4). The C# mirror lives in
//! `runtime/Buitenzorg.Runtime/Sys/` and MUST stay byte-for-byte compatible:
//!
//! * every constant here ↔ `SyscallNumbers.cs`
//! * every `#[repr(C)]` struct here ↔ a `[StructLayout(LayoutKind.Sequential)]`
//!   struct with identical field order and sizes
//!
//! Rules (requirements.md §4):
//! 1. C ABI only (`extern "C"`).
//! 2. Only primitives, pointers and `#[repr(C)]` structs cross the boundary.
//! 3. Syscall numbers are stable once released — never renumber, only append.
//! 4. Large data travels via shared memory, not by-value marshalling.
#![no_std]
#![deny(missing_docs)]

/// ABI major version. Bumped only on a breaking change of this contract.
pub const ABI_VERSION: u64 = 1;

/// Syscall numbers, stable v1 table. Append-only; never renumber.
pub mod sysno {
    /// Query the ABI version implemented by the kernel. Returns [`crate::ABI_VERSION`].
    pub const ABI_VERSION: u64 = 0;
    /// Write bytes to the kernel debug console (serial + screen).
    /// args: `a0 = ptr (u64)`, `a1 = len (u64)`. Returns bytes written.
    pub const DEBUG_WRITE: u64 = 1;
    /// Terminate the calling task. args: `a0 = exit code`.
    pub const EXIT: u64 = 2;
    /// Cooperatively yield the CPU to the scheduler.
    pub const YIELD: u64 = 3;
    /// Monotonic timer ticks since boot. Returns tick count.
    pub const TICKS: u64 = 4;
    /// Fill a [`crate::FramebufferInfo`] pointed to by `a0`. Returns 0 on success.
    pub const FB_INFO: u64 = 5;
    /// Create a window: `a0 = title ptr`, `a1 = title len`, `a2 = (w << 32) | h`.
    /// Returns the window id (0 on failure).
    pub const WIN_CREATE: u64 = 6;
    /// Execute a [`crate::DrawCmd`]: `a0 = window id`, `a1 = ptr to DrawCmd`.
    /// Returns 0 on success.
    pub const WIN_CMD: u64 = 7;
    /// Recompose the desktop so the window's canvas becomes visible.
    /// `a0 = window id`. Returns 0.
    pub const WIN_PRESENT: u64 = 8;
    /// Pop one keyboard character (Unicode scalar). Returns 0 when empty.
    pub const KEY_READ: u64 = 9;
    /// Fill an array of [`crate::ProcInfo`]: `a0 = buffer ptr`, `a1 = max count`.
    /// Returns the number of entries written.
    pub const PROC_LIST: u64 = 10;
    /// Terminate a task/process by id: `a0 = pid`. Returns 0 on success.
    pub const PROC_KILL: u64 = 11;
    /// Fill a [`crate::SysStat`] pointed to by `a0`. Returns 0 on success.
    pub const SYS_STAT: u64 = 12;
    /// Map anonymous user pages (v0.15 "Matang", managed-runtime PAL). Rounds
    /// `a0 = size` up to whole pages and maps them into the caller's address
    /// space with protection `a1` (see [`crate::mmap_prot`]). Returns the base
    /// virtual address, or a [`syserr`] value in the high range on failure.
    pub const MMAP: u64 = 13;
    /// Change protection on a mapped user range: `a0 = addr`, `a1 = size`,
    /// `a2 = prot` (see [`crate::mmap_prot`]). Returns 0 on success.
    pub const MPROTECT: u64 = 14;
    /// Unmap a user range: `a0 = addr`, `a1 = size`. Returns 0 on success.
    pub const MUNMAP: u64 = 15;
    /// Spawn a ring-3 thread in the current process (v0.15 "Matang" increment 2,
    /// managed-runtime PAL threading). `a0 = entry rip`, `a1 = arg (passed in
    /// rdi)`, `a2 = user stack top`. Returns a thread id (>= 1), or a [`syserr`]
    /// value on failure. Threads share the address space and are scheduled
    /// cooperatively (they yield via [`YIELD`]).
    pub const THREAD_CREATE: u64 = 16;
    /// Wait for a thread to finish: `a0 = thread id`. Returns 0 once it exited.
    pub const THREAD_JOIN: u64 = 17;
    /// Terminate the calling thread: `a0 = exit code`. Does not return.
    pub const THREAD_EXIT: u64 = 18;
    /// Futex wait (v0.15 "Matang" increment 3, thread sync PAL): if the u32 at
    /// `a0` still equals `a1`, block the calling thread until woken by
    /// [`FUTEX_WAKE`] on the same address. Returns 0.
    pub const FUTEX_WAIT: u64 = 19;
    /// Futex wake: make up to `a1` threads blocked on address `a0` runnable.
    /// Returns the number woken.
    pub const FUTEX_WAKE: u64 = 20;
    /// Return the calling thread's id (TLS/`pthread_self` foundation).
    pub const THREAD_SELF: u64 = 21;
    /// Monotonic high-resolution counter (CPU timestamp counter). Returns a
    /// non-decreasing cycle count (the PAL pairs it with a frequency).
    pub const CLOCK_MONO: u64 = 22;
    /// Fill an [`crate::AudioInfo`] pointed to by `a0` (v0.16 "Panen" audio
    /// subsystem). Returns 0 on success.
    pub const AUDIO_STAT: u64 = 23;
    /// Set the master output volume: `a0 = 0..=100` percent. Returns 0 on
    /// success. Also un-mutes when the volume is non-zero.
    pub const AUDIO_SET_VOLUME: u64 = 24;
    /// Play a generated sine tone on the speaker: `a0 = frequency (Hz)`,
    /// `a1 = duration (ms)`. Returns 0 on success. Non-blocking (DMA-backed).
    pub const AUDIO_TONE: u64 = 25;
    /// Play a buffer of 16-bit signed stereo PCM at 48 kHz: `a0 = samples ptr`
    /// (interleaved L/R `i16`), `a1 = length in bytes`. Copies into the kernel
    /// DMA buffer and starts playback. Returns 0 on success.
    pub const AUDIO_PLAY: u64 = 26;
    /// Fill an array of [`crate::PkgInfo`] from the package registry (v0.16
    /// "Panen" App Store): `a0 = buffer ptr`, `a1 = max count`. Returns the
    /// number of entries written.
    pub const PKG_LIST: u64 = 27;
    /// Install or remove a package by name: `a0 = name ptr`, `a1 = name len`,
    /// `a2 = action` (1 = install, 0 = remove). Returns 0 on success.
    pub const PKG_SET: u64 = 28;
    /// List a VFS directory (v0.16 "Panen" File Manager): `a0 = NUL-terminated
    /// path ptr` (an empty string lists the mounts), `a1 = buffer ptr` to an
    /// array of [`crate::FsEntry`], `a2 = max count`. Returns the number of
    /// entries written.
    pub const FS_LIST: u64 = 29;
    /// Read a file's bytes from the VFS (v0.16 "Panen" Image Viewer / file
    /// open): `a0 = NUL-terminated path ptr`, `a1 = out buffer ptr`,
    /// `a2 = max bytes`. Copies up to `a2` bytes of the file into the buffer
    /// and returns the number of bytes read (0 if the file is missing/empty).
    pub const FS_READ: u64 = 30;
    /// Whether the app is running in an interactive session (v0.16 "Panen"):
    /// returns 1 once the desktop/terminal is live (a shell-launched app can
    /// read the keyboard), 0 during the headless boot-demo runs. Interactive
    /// apps use it to decide whether to enter their live keyboard loop or exit
    /// immediately after their scripted demo. No arguments.
    pub const IS_INTERACTIVE: u64 = 31;
    /// Write a file's bytes to the VFS (pre-v1.0 `Buitenzorg.Bcl` `System.IO`):
    /// `a0 = NUL-terminated path ptr`, `a1 = source buffer ptr`,
    /// `a2 = byte count`. Creates or truncates the file. Returns the number of
    /// bytes written (0 if the mount is read-only or the write failed).
    pub const FS_WRITE: u64 = 32;
    /// Read the real-time clock (pre-v1.0 `System.Globalization` /
    /// `BzDateTime`): `a0 = out ptr` to a [`crate::RtcTime`]. Returns 0 on
    /// success. The value comes from the CMOS RTC, in local time as the
    /// firmware reports it.
    pub const CLOCK_RTC: u64 = 33;
    /// Create a network socket (pre-v1.0 `System.Net.Sockets`): `a0 = kind`
    /// (see [`sock_kind`]). Returns a socket handle (>= 1), or 0 on failure.
    pub const NET_SOCKET: u64 = 34;
    /// Bind a socket to a local port: `a0 = handle`, `a1 = port`. Returns 0 on
    /// success. Binding is required before [`NET_RECV`] delivers anything.
    pub const NET_BIND: u64 = 35;
    /// Send a datagram: `a0 = handle`, `a1 = buffer ptr` laid out as a
    /// [`crate::NetDatagram`] header immediately followed by the payload,
    /// `a2 = payload length`. Returns the number of payload bytes sent.
    pub const NET_SEND: u64 = 36;
    /// Receive a datagram: `a0 = handle`, `a1 = buffer ptr` (a
    /// [`crate::NetDatagram`] header followed by room for the payload),
    /// `a2 = maximum payload bytes`. Returns the payload length, or 0 when no
    /// datagram is queued (non-blocking).
    pub const NET_RECV: u64 = 37;
    /// Close a socket: `a0 = handle`. Returns 0 on success.
    pub const NET_CLOSE: u64 = 38;
    /// Interface information and counters: `a0 = out ptr` to a
    /// [`crate::NetInfo`]. Returns 0 on success.
    pub const NET_INFO: u64 = 39;
    /// Start a program as its own process: `a0 = path` (NUL-terminated; an
    /// app name like `"calc"` or a VFS path like `"/disk/X.ELF"`), `a1 =
    /// arguments` (NUL-terminated, may be empty; 0 = none). Returns the pid.
    pub const PROC_SPAWN: u64 = 40;
    /// Block until process `a0 = pid` exits. Returns its exit code, or
    /// `syserr::INVAL` if no such process ever ran (or it was forgotten).
    pub const PROC_WAIT: u64 = 41;
    /// Copy the calling process's argument string into `a0 = buffer` (at most
    /// `a1 = max` bytes, NUL-terminated if it fits). Returns its full length.
    pub const PROC_ARGS: u64 = 42;
    /// FUTEX_WAIT with a time limit (the managed runtime's `Monitor.Wait(ms)`,
    /// `Thread.Sleep`, timed condition variables): block while the 32-bit word
    /// at `a0` holds `a1`, for at most `a2` nanoseconds (0 = no limit).
    /// Returns 0 when woken or the word changed, 1 when the time ran out.
    pub const FUTEX_WAIT_TIMEOUT: u64 = 43;
    /// Monotonic nanoseconds since boot (CLOCK_MONO is raw counter ticks of
    /// an unspecified frequency). No arguments.
    pub const CLOCK_NANOS: u64 = 44;
    /// Next pointer event of a window the caller owns: `a0 = window id`.
    /// Returns 0 when none is queued, else a packed event (see
    /// [`crate::pointer_event`]). Events arrive while the pointer is over the
    /// window's client area, and a press is captured until its release.
    pub const WIN_EVENT: u64 = 45;
    /// Metrics of a text style (`a0` = 0 body, 1 strong, 2 heading):
    /// returns `(glyph width << 32) | line height`, in pixels (the font is
    /// monospaced).
    pub const TEXT_METRICS: u64 = 46;
    /// Board information and built-in sensors (Raspberry Pi 4): `a0` = a
    /// [`crate::hw_query`] code, `a1` = its argument (a clock or voltage id).
    /// Returns the value; `syserr::NOSYS` on a machine without board I/O,
    /// `syserr::INVAL` for an unknown query or when the firmware did not answer.
    pub const HW_BOARD: u64 = 47;
    /// GPIO: `a0` = a [`crate::hw_gpio`] operation, `a1` = pin, `a2` = value.
    /// Returns 0 (MODE / WRITE / PULL), the level (READ) or the mode (GET_MODE);
    /// `syserr::INVAL` for a pin applications may not use.
    pub const HW_GPIO: u64 = 48;
    /// I2C transfer: `a0` = ptr to a [`crate::HwTransfer`] (`bus`, `addr` =
    /// 7-bit address, `speed_hz` 0 = 100 kHz; write `tx`, then read `rx`).
    /// Returns a [`crate::hw_status`] code.
    pub const HW_I2C: u64 = 49;
    /// SPI full-duplex transfer: `a0` = ptr to a [`crate::HwTransfer`] (`bus`,
    /// `addr` = chip select, `speed_hz`, `mode` 0-3; `max(tx_len, rx_len)`
    /// bytes are clocked, zeros sent past `tx`). Returns a [`crate::hw_status`].
    pub const HW_SPI: u64 = 50;
    /// Serial port: `a0` = a [`crate::hw_uart`] operation, `a1` = ptr to a
    /// [`crate::HwTransfer`] (`bus` = port, `speed_hz` = baud for OPEN, `tx`
    /// for WRITE, `rx` for READ). Returns bytes written / read (READ does not
    /// block), 0 for OPEN / CLOSE.
    pub const HW_UART: u64 = 51;
    /// Built-in LEDs: `a0` = 0 activity (green) / 1 power (red), `a1` = 1 on,
    /// 0 off. Returns 0.
    pub const HW_LED: u64 = 52;
    /// Desktop settings: `a0` = a [`crate::setting_op`], `a1` = a
    /// [`crate::setting_key`] (for NAME / DESCRIPTION: `key | index << 16`),
    /// `a2` = the value (SET) or a 64-byte buffer (NAME / DESCRIPTION).
    /// Returns the value / count / text length, 0 after SET, `syserr::INVAL`.
    pub const SETTING: u64 = 53;
    /// File and folder operations: `a0` = a [`crate::fs_op`], `a1` = a path
    /// (NUL-terminated, absolute: `/disk/APPS/A.TXT`), `a2` = the second path
    /// (RENAME) or, for STAT, a pointer to 4 `u64`s: size, is-folder,
    /// FAT date << 16 | FAT time of the last change, 0. Returns 0 or
    /// `syserr::INVAL`.
    pub const FS_OP: u64 = 54;
    /// User accounts: `a0` = a [`crate::user_op`], `a1` = the request, a
    /// NUL-terminated string of fields separated by `\n` (at most 255 bytes),
    /// `a2` = the output buffer (LIST, 2048 bytes: one `name\tadmin\tavatar\n`
    /// line per account). Returns 0 (LIST: bytes written) or `syserr::INVAL`.
    pub const USER_OP: u64 = 55;
    /// 3D rendering (G3, `Buitenzorg.Graphics3D`): `a0` = a pointer to a
    /// [`crate::Gfx3dCmd`] whose `op` is a [`crate::gfx3d_op`]. Returns a new
    /// resource id (CREATE / MESH / TEXTURE), DRAW statistics
    /// (pixels << 32 | triangles), a pixel (READ_PIXEL), 0, or `syserr::INVAL`.
    pub const GFX3D: u64 = 56;
    /// Random bytes from the kernel's ChaCha20 DRBG (`rng.rs`): `a0` = the
    /// output buffer, `a1` = its length (at most 4096). Returns the length
    /// written or `syserr::INVAL`.
    pub const RANDOM: u64 = 57;
    /// Run one command line in the built-in shell (as typed at the Terminal,
    /// with the user's rights): `a0` = the NUL-terminated line (at most 4096
    /// bytes), `a1` = the output buffer, `a2` = its size. Returns the output
    /// bytes written (UTF-8 lines joined by '\n', cut to the buffer) in the low
    /// 32 bits, bit 32 set when the command failed; `syserr::INVAL` on a bad
    /// pointer. Used by Buitenzorg Studio's Run and terminal panels.
    pub const SHELL_RUN: u64 = 58;
    /// The next key press / release for the focused program (games): 0 =
    /// none, else `code | pressed << 16 | 1 << 17`, `code` = the PC set-1 make
    /// scancode (`| 0x100` for the 0xE0-prefixed keys: arrows 0x148/0x150/
    /// 0x14B/0x14D, ...). Reading events discards the program's pending
    /// characters (an app reads either characters or events).
    pub const KEY_EVENT: u64 = 59;
    /// Whether key `a0` (a [`KEY_EVENT`] code) is held down now: 1 / 0 (0 for
    /// a program that has not the focus).
    pub const KEY_STATE: u64 = 60;
    /// Sleep until CLOCK_NANOS reaches `a0` (absolute nanoseconds): blocks until
    /// shortly before, then yields until the deadline -- about 1 ms precision,
    /// for frame pacing. Returns 0 (at once when the deadline passed).
    pub const SLEEP_UNTIL: u64 = 61;
    /// Number of defined syscalls (exclusive upper bound of the v1 table).
    pub const COUNT: u64 = 62;
}

/// Operations of the [`sysno::GFX3D`] syscall ([`Gfx3dCmd::op`]). Every
/// resource belongs to the process that made it and is freed when it exits.
pub mod gfx3d_op {
    /// A render context (color + depth buffer): `a` = width, `b` = height
    /// (1..=1024 each). Returns its id.
    pub const CREATE: u32 = 0;
    /// Free the context, mesh or texture `target`.
    pub const FREE: u32 = 1;
    /// Clear context `target`: `a` = 0xRRGGBB color, `b` = [`super::gfx3d_clear`] bits.
    pub const CLEAR: u32 = 2;
    /// A mesh (vertex + index buffer): `a` = vertex count, `ptr` = that many
    /// [`super::Vertex3D`], `b` = index count (a multiple of 3, triangles),
    /// `ptr2` = that many `u32` indices. Returns its id.
    pub const MESH: u32 = 3;
    /// A texture: `a` = width, `b` = height (1..=1024), `ptr` = w*h 0xAARRGGBB
    /// pixels, top row first. Returns its id.
    pub const TEXTURE: u32 = 4;
    /// Lighting, fog and eye position of context `target`: `ptr` = `f32`s laid
    /// out as in [`super::gfx3d_light`], `len` = their size in bytes.
    pub const LIGHTS: u32 = 5;
    /// Draw mesh `a` into context `target` with texture `b` (0 = none),
    /// `c` = [`super::gfx3d_flags`], `d` = material color 0xAARRGGBB (alpha =
    /// opacity with BLEND), `ptr` = 32 `f32`s: the model matrix then the
    /// view-projection matrix, each row-major (translation in elements 3, 7,
    /// 11), applied to column vectors; clip space is OpenGL's (z in -w..w,
    /// counter-clockwise front faces). Returns pixels << 32 | triangles drawn.
    pub const DRAW: u32 = 6;
    /// Copy context `target`'s color buffer into window `a` at client
    /// position (`b`, `c`) (as `i32`); WIN_PRESENT then shows it.
    pub const PRESENT: u32 = 7;
    /// The 0x00RRGGBB pixel at (`a`, `b`) of context `target`.
    pub const READ_PIXEL: u32 = 8;
    /// Copy context `target`'s color buffer (w*h `u32`, top row first) to
    /// `ptr` (`len` bytes, at least w*h*4).
    pub const READ: u32 = 9;
    /// Which backend context `target` draws with: 0 the software
    /// rasterizer, 1 the host GPU through virtio-gpu 3D (virgl).
    pub const INFO: u32 = 10;
}

/// [`gfx3d_op::CLEAR`] bits.
pub mod gfx3d_clear {
    /// Fill the color buffer.
    pub const COLOR: u32 = 1;
    /// Reset the depth buffer to the far plane.
    pub const DEPTH: u32 = 2;
}

/// [`gfx3d_op::DRAW`] flags.
pub mod gfx3d_flags {
    /// Shading model (mask): color as given, no lights (three.js Basic).
    pub const UNLIT: u32 = 0;
    /// Diffuse per-vertex lighting (Lambert, Gouraud-interpolated).
    pub const LAMBERT: u32 = 1;
    /// Diffuse + Blinn-Phong specular per vertex.
    pub const PHONG: u32 = 2;
    /// Mask of the shading model.
    pub const SHADING_MASK: u32 = 3;
    /// Draw back faces too (no culling).
    pub const DOUBLE_SIDED: u32 = 4;
    /// Draw the triangle edges only.
    pub const WIREFRAME: u32 = 8;
    /// Ignore the depth buffer.
    pub const NO_DEPTH_TEST: u32 = 16;
    /// Do not update the depth buffer.
    pub const NO_DEPTH_WRITE: u32 = 32;
    /// Blend over the color buffer with the material alpha.
    pub const BLEND: u32 = 64;
    /// Bits 8-15: specular exponent (PHONG; 0 = 32).
    pub const SHININESS_SHIFT: u32 = 8;
    /// Bits 16-23: specular strength 0-255 (PHONG; 0 = 128).
    pub const SPECULAR_SHIFT: u32 = 16;
}

/// Layout of the [`gfx3d_op::LIGHTS`] block (indices of `f32`s).
pub mod gfx3d_light {
    /// Ambient light r, g, b (0..1 = none..full).
    pub const AMBIENT: usize = 0;
    /// Eye (camera) position x, y, z, for specular highlights.
    pub const EYE: usize = 3;
    /// Fog color r, g, b (0..1).
    pub const FOG_COLOR: usize = 6;
    /// Distance where fog starts.
    pub const FOG_START: usize = 9;
    /// Distance where fog is complete (fog is off when not beyond FOG_START).
    pub const FOG_END: usize = 10;
    /// Number of lights that follow (at most [`MAX_LIGHTS`]).
    pub const COUNT: usize = 11;
    /// First light; each takes [`STRIDE`] floats: kind (0 directional,
    /// 1 point), x, y, z (directional: the direction the light travels;
    /// point: its position), r, g, b (color times intensity), range (point:
    /// distance where it fades out, 0 = never).
    pub const LIGHTS: usize = 12;
    /// Floats per light.
    pub const STRIDE: usize = 8;
    /// Lights a context can hold.
    pub const MAX_LIGHTS: usize = 8;
}

/// Operations of the [`sysno::USER_OP`] syscall (request fields in order).
pub mod user_op {
    /// All accounts into the buffer at `a2`.
    pub const LIST: u64 = 0;
    /// name, password -> 0 if they match.
    pub const VERIFY: u64 = 1;
    /// admin, admin password, name, password, "1" for an administrator.
    pub const ADD: u64 = 2;
    /// admin, admin password, name (the last administrator stays).
    pub const REMOVE: u64 = 3;
    /// name, current password, new password.
    pub const PASSWD: u64 = 4;
    /// admin, admin password, name, new password.
    pub const RESET: u64 = 5;
    /// who, their password, name, picture path (own account, or an admin).
    pub const AVATAR: u64 = 6;
}

/// Operations of the [`sysno::FS_OP`] syscall.
pub mod fs_op {
    /// Create a folder (its parent must exist).
    pub const MKDIR: u64 = 0;
    /// Delete a file or an empty folder.
    pub const REMOVE: u64 = 1;
    /// Rename and / or move `a1` to `a2` (another mount: files are copied).
    pub const RENAME: u64 = 2;
    /// Size, kind and date of `a1` into the 4 `u64`s at `a2`.
    pub const STAT: u64 = 3;
}

/// Operations of the [`sysno::SETTING`] syscall.
pub mod setting_op {
    /// Current value (an index, seconds, or 0/1).
    pub const GET: u64 = 0;
    /// Change it (`a2` = the new value).
    pub const SET: u64 = 1;
    /// Number of choices of a list setting.
    pub const COUNT: u64 = 2;
    /// Name of choice `index` into a 64-byte buffer; returns its length.
    pub const NAME: u64 = 3;
    /// One-line description of choice `index` (wallpapers); returns its length.
    pub const DESCRIPTION: u64 = 4;
}

/// Keys of the [`sysno::SETTING`] syscall.
pub mod setting_key {
    /// Desktop theme (an index into the theme list).
    pub const THEME: u64 = 0;
    /// Official wallpaper (an index).
    pub const WALLPAPER: u64 = 1;
    /// Screensaver (an index; the last choice is "off").
    pub const SCREENSAVER: u64 = 2;
    /// Idle seconds before the screensaver starts (5-3600).
    pub const SCREENSAVER_DELAY: u64 = 3;
    /// Window animations on (1) / off (0).
    pub const ANIMATIONS: u64 = 4;
    /// Large mouse cursor (1) / normal (0).
    pub const LARGE_CURSOR: u64 = 5;
    /// Time zone (an index; NAME = "WIB", DESCRIPTION = places + UTC offset).
    pub const TIMEZONE: u64 = 6;
    /// Interface language (an index: 0 Bahasa Indonesia, 1 English).
    pub const LANGUAGE: u64 = 7;
    /// 24-hour clock (1) / 12-hour (0).
    pub const CLOCK_24H: u64 = 8;
    /// Local date and time as seconds since 1970-01-01 *local* midnight
    /// (GET reads it, SET sets the clock).
    pub const CLOCK: u64 = 9;
}

/// Queries of the [`sysno::HW_BOARD`] syscall.
pub mod hw_query {
    /// Which board: 1 = Raspberry Pi 4 (BCM2711).
    pub const BOARD: u64 = 0;
    /// SoC temperature, milli-degrees Celsius.
    pub const TEMPERATURE: u64 = 1;
    /// Temperature at which the firmware throttles, milli-degrees Celsius.
    pub const TEMPERATURE_MAX: u64 = 2;
    /// Measured clock rate in Hz; `a1` = firmware clock id (3 ARM, 4 core, ...).
    pub const CLOCK: u64 = 3;
    /// Voltage in microvolts; `a1` = 1 core, 2 SDRAM controller, 3 SDRAM PHY, 4 SDRAM I/O.
    pub const VOLTAGE: u64 = 4;
    /// Firmware throttle flags: bit 0 under-voltage now, 1 ARM frequency
    /// capped now, 2 throttled now, 3 soft temperature limit now; bits 16-19
    /// the same "has occurred since boot".
    pub const THROTTLED: u64 = 5;
    /// Board revision code.
    pub const REVISION: u64 = 6;
    /// Board serial number.
    pub const SERIAL: u64 = 7;
    /// Memory the firmware gave the ARM cores, bytes.
    pub const MEMORY: u64 = 8;
    /// Number of GPIO pins of the SoC.
    pub const GPIO_COUNT: u64 = 9;
}

/// Operations of the [`sysno::HW_GPIO`] syscall.
pub mod hw_gpio {
    /// Set a pin's function: value 0 input, 1 output, 2 + n alternate function n (0-5).
    pub const MODE: u64 = 0;
    /// Drive an output pin: value 0 low, 1 high.
    pub const WRITE: u64 = 1;
    /// Read a pin's level (0 / 1).
    pub const READ: u64 = 2;
    /// Pull resistor: value 0 none, 1 up, 2 down.
    pub const PULL: u64 = 3;
    /// A pin's current function, encoded as for MODE.
    pub const GET_MODE: u64 = 4;
}

/// Operations of the [`sysno::HW_UART`] syscall.
pub mod hw_uart {
    /// Configure the port's pins and baud rate (`speed_hz`).
    pub const OPEN: u64 = 0;
    /// Send `tx`; returns the bytes written.
    pub const WRITE: u64 = 1;
    /// Take up to `rx_len` received bytes without blocking; returns how many.
    pub const READ: u64 = 2;
    /// Stop using the port.
    pub const CLOSE: u64 = 3;
}

/// Results of [`sysno::HW_I2C`] / [`sysno::HW_SPI`] (small values, never
/// confused with the `syserr` range).
pub mod hw_status {
    /// The transfer completed.
    pub const OK: u64 = 0;
    /// No device acknowledged the address (I2C).
    pub const NACK: u64 = 1;
    /// The device held the clock too long (I2C).
    pub const CLOCK_STRETCH: u64 = 2;
    /// The transfer did not finish in time.
    pub const TIMEOUT: u64 = 3;
}

/// Socket kinds for the [`sysno::NET_SOCKET`] syscall.
///
/// Only UDP is implemented: the kernel stack is Ethernet + ARP + IPv4 +
/// ICMP + UDP over a loopback device. `STREAM` is reserved for the TCP work
/// that `System.Net.Http` needs and currently fails with an error.
pub mod sock_kind {
    /// Connectionless datagrams (UDP).
    pub const DGRAM: u64 = 0;
    /// Reserved for TCP; not implemented yet.
    pub const STREAM: u64 = 1;
}

/// Protection flags for the [`sysno::MMAP`] / [`sysno::MPROTECT`] syscalls
/// (v0.15 "Matang"). Combine with bitwise OR; `NONE` reserves address space
/// without committing frames.
pub mod mmap_prot {
    /// No access (reserve only).
    pub const NONE: u64 = 0;
    /// Readable.
    pub const READ: u64 = 1;
    /// Writable.
    pub const WRITE: u64 = 2;
    /// Executable.
    pub const EXEC: u64 = 4;
}

/// Error values returned in the high range of a syscall result.
pub mod syserr {
    /// Unknown syscall number.
    pub const NOSYS: u64 = u64::MAX;
    /// An argument was invalid (bad pointer, bad length, ...).
    pub const INVAL: u64 = u64::MAX - 1;
}

/// Description of the boot framebuffer, shared zero-copy with user-space.
///
/// C# mirror: `Buitenzorg.Runtime.Sys.FramebufferInfo` (Sequential, Pack = 8).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramebufferInfo {
    /// Physical address of the framebuffer start.
    pub address: u64,
    /// Total size of the framebuffer memory in bytes.
    pub size: u64,
    /// Visible width in pixels.
    pub width: u64,
    /// Visible height in pixels.
    pub height: u64,
    /// Bytes per row (may exceed `width * bytes_per_pixel` due to padding).
    pub stride: u64,
    /// Bytes per pixel.
    pub bytes_per_pixel: u64,
    /// Pixel format, one of the [`pixel_format`] constants.
    pub pixel_format: u64,
}

/// Drawing operation for the WIN_CMD syscall (v0.8 "Kembang" UI ABI).
///
/// C# mirror: `Buitenzorg.Runtime.Sys.DrawCmd` (Sequential, Pack = 8).
/// `op`: 0 = fill_rect (x,y,w,h,color), 1 = draw_text (x,y,color,text),
/// 2 = clear (color).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawCmd {
    /// One of the [`draw_op`] constants.
    pub op: u64,
    /// X coordinate in window-client pixels.
    pub x: i32,
    /// Y coordinate in window-client pixels.
    pub y: i32,
    /// Width in pixels (fill_rect).
    pub w: i32,
    /// Height in pixels (fill_rect).
    pub h: i32,
    /// Color as 0x00RRGGBB.
    pub color: u32,
    /// Second parameter of draw ops 8-13 (radius / second color / text
    /// style, see [`draw_op`]); 0 for the others. Keeps 8-byte alignment.
    pub _pad: u32,
    /// UTF-8 text pointer (draw_text), else 0.
    pub text_ptr: u64,
    /// UTF-8 text length in bytes (draw_text), else 0.
    pub text_len: u64,
}

/// Flags in the `a2` (dims) argument of [`sysno::WIN_CREATE`].
pub mod win_flags {
    /// The requested width/height are the *client* area (the window grows by
    /// its title bar and borders); without it they are the outer size.
    pub const CLIENT_SIZE: u64 = 1 << 63;
}

/// Packed pointer events returned by [`sysno::WIN_EVENT`]:
/// bits 0-15 x, 16-31 y (client coordinates, signed 16-bit), 32-39 buttons
/// (bit 0 = left), 40-55 wheel notches (signed 16-bit, [`WHEEL`] only),
/// 56-63 kind.
pub mod pointer_event {
    /// The pointer moved over the client area.
    pub const MOVE: u64 = 1;
    /// A button went down over the client area (the window captures).
    pub const DOWN: u64 = 2;
    /// The captured button went up (anywhere).
    pub const UP: u64 = 3;
    /// The pointer left the client area.
    pub const LEAVE: u64 = 4;
    /// The wheel turned over the client area: bits 40-55 = notches,
    /// positive = away from the user (scroll up).
    pub const WHEEL: u64 = 5;
}

/// Operations for [`DrawCmd::op`].
pub mod draw_op {
    /// Fill rectangle x,y,w,h with `color`.
    pub const FILL_RECT: u64 = 0;
    /// Draw UTF-8 text at x,y with `color`.
    pub const DRAW_TEXT: u64 = 1;
    /// Clear the whole client area with `color`.
    pub const CLEAR: u64 = 2;
    /// Draw a line from (x,y) to (x+w, y+h) with `color`.
    pub const LINE: u64 = 3;
    /// Draw an ellipse outline in the box x,y,w,h with `color`.
    pub const ELLIPSE: u64 = 4;
    /// Fill an ellipse in the box x,y,w,h with `color`.
    pub const FILL_ELLIPSE: u64 = 5;
    /// Outline rectangle x,y,w,h with `color` (1px).
    pub const RECT: u64 = 6;
    /// Blit a client ARGB pixel buffer (`text_ptr`, `w`×`h` `u32` pixels,
    /// `text_len` bytes) into the window canvas at (x,y). Enables full
    /// client-side software rendering (v0.16 "Panen" `Buitenzorg.Drawing`).
    pub const BLIT: u64 = 7;
    /// Fill a rounded rectangle x,y,w,h with `color`; radius = `_pad`.
    pub const FILL_ROUND_RECT: u64 = 8;
    /// Outline a rounded rectangle (1 px) with `color`; radius = `_pad`.
    pub const ROUND_RECT: u64 = 9;
    /// Fill x,y,w,h with a vertical gradient from `color` (top) to `_pad`.
    pub const GRADIENT_V: u64 = 10;
    /// A rounded rectangle filled with a vertical gradient: `color` top,
    /// `_pad` bottom, radius = `text_len` (`text_ptr` must be 0).
    pub const FILL_ROUND_GRADIENT_V: u64 = 11;
    /// Draw UTF-8 text in a style: `_pad` = 0 body, 1 strong (bold), 2
    /// heading (bold, 24 px).
    pub const DRAW_TEXT_STYLED: u64 = 12;
    /// Blend a translucent rounded rectangle: `color`, `_pad` bits 0-15 =
    /// radius, bits 16-23 = alpha (0-255) -- shadows, overlays, focus rings.
    pub const BLEND_ROUND_RECT: u64 = 13;
}

/// Process/task descriptor for the PROC_LIST syscall (v0.9 "Serbuk").
///
/// C# mirror: `Buitenzorg.Runtime.Sys.ProcInfo` (Sequential, Pack = 8).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ProcInfo {
    /// Task/process id.
    pub pid: u64,
    /// State: one of [`proc_state`].
    pub state: u64,
    /// Accumulated CPU time in timer ticks.
    pub cpu_ticks: u64,
    /// Kind: 0 = kernel task, 1 = user app.
    pub kind: u64,
    /// Null-padded ASCII name.
    pub name: [u8; 32],
}

/// States for [`ProcInfo::state`].
pub mod proc_state {
    /// Ready to run.
    pub const RUNNABLE: u64 = 0;
    /// Currently running.
    pub const RUNNING: u64 = 1;
    /// Finished / exited.
    pub const FINISHED: u64 = 2;
}

/// System resource statistics for the SYS_STAT syscall (v0.9 "Serbuk").
///
/// C# mirror: `Buitenzorg.Runtime.Sys.SysStat` (Sequential, Pack = 8).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SysStat {
    /// Uptime in timer ticks.
    pub uptime_ticks: u64,
    /// Timer frequency (Hz), for converting ticks to seconds.
    pub tick_hz: u64,
    /// Kernel heap bytes in use.
    pub heap_used: u64,
    /// Kernel heap bytes total.
    pub heap_total: u64,
    /// Number of tasks/processes.
    pub task_count: u64,
    /// Total usable physical memory (MiB).
    pub mem_total_mib: u64,
}

/// Audio device status for the AUDIO_STAT syscall (v0.16 "Panen").
///
/// C# mirror: `Buitenzorg.Runtime.Sys.AudioInfo` (Sequential, Pack = 8).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioInfo {
    /// 1 if a sound card was detected and initialized, else 0.
    pub present: u64,
    /// Output sample rate in Hz (48000 for AC'97 without VRA).
    pub sample_rate: u64,
    /// Number of output channels (2 = stereo).
    pub channels: u64,
    /// Bits per sample (16).
    pub bits: u64,
    /// Current master volume, 0..=100 percent.
    pub volume: u64,
    /// 1 if output is muted, else 0.
    pub muted: u64,
}

/// A package registry entry for the PKG_LIST syscall (v0.16 "Panen" App Store).
///
/// C# mirror: `Buitenzorg.Runtime.Sys.PkgInfo` (Sequential, Pack = 8).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PkgInfo {
    /// Null-padded ASCII package name.
    pub name: [u8; 24],
    /// Null-padded ASCII category label.
    pub category: [u8; 16],
    /// 1 if the package is currently installed, else 0.
    pub installed: u64,
}

/// A directory entry for the FS_LIST syscall (v0.16 "Panen" File Manager).
///
/// C# mirror: `Buitenzorg.Runtime.Sys.FsEntry` (Sequential, Pack = 8).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsEntry {
    /// Null-padded ASCII name.
    pub name: [u8; 24],
    /// 1 if this entry is a directory/mount, else 0 (a file).
    pub is_dir: u64,
}

/// Wall-clock time from the CMOS RTC, for the CLOCK_RTC syscall (pre-v1.0
/// `Buitenzorg.Bcl` `System.Globalization`).
///
/// C# mirror: `Buitenzorg.Runtime.Sys.RtcTime` (Sequential, Pack = 8).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RtcTime {
    /// Full year (e.g. 2026).
    pub year: u64,
    /// Month, 1..=12.
    pub month: u64,
    /// Day of month, 1..=31.
    pub day: u64,
    /// Hour, 0..=23.
    pub hour: u64,
    /// Minute, 0..=59.
    pub minute: u64,
    /// Second, 0..=59.
    pub second: u64,
}

/// Datagram header for the NET_SEND / NET_RECV syscalls (pre-v1.0
/// `System.Net.Sockets`). The payload follows immediately after this header in
/// the same buffer.
///
/// C# mirror: `Buitenzorg.Runtime.Sys.NetDatagram` (Sequential, Pack = 8).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NetDatagram {
    /// Peer IPv4 address, one octet per byte in network order (a.b.c.d).
    pub addr: [u8; 4],
    /// Peer port (host order).
    pub port: u32,
    /// Payload length in bytes.
    pub length: u64,
}

/// One bus transfer for [`sysno::HW_I2C`], [`sysno::HW_SPI`] and
/// [`sysno::HW_UART`]: which bus/device, and a write and a read buffer.
///
/// C# mirror: `Buitenzorg.Runtime.Sys.HwTransfer` (Sequential, Pack = 8).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HwTransfer {
    /// I2C / SPI bus number, or serial port number.
    pub bus: u32,
    /// I2C 7-bit address, or SPI chip select.
    pub addr: u32,
    /// Bus clock in Hz (0 = the bus default), or the baud rate for UART OPEN.
    pub speed_hz: u32,
    /// SPI mode 0-3 (CPOL << 1 | CPHA); unused otherwise.
    pub mode: u32,
    /// Bytes to write (user pointer).
    pub tx_ptr: u64,
    /// Number of bytes at `tx_ptr`.
    pub tx_len: u64,
    /// Buffer for the bytes read (user pointer).
    pub rx_ptr: u64,
    /// Size of the buffer at `rx_ptr`.
    pub rx_len: u64,
}

/// One [`sysno::GFX3D`] request.
///
/// C# mirror: `Buitenzorg.Runtime.Sys.Gfx3dCmd` (Sequential, Pack = 8).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gfx3dCmd {
    /// A [`gfx3d_op`].
    pub op: u32,
    /// The context / resource the operation acts on.
    pub target: u32,
    /// First operation parameter (see [`gfx3d_op`]).
    pub a: u32,
    /// Second operation parameter.
    pub b: u32,
    /// Third operation parameter.
    pub c: u32,
    /// Fourth operation parameter.
    pub d: u32,
    /// First data block (user pointer).
    pub ptr: u64,
    /// Size of the first data block in bytes.
    pub len: u64,
    /// Second data block (user pointer).
    pub ptr2: u64,
    /// Size of the second data block in bytes.
    pub len2: u64,
}

/// One vertex of a [`gfx3d_op::MESH`] (an array element, 40 bytes).
///
/// C# mirror: `Buitenzorg.Runtime.Sys.Vertex3D` (Sequential).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vertex3D {
    /// Position x.
    pub x: f32,
    /// Position y.
    pub y: f32,
    /// Position z.
    pub z: f32,
    /// Normal x (the normal need not be unit length).
    pub nx: f32,
    /// Normal y.
    pub ny: f32,
    /// Normal z.
    pub nz: f32,
    /// Texture coordinate u (0..1 across the texture; repeats outside).
    pub u: f32,
    /// Texture coordinate v (0 = top row).
    pub v: f32,
    /// Vertex color 0xAARRGGBB, multiplied with the material color.
    pub color: u32,
    /// Zero.
    pub _reserved: u32,
}

/// Interface information and counters for the NET_INFO syscall.
///
/// C# mirror: `Buitenzorg.Runtime.Sys.NetInfo` (Sequential, Pack = 8).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NetInfo {
    /// Local IPv4 address, one octet per byte (a.b.c.d), then 4 zero bytes.
    pub addr: [u8; 8],
    /// 1 if the stack is up, else 0.
    pub up: u64,
    /// Datagrams sent.
    pub tx_datagrams: u64,
    /// Datagrams received and delivered to a socket.
    pub rx_datagrams: u64,
    /// ICMP echo replies observed.
    pub icmp_replies: u64,
    /// ARP replies sent.
    pub arp_replies: u64,
}

/// Pixel format constants for [`FramebufferInfo::pixel_format`].
pub mod pixel_format {
    /// Red-green-blue, one byte each, red first.
    pub const RGB: u64 = 0;
    /// Blue-green-red, one byte each, blue first.
    pub const BGR: u64 = 1;
    /// Single-byte grayscale.
    pub const GRAY: u64 = 2;
    /// Unknown/other layout; consult stride and bpp only.
    pub const UNKNOWN: u64 = 255;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The v1 table is frozen: these numbers must never change.
    #[test]
    fn syscall_numbers_are_stable() {
        assert_eq!(sysno::ABI_VERSION, 0);
        assert_eq!(sysno::DEBUG_WRITE, 1);
        assert_eq!(sysno::EXIT, 2);
        assert_eq!(sysno::YIELD, 3);
        assert_eq!(sysno::TICKS, 4);
        assert_eq!(sysno::FB_INFO, 5);
        assert_eq!(sysno::WIN_CREATE, 6);
        assert_eq!(sysno::WIN_CMD, 7);
        assert_eq!(sysno::WIN_PRESENT, 8);
        assert_eq!(sysno::KEY_READ, 9);
        assert_eq!(sysno::PROC_LIST, 10);
        assert_eq!(sysno::PROC_KILL, 11);
        assert_eq!(sysno::SYS_STAT, 12);
        assert_eq!(sysno::MMAP, 13);
        assert_eq!(sysno::MPROTECT, 14);
        assert_eq!(sysno::MUNMAP, 15);
        assert_eq!(sysno::THREAD_CREATE, 16);
        assert_eq!(sysno::THREAD_JOIN, 17);
        assert_eq!(sysno::THREAD_EXIT, 18);
        assert_eq!(sysno::FUTEX_WAIT, 19);
        assert_eq!(sysno::FUTEX_WAKE, 20);
        assert_eq!(sysno::THREAD_SELF, 21);
        assert_eq!(sysno::CLOCK_MONO, 22);
        assert_eq!(sysno::AUDIO_STAT, 23);
        assert_eq!(sysno::AUDIO_SET_VOLUME, 24);
        assert_eq!(sysno::AUDIO_TONE, 25);
        assert_eq!(sysno::AUDIO_PLAY, 26);
        assert_eq!(sysno::PKG_LIST, 27);
        assert_eq!(sysno::PKG_SET, 28);
        assert_eq!(sysno::FS_LIST, 29);
        assert_eq!(sysno::FS_READ, 30);
        assert_eq!(sysno::IS_INTERACTIVE, 31);
        assert_eq!(sysno::FS_WRITE, 32);
        assert_eq!(sysno::CLOCK_RTC, 33);
        assert_eq!(sysno::NET_SOCKET, 34);
        assert_eq!(sysno::NET_BIND, 35);
        assert_eq!(sysno::NET_SEND, 36);
        assert_eq!(sysno::NET_RECV, 37);
        assert_eq!(sysno::NET_CLOSE, 38);
        assert_eq!(sysno::NET_INFO, 39);
        assert_eq!(sysno::PROC_SPAWN, 40);
        assert_eq!(sysno::PROC_WAIT, 41);
        assert_eq!(sysno::PROC_ARGS, 42);
        assert_eq!(sysno::FUTEX_WAIT_TIMEOUT, 43);
        assert_eq!(sysno::CLOCK_NANOS, 44);
        assert_eq!(sysno::WIN_EVENT, 45);
        assert_eq!(sysno::TEXT_METRICS, 46);
        assert_eq!(sysno::HW_BOARD, 47);
        assert_eq!(sysno::HW_GPIO, 48);
        assert_eq!(sysno::HW_I2C, 49);
        assert_eq!(sysno::HW_SPI, 50);
        assert_eq!(sysno::HW_UART, 51);
        assert_eq!(sysno::HW_LED, 52);
        assert_eq!(sysno::SETTING, 53);
        assert_eq!(sysno::FS_OP, 54);
        assert_eq!(fs_op::STAT, 3);
        assert_eq!(sysno::USER_OP, 55);
        assert_eq!(user_op::AVATAR, 6);
        assert_eq!(sysno::GFX3D, 56);
        assert_eq!(gfx3d_op::READ, 9);
        assert_eq!(gfx3d_op::INFO, 10);
        assert_eq!(gfx3d_light::LIGHTS, 12);
        assert_eq!(sysno::RANDOM, 57);
        assert_eq!(sysno::SHELL_RUN, 58);
        assert_eq!(sysno::KEY_EVENT, 59);
        assert_eq!(sysno::KEY_STATE, 60);
        assert_eq!(sysno::SLEEP_UNTIL, 61);
        assert_eq!(sysno::COUNT, 62);
    }

    /// **ABI freeze gate (v1.0).** The v1 table is frozen: numbers are
    /// append-only and existing struct layouts may never change. This test is
    /// the mechanical guard — it pins the total count and the byte size of
    /// every struct that crosses the boundary, so *any* renumbering, reordering
    /// or field-width change fails here (and in `AbiContractTests.cs`) before
    /// it can reach a released image.
    ///
    /// Adding a syscall means: append the constant, bump `COUNT`, extend this
    /// test *and* the C# mirror's. Changing an existing one means a new ABI
    /// major version, not an edit.
    #[test]
    fn abi_v1_is_frozen() {
        assert_eq!(ABI_VERSION, 1, "ABI major version changed - that is a breaking change");
        assert_eq!(sysno::COUNT, 62, "syscall count changed: append only, and update both mirrors");

        // Sizes are what the C# mirrors and the raw syscall decoders hard-code.
        assert_eq!(core::mem::size_of::<FramebufferInfo>(), 56);
        assert_eq!(core::mem::size_of::<DrawCmd>(), 48);
        assert_eq!(core::mem::size_of::<ProcInfo>(), 64);
        assert_eq!(core::mem::size_of::<SysStat>(), 48);
        assert_eq!(core::mem::size_of::<AudioInfo>(), 48);
        assert_eq!(core::mem::size_of::<PkgInfo>(), 48);
        assert_eq!(core::mem::size_of::<FsEntry>(), 32);
        assert_eq!(core::mem::size_of::<RtcTime>(), 48);
        assert_eq!(core::mem::size_of::<NetDatagram>(), 16);
        assert_eq!(core::mem::size_of::<NetInfo>(), 48);
        assert_eq!(core::mem::size_of::<HwTransfer>(), 48);
        assert_eq!(core::mem::size_of::<Gfx3dCmd>(), 56);
        assert_eq!(core::mem::size_of::<Vertex3D>(), 40);
        assert_eq!(core::mem::align_of::<Gfx3dCmd>(), 8);
        // Vertex3D is an array element of f32s / u32s: 4-byte aligned.
        assert_eq!(core::mem::align_of::<Vertex3D>(), 4);

        // Every struct crossing the boundary must be 8-byte aligned so the C#
        // `Pack = 8` mirrors line up field for field.
        assert_eq!(core::mem::align_of::<FramebufferInfo>(), 8);
        assert_eq!(core::mem::align_of::<DrawCmd>(), 8);
        assert_eq!(core::mem::align_of::<ProcInfo>(), 8);
        assert_eq!(core::mem::align_of::<SysStat>(), 8);
        assert_eq!(core::mem::align_of::<AudioInfo>(), 8);
        assert_eq!(core::mem::align_of::<PkgInfo>(), 8);
        assert_eq!(core::mem::align_of::<FsEntry>(), 8);
        assert_eq!(core::mem::align_of::<RtcTime>(), 8);
        assert_eq!(core::mem::align_of::<NetDatagram>(), 8);
        assert_eq!(core::mem::align_of::<NetInfo>(), 8);
        assert_eq!(core::mem::align_of::<HwTransfer>(), 8);
        assert_eq!(core::mem::offset_of!(HwTransfer, tx_ptr), 16);
        assert_eq!(core::mem::offset_of!(HwTransfer, rx_ptr), 32);

        // Board I/O codes are part of the contract too.
        assert_eq!(hw_query::GPIO_COUNT, 9);
        assert_eq!(hw_gpio::GET_MODE, 4);
        assert_eq!(hw_uart::CLOSE, 3);
        assert_eq!(hw_status::TIMEOUT, 3);
        assert_eq!(setting_op::DESCRIPTION, 4);
        assert_eq!(setting_key::LARGE_CURSOR, 5);
        assert_eq!(setting_key::TIMEZONE, 6);
        assert_eq!(setting_key::LANGUAGE, 7);
        assert_eq!(setting_key::CLOCK_24H, 8);
        assert_eq!(setting_key::CLOCK, 9);

        // Error codes are part of the contract too.
        assert_eq!(syserr::NOSYS, u64::MAX);
        assert_eq!(syserr::INVAL, u64::MAX - 1);
    }

    /// Layouts of the pre-v1.0 BCL structs must match the C# mirrors.
    #[test]
    fn bcl_struct_layouts() {
        assert_eq!(core::mem::size_of::<RtcTime>(), 48);
        assert_eq!(core::mem::align_of::<RtcTime>(), 8);
        assert_eq!(core::mem::size_of::<NetDatagram>(), 16);
        assert_eq!(core::mem::align_of::<NetDatagram>(), 8);
        assert_eq!(core::mem::size_of::<NetInfo>(), 48);
        assert_eq!(core::mem::align_of::<NetInfo>(), 8);
        // Field offsets the syscall layer and the C# side both hard-code.
        assert_eq!(core::mem::offset_of!(NetDatagram, addr), 0);
        assert_eq!(core::mem::offset_of!(NetDatagram, port), 4);
        assert_eq!(core::mem::offset_of!(NetDatagram, length), 8);
        assert_eq!(core::mem::offset_of!(NetInfo, up), 8);
    }

    /// Layout must match the C# `DrawCmd` mirror (op + 4×i32 + 2×u32 + 2×u64).
    #[test]
    fn draw_cmd_layout() {
        assert_eq!(core::mem::size_of::<DrawCmd>(), 48);
        assert_eq!(core::mem::align_of::<DrawCmd>(), 8);
    }

    /// Layouts must match the C# mirrors.
    #[test]
    fn proc_and_stat_layout() {
        assert_eq!(core::mem::size_of::<ProcInfo>(), 64); // 4×u64 + 32
        assert_eq!(core::mem::size_of::<SysStat>(), 48); // 6×u64
        assert_eq!(core::mem::align_of::<ProcInfo>(), 8);
        assert_eq!(core::mem::size_of::<AudioInfo>(), 48); // 6×u64
        assert_eq!(core::mem::align_of::<AudioInfo>(), 8);
        assert_eq!(core::mem::size_of::<PkgInfo>(), 48); // 24 + 16 + u64
        assert_eq!(core::mem::align_of::<PkgInfo>(), 8);
        assert_eq!(core::mem::size_of::<FsEntry>(), 32); // 24 + u64
        assert_eq!(core::mem::align_of::<FsEntry>(), 8);
    }

    /// Layout must match the C# `FramebufferInfo` mirror (7 × u64).
    #[test]
    fn framebuffer_info_layout() {
        assert_eq!(core::mem::size_of::<FramebufferInfo>(), 56);
        assert_eq!(core::mem::align_of::<FramebufferInfo>(), 8);
    }
}
