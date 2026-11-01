//! The EL1 virtual generic timer (CNTV): a down-counter reloaded on every
//! interrupt, giving the kernel its periodic tick (PPI 27 on GICv3).

use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

/// GIC interrupt id of the EL1 virtual timer (PPI 11).
pub const INTID: u32 = 27;

static TICKS: AtomicU64 = AtomicU64::new(0);
static RELOAD: AtomicU64 = AtomicU64::new(0);

fn set_tval(v: u64) {
    unsafe { asm!("msr cntv_tval_el0, {}", "isb", in(reg) v, options(nostack)) }
}

/// Start ticking at `hz` (the PPI must be enabled in the GIC).
pub fn start(hz: u64) {
    let reload = super::cpu::counter_hz() / hz.max(1);
    RELOAD.store(reload, Ordering::SeqCst);
    set_tval(reload);
    unsafe { asm!("msr cntv_ctl_el0, {}", "isb", in(reg) 1u64, options(nostack)) }; // enable, unmasked
}

/// Every CPU's timer interrupt reloads its own timer; only CPU 0's advances
/// the system tick count (as the PIT, routed to CPU 0, does on PCs) -- the
/// other CPUs' ticks drive preemption only.
pub fn on_tick() {
    set_tval(RELOAD.load(Ordering::Relaxed));
    if super::cpu::id() == 0 {
        TICKS.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}
