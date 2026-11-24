//! HPET -- the High Precision Event Timer (IA-PC HPET spec 1.0a).
//!
//! Found through the ACPI `HPET` table (Generic Address Structure at +40, the
//! base address at +44). Only the main counter is used: a free-running
//! counter (32 or 64 bits) with its period in femtoseconds in the general
//! capabilities register. It is a calibration source when the 8254 PIT does
//! not count (S0ix laptops gate its clock; see `smp::calibrate_without_pit`),
//! ahead of the ACPI PM timer: 10+ MHz instead of 3.58 MHz, and memory-mapped
//! instead of a port read that costs microseconds. The comparators (and with
//! them HPET interrupts) are not used: the LAPIC timer + deadline timer do
//! that work.

use core::sync::atomic::{AtomicU64, Ordering};

const CAP: u64 = 0x000; // bits 63:32 period (fs), bit 13 64-bit counter, 12:8 timers - 1
const CONFIG: u64 = 0x010; // bit 0 ENABLE_CNF
const COUNTER: u64 = 0x0F0;

/// Virtual address of the register block (0 = none / not probed yet, 1 = absent).
static BASE: AtomicU64 = AtomicU64::new(0);
static HZ: AtomicU64 = AtomicU64::new(0);
static WIDE: AtomicU64 = AtomicU64::new(0);
static PHYS: AtomicU64 = AtomicU64::new(0);
static TIMERS: AtomicU64 = AtomicU64::new(0);

fn reg(base: u64, off: u64) -> u64 {
    unsafe { core::ptr::read_volatile((base + off) as *const u64) }
}

fn set_reg(base: u64, off: u64, v: u64) {
    unsafe { core::ptr::write_volatile((base + off) as *mut u64, v) }
}

/// Find, map and start the HPET (once). False when the machine has none.
pub fn init() -> bool {
    match BASE.load(Ordering::Acquire) {
        0 => {}
        1 => return false,
        _ => return true,
    }
    let Some(table) = crate::power::find_table(b"HPET") else {
        BASE.store(1, Ordering::Release);
        return false;
    };
    let gas = crate::power::table_u64(table, 44);
    let space = crate::power::table_u32(table, 40) & 0xFF;
    if gas == 0 || space != 0 {
        // address space 0 = system memory; an I/O-port HPET does not exist in practice
        BASE.store(1, Ordering::Release);
        return false;
    }
    let Some(base) = crate::memory::map_mmio(gas, 0x400) else {
        BASE.store(1, Ordering::Release);
        return false;
    };
    let cap = reg(base, CAP);
    let period_fs = cap >> 32;
    // the spec bounds the period to (0, 100 ns]
    if period_fs == 0 || period_fs > 100_000_000 {
        BASE.store(1, Ordering::Release);
        return false;
    }
    let cfg = reg(base, CONFIG);
    if cfg & 1 == 0 {
        set_reg(base, CONFIG, cfg | 1);
    }
    HZ.store(1_000_000_000_000_000 / period_fs, Ordering::Relaxed);
    WIDE.store((cap >> 13) & 1, Ordering::Relaxed);
    TIMERS.store(((cap >> 8) & 0x1F) + 1, Ordering::Relaxed);
    PHYS.store(gas, Ordering::Relaxed);
    BASE.store(base, Ordering::Release);
    true
}

/// Counter frequency in Hz (0 without an HPET).
pub fn hz() -> u64 {
    HZ.load(Ordering::Relaxed)
}

/// The main counter (masked to 32 bits on a 32-bit HPET).
pub fn now() -> u64 {
    let base = BASE.load(Ordering::Acquire);
    if base <= 1 {
        return 0;
    }
    let v = reg(base, COUNTER);
    if WIDE.load(Ordering::Relaxed) == 1 { v } else { v & 0xFFFF_FFFF }
}

fn elapsed(from: u64, to: u64) -> u64 {
    if WIDE.load(Ordering::Relaxed) == 1 { to.wrapping_sub(from) } else { to.wrapping_sub(from) & 0xFFFF_FFFF }
}

/// The TSC rate measured against the HPET over `us` microseconds (None
/// without an HPET, or when its counter does not move).
pub fn tsc_hz(us: u64) -> Option<u64> {
    if !init() {
        return None;
    }
    let hz = hz();
    let want = hz * us / 1_000_000;
    let start = now();
    let t = unsafe { core::arch::x86_64::_rdtsc() };
    while now() == start {
        if unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(t) > 50_000_000 {
            return None;
        }
    }
    let h0 = now();
    let c0 = unsafe { core::arch::x86_64::_rdtsc() };
    loop {
        let d = elapsed(h0, now());
        if d >= want {
            let c = unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(c0);
            return Some(c * hz / d);
        }
    }
}

/// One line for `cpu`.
pub fn summary() -> alloc::string::String {
    if !init() {
        return alloc::string::String::from("HPET: tidak ada");
    }
    alloc::format!(
        "HPET: {}.{:03} MHz, counter {}-bit, {} comparator, MMIO {:#x}",
        hz() / 1_000_000,
        hz() / 1_000 % 1_000,
        if WIDE.load(Ordering::Relaxed) == 1 { 64 } else { 32 },
        TIMERS.load(Ordering::Relaxed),
        PHYS.load(Ordering::Relaxed)
    )
}

/// The counter runs, at its stated rate: 20 ms of HPET against the TSC
/// (which the PIT, or this, calibrated). Some(report) or Err(reason); None
/// when the machine has no HPET.
pub fn self_test() -> Option<Result<alloc::string::String, alloc::string::String>> {
    if !init() {
        return None;
    }
    let a = now();
    let mut spins = 0u32;
    while now() == a {
        spins += 1;
        if spins > 10_000_000 {
            return Some(Err(alloc::string::String::from("the main counter does not move")));
        }
    }
    let tsc = crate::smp::tsc_hz();
    let Some(measured) = tsc_hz(20_000) else {
        return Some(Err(alloc::string::String::from("could not time the TSC against it")));
    };
    // under TCG both clocks are emulated: accept 25 % off the calibrated TSC
    if tsc != 0 {
        let diff = measured.abs_diff(tsc) * 100 / tsc;
        if diff > 25 {
            return Some(Err(alloc::format!("TSC by HPET {} MHz vs calibrated {} MHz", measured / 1_000_000, tsc / 1_000_000)));
        }
    }
    Some(Ok(alloc::format!("{}; TSC by HPET {} MHz (calibrated {} MHz)", summary(), measured / 1_000_000, tsc / 1_000_000)))
}
