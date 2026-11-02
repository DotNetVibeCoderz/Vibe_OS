//! Buitenzorg syscalls from the host (bz-abi numbers; the same table on every
//! architecture): x86-64 `syscall` (rax = number, rdi/rsi/rdx), ARM64
//! `svc #0` (x8, x0..x2), RISC-V `ecall` (a7, a0..a2). The result comes back
//! in rax / x0 / a0.

use alloc::string::String;
use alloc::vec::Vec;
use core::arch::asm;

pub const DEBUG_WRITE: u64 = 1;
pub const EXIT: u64 = 2;
pub const TICKS: u64 = 4;
pub const MMAP: u64 = 13;
pub const MPROTECT: u64 = 14;
pub const FS_READ: u64 = 30;
pub const CLOCK_RTC: u64 = 33;
pub const PROC_ARGS: u64 = 42;

pub const PROT_NONE: u64 = 0;
pub const PROT_RW: u64 = 1 | 2;

/// Error results live at the top of the u64 range (bz-abi `syserr`).
pub fn is_err(r: u64) -> bool {
    r >= u64::MAX - 16
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub fn syscall(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let ret: u64;
    // The kernel restores only rcx/r11/rsp before sysretq: every argument
    // register and r8-r10 must be declared clobbered (see the notes in
    // userland/hello-csharp/bzstart.rs -- both bugs bit before).
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") nr => ret,
            inlateout("rdi") a0 => _,
            inlateout("rsi") a1 => _,
            inlateout("rdx") a2 => _,
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r8") _,
            lateout("r9") _,
            lateout("r10") _,
            options(nostack),
        );
    }
    ret
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub fn syscall(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "svc #0",
            inlateout("x0") a0 => ret,
            inlateout("x1") a1 => _,
            inlateout("x2") a2 => _,
            in("x8") nr,
            options(nostack),
        );
    }
    ret
}

#[cfg(target_arch = "riscv64")]
#[inline(always)]
pub fn syscall(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "ecall",
            inlateout("a0") a0 => ret,
            inlateout("a1") a1 => _,
            inlateout("a2") a2 => _,
            in("a7") nr,
            options(nostack),
        );
    }
    ret
}

/// Write text to the console (serial + framebuffer), in chunks the kernel
/// accepts.
pub fn print(s: &str) {
    for chunk in s.as_bytes().chunks(16 * 1024) {
        syscall(DEBUG_WRITE, chunk.as_ptr() as u64, chunk.len() as u64, 0);
    }
}

pub fn exit(code: u64) -> ! {
    syscall(EXIT, code, 0, 0);
    loop {}
}

/// Ticks since boot in PIT units (18.2 per second) -> milliseconds.
pub fn uptime_millis() -> u64 {
    syscall(TICKS, 0, 0, 0) * 10_000 / 182
}

/// The real-time clock as Unix milliseconds (0 if unavailable).
pub fn unix_millis() -> i64 {
    let mut t = [0u64; 6];
    if syscall(CLOCK_RTC, t.as_mut_ptr() as u64, 0, 0) != 0 || t[0] < 1970 {
        return 0;
    }
    let (y, m, d) = (t[0] as i64, t[1] as i64, t[2] as i64);
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    ((days * 86_400) + (t[3] * 3600 + t[4] * 60 + t[5]) as i64) * 1000
}

/// This process's argument string (PROC_ARGS).
pub fn args() -> String {
    let mut buf = alloc::vec![0u8; 1024];
    let n = syscall(PROC_ARGS, buf.as_mut_ptr() as u64, buf.len() as u64, 0);
    if is_err(n) {
        return String::new();
    }
    buf.truncate((n as usize).min(buf.len() - 1));
    String::from_utf8_lossy(&buf).into_owned()
}

/// Read a whole VFS file (FS_READ), growing the buffer until it fits.
pub fn read_file(path: &str) -> Option<Vec<u8>> {
    let mut cpath = Vec::with_capacity(path.len() + 1);
    cpath.extend_from_slice(path.as_bytes());
    cpath.push(0);
    let mut cap = 256 * 1024;
    loop {
        let mut buf = alloc::vec![0u8; cap];
        let n = syscall(FS_READ, cpath.as_ptr() as u64, buf.as_mut_ptr() as u64, cap as u64);
        if is_err(n) || n == 0 {
            return None;
        }
        if (n as usize) < cap {
            buf.truncate(n as usize);
            return Some(buf);
        }
        if cap >= 64 * 1024 * 1024 {
            return None;
        }
        cap *= 4;
    }
}

/// Run `f` on a fresh stack whose top is `top` (never returns).
pub fn run_on_stack(top: u64, f: extern "C" fn() -> !) -> ! {
    unsafe {
        #[cfg(target_arch = "x86_64")]
        asm!("mov rsp, {0}", "call {1}", "ud2", in(reg) top, in(reg) f, options(noreturn));
        #[cfg(target_arch = "aarch64")]
        asm!("mov sp, {0}", "blr {1}", "brk #0", in(reg) top, in(reg) f, options(noreturn));
        #[cfg(target_arch = "riscv64")]
        asm!("mv sp, {0}", "jalr {1}", "unimp", in(reg) top, in(reg) f, options(noreturn));
    }
}
