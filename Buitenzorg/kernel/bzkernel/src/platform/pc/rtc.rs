//! CMOS real-time clock (pre-v1.0 `Buitenzorg.Bcl` `System.Globalization`).
//!
//! The desktop tray clock (`wm.rs`) and the CLOCK_RTC syscall both read the
//! date/time from here. Values come back in local time exactly as the firmware
//! reports them; there is no timezone database.

use x86_64::instructions::port::Port;

/// Read a CMOS register through the index/data port pair.
unsafe fn cmos(reg: u8) -> u8 {
    let mut sel = Port::<u8>::new(0x70);
    let mut data = Port::<u8>::new(0x71);
    sel.write(reg);
    data.read()
}

/// True while the RTC is mid-update and its registers are unstable.
unsafe fn updating() -> bool {
    cmos(0x0A) & 0x80 != 0
}

/// Read (year, month, day, hour, minute, second) from the CMOS RTC.
///
/// Handles both BCD and binary encodings and both 12- and 24-hour modes, and
/// re-reads until two consecutive samples agree so an update can't tear the
/// value across a second/minute boundary.
pub fn read() -> (u16, u8, u8, u8, u8, u8) {
    unsafe {
        let mut last = read_raw();
        for _ in 0..64 {
            // Wait out an in-progress update, then sample again.
            for _ in 0..100_000 {
                if !updating() {
                    break;
                }
                core::hint::spin_loop();
            }
            let now = read_raw();
            if now == last {
                return decode(now);
            }
            last = now;
        }
        decode(last)
    }
}

/// Raw register snapshot: (sec, min, hour, day, month, year, century, statusB).
unsafe fn read_raw() -> (u8, u8, u8, u8, u8, u8, u8, u8) {
    (
        cmos(0x00),
        cmos(0x02),
        cmos(0x04),
        cmos(0x07),
        cmos(0x08),
        cmos(0x09),
        cmos(0x32),
        cmos(0x0B),
    )
}

fn decode(raw: (u8, u8, u8, u8, u8, u8, u8, u8)) -> (u16, u8, u8, u8, u8, u8) {
    let (sec, min, hour_raw, day, month, year, century, statb) = raw;
    let bcd = statb & 0x04 == 0;
    let conv = |v: u8| if bcd { (v & 0x0F) + ((v >> 4) * 10) } else { v };

    // 12-hour mode: bit 0x80 of the raw hour register is the PM flag.
    let mut hour = conv(hour_raw & 0x7F);
    if statb & 0x02 == 0 && hour_raw & 0x80 != 0 {
        hour = (hour % 12) + 12;
    }

    let yy = conv(year) as u16;
    let cc = conv(century) as u16;
    // The century register is optional; fall back to the 2000s when it is
    // absent or implausible (QEMU's CMOS reports 0 on some machine types).
    let full_year = if (19..=21).contains(&cc) { cc * 100 + yy } else { 2000 + yy };

    (full_year, conv(month), conv(day), hour, conv(min), conv(sec))
}

/// Just (hour, minute) — what the taskbar tray clock needs.
pub fn read_hm() -> (u8, u8) {
    let (_, _, _, h, m, _) = read();
    (h, m)
}
