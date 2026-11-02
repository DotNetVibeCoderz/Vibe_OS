//! ARM PL031 real-time clock (QEMU `virt`: `compatible = "arm,pl031"`) — the
//! ARM64 counterpart of the PC CMOS RTC, with the same API (`crate::rtc`).
//! The device keeps seconds since the Unix epoch in RTCDR; the calendar date
//! is computed from that (UTC, proleptic Gregorian).
//!
//! Without a PL031 (a Raspberry Pi 4 has no real-time clock at all) the time
//! comes from a **software clock**: a base plus the monotonic time since boot.
//! The base starts at the kernel's build time and `set_time` (SNTP over the
//! network) replaces it, so the clock always runs and is right once synced.

use core::sync::atomic::{AtomicU64, Ordering};

const RTCDR: u64 = 0x000;

static BASE: AtomicU64 = AtomicU64::new(0);
/// Software clock: Unix seconds at monotonic time 0 (0 = not set yet).
static SOFT_EPOCH: AtomicU64 = AtomicU64::new(0);

const BUILD_UNIX: u64 = match u64::from_str_radix(env!("BZ_BUILD_UNIX"), 10) {
    Ok(v) => v,
    Err(_) => 0,
};

fn uptime_s() -> u64 {
    crate::arch::cpu::nanos() / 1_000_000_000
}

/// Is a hardware clock present (else the software clock is used)?
pub fn hardware() -> bool {
    BASE.load(Ordering::Relaxed) != 0
}

/// Set the clock (software clock only; a PL031 keeps its own time).
pub fn set_time(unix: u64) {
    SOFT_EPOCH.store(unix.saturating_sub(uptime_s()), Ordering::SeqCst);
}

/// Was the software clock set from a real source (not the build time)?
pub fn synced() -> bool {
    SOFT_EPOCH.load(Ordering::Relaxed) != 0
}

/// Start using the PL031 mapped at virtual address `va`.
pub fn init(va: u64) {
    BASE.store(va, Ordering::SeqCst);
}

/// Seconds since 1970-01-01 00:00:00 UTC (0 if the device is absent).
pub fn unix_time() -> u64 {
    let base = BASE.load(Ordering::Relaxed);
    if base == 0 {
        let epoch = match SOFT_EPOCH.load(Ordering::Relaxed) {
            // the build time is UTC; the clock shows local time (WIB, as SNTP sets it)
            0 => BUILD_UNIX + 7 * 3600,
            e => e,
        };
        return epoch + uptime_s();
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
