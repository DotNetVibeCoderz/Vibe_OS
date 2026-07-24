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
    /// Number of defined syscalls (exclusive upper bound of the v1 table).
    pub const COUNT: u64 = 40;
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
    /// Reserved, keeps 8-byte alignment for the pointer below.
    pub _pad: u32,
    /// UTF-8 text pointer (draw_text), else 0.
    pub text_ptr: u64,
    /// UTF-8 text length in bytes (draw_text), else 0.
    pub text_len: u64,
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
        assert_eq!(sysno::COUNT, 40);
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
        assert_eq!(sysno::COUNT, 40, "syscall count changed: append only, and update both mirrors");

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
