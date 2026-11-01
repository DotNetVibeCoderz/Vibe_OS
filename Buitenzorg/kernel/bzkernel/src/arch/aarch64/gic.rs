//! GICv3 (ARM Generic Interrupt Controller v3): the distributor (shared,
//! SPIs), one redistributor per CPU (SGIs/PPIs — the timer is PPI 27 here),
//! and the CPU interface through ICC_* system registers. Addresses come from
//! the Device Tree (`arm,gic-v3`: reg[0] = GICD, reg[1] = GICR region).

use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

const GICD_CTLR: u64 = 0x0000;
const GICR_WAKER: u64 = 0x0014;
const SGI_BASE: u64 = 0x1_0000; // second 64 KiB frame of a redistributor
const GICR_IGROUPR0: u64 = 0x0080;
const GICR_ISENABLER0: u64 = 0x0100;
const GICR_IPRIORITYR: u64 = 0x0400;

static GICD: AtomicU64 = AtomicU64::new(0);
/// Base of the redistributor region: CPU n's frames are at + n * 0x20000
/// (QEMU `virt` lays them out in MPIDR order).
static GICR: AtomicU64 = AtomicU64::new(0);
const GICR_STRIDE: u64 = 0x2_0000;

fn my_gicr() -> u64 {
    GICR.load(Ordering::SeqCst) + super::cpu::id() as u64 * GICR_STRIDE
}
static SPURIOUS: AtomicU64 = AtomicU64::new(0);

fn w32(addr: u64, v: u32) {
    unsafe { (addr as *mut u32).write_volatile(v) }
}
fn r32(addr: u64) -> u32 {
    unsafe { (addr as *const u32).read_volatile() }
}

/// Bring the GIC up (boot CPU): `gicd` / `gicr` are the mapped virtual
/// addresses of the distributor and of the redistributor region.
pub fn init(gicd: u64, gicr: u64) -> Result<(), &'static str> {
    GICD.store(gicd, Ordering::SeqCst);
    GICR.store(gicr, Ordering::SeqCst);
    // Distributor: affinity routing + group 1 enabled.
    w32(gicd + GICD_CTLR, (1 << 4) | (1 << 1) | 1);
    init_cpu()
}

/// This CPU's part: wake its redistributor, enable its CPU interface.
pub fn init_cpu() -> Result<(), &'static str> {
    let gicr = my_gicr();
    // Wake this CPU's redistributor.
    let waker = r32(gicr + GICR_WAKER);
    w32(gicr + GICR_WAKER, waker & !(1 << 1));
    let mut n = 0;
    while r32(gicr + GICR_WAKER) & (1 << 2) != 0 {
        n += 1;
        if n > 1_000_000 {
            return Err("gic: redistributor did not wake");
        }
        core::hint::spin_loop();
    }
    // CPU interface: system registers, all priorities, group 1 on.
    unsafe {
        let mut sre: u64;
        asm!("mrs {}, icc_sre_el1", out(reg) sre, options(nomem, nostack));
        sre |= 1;
        asm!("msr icc_sre_el1, {}", "isb", in(reg) sre, options(nostack));
        asm!("msr icc_pmr_el1, {}", in(reg) 0xFFu64, options(nostack));
        asm!("msr icc_bpr1_el1, {}", in(reg) 0u64, options(nostack));
        asm!("msr icc_igrpen1_el1, {}", "isb", in(reg) 1u64, options(nostack));
    }
    Ok(())
}

/// Enable private interrupt (SGI/PPI) `intid` (< 32) on this CPU, group 1.
pub fn enable_ppi(intid: u32) {
    let sgi = my_gicr() + SGI_BASE;
    w32(sgi + GICR_IGROUPR0, r32(sgi + GICR_IGROUPR0) | (1 << intid));
    unsafe { ((sgi + GICR_IPRIORITYR + intid as u64) as *mut u8).write_volatile(0x80) };
    w32(sgi + GICR_ISENABLER0, 1 << intid);
}

/// IRQ entry: acknowledge, dispatch, end of interrupt. Returns the interrupt
/// id handled (None if spurious).
pub fn handle_irq() -> Option<u32> {
    let intid: u64;
    unsafe { asm!("mrs {}, icc_iar1_el1", out(reg) intid, options(nomem, nostack)) };
    let id = intid & 0xFF_FFFF;
    if id >= 1020 {
        SPURIOUS.fetch_add(1, Ordering::Relaxed);
        return None; // spurious: no EOI
    }
    if id == super::timer::INTID as u64 {
        super::timer::on_tick();
    }
    unsafe { asm!("msr icc_eoir1_el1, {}", "isb", in(reg) intid, options(nostack)) };
    Some(id as u32)
}
