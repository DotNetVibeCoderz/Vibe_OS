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

/// A boot mouse report: buttons, dx, dy and -- most mice send it even in the
/// boot protocol -- a wheel byte (positive = away from the user).
pub fn mouse(r: &[u8]) {
    if r.len() < 3 {
        return;
    }
    crate::mouse::inject_relative(r[1] as i8 as i64, r[2] as i8 as i64, r[0] & 7);
    if r.len() >= 4 {
        crate::mouse::inject_wheel(r[3] as i8 as i64);
    }
    POINTER.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Gamepads: HID report descriptors (generic USB gamepads / joysticks) and the
// fixed Xbox 360 (XInput) report. Every pad is reduced to one standard layout
// of raw key codes (`keyboard::PAD_A` ..): A B X Y L R Select Start and the
// d-pad, which a hat switch, d-pad usages or the left stick drive.

pub static PADS: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, PartialEq, Debug)]
enum PadUse {
    /// HID button n (1-based).
    Button(u32),
    X,
    Y,
    Hat,
    DUp,
    DDown,
    DRight,
    DLeft,
}

#[derive(Clone, Copy, Debug)]
struct PadField {
    id: u8,
    bit: u32,
    size: u32,
    min: i32,
    max: i32,
    what: PadUse,
}

/// One gamepad's report layout and the buttons it holds down now.
pub struct Pad {
    fields: alloc::vec::Vec<PadField>,
    uses_ids: bool,
    xinput: bool,
    /// bits 0..7 = A B X Y L R Select Start, 16..19 = Up Down Left Right
    pub state: u32,
}

/// Bit of `Pad::state` -> raw key code.
fn pad_code(bit: u32) -> u32 {
    if bit < 16 {
        crate::keyboard::PAD_A + bit
    } else {
        crate::keyboard::PAD_UP + bit - 16
    }
}

const UP: u32 = 1 << 16;
const DOWN: u32 = 1 << 17;
const LEFT: u32 = 1 << 18;
const RIGHT: u32 = 1 << 19;

fn bits_le(r: &[u8], bit: u32, size: u32) -> u32 {
    let mut v = 0u32;
    for k in 0..size.min(32) {
        let b = bit + k;
        let byte = (b / 8) as usize;
        if byte < r.len() && r[byte] >> (b % 8) & 1 != 0 {
            v |= 1 << k;
        }
    }
    v
}

impl Pad {
    /// The layout of an Xbox 360 controller's 20-byte input report (vendor
    /// class 0xFF, subclass 0x5D, protocol 1: no report descriptor).
    pub fn xinput() -> Pad {
        Pad { fields: alloc::vec::Vec::new(), uses_ids: false, xinput: true, state: 0 }
    }

    /// Parse a HID report descriptor; None unless its application collection
    /// is a joystick (usage 4), gamepad (5) or multi-axis controller (8) with
    /// buttons.
    pub fn parse(d: &[u8]) -> Option<Pad> {
        let (mut page, mut lmin, mut lmax, mut rsize, mut rcount, mut rid) = (0u32, 0i32, 0i32, 0u32, 0u32, 0u8);
        let mut stack: alloc::vec::Vec<(u32, i32, i32, u32, u32, u8)> = alloc::vec::Vec::new();
        let mut usages: alloc::vec::Vec<u32> = alloc::vec::Vec::new();
        let (mut umin, mut umax) = (None::<u32>, None::<u32>);
        let mut offsets = [0u32; 256];
        let mut fields = alloc::vec::Vec::new();
        let (mut is_pad, mut uses_ids, mut depth) = (false, false, 0u32);
        let mut i = 0usize;
        while i < d.len() {
            let pre = d[i];
            if pre == 0xFE {
                // long item: skip it
                let n = d.get(i + 1).copied().unwrap_or(0) as usize;
                i += 3 + n;
                continue;
            }
            let size = match pre & 3 {
                3 => 4,
                s => s as usize,
            };
            if i + 1 + size > d.len() {
                break;
            }
            let mut u = 0u32;
            for k in 0..size {
                u |= (d[i + 1 + k] as u32) << (8 * k);
            }
            let signed = match size {
                1 => u as u8 as i8 as i32,
                2 => u as u16 as i16 as i32,
                _ => u as i32,
            };
            let (typ, tag) = ((pre >> 2) & 3, pre >> 4);
            i += 1 + size;
            let full = |usage: u32| if size == 4 { usage } else { page << 16 | usage };
            match (typ, tag) {
                (1, 0) => page = u,
                (1, 1) => lmin = signed,
                // a positive minimum: read the maximum unsigned (0..255 as 0x26 FF 00 or 0x25 FF)
                (1, 2) => lmax = if lmin >= 0 && size < 4 { u as i32 } else { signed },
                (1, 7) => rsize = u,
                (1, 8) => {
                    rid = u as u8;
                    uses_ids = true;
                }
                (1, 9) => rcount = u,
                (1, 10) => stack.push((page, lmin, lmax, rsize, rcount, rid)),
                (1, 11) => {
                    if let Some(s) = stack.pop() {
                        (page, lmin, lmax, rsize, rcount, rid) = s;
                    }
                }
                (2, 0) => usages.push(full(u)),
                (2, 1) => umin = Some(full(u)),
                (2, 2) => umax = Some(full(u)),
                (0, 10) => {
                    // collection: the outermost application one decides
                    if depth == 0 && u == 1 {
                        if let Some(&us) = usages.first() {
                            is_pad |= us == 0x1_0004 || us == 0x1_0005 || us == 0x1_0008;
                        }
                    }
                    depth += 1;
                    usages.clear();
                    (umin, umax) = (None, None);
                }
                (0, 12) => depth = depth.saturating_sub(1),
                (0, 8) => {
                    // Input
                    let var = u & 2 != 0;
                    let cnst = u & 1 != 0;
                    let base = offsets[rid as usize];
                    if var && !cnst {
                        for k in 0..rcount {
                            let usage = if let (Some(a), Some(b)) = (umin, umax) {
                                (a + k).min(b)
                            } else if !usages.is_empty() {
                                usages[(k as usize).min(usages.len() - 1)]
                            } else {
                                0
                            };
                            let what = match usage {
                                x if x >> 16 == 9 => Some(PadUse::Button(x & 0xFFFF)),
                                0x1_0030 => Some(PadUse::X),
                                0x1_0031 => Some(PadUse::Y),
                                0x1_0039 => Some(PadUse::Hat),
                                0x1_0090 => Some(PadUse::DUp),
                                0x1_0091 => Some(PadUse::DDown),
                                0x1_0092 => Some(PadUse::DRight),
                                0x1_0093 => Some(PadUse::DLeft),
                                _ => None,
                            };
                            if let Some(what) = what {
                                fields.push(PadField { id: rid, bit: base + k * rsize, size: rsize, min: lmin, max: lmax, what });
                            }
                        }
                    }
                    offsets[rid as usize] = base + rsize * rcount;
                    usages.clear();
                    (umin, umax) = (None, None);
                }
                (0, _) => {
                    usages.clear();
                    (umin, umax) = (None, None);
                }
                _ => {}
            }
        }
        let buttons = fields.iter().any(|f| matches!(f.what, PadUse::Button(_)));
        (is_pad && buttons).then_some(Pad { fields, uses_ids, xinput: false, state: 0 })
    }

    /// The buttons held according to one input report.
    pub fn decode(&self, r: &[u8]) -> u32 {
        if self.xinput {
            if r.len() < 10 || r[0] != 0 {
                return self.state; // not an input report (LED / rumble status)
            }
            let (a, b) = (r[2] as u32, r[3] as u32);
            let mut s = 0;
            // (byte, bit, state bit): A B X Y LB RB, Back -> Select, Start, d-pad
            let map = [(b, 4, 0), (b, 5, 1), (b, 6, 2), (b, 7, 3), (b, 0, 4), (b, 1, 5), (a, 5, 6), (a, 4, 7), (a, 0, 16), (a, 1, 17), (a, 2, 18), (a, 3, 19)];
            for (byte, bit, to) in map {
                if byte >> bit & 1 != 0 {
                    s |= 1 << to;
                }
            }
            let lx = i16::from_le_bytes([r[6], r[7]]) as i32;
            let ly = i16::from_le_bytes([r[8], r[9]]) as i32;
            if lx < -16384 {
                s |= LEFT;
            }
            if lx > 16384 {
                s |= RIGHT;
            }
            if ly > 16384 {
                s |= UP; // XInput: up is positive
            }
            if ly < -16384 {
                s |= DOWN;
            }
            return s;
        }
        let (id, body) = if self.uses_ids && !r.is_empty() { (r[0], &r[1..]) } else { (0, r) };
        if self.uses_ids && !self.fields.iter().any(|f| f.id == id) {
            return self.state; // another report of a composite device
        }
        let mut s = 0u32;
        for f in self.fields.iter().filter(|f| f.id == id) {
            let raw = bits_le(body, f.bit, f.size);
            let v = if f.min < 0 && f.size > 0 && f.size < 32 && raw >> (f.size - 1) & 1 != 0 { (raw as i32) - (1 << f.size) } else { raw as i32 };
            let range = (f.max - f.min).max(1);
            match f.what {
                PadUse::Button(n) => {
                    if v != 0 {
                        // 1..8 = A B X Y L R Select Start; 9/10 are Select/Start on many pads
                        let bit = match n {
                            1..=8 => n - 1,
                            9 => 6,
                            10 => 7,
                            _ => continue,
                        };
                        s |= 1 << bit;
                    }
                }
                PadUse::X => {
                    if v < f.min + range / 4 {
                        s |= LEFT;
                    }
                    if v > f.max - range / 4 {
                        s |= RIGHT;
                    }
                }
                PadUse::Y => {
                    if v < f.min + range / 4 {
                        s |= UP;
                    }
                    if v > f.max - range / 4 {
                        s |= DOWN;
                    }
                }
                PadUse::Hat => {
                    if v >= f.min && v <= f.max {
                        let p = (v - f.min) as usize;
                        s |= if range == 3 {
                            [UP, RIGHT, DOWN, LEFT][p]
                        } else {
                            [UP, UP | RIGHT, RIGHT, DOWN | RIGHT, DOWN, DOWN | LEFT, LEFT, UP | LEFT][p.min(7)]
                        };
                    }
                }
                PadUse::DUp if v != 0 => s |= UP,
                PadUse::DDown if v != 0 => s |= DOWN,
                PadUse::DRight if v != 0 => s |= RIGHT,
                PadUse::DLeft if v != 0 => s |= LEFT,
                _ => {}
            }
        }
        s
    }

    /// Feed one report: every button that changed becomes a raw key event.
    pub fn feed(&mut self, r: &[u8]) {
        let new = self.decode(r);
        self.set(new);
    }

    /// Release everything held (the pad was unplugged).
    pub fn release(&mut self) {
        self.set(0);
    }

    fn set(&mut self, new: u32) {
        let diff = self.state ^ new;
        for bit in 0..32 {
            if diff >> bit & 1 != 0 {
                crate::keyboard::key_change(pad_code(bit), new >> bit & 1 != 0);
            }
        }
        self.state = new;
    }

    pub fn summary(&self) -> alloc::string::String {
        if self.xinput {
            return alloc::string::String::from("XInput layout");
        }
        let buttons = self.fields.iter().filter(|f| matches!(f.what, PadUse::Button(_))).count();
        let hat = self.fields.iter().any(|f| f.what == PadUse::Hat);
        let axes = self.fields.iter().filter(|f| matches!(f.what, PadUse::X | PadUse::Y)).count();
        alloc::format!("{} buttons, {} axes{}", buttons, axes, if hat { ", hat switch" } else { "" })
    }
}

/// Self-test (QEMU models no gamepad): a typical generic USB gamepad
/// descriptor (X/Y bytes, a 4-bit hat, 12 buttons) and an Xbox 360 report are
/// decoded, and decoded presses reach the raw key state and event queue.
/// Requires every mapping; prints `MILESTONE: GAMEPAD OK`.
pub fn pad_self_test() -> bool {
    #[rustfmt::skip]
    const DESC: &[u8] = &[
        0x05, 0x01, 0x09, 0x05, 0xA1, 0x01,             // Generic Desktop, Gamepad, Application
        0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x02, // 0..255, 8 bits x 2
        0x09, 0x30, 0x09, 0x31, 0x81, 0x02,             // X, Y: Input (Var)
        0x15, 0x00, 0x25, 0x07, 0x75, 0x04, 0x95, 0x01, // 0..7, 4 bits
        0x65, 0x14, 0x09, 0x39, 0x81, 0x42,             // Hat switch (Var, Null state)
        0x75, 0x04, 0x95, 0x01, 0x81, 0x03,             // 4 bits padding (Const)
        0x05, 0x09, 0x19, 0x01, 0x29, 0x0C,             // Buttons 1..12
        0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x0C, 0x81, 0x02,
        0x75, 0x04, 0x95, 0x01, 0x81, 0x03,             // padding
        0xC0,
    ];
    let mut ok = true;
    let mut check = |c: bool, what: &str| {
        if !c {
            crate::println!("[gamepad] FAIL: {}", what);
            ok = false;
        }
    };
    let Some(mut p) = Pad::parse(DESC) else {
        crate::println!("[gamepad] FAIL: descriptor not recognized as a gamepad");
        return false;
    };
    check(p.fields.len() == 15, "2 axes + hat + 12 buttons");
    // centered stick, hat neutral (8 = outside 0..7), nothing pressed
    check(p.decode(&[0x80, 0x80, 0x08, 0x00, 0x00]) == 0, "idle report");
    check(p.decode(&[0x80, 0x80, 0x08, 0x01, 0x00]) == 1, "button 1 = A");
    check(p.decode(&[0x80, 0x80, 0x08, 0x00, 0x02]) == 1 << 7, "button 10 = Start");
    check(p.decode(&[0x80, 0x80, 0x00, 0x00, 0x00]) == UP, "hat 0 = up");
    check(p.decode(&[0x80, 0x80, 0x03, 0x00, 0x00]) == DOWN | RIGHT, "hat 3 = down-right");
    check(p.decode(&[0x00, 0xFF, 0x08, 0x00, 0x00]) == LEFT | DOWN, "stick left + down");
    // the bits reach the raw key state and the event queue
    crate::keyboard::clear_events();
    p.feed(&[0x80, 0x80, 0x06, 0x02, 0x00]); // hat left + button 2 (B)
    let down = crate::keyboard::is_down(crate::keyboard::PAD_A + 1) && crate::keyboard::is_down(crate::keyboard::PAD_UP + 2);
    p.release();
    let up = !crate::keyboard::is_down(crate::keyboard::PAD_A + 1) && !crate::keyboard::is_down(crate::keyboard::PAD_UP + 2);
    let mut events = 0;
    while crate::keyboard::pop_event().is_some() {
        events += 1;
    }
    check(down && up && events == 4, "presses + releases as raw key events");
    // a mouse descriptor is not a gamepad
    #[rustfmt::skip]
    let mouse = [0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x75, 0x01, 0x95, 0x03, 0x81, 0x02, 0xC0];
    check(Pad::parse(&mouse).is_none(), "a mouse is not a pad");
    // Xbox 360: A + d-pad right, then left stick up
    let x = Pad::xinput();
    let mut r = [0u8; 20];
    r[1] = 20;
    r[2] = 1 << 3;
    r[3] = 1 << 4;
    check(x.decode(&r) == RIGHT | 1, "XInput A + d-pad right");
    r[2] = 0;
    r[3] = 0;
    r[8..10].copy_from_slice(&30000i16.to_le_bytes());
    check(x.decode(&r) == UP, "XInput left stick up");
    if ok {
        crate::println!("MILESTONE: GAMEPAD OK (HID descriptor parser: {}; XInput; raw key events)", p.summary());
    }
    ok
}
