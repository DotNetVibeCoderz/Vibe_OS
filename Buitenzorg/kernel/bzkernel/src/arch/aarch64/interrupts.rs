//! Time base for generic code (`crate::interrupts::ticks()`), in the same
//! unit as on the PC: PIT ticks at ~18.2 Hz. Generic code times things in
//! those (e.g. "36 ticks = ~2 s"), so the 100 Hz ARM timer is scaled.

/// Ticks since boot in PIT units (18.2 per second).
pub fn ticks() -> u64 {
    super::timer::ticks() * 182 / 1000
}
