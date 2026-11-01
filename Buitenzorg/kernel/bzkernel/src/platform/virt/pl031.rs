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

/// Days since the epoch -> (year, month, day). Howard Hinnant's
/// `civil_from_days`, valid for every date the 32-bit counter can hold.
fn civil_from_days(z: i64) -> (i64, u8, u8) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (year, month, day, hour, minute, second), UTC.
pub fn read() -> (u16, u8, u8, u8, u8, u8) {
    let t = unix_time();
    let (y, m, d) = civil_from_days((t / 86_400) as i64);
    let s = t % 86_400;
    (y as u16, m, d, (s / 3600) as u8, (s / 60 % 60) as u8, (s % 60) as u8)
}

/// (hour, minute) for the tray clock.
#[allow(dead_code)]
pub fn read_hm() -> (u8, u8) {
    let (_, _, _, h, m, _) = read();
    (h, m)
}
