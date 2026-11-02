//! ARM Generic Interrupt Controller, both generations the kernel meets:
//!
//! - **GICv3** (QEMU `virt`): a distributor (shared, SPIs), one redistributor
//!   per CPU (SGIs/PPIs) and a CPU interface in ICC_* system registers.
//!   `arm,gic-v3`: reg[0] = GICD, reg[1] = GICR region.
//! - **GICv2** (GIC-400, Raspberry Pi 4 and 5): no redistributors; a distributor
//!   whose first registers are banked per CPU, and a memory-mapped CPU interface,
//!   also banked. `arm,gic-400`: reg[0] = GICD, reg[1] = GICC.
//!
//! The timer is PPI 27 on both.

use core::arch::asm;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

const GICD_CTLR: u64 = 0x0000;
// v3
const GICR_WAKER: u64 = 0x0014;
const SGI_BASE: u64 = 0x1_0000; // second 64 KiB frame of a redistributor
const GICR_IGROUPR0: u64 = 0x0080;
const GICR_ISENABLER0: u64 = 0x0100;
const GICR_IPRIORITYR: u64 = 0x0400;
const GICR_STRIDE: u64 = 0x2_0000;
// v2 distributor (the private interrupts' registers are banked per CPU)
const GICD_ISENABLER0: u64 = 0x0100;
const GICD_IPRIORITYR: u64 = 0x0400;
// v2 CPU interface
const GICC_CTLR: u64 = 0x0000;
const GICC_PMR: u64 = 0x0004;
const GICC_BPR: u64 = 0x0008;
const GICC_IAR: u64 = 0x000C;
const GICC_EOIR: u64 = 0x0010;

/// 2 or 3 once a GIC is up.
static VERSION: AtomicU32 = AtomicU32::new(0);
static GICD: AtomicU64 = AtomicU64::new(0);
/// v3: base of the redistributor region (CPU n's frames at + n * 0x20000, as
/// QEMU `virt` lays them out in MPIDR order). v2: the CPU interface.
static CPU_BASE: AtomicU64 = AtomicU64::new(0);
static SPURIOUS: AtomicU64 = AtomicU64::new(0);

fn v2() -> bool {
    VERSION.load(Ordering::Relaxed) == 2
}

fn my_gicr() -> u64 {
    CPU_BASE.load(Ordering::SeqCst) + super::cpu::id() as u64 * GICR_STRIDE
}

fn w32(addr: u64, v: u32) {
    unsafe { (addr as *mut u32).write_volatile(v) }
}
fn r32(addr: u64) -> u32 {
    unsafe { (addr as *const u32).read_volatile() }
}

/// Bring a GICv3 up (boot CPU): `gicd` / `gicr` are the mapped virtual
/// addresses of the distributor and of the redistributor region.
pub fn init(gicd: u64, gicr: u64) -> Result<(), &'static str> {
    VERSION.store(3, Ordering::SeqCst);
    GICD.store(gicd, Ordering::SeqCst);
    CPU_BASE.store(gicr, Ordering::SeqCst);
    // Distributor: affinity routing + group 1 enabled.
    w32(gicd + GICD_CTLR, (1 << 4) | (1 << 1) | 1);
    init_cpu()
}

/// Bring a GICv2 up (boot CPU): mapped distributor and CPU interface.
pub fn init_v2(gicd: u64, gicc: u64) -> Result<(), &'static str> {
    VERSION.store(2, Ordering::SeqCst);
    GICD.store(gicd, Ordering::SeqCst);
    CPU_BASE.store(gicc, Ordering::SeqCst);
    // Both groups on: which one we are depends on the security state the
    // firmware left (the bit for the other one is ignored).
    w32(gicd + GICD_CTLR, 3);
    init_cpu()
}

/// This CPU's part. v3: wake its redistributor, enable its system-register
/// CPU interface. v2: its banked CPU interface.
pub fn init_cpu() -> Result<(), &'static str> {
    if v2() {
        let gicc = CPU_BASE.load(Ordering::SeqCst);
        w32(gicc + GICC_PMR, 0xFF);
        w32(gicc + GICC_BPR, 0);
        w32(gicc + GICC_CTLR, 3);
        return Ok(());
    }
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
    let (priority, enable) = if v2() {
        // The group is left alone. On a real Pi the firmware's armstub set it
        // and the register ignores Non-secure writes; on a GIC without security
        // extensions (QEMU) group 1 could not even be acknowledged: IAR reads
        // 1022 for ever, an interrupt storm.
        let d = GICD.load(Ordering::SeqCst);
        (d + GICD_IPRIORITYR, d + GICD_ISENABLER0)
    } else {
        let sgi = my_gicr() + SGI_BASE;
        w32(sgi + GICR_IGROUPR0, r32(sgi + GICR_IGROUPR0) | (1 << intid));
        (sgi + GICR_IPRIORITYR, sgi + GICR_ISENABLER0)
    };
    unsafe { ((priority + intid as u64) as *mut u8).write_volatile(0x80) };
    w32(enable, 1 << intid);
}

/// IRQ entry: acknowledge, dispatch, end of interrupt. Returns the interrupt
/// id handled (None if spurious).
pub fn handle_irq() -> Option<u32> {
    let is_v2 = v2();
    let gicc = CPU_BASE.load(Ordering::Relaxed);
    let iar: u64 = if is_v2 {
        r32(gicc + GICC_IAR) as u64
    } else {
        let v: u64;
        unsafe { asm!("mrs {}, icc_iar1_el1", out(reg) v, options(nomem, nostack)) };
        v
    };
    let id = if is_v2 { iar & 0x3FF } else { iar & 0xFF_FFFF };
    if id >= 1020 {
        SPURIOUS.fetch_add(1, Ordering::Relaxed);
        return None; // spurious: no EOI
    }
    if id == super::timer::INTID as u64 {
        super::timer::on_tick();
    }
    if is_v2 {
        w32(gicc + GICC_EOIR, iar as u32);
    } else {
        unsafe { asm!("msr icc_eoir1_el1, {}", "isb", in(reg) iar, options(nostack)) };
    }
    Some(id as u32)
}
