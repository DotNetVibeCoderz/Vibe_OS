//! The Goldfish real-time clock of QEMU's RISC-V `virt` machine
//! (`compatible = "google,goldfish-rtc"`) -- the RISC-V counterpart of the PC
//! CMOS RTC and the ARM PL031, with the same API (`crate::rtc`). The device
//! counts nanoseconds since the Unix epoch in TIME_LOW / TIME_HIGH; reading
//! TIME_LOW latches TIME_HIGH, so the low word is read first.

use core::sync::atomic::{AtomicU64, Ordering};

const TIME_LOW: u64 = 0x00;
const TIME_HIGH: u64 = 0x04;

static BASE: AtomicU64 = AtomicU64::new(0);

/// Start using the RTC mapped at virtual address `va`.
pub fn init(va: u64) {
    BASE.store(va, Ordering::SeqCst);
}

/// Nanoseconds since the epoch (0 if the device is absent).
pub fn unix_nanos() -> u64 {
    let base = BASE.load(Ordering::Relaxed);
    if base == 0 {
        return 0;
    }
    crate::arch::cpu::without_interrupts(|| unsafe {
        let lo = ((base + TIME_LOW) as *const u32).read_volatile() as u64;
        let hi = ((base + TIME_HIGH) as *const u32).read_volatile() as u64;
        (hi << 32) | lo
    })
}

/// Seconds since 1970-01-01 00:00:00 UTC (0 if the device is absent).
pub fn unix_time() -> u64 {
    unix_nanos() / 1_000_000_000
}

/// (year, month, day, hour, minute, second), UTC.
pub fn read() -> (u16, u8, u8, u8, u8, u8) {
    super::calendar::from_unix(unix_time())
}

/// (hour, minute) for the tray clock.
#[allow(dead_code)]
pub fn read_hm() -> (u8, u8) {
    let (_, _, _, h, m, _) = read();
    (h, m)
}
