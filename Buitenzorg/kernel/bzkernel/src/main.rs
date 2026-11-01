//! Buitenzorg OS kernel.
//!
//! Boot flow: loader (the `bootloader` crate on PCs, or Limine) → protocol
//! glue → `kernel_main(&boot::BootInfo)` → serial + framebuffer console → boot
//! logo → CPU/MMU/interrupt setup → milestone self-tests → desktop.
//!
//! One crate, one kernel per architecture: `pc_main.rs` is the full x86-64
//! kernel; `arm_main.rs` is the ARM64 port (multi-arch M3), which grows as
//! subsystems gain an `arch/aarch64` backend. Generic modules are shared.

#![no_std]
#![no_main]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]
// The ARM64 port compiles a growing subset; HAL facades it does not call yet
// are not dead code on x86-64.
#![cfg_attr(target_arch = "aarch64", allow(dead_code, unused_imports))]

extern crate alloc;

/// Print to the serial console (COM1 / PL011) and the framebuffer console.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        $crate::serial::_print(format_args!($($arg)*));
        $crate::framebuffer::_print(format_args!($($arg)*));
    }};
}

/// Like [`print!`], with a trailing newline.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[cfg(target_arch = "x86_64")]
include!("pc_main.rs");

#[cfg(target_arch = "aarch64")]
include!("arm_main.rs");
