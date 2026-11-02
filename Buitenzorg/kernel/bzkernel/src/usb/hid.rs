//! HID boot-protocol reports -> the kernel's keyboard and mouse input.
//!
//! A boot keyboard report is 8 bytes: modifier bits, a reserved byte, and up
//! to six usage codes of keys held down. Comparing it with the previous report
//! gives presses and releases, which become PC set-1 scancodes for
//! `keyboard::feed_scancode` -- the decoder PS/2 and virtio-input use, so
//! layouts, modifiers and the terminal/app routing all stay the same. A boot
//! mouse report is buttons, dx, dy (signed bytes).

use core::sync::atomic::{AtomicUsize, Ordering};

pub static KEYS: AtomicUsize = AtomicUsize::new(0);
pub static POINTER: AtomicUsize = AtomicUsize::new(0);

/// Modifier bit -> (extended, set-1 make code): LCtrl LShift LAlt LGUI RCtrl RShift RAlt RGUI.
const MODIFIERS: [(bool, u8); 8] = [
    (false, 0x1D),
    (false, 0x2A),
    (false, 0x38),
    (true, 0x5B),
    (true, 0x1D),
    (false, 0x36),
    (true, 0x38),
    (true, 0x5C),
];

/// HID keyboard usage (page 7) -> (extended, set-1 make code).
fn scancode(usage: u8) -> Option<(bool, u8)> {
    const LETTERS: [u8; 26] = [
        0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18, 0x19, 0x10, 0x13, 0x1F, 0x14,
        0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C,
    ];
    let plain = |c| Some((false, c));
    let ext = |c| Some((true, c));
    match usage {
        0x04..=0x1D => plain(LETTERS[(usage - 0x04) as usize]),
        0x1E..=0x27 => plain(usage - 0x1E + 0x02), // 1..9, 0
        0x28 => plain(0x1C),                       // Enter
        0x29 => plain(0x01),                       // Esc
        0x2A => plain(0x0E),                       // Backspace
        0x2B => plain(0x0F),                       // Tab
        0x2C => plain(0x39),                       // Space
        0x2D => plain(0x0C),                       // -
        0x2E => plain(0x0D),                       // =
        0x2F => plain(0x1A),                       // [
        0x30 => plain(0x1B),                       // ]
        0x31 | 0x32 => plain(0x2B),                // \ and non-US #
        0x33 => plain(0x27),                       // ;
        0x34 => plain(0x28),                       // '
        0x35 => plain(0x29),                       // `
        0x36 => plain(0x33),                       // ,
        0x37 => plain(0x34),                       // .
        0x38 => plain(0x35),                       // /
        0x39 => plain(0x3A),                       // Caps Lock
        0x3A..=0x43 => plain(usage - 0x3A + 0x3B), // F1..F10
        0x44 => plain(0x57),                       // F11
        0x45 => plain(0x58),                       // F12
        0x47 => plain(0x46),                       // Scroll Lock
        0x49 => ext(0x52),                         // Insert
        0x4A => ext(0x47),                         // Home
        0x4B => ext(0x49),                         // Page Up
        0x4C => ext(0x53),                         // Delete
        0x4D => ext(0x4F),                         // End
        0x4E => ext(0x51),                         // Page Down
        0x4F => ext(0x4D),                         // Right
        0x50 => ext(0x4B),                         // Left
        0x51 => ext(0x50),                         // Down
        0x52 => ext(0x48),                         // Up
        0x53 => plain(0x45),                       // Num Lock
        0x54 => ext(0x35),                         // keypad /
        0x55 => plain(0x37),                       // keypad *
        0x56 => plain(0x4A),                       // keypad -
        0x57 => plain(0x4E),                       // keypad +
        0x58 => ext(0x1C),                         // keypad Enter
        0x59 => plain(0x4F),                       // keypad 1
        0x5A => plain(0x50),
        0x5B => plain(0x51),
        0x5C => plain(0x4B),
        0x5D => plain(0x4C),
        0x5E => plain(0x4D),
        0x5F => plain(0x47),
        0x60 => plain(0x48),
        0x61 => plain(0x49),
        0x62 => plain(0x52), // keypad 0
        0x63 => plain(0x53), // keypad .
        0x64 => plain(0x56), // non-US \
        _ => None,
    }
}

fn emit((extended, code): (bool, u8), pressed: bool) {
    if extended {
        crate::keyboard::feed_scancode(0xE0);
    }
    crate::keyboard::feed_scancode(if pressed { code } else { code | 0x80 });
    KEYS.fetch_add(1, Ordering::Relaxed);
}

/// Turn the change between two boot keyboard reports into key events.
pub fn keyboard(old: &[u8; 8], new: &[u8; 8]) {
    // usage 0x01 in every slot = "too many keys" (phantom state): ignore it
    if new[2..].iter().all(|&k| k == 0x01) {
        return;
    }
    let changed = old[0] ^ new[0];
    for (bit, m) in MODIFIERS.iter().enumerate() {
        if changed & (1 << bit) != 0 {
            emit(*m, new[0] & (1 << bit) != 0);
        }
    }
    for &k in &old[2..] {
        if k > 3 && !new[2..].contains(&k) {
            if let Some(sc) = scancode(k) {
                emit(sc, false);
            }
        }
    }
    for &k in &new[2..] {
        if k > 3 && !old[2..].contains(&k) {
            if let Some(sc) = scancode(k) {
                emit(sc, true);
            }
        }
    }
}

/// A boot mouse report: buttons, dx, dy (and maybe a wheel byte, ignored).
pub fn mouse(r: &[u8]) {
    if r.len() < 3 {
        return;
    }
    crate::mouse::inject_relative(r[1] as i8 as i64, r[2] as i8 as i64, r[0] & 7);
    POINTER.fetch_add(1, Ordering::Relaxed);
}
