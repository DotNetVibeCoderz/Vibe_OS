//! Minimal 16550 UART driver for COM1 (kernel debug console over serial).

use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use x86_64::instructions::port::Port;

const COM1: u16 = 0x3F8;

pub struct SerialPort {
    data: Port<u8>,
    int_en: Port<u8>,
    fifo_ctrl: Port<u8>,
    line_ctrl: Port<u8>,
    modem_ctrl: Port<u8>,
    line_status: Port<u8>,
}

impl SerialPort {
    const fn new(base: u16) -> Self {
        Self {
            data: Port::new(base),
            int_en: Port::new(base + 1),
            fifo_ctrl: Port::new(base + 2),
            line_ctrl: Port::new(base + 3),
            modem_ctrl: Port::new(base + 4),
            line_status: Port::new(base + 5),
        }
    }

    fn init(&mut self) {
        unsafe {
            self.int_en.write(0x00); // disable interrupts
            self.line_ctrl.write(0x80); // enable DLAB
            self.data.write(0x01); // divisor lo: 115200 baud
            self.int_en.write(0x00); // divisor hi
            self.line_ctrl.write(0x03); // 8N1, DLAB off
            self.fifo_ctrl.write(0xC7); // FIFO on, clear, 14-byte threshold
            self.modem_ctrl.write(0x0B); // DTR + RTS + OUT2
        }
    }

    fn is_transmit_empty(&mut self) -> bool {
        unsafe { self.line_status.read() & 0x20 != 0 }
    }

    /// Bounded: a PC without COM1 (most laptops) may never report "empty",
    /// and an unbounded wait here hung the first println of the boot -- before
    /// anything reached the screen (a black screen on a Dell Latitude 7290).
    fn send(&mut self, byte: u8) {
        if !PRESENT.load(Ordering::Relaxed) {
            return;
        }
        let mut tries = 0u32;
        while !self.is_transmit_empty() {
            tries += 1;
            if tries > 100_000 {
                PRESENT.store(false, Ordering::Relaxed); // give up on this port
                return;
            }
            core::hint::spin_loop();
        }
        unsafe { self.data.write(byte) }
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.send(b'\r');
            }
            self.send(byte);
        }
        Ok(())
    }
}

pub static SERIAL1: Mutex<SerialPort> = Mutex::new(SerialPort::new(COM1));

/// Whether COM1 exists (its scratch register keeps a value).
static PRESENT: AtomicBool = AtomicBool::new(false);

pub fn init() {
    let mut s = SERIAL1.lock();
    let mut scratch = Port::<u8>::new(COM1 + 7);
    let present = unsafe {
        scratch.write(0x5A);
        let a = scratch.read();
        scratch.write(0xA5);
        a == 0x5A && scratch.read() == 0xA5
    };
    PRESENT.store(present, Ordering::Relaxed);
    if present {
        s.init();
    }
}

/// One received byte, if any (never blocks): typing on the serial console.
pub fn read_byte() -> Option<u8> {
    crate::arch::cpu::without_interrupts(|| {
        let mut s = SERIAL1.lock();
        unsafe {
            if !PRESENT.load(Ordering::Relaxed) || s.line_status.read() & 1 == 0 {
                return None;
            }
            Some(s.data.read())
        }
    })
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    // Avoid deadlocks when an interrupt handler prints while the lock is held.
    x86_64::instructions::interrupts::without_interrupts(|| {
        SERIAL1.lock().write_fmt(args).ok();
    });
}

// ---- COM2: a second port for data (bz-link over serial, deploy.rs) ----------

const COM2: u16 = 0x2F8;
static AUX: Mutex<SerialPort> = Mutex::new(SerialPort::new(COM2));
static AUX_PRESENT: AtomicBool = AtomicBool::new(false);
static AUX_INIT: AtomicBool = AtomicBool::new(false);

/// Whether COM2 exists (probed once, then set up at 115200 8N1).
pub fn aux_available() -> bool {
    if !AUX_INIT.swap(true, Ordering::SeqCst) {
        let mut scratch = Port::<u8>::new(COM2 + 7);
        let present = unsafe {
            scratch.write(0x5A);
            let a = scratch.read();
            scratch.write(0xA5);
            a == 0x5A && scratch.read() == 0xA5
        };
        if present {
            AUX.lock().init();
        }
        AUX_PRESENT.store(present, Ordering::SeqCst);
    }
    AUX_PRESENT.load(Ordering::Relaxed)
}

/// One byte from COM2, if any (never blocks).
pub fn aux_read() -> Option<u8> {
    if !aux_available() {
        return None;
    }
    crate::arch::cpu::without_interrupts(|| {
        let mut s = AUX.lock();
        unsafe {
            if s.line_status.read() & 1 == 0 {
                return None;
            }
            Some(s.data.read())
        }
    })
}

/// Write bytes to COM2 (raw: no newline translation; bounded waits).
pub fn aux_write(bytes: &[u8]) {
    if !aux_available() {
        return;
    }
    let mut s = AUX.lock();
    for &b in bytes {
        let mut tries = 0u32;
        while unsafe { s.line_status.read() } & 0x20 == 0 {
            tries += 1;
            if tries > 1_000_000 {
                return;
            }
            core::hint::spin_loop();
        }
        unsafe { s.data.write(b) }
    }
}
