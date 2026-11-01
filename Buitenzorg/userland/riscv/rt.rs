//! A minimal runtime for Buitenzorg RISC-V user programs (multi-arch M4):
//! `_start`, raw syscalls (`ECALL`, a7 = number, a0..a2 = arguments, result
//! in a0 -- bz-abi numbers), printing, threads, and a futex-backed mutex.
//! Shared by every program in this directory (`mod rt;`). These programs are
//! Rust because bflat (the C# compiler the other ports use) has no RISC-V
//! target yet.

#![allow(dead_code)]

use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicU32, Ordering};

pub const ABI_VERSION: u64 = 0;
pub const DEBUG_WRITE: u64 = 1;
pub const EXIT: u64 = 2;
pub const YIELD: u64 = 3;
pub const TICKS: u64 = 4;
pub const MMAP: u64 = 13;
pub const MPROTECT: u64 = 14;
pub const MUNMAP: u64 = 15;
pub const THREAD_CREATE: u64 = 16;
pub const THREAD_JOIN: u64 = 17;
pub const THREAD_EXIT: u64 = 18;
pub const FUTEX_WAIT: u64 = 19;
pub const FUTEX_WAKE: u64 = 20;
pub const THREAD_SELF: u64 = 21;
pub const CLOCK_MONO: u64 = 22;

pub const PROT_NONE: u64 = 0;
pub const PROT_READ: u64 = 1;
pub const PROT_WRITE: u64 = 2;

#[inline(always)]
pub fn syscall(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let r;
    unsafe {
        asm!("ecall", inlateout("a0") a0 => r, in("a1") a1, in("a2") a2, in("a7") nr, options(nostack));
    }
    r
}

pub fn print(s: &str) {
    syscall(DEBUG_WRITE, s.as_ptr() as u64, s.len() as u64, 0);
}

/// Print `v` in decimal.
pub fn print_u64(mut v: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    print(unsafe { core::str::from_utf8_unchecked(&buf[i..]) });
}

pub fn exit(code: u64) -> ! {
    syscall(EXIT, code, 0, 0);
    loop {}
}

pub fn mmap(len: u64, prot: u64) -> u64 {
    syscall(MMAP, len, prot, 0)
}

pub fn yield_now() {
    syscall(YIELD, 0, 0, 0);
}

/// Start a thread at `entry(arg)` on a fresh 64 KiB mmap stack; its id.
pub fn spawn(entry: extern "C" fn(u64) -> !, arg: u64) -> u64 {
    const STACK: u64 = 64 * 1024;
    let base = mmap(STACK, PROT_READ | PROT_WRITE);
    syscall(THREAD_CREATE, entry as usize as u64, arg, (base + STACK) & !0xF)
}

pub fn join(id: u64) {
    syscall(THREAD_JOIN, id, 0, 0);
}

pub fn thread_exit() -> ! {
    syscall(THREAD_EXIT, 0, 0, 0);
    loop {}
}

/// A futex-backed mutex: 0 = free, 1 = held, 2 = held with waiters.
pub struct Mutex(pub AtomicU32);

impl Mutex {
    pub const fn new() -> Mutex {
        Mutex(AtomicU32::new(0))
    }
    pub fn lock(&self) {
        if self.0.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            return;
        }
        while self.0.swap(2, Ordering::Acquire) != 0 {
            syscall(FUTEX_WAIT, self.0.as_ptr() as u64, 2, 0);
        }
    }
    pub fn unlock(&self) {
        if self.0.swap(0, Ordering::Release) == 2 {
            syscall(FUTEX_WAKE, self.0.as_ptr() as u64, 1, 0);
        }
    }
}

// Entry: the kernel puts the stack in sp and the argument in a0; `main`
// returns the exit code.
global_asm!(
    r#"
.section .text._start, "ax"
.global _start
_start:
    call bz_main_entry
1:  j 1b
"#
);

#[no_mangle]
extern "C" fn bz_main_entry(arg: u64) -> ! {
    exit(crate::main(arg))
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    print("panic in user program\n");
    exit(0xBAD)
}
