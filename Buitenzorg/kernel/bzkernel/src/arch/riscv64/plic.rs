//! The RISC-V Platform-Level Interrupt Controller (`sifive,plic-1.0.0` /
//! `riscv,plic0`): routes device interrupts (UART, virtio-mmio, ...) to hart
//! contexts. Each source has a priority; each context (one per hart and
//! privilege mode -- the DTB's `interrupts-extended` lists them in order) has
//! an enable bitmap, a priority threshold and a claim/complete register. An
//! enabled source above the threshold raises the context's external
//! interrupt (`scause` 9 for S-mode); the handler claims the source id,
//! services the device, and writes the id back to complete it.

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const PRIORITY: u64 = 0x0000;
const ENABLE: u64 = 0x2000;
const ENABLE_STRIDE: u64 = 0x80;
const CONTEXT: u64 = 0x20_0000;
const CONTEXT_STRIDE: u64 = 0x1000;

/// Interrupt sources (QEMU `virt` has 96, the PLIC up to 1023).
pub const MAX_SOURCES: usize = 128;

static BASE: AtomicU64 = AtomicU64::new(0);
/// The S-mode context of each hart (index = kernel CPU index).
static CONTEXT_OF: [AtomicUsize; crate::arch::cpu::MAX_CPUS] = [const { AtomicUsize::new(usize::MAX) }; crate::arch::cpu::MAX_CPUS];
static COUNTS: [AtomicU64; MAX_SOURCES] = [const { AtomicU64::new(0) }; MAX_SOURCES];
static HANDLERS: spin::Mutex<[Option<fn()>; MAX_SOURCES]> = spin::Mutex::new([None; MAX_SOURCES]);

fn reg(off: u64) -> *mut u32 {
    (BASE.load(Ordering::Relaxed) + off) as *mut u32
}

/// Size of the register window to map.
pub const WINDOW: u64 = 0x40_0000;

/// Start using the PLIC mapped at `va`; `cpu`'s S-mode context is `context`.
/// Its threshold is set to 0 (every priority >= 1 gets through).
pub fn init(va: u64, cpu: usize, context: usize) {
    BASE.store(va, Ordering::SeqCst);
    set_context(cpu, context);
}

/// Bind `cpu` to its S-mode context (secondary harts, later).
pub fn set_context(cpu: usize, context: usize) {
    CONTEXT_OF[cpu].store(context, Ordering::SeqCst);
    unsafe { reg(CONTEXT + CONTEXT_STRIDE * context as u64).write_volatile(0) };
}

pub fn active() -> bool {
    BASE.load(Ordering::Relaxed) != 0
}

/// Route source `irq` to `cpu` with `priority` (1..=7), calling `handler`
/// when it fires.
pub fn enable(irq: usize, priority: u32, cpu: usize, handler: fn()) {
    if irq == 0 || irq >= MAX_SOURCES || !active() {
        return;
    }
    crate::arch::cpu::without_interrupts(|| HANDLERS.lock()[irq] = Some(handler));
    let ctx = CONTEXT_OF[cpu].load(Ordering::SeqCst) as u64;
    unsafe {
        reg(PRIORITY + 4 * irq as u64).write_volatile(priority);
        let word = reg(ENABLE + ENABLE_STRIDE * ctx + 4 * (irq as u64 / 32));
        word.write_volatile(word.read_volatile() | 1 << (irq % 32));
    }
}

/// Stop routing source `irq` to `cpu`.
pub fn disable(irq: usize, cpu: usize) {
    if irq == 0 || irq >= MAX_SOURCES || !active() {
        return;
    }
    let ctx = CONTEXT_OF[cpu].load(Ordering::SeqCst) as u64;
    unsafe {
        let word = reg(ENABLE + ENABLE_STRIDE * ctx + 4 * (irq as u64 / 32));
        word.write_volatile(word.read_volatile() & !(1 << (irq % 32)));
    }
}

/// Interrupts source `irq` has delivered.
pub fn count(irq: usize) -> u64 {
    COUNTS.get(irq).map_or(0, |c| c.load(Ordering::Relaxed))
}

/// The S-mode external interrupt (scause 9): claim every pending source of
/// this hart's context, run its handler, complete it.
pub fn handle_external() {
    if !active() {
        return;
    }
    let ctx = CONTEXT_OF[super::cpu::id()].load(Ordering::Relaxed);
    if ctx == usize::MAX {
        return;
    }
    let claim = reg(CONTEXT + CONTEXT_STRIDE * ctx as u64 + 4);
    loop {
        let irq = unsafe { claim.read_volatile() } as usize;
        if irq == 0 {
            break;
        }
        if irq < MAX_SOURCES {
            COUNTS[irq].fetch_add(1, Ordering::Relaxed);
            let h = HANDLERS.lock()[irq];
            if let Some(h) = h {
                h();
            }
        }
        unsafe { claim.write_volatile(irq as u32) };
    }
}
