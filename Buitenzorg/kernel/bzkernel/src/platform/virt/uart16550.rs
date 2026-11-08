//! The NS16550-compatible UART of QEMU `virt` (RISC-V) and many boards,
//! memory-mapped, one byte per register (`reg-shift` 0). Its address comes
//! from the DTB (`/chosen` `stdout-path`); until it is mapped, output goes
//! through the SBI firmware console.

use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

const THR: u64 = 0;
const IER: u64 = 1;
const IIR: u64 = 2;
const LSR: u64 = 5;
const LSR_THRE: u8 = 1 << 5;
const IER_THRE: u8 = 1 << 1;

static BASE: AtomicU64 = AtomicU64::new(0);
static LOCK: Mutex<()> = Mutex::new(());

/// One received byte, if any (never blocks): typing on the serial console.
pub fn read_byte() -> Option<u8> {
    let base = BASE.load(Ordering::Relaxed);
    if base == 0 {
        return None;
    }
    unsafe {
        if ((base + LSR) as *const u8).read_volatile() & 1 == 0 {
            return None;
        }
        Some(((base + THR) as *const u8).read_volatile())
    }
}

/// Nothing to do before the device is known (kept for the boot glue).
pub fn init() {}

/// Start using the UART mapped at virtual address `va` (the firmware has
/// already programmed its line settings).
pub fn set_base(va: u64) {
    BASE.store(va, Ordering::SeqCst);
}

/// Ask for a "transmitter holding register empty" interrupt: the UART
/// raises its line (through the PLIC) as soon as THR is empty -- a real
/// device interrupt the PLIC self-test can wait for. [`on_irq`] turns it off.
pub fn request_tx_irq() {
    let base = BASE.load(Ordering::Relaxed);
    if base != 0 {
        unsafe { ((base + IER) as *mut u8).write_volatile(IER_THRE) };
    }
}

/// The UART's PLIC handler: read IIR (acknowledges a THRE interrupt) and
/// mask the transmit interrupt again.
pub fn on_irq() {
    let base = BASE.load(Ordering::Relaxed);
    if base != 0 {
        unsafe {
            let _ = ((base + IIR) as *const u8).read_volatile();
            ((base + IER) as *mut u8).write_volatile(0);
        }
    }
}

fn put(base: u64, b: u8) {
    if base == 0 {
        crate::arch::riscv64::sbi::console_putchar(b);
        return;
    }
    unsafe {
        while ((base + LSR) as *const u8).read_volatile() & LSR_THRE == 0 {
            core::hint::spin_loop();
        }
        ((base + THR) as *mut u8).write_volatile(b);
    }
}

struct Writer(u64);

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                put(self.0, b'\r');
            }
            put(self.0, b);
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    let base = BASE.load(Ordering::Relaxed);
    crate::arch::cpu::without_interrupts(|| {
        let _g = LOCK.lock();
        Writer(base).write_fmt(args).ok();
    });
}

// No second UART on this platform (bz-link over serial is PC COM2 only).
pub fn aux_available() -> bool {
    false
}
pub fn aux_read() -> Option<u8> {
    None
}
pub fn aux_write(_bytes: &[u8]) {}
