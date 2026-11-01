//! RISC-V CPU primitives behind `crate::arch::cpu`.
//!
//! The CPU index lives in `tp` while the kernel runs (the trap vector reloads
//! it on entry from U-mode, where user code may have changed it). Rust code in
//! the kernel never touches `tp` (no thread-locals).

use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

const SSTATUS_SIE: u64 = 1 << 1;
/// Supervisor may access user pages (needed to copy syscall arguments).
pub const SSTATUS_SUM: u64 = 1 << 18;

/// `sie` bits: software (IPI), timer, external interrupts.
pub const SIE_SSIE: u64 = 1 << 1;
pub const SIE_STIE: u64 = 1 << 5;
pub const SIE_SEIE: u64 = 1 << 9;

static TIMEBASE_HZ: AtomicU64 = AtomicU64::new(10_000_000);

/// Wait for an interrupt (also wakes on a pending, masked one).
#[inline]
pub fn halt() {
    unsafe { asm!("wfi", options(nomem, nostack)) }
}

/// Sleep until an interrupt, then take it: `wfi` wakes on a pending interrupt
/// even with `sstatus.SIE` clear, so waiting first and unmasking after cannot
/// lose a wake-up.
#[inline]
pub fn enable_and_halt() {
    unsafe { asm!("wfi", "csrsi sstatus, 2", options(nomem, nostack)) }
}

#[inline]
pub fn interrupts_enabled() -> bool {
    let s: u64;
    unsafe { asm!("csrr {}, sstatus", out(reg) s, options(nomem, nostack)) };
    s & SSTATUS_SIE != 0
}

#[inline]
pub fn enable_interrupts() {
    unsafe { asm!("csrsi sstatus, 2", options(nomem, nostack)) }
}

#[inline]
pub fn disable_interrupts() {
    unsafe { asm!("csrci sstatus, 2", options(nomem, nostack)) }
}

#[inline]
pub fn without_interrupts<R>(f: impl FnOnce() -> R) -> R {
    let was = interrupts_enabled();
    disable_interrupts();
    let r = f();
    if was {
        enable_interrupts();
    }
    r
}

/// The `time` CSR (wall-clock counter at the timebase frequency).
#[inline]
pub fn timestamp() -> u64 {
    let v: u64;
    unsafe { asm!("rdtime {}", out(reg) v, options(nomem, nostack)) };
    v
}

/// Frequency of `time` in Hz (from the DTB `/cpus` `timebase-frequency`).
pub fn counter_hz() -> u64 {
    TIMEBASE_HZ.load(Ordering::Relaxed)
}

pub fn set_counter_hz(hz: u64) {
    if hz != 0 {
        TIMEBASE_HZ.store(hz, Ordering::SeqCst);
    }
}

/// Invalidate every TLB entry of this hart.
#[inline]
pub fn flush_tlb() {
    unsafe { asm!("sfence.vma", options(nostack)) }
}

/// This CPU's kernel index (0 = the boot hart), kept in `tp`.
#[inline]
pub fn id() -> usize {
    let v: u64;
    unsafe { asm!("mv {}, tp", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v as usize
}

pub fn set_id(cpu: usize) {
    unsafe { asm!("mv tp, {}", in(reg) cpu as u64, options(nomem, nostack)) };
}

/// Enable interrupt sources in `sie`.
pub fn sie_set(bits: u64) {
    unsafe { asm!("csrs sie, {}", in(reg) bits, options(nomem, nostack)) };
}

/// Let the kernel read and write user pages (sstatus.SUM).
pub fn allow_user_access() {
    unsafe { asm!("csrs sstatus, {}", in(reg) SSTATUS_SUM, options(nomem, nostack)) };
}

pub fn sstatus() -> u64 {
    let s: u64;
    unsafe { asm!("csrr {}, sstatus", out(reg) s, options(nomem, nostack)) };
    s
}

/// `satp` (paging mode + root page number).
pub fn satp() -> u64 {
    let v: u64;
    unsafe { asm!("csrr {}, satp", out(reg) v, options(nomem, nostack)) };
    v
}
