//! AArch64 CPU primitives behind `crate::arch::cpu`.

use core::arch::asm;

/// Wait for an interrupt (also wakes on a pending, masked one).
#[inline]
pub fn halt() {
    unsafe { asm!("wfi", options(nomem, nostack, preserves_flags)) }
}

/// Sleep until an interrupt, then take it. On ARM a pending interrupt wakes
/// `wfi` even while masked, so waiting first and unmasking after cannot lose
/// a wake-up (the x86 `sti; hlt` pairing in one step).
#[inline]
pub fn enable_and_halt() {
    unsafe { asm!("wfi", "msr daifclr, #2", options(nomem, nostack)) }
}

#[inline]
pub fn interrupts_enabled() -> bool {
    let daif: u64;
    unsafe { asm!("mrs {}, daif", out(reg) daif, options(nomem, nostack, preserves_flags)) };
    daif & (1 << 7) == 0 // I bit clear = IRQs enabled
}

#[inline]
pub fn enable_interrupts() {
    unsafe { asm!("msr daifclr, #2", options(nomem, nostack)) }
}

#[inline]
pub fn disable_interrupts() {
    unsafe { asm!("msr daifset, #2", options(nomem, nostack)) }
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

/// The virtual counter (CNTVCT_EL0), ticking at CNTFRQ_EL0 Hz.
#[inline]
pub fn timestamp() -> u64 {
    let v: u64;
    unsafe { asm!("isb", "mrs {}, cntvct_el0", out(reg) v, options(nomem, nostack)) };
    v
}

/// Counter frequency in Hz (CNTFRQ_EL0).
pub fn counter_hz() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, cntfrq_el0", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

/// Invalidate all EL1 TLB entries of this CPU (inner shareable).
#[inline]
pub fn flush_tlb() {
    unsafe { asm!("dsb ishst", "tlbi vmalle1is", "dsb ish", "isb", options(nostack)) }
}

/// Current exception level (1 or 2).
pub fn current_el() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, CurrentEL", out(reg) v, options(nomem, nostack, preserves_flags)) };
    (v >> 2) & 3
}

/// MIDR_EL1: implementer / part number of this core.
pub fn midr() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, midr_el1", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}
