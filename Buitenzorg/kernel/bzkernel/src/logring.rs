//! The last 16 KiB of console output (every kernel `print!`, which includes
//! what apps write with DEBUG_WRITE), for the remote log of the deploy
//! service (`deploy.rs`, BZDP op LOG). Bytes are numbered from boot, so a
//! reader asks "from byte N on" and learns where to continue.
//!
//! Fed from `_print`, which may run in an interrupt handler or while another
//! lock is held: it never allocates and only `try_lock`s (a line written
//! while another CPU appends is dropped from the ring, not from serial).

use spin::Mutex;

const SIZE: usize = 16 * 1024;

struct Ring {
    buf: [u8; SIZE],
    /// Bytes written since boot (the next byte's number).
    total: u64,
}

static RING: Mutex<Ring> = Mutex::new(Ring { buf: [0; SIZE], total: 0 });

/// Append formatted console text (from `_print`).
pub fn push_fmt(args: core::fmt::Arguments) {
    struct W<'a>(&'a mut Ring);
    impl core::fmt::Write for W<'_> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for &b in s.as_bytes() {
                let i = (self.0.total % SIZE as u64) as usize;
                self.0.buf[i] = b;
                self.0.total += 1;
            }
            Ok(())
        }
    }
    let Some(mut r) = RING.try_lock() else { return };
    let _ = core::fmt::Write::write_fmt(&mut W(&mut r), args);
}

/// Copy output from byte `from` on into `out` (as much as fits). Returns
/// (bytes copied, number of the byte after them). A `from` older than the
/// ring starts at the oldest byte still kept.
pub fn read(from: u64, out: &mut [u8]) -> (usize, u64) {
    let r = RING.lock();
    let oldest = r.total.saturating_sub(SIZE as u64);
    let mut at = from.max(oldest).min(r.total);
    let mut n = 0;
    while at < r.total && n < out.len() {
        out[n] = r.buf[(at % SIZE as u64) as usize];
        n += 1;
        at += 1;
    }
    (n, at)
}
