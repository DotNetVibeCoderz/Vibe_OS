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
    set_buttons(buttons);
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
    set_buttons(buttons);
    PACKETS.fetch_add(1, Ordering::Relaxed);
}

/// Wheel notches not yet taken by the desktop (positive = away from the user,
/// i.e. scroll up -- the Linux REL_WHEEL convention).
static WHEEL: AtomicI64 = AtomicI64::new(0);

/// Wheel notches from any pointing device (PS/2 IntelliMouse, virtio, USB).
pub fn inject_wheel(notches: i64) {
    if notches != 0 {
        WHEEL.fetch_add(notches, Ordering::Relaxed);
        PACKETS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Take the wheel notches accumulated since the last call.
pub fn take_wheel() -> i64 {
    WHEEL.swap(0, Ordering::Relaxed)
}

/// Left-button presses seen (0 -> 1 transitions), so the desktop loop can
/// replay a click that went down and up between two of its samples -- a busy
/// app (a managed one redrawing a blinking caret) can delay the loop past a
/// short click.
static PRESSES: AtomicU64 = AtomicU64::new(0);

/// Right-button presses, counted like `PRESSES` (the desktop's context menu).
static RIGHT_PRESSES: AtomicU64 = AtomicU64::new(0);

fn set_buttons(buttons: u8) {
    let old = BUTTONS.swap(buttons & 0x07, Ordering::Relaxed);
    if old & 1 == 0 && buttons & 1 != 0 {
        PRESSES.fetch_add(1, Ordering::Relaxed);
    }
    if old & 2 == 0 && buttons & 2 != 0 {
        RIGHT_PRESSES.fetch_add(1, Ordering::Relaxed);
    }
}

/// Number of left-button presses so far.
pub fn presses() -> u64 {
    PRESSES.load(Ordering::Relaxed)
}

/// Number of right-button presses so far (a press and release between two
/// samples of the desktop loop still counts).
pub fn right_presses() -> u64 {
    RIGHT_PRESSES.load(Ordering::Relaxed)
}

/// (x, y, buttons, packet count).
pub fn state() -> (i64, i64, u8, u64) {
    (X.load(Ordering::Relaxed), Y.load(Ordering::Relaxed), BUTTONS.load(Ordering::Relaxed), PACKETS.load(Ordering::Relaxed))
}
