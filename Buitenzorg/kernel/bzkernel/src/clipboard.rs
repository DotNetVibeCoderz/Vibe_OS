//! The system clipboard: one UTF-8 text shared by every program (syscall
//! `CLIPBOARD`) and the Terminal (Ctrl+V pastes, `clip` copies, `paste`
//! prints). Generic code, every architecture. Its lock is taken on its own
//! (the syscall runs without the BKL).

use alloc::string::String;
use spin::Mutex;

/// Largest text kept, in bytes (`bz_abi::clipboard_op::MAX_BYTES`).
pub const MAX_BYTES: usize = bz_abi::clipboard_op::MAX_BYTES as usize;

/// (text, change counter)
static CLIP: Mutex<(String, u64)> = Mutex::new((String::new(), 0));

/// Replace the clipboard (cut at [`MAX_BYTES`] on a character boundary).
pub fn set(text: &str) {
    let mut end = text.len().min(MAX_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut c = CLIP.lock();
    c.0 = String::from(&text[..end]);
    c.1 += 1;
}

/// The clipboard's text.
pub fn get() -> String {
    CLIP.lock().0.clone()
}

/// Grows with every `set` (a program can watch for changes).
pub fn seq() -> u64 {
    CLIP.lock().1
}
