//! AArch64 MMU, increment 1: map device memory (MMIO) into the kernel half.
//!
//! Limine leaves the MMU on with 4 KiB granules and 4-level tables; the
//! kernel half (TTBR1_EL1) holds the kernel image and the direct map of RAM
//! (HHDM). Device registers are not in the HHDM, so `map_device` adds
//! Device-nGnRnE pages in a window of its own, creating intermediate tables
//! from frames taken off the top of the largest usable RAM region.
//! MAIR_EL1 attribute 2 is programmed as Device-nGnRnE (Limine, base revision
//! >= 4, only uses attributes 0 and 1).

use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

const MMIO_WINDOW: u64 = 0xFFFF_C000_0000_0000;
const ATTR_DEVICE: u64 = 2;

const DESC_VALID: u64 = 1 << 0;
const DESC_TABLE: u64 = 1 << 1; // table (levels 0-2) / page (level 3)
const DESC_AF: u64 = 1 << 10;
const DESC_PXN: u64 = 1 << 53;
const DESC_UXN: u64 = 1 << 54;
const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;

static NEXT_VA: AtomicU64 = AtomicU64::new(MMIO_WINDOW);
/// Next free frame for page tables (allocated downward), and the floor.
static FRAME_TOP: AtomicU64 = AtomicU64::new(0);
static FRAME_FLOOR: AtomicU64 = AtomicU64::new(0);
static TABLES: AtomicU64 = AtomicU64::new(0);

fn hhdm(phys: u64) -> *mut u64 {
    (crate::boot::info().phys_offset + phys) as *mut u64
}

/// Prepare: device attribute in MAIR, and a frame pool for new tables (the
/// top 1 MiB of the largest usable RAM region).
pub fn init() {
    let info = crate::boot::info();
    if let Some(r) = info.regions().iter().filter(|r| r.usable).max_by_key(|r| r.end - r.start) {
        FRAME_TOP.store(r.end & !0xFFF, Ordering::SeqCst);
        FRAME_FLOOR.store((r.end - (1 << 20)).max(r.start), Ordering::SeqCst);
    }
    unsafe {
        let mut mair: u64;
        asm!("mrs {}, mair_el1", out(reg) mair, options(nomem, nostack));
        mair &= !(0xFF << (8 * ATTR_DEVICE));
        mair |= 0x00 << (8 * ATTR_DEVICE); // Device-nGnRnE
        asm!("msr mair_el1, {}", "isb", in(reg) mair, options(nostack));
    }
}

fn alloc_table() -> Option<u64> {
    let top = FRAME_TOP.load(Ordering::SeqCst);
    let next = top.checked_sub(4096)?;
    if next < FRAME_FLOOR.load(Ordering::SeqCst) {
        return None;
    }
    FRAME_TOP.store(next, Ordering::SeqCst);
    unsafe { core::ptr::write_bytes(hhdm(next) as *mut u8, 0, 4096) };
    TABLES.fetch_add(1, Ordering::Relaxed);
    Some(next)
}

/// Page tables created so far.
pub fn tables_created() -> u64 {
    TABLES.load(Ordering::Relaxed)
}

fn ttbr1_root() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, ttbr1_el1", out(reg) v, options(nomem, nostack)) };
    v & ADDR_MASK
}

/// Map one 4 KiB device page `va` -> `pa` in the kernel half.
fn map_page(va: u64, pa: u64) -> Result<(), &'static str> {
    let mut table = ttbr1_root();
    for level in 0..3 {
        let idx = (va >> (39 - 9 * level)) & 0x1FF;
        let e = unsafe { hhdm(table).add(idx as usize) };
        let d = unsafe { e.read_volatile() };
        if d & DESC_VALID == 0 {
            let t = alloc_table().ok_or("mmu: out of page-table frames")?;
            unsafe { e.write_volatile(t | DESC_VALID | DESC_TABLE) };
            table = t;
        } else if d & DESC_TABLE == 0 {
            return Err("mmu: a block mapping covers the MMIO window");
        } else {
            table = d & ADDR_MASK;
        }
    }
    let idx = (va >> 12) & 0x1FF;
    let page = (pa & ADDR_MASK) | DESC_VALID | DESC_TABLE | DESC_AF | (ATTR_DEVICE << 2) | DESC_PXN | DESC_UXN;
    unsafe {
        hhdm(table).add(idx as usize).write_volatile(page);
        asm!("dsb ishst", "tlbi vaae1is, {}", "dsb ish", "isb", in(reg) (va >> 12) & 0xF_FFFF_FFFF, options(nostack));
    }
    Ok(())
}

/// Map `len` bytes of device memory at physical `phys`; returns the virtual
/// address of `phys`.
pub fn map_device(phys: u64, len: u64) -> Result<u64, &'static str> {
    let first = phys & !0xFFF;
    let pages = ((phys + len.max(1) + 0xFFF) & !0xFFF).saturating_sub(first) / 4096;
    let va = NEXT_VA.fetch_add(pages * 4096, Ordering::SeqCst);
    for i in 0..pages {
        map_page(va + i * 4096, first + i * 4096)?;
    }
    Ok(va + (phys - first))
}
