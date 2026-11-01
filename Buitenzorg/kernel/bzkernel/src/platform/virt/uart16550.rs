//! The NS16550-compatible UART of QEMU `virt` (RISC-V) and many boards,
//! memory-mapped, one byte per register (`reg-shift` 0). Its address comes
//! from the DTB (`/chosen` `stdout-path`); until it is mapped, output goes
//! through the SBI firmware console.

use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

const THR: u64 = 0;
const LSR: u64 = 5;
const LSR_THRE: u8 = 1 << 5;

static BASE: AtomicU64 = AtomicU64::new(0);
static LOCK: Mutex<()> = Mutex::new(());

/// Nothing to do before the device is known (kept for the boot glue).
pub fn init() {}

/// Start using the UART mapped at virtual address `va` (the firmware has
/// already programmed its line settings).
pub fn set_base(va: u64) {
    BASE.store(va, Ordering::SeqCst);
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
