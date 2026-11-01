//! A GUI app on RISC-V: create a window through the shared window manager
//! (WIN_CREATE), draw into it with WIN_CMD -- a gradient, shapes, text with
//! the current time from CLOCK_RTC and the screen size from FB_INFO -- and
//! present it (WIN_PRESENT). The window stays on the desktop after exit.
#![no_std]
#![no_main]
mod rt;
use rt::*;

const FB_INFO: u64 = 5;
const WIN_CREATE: u64 = 6;
const WIN_CMD: u64 = 7;
const WIN_PRESENT: u64 = 8;
const CLOCK_RTC: u64 = 33;

const FILL_RECT: u64 = 0;
const DRAW_TEXT: u64 = 1;
const CLEAR: u64 = 2;
const LINE: u64 = 3;
const FILL_ELLIPSE: u64 = 5;
const RECT: u64 = 6;

/// `bz_abi::DrawCmd`.
#[repr(C)]
struct DrawCmd {
    op: u64,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: u32,
    _pad: u32,
    text_ptr: u64,
    text_len: u64,
}

fn cmd(win: u64, op: u64, x: i32, y: i32, w: i32, h: i32, color: u32, text: &[u8]) -> bool {
    let c = DrawCmd { op, x, y, w, h, color, _pad: 0, text_ptr: text.as_ptr() as u64, text_len: text.len() as u64 };
    syscall(WIN_CMD, win, &c as *const DrawCmd as u64, 0) == 0
}

/// Write `v` as two digits at `out[0..2]`.
fn two(out: &mut [u8], v: u64) {
    out[0] = b'0' + (v / 10 % 10) as u8;
    out[1] = b'0' + (v % 10) as u8;
}

/// Decimal digits of `v` into `out`; the length.
fn digits(out: &mut [u8], mut v: u64) -> usize {
    let mut tmp = [0u8; 20];
    let mut n = 0;
    loop {
        tmp[n] = b'0' + (v % 10) as u8;
        n += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    for i in 0..n {
        out[i] = tmp[n - 1 - i];
    }
    n
}

fn main(_arg: u64) -> u64 {
    let title = "Halo RISC-V";
    let win = syscall(WIN_CREATE, title.as_ptr() as u64, title.len() as u64, (360u64 << 32) | 220);
    if win == 0 {
        print("gui: WIN_CREATE failed\n");
        return 1;
    }
    let mut ok = cmd(win, CLEAR, 0, 0, 0, 0, 0x10_2A1C, b"");
    // A green-to-teal gradient in 8 bands.
    for i in 0..8 {
        let g = 0x60 + i * 0x14;
        ok &= cmd(win, FILL_RECT, 0, (i * 12) as i32, 360, 12, (0x20 << 16) | (g << 8) | (0x40 + i * 0x10), b"");
    }
    ok &= cmd(win, FILL_ELLIPSE, 270, 110, 70, 70, 0xF0_C040, b"");
    ok &= cmd(win, RECT, 12, 104, 240, 96, 0xE0_FFE0, b"");
    ok &= cmd(win, LINE, 12, 104, 240, 96, 0x80_C080, b"");
    ok &= cmd(win, DRAW_TEXT, 16, 8, 0, 0, 0xFF_FFFF, b"Buitenzorg OS di RISC-V");
    ok &= cmd(win, DRAW_TEXT, 16, 30, 0, 0, 0xE0_FFE0, b"aplikasi U-mode (Rust), ECALL");

    // "Jam HH:MM:SS" from the RTC.
    let mut t = [0u64; 6];
    let rtc_ok = syscall(CLOCK_RTC, t.as_mut_ptr() as u64, 0, 0) == 0 && t[0] >= 2024;
    let mut line = *b"Jam 00:00:00 UTC";
    two(&mut line[4..], t[3]);
    two(&mut line[7..], t[4]);
    two(&mut line[10..], t[5]);
    ok &= cmd(win, DRAW_TEXT, 24, 120, 0, 0, 0xFF_FFFF, &line);

    // "Layar WxH" from FB_INFO.
    let mut fb = [0u64; 7];
    let fb_ok = syscall(FB_INFO, fb.as_mut_ptr() as u64, 0, 0) == 0 && fb[2] > 0;
    let mut s = [0u8; 40];
    let mut n = 0;
    for &b in b"Layar " {
        s[n] = b;
        n += 1;
    }
    n += digits(&mut s[n..], fb[2]);
    s[n] = b'x';
    n += 1;
    n += digits(&mut s[n..], fb[3]);
    ok &= cmd(win, DRAW_TEXT, 24, 150, 0, 0, 0xFF_FFFF, &s[..n]);
    syscall(WIN_PRESENT, win, 0, 0);

    print("gui: window ");
    print_u64(win);
    print(if ok && rtc_ok && fb_ok { " drawn (shapes, text, RTC, FB_INFO) and presented\n" } else { " FAILED\n" });
    if ok && rtc_ok && fb_ok { 0 } else { 1 }
}
