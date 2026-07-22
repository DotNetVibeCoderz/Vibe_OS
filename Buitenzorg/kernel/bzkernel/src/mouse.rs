//! PS/2 mouse driver (v0.3 "Batang": input baseline alongside the keyboard).
//! Streams 3-byte packets on IRQ12; position/buttons are tracked in atomics.

use core::sync::atomic::{AtomicI64, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use x86_64::instructions::port::Port;

const DATA: u16 = 0x60;
const STATUS_CMD: u16 = 0x64;

static PACKETS: AtomicU64 = AtomicU64::new(0);
static X: AtomicI64 = AtomicI64::new(0);
static Y: AtomicI64 = AtomicI64::new(0);
static BUTTONS: AtomicU8 = AtomicU8::new(0);
static PHASE: AtomicUsize = AtomicUsize::new(0);
static PACKET: [AtomicU8; 2] = [AtomicU8::new(0), AtomicU8::new(0)];

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

        mouse_cmd(0xF6) && mouse_cmd(0xF4) // defaults, then enable streaming
    })
}

/// IRQ12 handler body: assemble 3-byte packets.
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
        _ => {
            PHASE.store(0, Ordering::Relaxed);
            let flags = PACKET[0].load(Ordering::Relaxed);
            let dx = PACKET[1].load(Ordering::Relaxed) as i64
                - if flags & 0x10 != 0 { 256 } else { 0 };
            let dy = byte as i64 - if flags & 0x20 != 0 { 256 } else { 0 };
            X.fetch_add(dx, Ordering::Relaxed);
            Y.fetch_sub(dy, Ordering::Relaxed); // screen y grows downward
            BUTTONS.store(flags & 0x07, Ordering::Relaxed);
            PACKETS.fetch_add(1, Ordering::Relaxed);
        }
    }
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
