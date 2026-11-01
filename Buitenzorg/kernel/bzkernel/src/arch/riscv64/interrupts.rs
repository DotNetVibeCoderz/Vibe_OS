//! Time base for generic code (`crate::interrupts::ticks()`), in the same
//! unit as on the PC: PIT ticks at ~18.2 Hz (the RISC-V timer runs at 100 Hz).

/// Ticks since boot in PIT units (18.2 per second).
pub fn ticks() -> u64 {
    super::timer::ticks() * 182 / 1000
}
