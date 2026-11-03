//! PS/2 mouse driver (v0.3 "Batang": input baseline alongside the keyboard).
//! Streams 3-byte packets on IRQ12 -- 4-byte IntelliMouse packets with the
//! wheel once the mouse accepted the magic sample-rate sequence; position,
//! buttons and wheel are tracked in atomics.

use core::sync::atomic::{AtomicI64, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use x86_64::instructions::port::Port;

const DATA: u16 = 0x60;
const STATUS_CMD: u16 = 0x64;

static PACKETS: AtomicU64 = AtomicU64::new(0);
static X: AtomicI64 = AtomicI64::new(0);
static Y: AtomicI64 = AtomicI64::new(0);
static BUTTONS: AtomicU8 = AtomicU8::new(0);
static PHASE: AtomicUsize = AtomicUsize::new(0);
static PACKET: [AtomicU8; 3] = [AtomicU8::new(0), AtomicU8::new(0), AtomicU8::new(0)];
/// 4 once the mouse reported IntelliMouse id 3 (a wheel byte per packet).
static PACKET_LEN: AtomicUsize = AtomicUsize::new(3);
/// Largest valid x / y (screen size - 1); 0 = unbounded until the desktop sets it.
static MAX_X: AtomicI64 = AtomicI64::new(0);
static MAX_Y: AtomicI64 = AtomicI64::new(0);

/// Confine the pointer to a `w` x `h` screen and center it. Without bounds the
/// position kept accumulating past the edges, so after pushing the mouse far
/// off-screen it took the same distance back before the cursor moved again.
pub fn set_bounds(w: usize, h: usize) {
    MAX_X.store(w.saturating_sub(1) as i64, Ordering::Relaxed);
    MAX_Y.store(h.saturating_sub(1) as i64, Ordering::Relaxed);
    X.store(w as i64 / 2, Ordering::Relaxed);
    Y.store(h as i64 / 2, Ordering::Relaxed);
}

/// Add `d` to `axis`, clamped to `0..=max` when bounds are set. Only the IRQ12
/// handler writes the position, so a plain load/store is race-free.
fn move_axis(axis: &AtomicI64, d: i64, max: &AtomicI64) {
    let max = max.load(Ordering::Relaxed);
    let mut v = axis.load(Ordering::Relaxed) + d;
    if max > 0 {
        v = v.clamp(0, max);
    }
    axis.store(v, Ordering::Relaxed);
}

fn wait_input_clear() -> bool {
    let mut status = Port::<u8>::new(STATUS_CMD);
    for _ in 0..100_000 {
        if unsafe { status.read() } & 0x02 == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn wait_output_full() -> bool {
    let mut status = Port::<u8>::new(STATUS_CMD);
    for _ in 0..100_000 {
        if unsafe { status.read() } & 0x01 != 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn controller_cmd(cmd: u8) -> bool {
    if !wait_input_clear() {
        return false;
    }
    unsafe { Port::<u8>::new(STATUS_CMD).write(cmd) };
    true
}

/// Send a byte to the mouse (0xD4 prefix) and wait for its 0xFA ACK.
fn mouse_cmd(byte: u8) -> bool {
    if !controller_cmd(0xD4) || !wait_input_clear() {
        return false;
    }
    unsafe { Port::<u8>::new(DATA).write(byte) };
    if !wait_output_full() {
        return false;
    }
    unsafe { Port::<u8>::new(DATA).read() == 0xFA }
}

/// Drain any pending bytes so init responses are not mixed with stale data.
fn flush_output() {
    let mut status = Port::<u8>::new(STATUS_CMD);
    let mut data = Port::<u8>::new(DATA);
    for _ in 0..64 {
        if unsafe { status.read() } & 0x01 == 0 {
            break;
        }
        unsafe { data.read() };
    }
}

/// Enable the auxiliary PS/2 port, unmask its IRQ in the controller config,
/// and put the mouse into streaming mode.
///
/// Runs with interrupts disabled: the keyboard/mouse IRQ handlers would
/// otherwise consume the controller's response bytes mid-handshake.
pub fn init() -> bool {
    x86_64::instructions::interrupts::without_interrupts(|| {
        flush_output();
        if !controller_cmd(0xA8) {
            return false; // enable aux device
        }
        // Read controller config, set bit 1 (IRQ12), clear bit 5 (aux clock off).
        if !controller_cmd(0x20) || !wait_output_full() {
            return false;
        }
        let mut config = unsafe { Port::<u8>::new(DATA).read() };
        config |= 0x02;
        config &= !0x20;
        if !controller_cmd(0x60) || !wait_input_clear() {
            return false;
        }
        unsafe { Port::<u8>::new(DATA).write(config) };

        if !mouse_cmd(0xF6) {
            return false; // defaults
        }
        // IntelliMouse: sample rates 200, 100, 80 then "get id" -> 3 = wheel.
        let wheel = [200u8, 100, 80].iter().all(|&r| mouse_cmd(0xF3) && mouse_cmd(r))
            && mouse_cmd(0xF2)
            && wait_output_full()
            && unsafe { Port::<u8>::new(DATA).read() } == 3;
        PACKET_LEN.store(if wheel { 4 } else { 3 }, Ordering::Relaxed);
        PHASE.store(0, Ordering::Relaxed);
        mouse_cmd(0xF4) // enable streaming
    })
}

/// True when the mouse sends a wheel byte (IntelliMouse mode).
pub fn has_wheel() -> bool {
    PACKET_LEN.load(Ordering::Relaxed) == 4
}

/// Start a new packet at the next byte (after bytes were injected for a test).
pub fn resync() {
    PHASE.store(0, Ordering::Relaxed);
}

/// IRQ12 handler body: assemble 3-byte (4 with a wheel) packets.
pub fn handle_byte(byte: u8) {
    let phase = PHASE.load(Ordering::Relaxed);
    match phase {
        0 => {
            // Bit 3 must be set in the first byte; otherwise we are out of
            // sync and drop bytes until it lines up again.
            if byte & 0x08 == 0 {
                return;
            }
            PACKET[0].store(byte, Ordering::Relaxed);
            PHASE.store(1, Ordering::Relaxed);
        }
        1 => {
            PACKET[1].store(byte, Ordering::Relaxed);
            PHASE.store(2, Ordering::Relaxed);
        }
        2 if PACKET_LEN.load(Ordering::Relaxed) == 4 => {
            PACKET[2].store(byte, Ordering::Relaxed);
            PHASE.store(3, Ordering::Relaxed);
        }
        _ => {
            PHASE.store(0, Ordering::Relaxed);
            let wheel = PACKET_LEN.load(Ordering::Relaxed) == 4;
            let (ybyte, z) = if wheel { (PACKET[2].load(Ordering::Relaxed), byte) } else { (byte, 0) };
            let flags = PACKET[0].load(Ordering::Relaxed);
            let dx = PACKET[1].load(Ordering::Relaxed) as i64
                - if flags & 0x10 != 0 { 256 } else { 0 };
            let dy = ybyte as i64 - if flags & 0x20 != 0 { 256 } else { 0 };
            // Z: low nibble, signed; positive = toward the user (scroll down).
            let z = ((z << 4) as i8 >> 4) as i64;
            if z != 0 {
                WHEEL.fetch_add(-z, Ordering::Relaxed);
            }
            move_axis(&X, dx, &MAX_X);
            move_axis(&Y, -dy, &MAX_Y); // screen y grows downward
            set_buttons(flags);
            PACKETS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Pointer motion from another device (virtio mouse): relative move plus
/// button state (bit 0 left, 1 right, 2 middle), counted as one packet.
pub fn inject_relative(dx: i64, dy: i64, buttons: u8) {
    move_axis(&X, dx, &MAX_X);
    move_axis(&Y, dy, &MAX_Y);
    set_buttons(buttons);
    PACKETS.fetch_add(1, Ordering::Relaxed);
}

/// Absolute pointer position from a tablet (virtio tablet): `x`/`y` within
/// `0..=max`, scaled to the screen.
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

/// (x, y, buttons, packet count) — for diagnostics and the future compositor.
pub fn state() -> (i64, i64, u8, u64) {
    (
        X.load(Ordering::Relaxed),
        Y.load(Ordering::Relaxed),
        BUTTONS.load(Ordering::Relaxed),
        PACKETS.load(Ordering::Relaxed),
    )
}
