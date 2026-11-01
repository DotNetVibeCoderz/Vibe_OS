//! Minimal 16550 UART driver for COM1 (kernel debug console over serial).

use core::fmt;
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

    fn send(&mut self, byte: u8) {
        while !self.is_transmit_empty() {
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

pub fn init() {
    SERIAL1.lock().init();
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    // Avoid deadlocks when an interrupt handler prints while the lock is held.
    x86_64::instructions::interrupts::without_interrupts(|| {
        SERIAL1.lock().write_fmt(args).ok();
    });
}
