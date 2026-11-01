//! The periodic tick from the SBI timer: each interrupt programs the next
//! deadline (`sbi::set_timer`, absolute `time` value). Only CPU 0 advances
//! the system tick (as the PIT routed to CPU 0 does on PCs).

use core::sync::atomic::{AtomicU64, Ordering};

static TICKS: AtomicU64 = AtomicU64::new(0);
static RELOAD: AtomicU64 = AtomicU64::new(0);

/// Start ticking at `hz` on this hart.
pub fn start(hz: u64) {
    let reload = super::cpu::counter_hz() / hz.max(1);
    RELOAD.store(reload, Ordering::SeqCst);
    super::sbi::set_timer(super::cpu::timestamp() + reload);
    super::cpu::sie_set(super::cpu::SIE_STIE);
}

pub fn on_tick() {
    super::sbi::set_timer(super::cpu::timestamp() + RELOAD.load(Ordering::Relaxed));
    if super::cpu::id() == 0 {
        TICKS.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}
