//! Keyboard input queue (v0.7 "Kanopi"). The PS/2 IRQ handler decodes
//! scancodes and pushes Unicode characters here; the desktop loop drains them
//! and routes them to the focused terminal. Before a terminal is attached,
//! characters are echoed to the console (early boot behavior).

use alloc::collections::VecDeque;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use spin::Mutex;

static QUEUE: Mutex<VecDeque<char>> = Mutex::new(VecDeque::new());
/// Characters ever queued. The desktop compares it with what it last saw to
/// notice typing even when the keys stay queued for an app's KEY_READ.
static PUSHED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Characters queued since boot (see [`PUSHED`]).
pub fn pushed() -> u64 {
    PUSHED.load(core::sync::atomic::Ordering::Relaxed)
}

static DECODER: Mutex<Option<Keyboard<layouts::Us104Key, ScancodeSet1>>> = Mutex::new(None);

// ---- raw key events (KEY_EVENT / KEY_STATE, for games) -----------------------

/// The last byte was the 0xE0 prefix.
static E0: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// Which keys are down: bit `code` (set-1 make code, | 0x100 for E0 keys).
/// Key state: set-1 codes (0x000..0x1FF) and gamepad buttons (0x200.., `PAD_*`).
static DOWN: [core::sync::atomic::AtomicU64; 10] = [const { core::sync::atomic::AtomicU64::new(0) }; 10];

/// Gamepad buttons as raw key codes (USB HID gamepads, `usb::hid::Pad`):
/// A B X Y L R Select Start, then the d-pad Up Down Left Right.
pub const PAD_A: u32 = 0x200;
pub const PAD_UP: u32 = 0x210;
/// Press / release events: `code | pressed << 16`.
static EVENTS: Mutex<VecDeque<u32>> = Mutex::new(VecDeque::new());

/// Keys the desktop itself acts on, whoever has the focus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hotkey {
    /// Print Screen: save the screen as a picture.
    Screenshot,
    VolumeUp,
    VolumeDown,
    Mute,
}

static HOTKEYS: Mutex<VecDeque<Hotkey>> = Mutex::new(VecDeque::new());

/// The hotkey a set-1 code (`| 0x100` for E0 keys) stands for: Print Screen
/// (E0 37) and the multimedia keys a laptop's Fn row sends (E0 30 / 2E / 20).
fn hotkey_of(code: u32) -> Option<Hotkey> {
    match code {
        0x137 => Some(Hotkey::Screenshot),
        0x130 => Some(Hotkey::VolumeUp),
        0x12E => Some(Hotkey::VolumeDown),
        0x120 => Some(Hotkey::Mute),
        _ => None,
    }
}

/// The next hotkey press, for the desktop loop.
pub fn take_hotkey() -> Option<Hotkey> {
    crate::arch::cpu::without_interrupts(|| HOTKEYS.lock().pop_front())
}

/// Track one raw scancode byte (interrupts off): key state + an event per
/// change (typematic repeats of a held key are not events).
fn raw_scancode(byte: u8) {
    use core::sync::atomic::Ordering::Relaxed;
    match byte {
        0xE0 => {
            E0.store(true, Relaxed);
            return;
        }
        0xE1 | 0xFA | 0xFE | 0x00 | 0xFF => return, // pause prefix, acks, errors
        _ => {}
    }
    let ext = E0.swap(false, Relaxed);
    let code = (byte & 0x7F) as u32 | if ext { 0x100 } else { 0 };
    if ext && (code == 0x12A || code == 0x136) {
        return; // the fake shifts around E0 keys
    }
    let pressed = byte & 0x80 == 0;
    key_change(code, pressed);
}

/// A key or gamepad button went down / up: state bitmap, hotkeys, one event
/// per change. Callable with interrupts on (the queues are shared with the
/// PS/2 interrupt handler, so they are taken with interrupts off).
pub fn key_change(code: u32, pressed: bool) {
    crate::arch::cpu::without_interrupts(|| key_change_locked(code, pressed));
}

fn key_change_locked(code: u32, pressed: bool) {
    use core::sync::atomic::Ordering::Relaxed;
    if code >= 640 {
        return;
    }
    let (w, bit) = ((code / 64) as usize, 1u64 << (code % 64));
    let was = DOWN[w].load(Relaxed) & bit != 0;
    if pressed {
        DOWN[w].fetch_or(bit, Relaxed);
    } else {
        DOWN[w].fetch_and(!bit, Relaxed);
    }
    if pressed && !was {
        if let Some(h) = hotkey_of(code) {
            let mut q = HOTKEYS.lock();
            if q.len() < 16 {
                q.push_back(h);
            }
        }
    }
    if was != pressed {
        let mut q = EVENTS.lock();
        if q.len() >= 128 {
            q.pop_front();
        }
        q.push_back(code | (pressed as u32) << 16);
    }
}

/// The next raw key event (`code | pressed << 16`).
pub fn pop_event() -> Option<u32> {
    crate::arch::cpu::without_interrupts(|| EVENTS.lock().pop_front())
}

/// Forget queued events (they were typed for someone else).
pub fn clear_events() {
    crate::arch::cpu::without_interrupts(|| EVENTS.lock().clear());
}

/// Forget queued characters (a program reading events does not want them).
pub fn clear_chars() {
    crate::arch::cpu::without_interrupts(|| QUEUE.lock().clear());
}

/// Whether key `code` is held down.
pub fn is_down(code: u32) -> bool {
    code < 640 && DOWN[(code / 64) as usize].load(core::sync::atomic::Ordering::Relaxed) & (1u64 << (code % 64)) != 0
}

/// Feed one PC scancode (set 1, `0xE0` prefixes included) from any keyboard:
/// the PS/2 IRQ handler, or the virtio-input driver translating Linux key
/// codes. Decoded characters go to the queue once the desktop is up, and are
/// echoed to the console before that (early boot).
pub fn feed_scancode(byte: u8) {
    crate::arch::cpu::without_interrupts(|| {
        let mut guard = DECODER.lock();
        let kb = guard.get_or_insert_with(|| {
            Keyboard::new(ScancodeSet1::new(), layouts::Us104Key, HandleControl::MapLettersToUnicode)
        });
        raw_scancode(byte);
        // pc-keyboard 0.7 decodes the backslash key (set 1: 0x2B) as Oem7,
        // which its Us104Key layout has no character for ('\' and '|' sit on
        // Oem5 = 0x56 there), so the key produced nothing -- no pipes in the
        // shell. Feed it as 0x56 (make and break).
        let byte = match byte {
            0x2B => 0x56,
            0xAB => 0xD6,
            b => b,
        };
        if let Ok(Some(event)) = kb.add_byte(byte) {
            match kb.process_keyevent(event) {
                Some(DecodedKey::Unicode(c)) => {
                    if crate::terminal::is_active() {
                        push(c);
                    } else {
                        crate::print!("{}", c);
                    }
                }
                // Keys with no character reach apps as private-use code
                // points (U+F700..), so KEY_READ can deliver navigation.
                Some(DecodedKey::RawKey(code)) => {
                    if let Some(c) = nav_char(code) {
                        // with Shift held the arrows / Home / End / PgUp / PgDn /
                        // Delete arrive 0x20 higher (U+F720..F728): text selection
                        let n = c as u32 - NAV_FIRST as u32;
                        let shift = is_down(0x2A) || is_down(0x36);
                        let c = if shift && n <= 8 { char::from_u32(c as u32 + SHIFT_OFFSET).unwrap_or(c) } else { c };
                        if crate::terminal::is_active() {
                            push(c);
                        }
                    }
                }
                None => {}
            }
        }
    })
}

/// Navigation keys as private-use characters (KEY_READ): U+F700 up, F701
/// down, F702 left, F703 right, F704 home, F705 end, F706 delete, F707 page
/// up, F708 page down.
pub const NAV_FIRST: char = '\u{F700}';
/// Shift + a navigation key (up .. page down, delete): the same code + 0x20
/// (U+F720..U+F728), for selecting text.
pub const SHIFT_OFFSET: u32 = 0x20;

/// A shifted navigation key back to its plain one (the terminal has no selection).
pub fn unshift_nav(c: char) -> char {
    let v = c as u32;
    if (0xF720..=0xF728).contains(&v) {
        char::from_u32(v - SHIFT_OFFSET).unwrap_or(c)
    } else {
        c
    }
}
fn nav_char(code: pc_keyboard::KeyCode) -> Option<char> {
    use pc_keyboard::KeyCode as K;
    let n = match code {
        K::ArrowUp => 0,
        K::ArrowDown => 1,
        K::ArrowLeft => 2,
        K::ArrowRight => 3,
        K::Home => 4,
        K::End => 5,
        K::Delete => 6,
        K::PageUp => 7,
        K::PageDown => 8,
        // F1..F12 -> F709..F714 (F2 rename, F5 refresh in the managed apps)
        K::F1 => 9,
        K::F2 => 10,
        K::F3 => 11,
        K::F4 => 12,
        K::F5 => 13,
        K::F6 => 14,
        K::F7 => 15,
        K::F8 => 16,
        K::F9 => 17,
        K::F10 => 18,
        K::F11 => 19,
        K::F12 => 20,
        _ => return None,
    };
    char::from_u32(NAV_FIRST as u32 + n)
}

/// Where `feed_serial` is inside an escape sequence: 0 none, 1 after ESC,
/// 2 after "ESC [", 3.. after "ESC [ <digit>" (digit + 3).
static SERIAL_STATE: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// A byte typed on the serial console, as a terminal program sends it:
/// Enter is CR, Backspace is DEL, arrows and Home/End/Delete/PageUp/PageDown
/// are VT100 escape sequences. Lets a board without a keyboard driver (the
/// Raspberry Pi before USB) be used from a PC.
pub fn feed_serial(byte: u8) {
    use core::sync::atomic::Ordering::Relaxed;
    let nav = |n: u32| char::from_u32(NAV_FIRST as u32 + n);
    match SERIAL_STATE.load(Relaxed) {
        1 => {
            SERIAL_STATE.store(if byte == b'[' { 2 } else { 0 }, Relaxed);
            if byte != b'[' {
                push('\u{1b}');
            }
            return;
        }
        2 => {
            SERIAL_STATE.store(0, Relaxed);
            let key = match byte {
                b'A' => nav(0),
                b'B' => nav(1),
                b'D' => nav(2),
                b'C' => nav(3),
                b'H' => nav(4),
                b'F' => nav(5),
                b'0'..=b'9' => {
                    SERIAL_STATE.store(byte - b'0' + 3, Relaxed);
                    None
                }
                _ => None,
            };
            if let Some(c) = key {
                push(c);
            }
            return;
        }
        s @ 3.. => {
            SERIAL_STATE.store(0, Relaxed);
            if byte == b'~' {
                let key = match s - 3 {
                    1 => nav(4),
                    3 => nav(6),
                    4 => nav(5),
                    5 => nav(7),
                    6 => nav(8),
                    _ => None,
                };
                if let Some(c) = key {
                    push(c);
                }
            }
            return;
        }
        _ => {}
    }
    match byte {
        0x1b => SERIAL_STATE.store(1, Relaxed),
        b'\r' => push('\n'),
        b'\n' => {} // CR LF from some terminals: the CR already was Enter
        0x7f | 0x08 => push('\u{8}'),
        0x01..=0x7e => push(byte as char),
        _ => {}
    }
}

/// Called from the keyboard IRQ handler with a decoded character.
pub fn push(c: char) {
    let mut q = QUEUE.lock();
    if q.len() < 256 {
        q.push_back(c);
        PUSHED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }
}

/// Drain one queued character. Interrupts are held off while the lock is
/// taken: the keyboard IRQ pushes under the same spin lock, so an IRQ landing
/// while the desktop loop holds it would deadlock.
pub fn pop() -> Option<char> {
    crate::arch::cpu::without_interrupts(|| QUEUE.lock().pop_front())
}

/// Boot self-test of the raw key path (KEY_EVENT / KEY_STATE): press and
/// release 'A' (0x1E), an E0 arrow (0xE0 0x48 = 0x148), a typematic repeat
/// that must not become a second event. Fed below the character decoder, so
/// nothing reaches the terminal.
pub fn self_test() -> bool {
    clear_events();
    let feed = |b: &[u8]| crate::arch::cpu::without_interrupts(|| b.iter().for_each(|&x| raw_scancode(x)));
    feed(&[0x1E]);
    let a_down = is_down(0x1E);
    feed(&[0x1E]); // repeat
    feed(&[0xE0, 0x48]);
    let up_down = is_down(0x148);
    feed(&[0x9E, 0xE0, 0xC8]);
    let released = !is_down(0x1E) && !is_down(0x148);
    let mut got = alloc::vec::Vec::new();
    while let Some(e) = pop_event() {
        got.push(e);
    }
    let want = [0x1E | 1 << 16, 0x148 | 1 << 16, 0x1E, 0x148];
    // media keys and Print Screen become hotkeys (once per press)
    while take_hotkey().is_some() {}
    feed(&[0xE0, 0x30, 0xE0, 0x30, 0xE0, 0xB0, 0xE0, 0x2A, 0xE0, 0x37, 0xE0, 0xB7, 0xE0, 0xAA, 0xE0, 0x20, 0xE0, 0xA0]);
    let mut hot = alloc::vec::Vec::new();
    while let Some(h) = take_hotkey() {
        hot.push(h);
    }
    let hot_ok = hot == [Hotkey::VolumeUp, Hotkey::Screenshot, Hotkey::Mute];
    clear_events();
    let ok = a_down && up_down && released && got == want && hot_ok;
    if !hot_ok {
        crate::println!("[keyboard] hotkeys: {:?}", hot);
    }
    if ok {
        crate::println!("[kernel] MILESTONE: KEY EVENTS OK (press/release + state for A and E0 up-arrow, repeat ignored; volume/mute/Print Screen hotkeys)");
    } else {
        crate::println!("[keyboard] self-test failed: down {} {} released {} events {:x?}", a_down, up_down, released, got);
    }
    ok
}
