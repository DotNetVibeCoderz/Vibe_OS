//! Seconds since the Unix epoch -> calendar date (UTC, proleptic Gregorian),
//! shared by the `virt` boards' clocks (ARM PL031, RISC-V goldfish RTC).

/// Days since the epoch -> (year, month, day). Howard Hinnant's
/// `civil_from_days`, valid far beyond any date these clocks can hold.
pub fn civil_from_days(z: i64) -> (i64, u8, u8) {
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

/// Unix seconds -> (year, month, day, hour, minute, second), UTC.
pub fn from_unix(t: u64) -> (u16, u8, u8, u8, u8, u8) {
    let (y, m, d) = civil_from_days((t / 86_400) as i64);
    let s = t % 86_400;
    (y as u16, m, d, (s / 3600) as u8, (s / 60 % 60) as u8, (s % 60) as u8)
}
