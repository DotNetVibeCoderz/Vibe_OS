//! Keyboard input queue (v0.7 "Kanopi"). The PS/2 IRQ handler decodes
//! scancodes and pushes Unicode characters here; the desktop loop drains them
//! and routes them to the focused terminal. Before a terminal is attached,
//! characters are echoed to the console (early boot behavior).

use alloc::collections::VecDeque;
use spin::Mutex;

static QUEUE: Mutex<VecDeque<char>> = Mutex::new(VecDeque::new());

/// Called from the keyboard IRQ handler with a decoded character.
pub fn push(c: char) {
    let mut q = QUEUE.lock();
    if q.len() < 256 {
        q.push_back(c);
    }
}

/// Drain one queued character.
pub fn pop() -> Option<char> {
    QUEUE.lock().pop_front()
}
