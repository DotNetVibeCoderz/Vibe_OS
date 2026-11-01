//! ARM PL031 real-time clock (QEMU `virt`: `compatible = "arm,pl031"`) — the
//! ARM64 counterpart of the PC CMOS RTC, with the same API (`crate::rtc`).
//! The device keeps seconds since the Unix epoch in RTCDR; the calendar date
//! is computed from that (UTC, proleptic Gregorian).

use core::sync::atomic::{AtomicU64, Ordering};

const RTCDR: u64 = 0x000;

static BASE: AtomicU64 = AtomicU64::new(0);

/// Start using the PL031 mapped at virtual address `va`.
pub fn init(va: u64) {
    BASE.store(va, Ordering::SeqCst);
}

/// Seconds since 1970-01-01 00:00:00 UTC (0 if the device is absent).
pub fn unix_time() -> u64 {
    let base = BASE.load(Ordering::Relaxed);
    if base == 0 {
        return 0;
    }
    unsafe { ((base + RTCDR) as *const u32).read_volatile() as u64 }
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
