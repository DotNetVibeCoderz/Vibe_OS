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
