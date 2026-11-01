//! Keyboard input queue (v0.7 "Kanopi"). The PS/2 IRQ handler decodes
//! scancodes and pushes Unicode characters here; the desktop loop drains them
//! and routes them to the focused terminal. Before a terminal is attached,
//! characters are echoed to the console (early boot behavior).

use alloc::collections::VecDeque;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use spin::Mutex;

static QUEUE: Mutex<VecDeque<char>> = Mutex::new(VecDeque::new());

static DECODER: Mutex<Option<Keyboard<layouts::Us104Key, ScancodeSet1>>> = Mutex::new(None);

/// Feed one PC scancode (set 1, `0xE0` prefixes included) from any keyboard:
/// the PS/2 IRQ handler, or the virtio-input driver translating Linux key
/// codes. Decoded characters go to the queue once the desktop is up, and are
/// echoed to the console before that (early boot).
pub fn feed_scancode(byte: u8) {
    crate::arch::cpu::without_interrupts(|| {
        let mut guard = DECODER.lock();
        let kb = guard.get_or_insert_with(|| {
            Keyboard::new(ScancodeSet1::new(), layouts::Us104Key, HandleControl::Ignore)
        });
        if let Ok(Some(event)) = kb.add_byte(byte) {
            if let Some(DecodedKey::Unicode(c)) = kb.process_keyevent(event) {
                if crate::terminal::is_active() {
                    push(c);
                } else {
                    crate::print!("{}", c);
                }
            }
        }
    })
}

/// Called from the keyboard IRQ handler with a decoded character.
pub fn push(c: char) {
    let mut q = QUEUE.lock();
    if q.len() < 256 {
        q.push_back(c);
    }
}

/// Drain one queued character. Interrupts are held off while the lock is
/// taken: the keyboard IRQ pushes under the same spin lock, so an IRQ landing
/// while the desktop loop holds it would deadlock.
pub fn pop() -> Option<char> {
    crate::arch::cpu::without_interrupts(|| QUEUE.lock().pop_front())
}
