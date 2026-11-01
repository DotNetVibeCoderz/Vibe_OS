//! Pointer state on Device-Tree boards: there is no PS/2 port, so positions
//! come only from virtio input (mouse / tablet) through `inject_*`. Same API
//! as the PC's `mouse.rs` (state, bounds, injection).

use core::sync::atomic::{AtomicI64, AtomicU64, AtomicU8, Ordering};

static PACKETS: AtomicU64 = AtomicU64::new(0);
static X: AtomicI64 = AtomicI64::new(0);
static Y: AtomicI64 = AtomicI64::new(0);
static BUTTONS: AtomicU8 = AtomicU8::new(0);
static MAX_X: AtomicI64 = AtomicI64::new(0);
static MAX_Y: AtomicI64 = AtomicI64::new(0);

/// No PS/2 controller here.
#[allow(dead_code)]
pub fn init() -> bool {
    false
}

pub fn set_bounds(w: usize, h: usize) {
    MAX_X.store(w.saturating_sub(1) as i64, Ordering::Relaxed);
    MAX_Y.store(h.saturating_sub(1) as i64, Ordering::Relaxed);
    X.store(w as i64 / 2, Ordering::Relaxed);
    Y.store(h as i64 / 2, Ordering::Relaxed);
}

fn move_axis(axis: &AtomicI64, d: i64, max: &AtomicI64) {
    let max = max.load(Ordering::Relaxed);
    let mut v = axis.load(Ordering::Relaxed) + d;
    if max > 0 {
        v = v.clamp(0, max);
    }
    axis.store(v, Ordering::Relaxed);
}

pub fn inject_relative(dx: i64, dy: i64, buttons: u8) {
    move_axis(&X, dx, &MAX_X);
    move_axis(&Y, dy, &MAX_Y);
    BUTTONS.store(buttons & 0x07, Ordering::Relaxed);
    PACKETS.fetch_add(1, Ordering::Relaxed);
}

pub fn inject_absolute(x: i64, y: i64, max_x: i64, max_y: i64, buttons: u8) {
    let sx = MAX_X.load(Ordering::Relaxed);
    let sy = MAX_Y.load(Ordering::Relaxed);
    if max_x > 0 && sx > 0 {
        X.store((x.clamp(0, max_x) * sx) / max_x, Ordering::Relaxed);
    }
    if max_y > 0 && sy > 0 {
        Y.store((y.clamp(0, max_y) * sy) / max_y, Ordering::Relaxed);
    }
    BUTTONS.store(buttons & 0x07, Ordering::Relaxed);
    PACKETS.fetch_add(1, Ordering::Relaxed);
}

/// (x, y, buttons, packet count).
pub fn state() -> (i64, i64, u8, u64) {
    (X.load(Ordering::Relaxed), Y.load(Ordering::Relaxed), BUTTONS.load(Ordering::Relaxed), PACKETS.load(Ordering::Relaxed))
}
