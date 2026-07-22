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
    /// Number of defined syscalls (exclusive upper bound of the v1 table).
    pub const COUNT: u64 = 13;
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
        assert_eq!(sysno::COUNT, 13);
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
    }

    /// Layout must match the C# `FramebufferInfo` mirror (7 × u64).
    #[test]
    fn framebuffer_info_layout() {
        assert_eq!(core::mem::size_of::<FramebufferInfo>(), 56);
        assert_eq!(core::mem::align_of::<FramebufferInfo>(), 8);
    }
}
