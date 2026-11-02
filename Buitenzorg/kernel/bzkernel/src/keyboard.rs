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
