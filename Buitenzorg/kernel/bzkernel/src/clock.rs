//! Local time, time zone and region settings (the "Tanggal & Waktu" form and
//! the tray clock).
//!
//! Every architecture's `crate::rtc` is read as **UTC** (QEMU's CMOS RTC,
//! PL031 and goldfish RTC hold UTC; the Pi's software clock is set to UTC by
//! SNTP). Local time = that + the chosen time zone + the user's correction
//! (what "set date / time" changes -- it is kept as an offset, so it also
//! works where the clock is not writable). Settings persist in
//! `/disk/REGION.CFG` (`tz=`, `lang=`, `h24=`, `adjust=` lines).

use core::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};

/// (short name, places, minutes east of UTC).
pub const ZONES: [(&str, &str, i64); 12] = [
    ("WIB", "Jakarta, Bandung, Pontianak (UTC+7)", 7 * 60),
    ("WITA", "Denpasar, Makassar, Balikpapan (UTC+8)", 8 * 60),
    ("WIT", "Jayapura, Ambon, Manokwari (UTC+9)", 9 * 60),
    ("UTC", "Waktu universal (UTC+0)", 0),
    ("SGT", "Singapura, Kuala Lumpur (UTC+8)", 8 * 60),
    ("ICT", "Bangkok, Hanoi (UTC+7)", 7 * 60),
    ("JST", "Tokyo, Seoul (UTC+9)", 9 * 60),
    ("AEST", "Sydney, Melbourne (UTC+10)", 10 * 60),
    ("IST", "New Delhi, Mumbai (UTC+5:30)", 5 * 60 + 30),
    ("GST", "Dubai, Abu Dhabi (UTC+4)", 4 * 60),
    ("CET", "Amsterdam, Berlin, Paris (UTC+1)", 60),
    ("EST", "New York, Toronto (UTC-5)", -5 * 60),
];

/// (code, name) of the interface languages.
pub const LANGUAGES: [(&str, &str); 2] = [("id", "Bahasa Indonesia"), ("en", "English")];

static ZONE: AtomicUsize = AtomicUsize::new(0); // WIB
static LANG: AtomicUsize = AtomicUsize::new(0);
static H24: AtomicBool = AtomicBool::new(true);
static ADJUST: AtomicI64 = AtomicI64::new(0);

const CONFIG: &str = "/disk/REGION.CFG";

const DAYS_ID: [&str; 7] = ["Minggu", "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu"];
const DAYS_EN: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS_ID: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];
const MONTHS_EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Days since 1970-01-01 of a civil date (Hinnant).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Civil date of a day count since 1970-01-01 (Hinnant).
pub fn civil_from_days(z: i64) -> (i64, u8, u8) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

/// The hardware clock as Unix seconds (UTC).
pub fn utc_unix() -> i64 {
    let (y, mo, d, h, mi, s) = crate::rtc::read();
    days_from_civil(y as i64, mo as i64, d as i64) * 86_400 + h as i64 * 3600 + mi as i64 * 60 + s as i64
}

/// Seconds of the chosen zone east of UTC.
pub fn zone_offset_s() -> i64 {
    ZONES[ZONE.load(Ordering::Relaxed).min(ZONES.len() - 1)].2 * 60
}

/// Local time as Unix-style seconds (local midnight = a multiple of 86400).
pub fn local_unix() -> i64 {
    utc_unix() + zone_offset_s() + ADJUST.load(Ordering::Relaxed)
}

/// Local (year, month, day, hour, minute, second).
pub fn now() -> (u16, u8, u8, u8, u8, u8) {
    split(local_unix())
}

pub fn split(t: i64) -> (u16, u8, u8, u8, u8, u8) {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    (y as u16, m, d, (secs / 3600) as u8, (secs / 60 % 60) as u8, (secs % 60) as u8)
}

/// Set the local date and time (seconds as from [`local_unix`]).
pub fn set_local(t: i64) {
    ADJUST.store(t - utc_unix() - zone_offset_s(), Ordering::Relaxed);
    save();
}

pub fn zone() -> usize {
    ZONE.load(Ordering::Relaxed)
}

/// Change the time zone; the user's correction stays as it is.
pub fn set_zone(i: usize) -> bool {
    if i >= ZONES.len() {
        return false;
    }
    ZONE.store(i, Ordering::Relaxed);
    save();
    true
}

pub fn language() -> usize {
    LANG.load(Ordering::Relaxed)
}

pub fn set_language(i: usize) -> bool {
    if i >= LANGUAGES.len() {
        return false;
    }
    LANG.store(i, Ordering::Relaxed);
    save();
    true
}

pub fn h24() -> bool {
    H24.load(Ordering::Relaxed)
}

pub fn set_h24(on: bool) {
    H24.store(on, Ordering::Relaxed);
    save();
}

/// Day name in the interface language.
pub fn day_name(wd: usize) -> &'static str {
    if language() == 1 { DAYS_EN[wd % 7] } else { DAYS_ID[wd % 7] }
}

/// Short month name in the interface language.
pub fn month_name(m: u8) -> &'static str {
    let i = (m.clamp(1, 12) - 1) as usize;
    if language() == 1 { MONTHS_EN[i] } else { MONTHS_ID[i] }
}

/// Read `/disk/REGION.CFG` (call once `/disk` is mounted).
pub fn load() {
    let Ok(bytes) = crate::vfs::read(CONFIG) else { return };
    let Ok(text) = core::str::from_utf8(&bytes) else { return };
    for line in text.lines() {
        let Some((k, v)) = line.trim().split_once('=') else { continue };
        let Ok(n) = v.trim().parse::<i64>() else { continue };
        match k.trim() {
            "tz" if (0..ZONES.len() as i64).contains(&n) => ZONE.store(n as usize, Ordering::Relaxed),
            "lang" if (0..LANGUAGES.len() as i64).contains(&n) => LANG.store(n as usize, Ordering::Relaxed),
            "h24" => H24.store(n != 0, Ordering::Relaxed),
            "adjust" if n.abs() < 100 * 365 * 86_400 => ADJUST.store(n, Ordering::Relaxed),
            _ => {}
        }
    }
    crate::println!(
        "[clock] region: {} ({}), language {}, {}-hour clock, correction {} s",
        ZONES[zone()].0,
        ZONES[zone()].1,
        LANGUAGES[language()].0,
        if h24() { 24 } else { 12 },
        ADJUST.load(Ordering::Relaxed)
    );
}

fn save() {
    let s = alloc::format!(
        "tz={}\r\nlang={}\r\nh24={}\r\nadjust={}\r\n",
        zone(),
        language(),
        h24() as u8,
        ADJUST.load(Ordering::Relaxed)
    );
    let _ = crate::vfs::write(CONFIG, s.as_bytes());
}
