//! ARM PL011 UART — the console of QEMU `virt` (and many ARM boards). Its
//! address comes from the DTB (`/chosen` `stdout-path`); until it is mapped,
//! output goes to the framebuffer console only.

use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

const UARTDR: u64 = 0x00;
const UARTFR: u64 = 0x18;
const FR_TXFF: u32 = 1 << 5;

static BASE: AtomicU64 = AtomicU64::new(0);
static LOCK: Mutex<()> = Mutex::new(());

/// Nothing to do before the device is known (kept for the boot glue).
pub fn init() {}

/// Start using the UART mapped at virtual address `va`.
pub fn set_base(va: u64) {
    BASE.store(va, Ordering::SeqCst);
}

fn put(base: u64, b: u8) {
    unsafe {
        while ((base + UARTFR) as *const u32).read_volatile() & FR_TXFF != 0 {
            core::hint::spin_loop();
        }
        ((base + UARTDR) as *mut u32).write_volatile(b as u32);
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
    if base == 0 {
        return;
    }
    crate::arch::cpu::without_interrupts(|| {
        let _g = LOCK.lock();
        Writer(base).write_fmt(args).ok();
    });
}
