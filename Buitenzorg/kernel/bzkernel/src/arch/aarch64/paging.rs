//! AArch64 stage-1 paging (the `arch::paging` HAL), 4 KiB granule, 4 levels,
//! 48-bit virtual addresses.
//!
//! Two roots, as the architecture intends: **TTBR1_EL1** translates the upper
//! half (the kernel image, the direct map Limine built, and the kernel's own
//! windows — heap, MMIO), **TTBR0_EL1** the lower half, where user programs
//! live (below 2 GiB, like on x86). Generic code calls the same
//! map / unmap / protect / is_mapped / user_access as on x86; addresses below
//! [`USER_TOP`] go through the active TTBR0 root, everything else through the
//! kernel root. Page tables are reached through the direct map.
//!
//! Memory attributes (MAIR_EL1): index 0 = Normal write-back (Limine,
//! base revision >= 4), index 2 = Device-nGnRnE (set here).

use core::arch::asm;

use crate::memory::{phys_to_virt, BootInfoFrameAllocator};

/// Generic page protection (shared with the x86 HAL).
#[derive(Clone, Copy)]
pub struct Flags {
    pub write: bool,
    pub user: bool,
    pub exec: bool,
}

/// ELF `e_machine` of native executables (EM_AARCH64).
pub const ELF_MACHINE: u16 = 183;

/// The kernel heap, in the upper half (the lower half belongs to processes).
pub const KERNEL_HEAP_BASE: u64 = 0xFFFF_D000_0000_0000;
/// The MMIO window for device registers.
pub const MMIO_BASE: u64 = 0xFFFF_C000_0000_0000;

/// User addresses: the per-process lower half used by programs.
pub const USER_TOP: u64 = 0x8000_0000;

const VALID: u64 = 1 << 0;
const TABLE: u64 = 1 << 1; // table (levels 0-2) or page (level 3)
const ATTR_NORMAL: u64 = 0 << 2;
const ATTR_DEVICE: u64 = 2 << 2;
const AP_EL0: u64 = 1 << 6; // EL0 may access
const AP_RO: u64 = 1 << 7; // read-only (both levels)
const SH_INNER: u64 = 3 << 8;
const AF: u64 = 1 << 10;
const NG: u64 = 1 << 11;
const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;
const ADDR: u64 = 0x0000_FFFF_FFFF_F000;

/// Handle on the kernel's page tables (the TTBR1 root).
pub struct PageMap {
    kernel_root: u64,
}

fn ttbr0() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, ttbr0_el1", out(reg) v, options(nomem, nostack)) };
    v & ADDR
}

fn root_for(map: &PageMap, va: u64) -> u64 {
    if va < USER_TOP {
        ttbr0()
    } else {
        map.kernel_root
    }
}

fn table(phys: u64) -> *mut u64 {
    phys_to_virt(phys) as *mut u64
}

fn tlbi_page(va: u64) {
    unsafe {
        asm!("dsb ishst", "tlbi vaae1is, {}", "dsb ish", "isb", in(reg) (va >> 12) & 0xF_FFFF_FFFF, options(nostack))
    }
}

/// Take over the MMU state Limine left: Device-nGnRnE in MAIR index 2, and a
/// fresh, empty TTBR0 root (with the walk enabled in TCR_EL1) for user space.
///
/// # Safety
/// Physical memory must be mapped at `phys_offset`; call once.
pub unsafe fn init(_phys_offset: u64) -> PageMap {
    let kernel_root: u64;
    unsafe {
        asm!("mrs {}, ttbr1_el1", out(reg) kernel_root, options(nomem, nostack));
        let mut mair: u64;
        asm!("mrs {}, mair_el1", out(reg) mair, options(nomem, nostack));
        mair &= !(0xFF << 16); // attr 2 = 0x00: Device-nGnRnE
        asm!("msr mair_el1, {}", "isb", in(reg) mair, options(nostack));
    }
    PageMap { kernel_root: kernel_root & ADDR }
}

/// Give the lower half its own empty root and make sure TCR_EL1 walks it
/// (T0SZ = 16, 4 KiB granule, write-back inner-shareable walks, EPD0 = 0).
pub fn init_user_half(frames: &mut BootInfoFrameAllocator) -> Option<u64> {
    let root = frames.alloc()?;
    unsafe {
        core::ptr::write_bytes(table(root) as *mut u8, 0, 4096);
        let mut tcr: u64;
        asm!("mrs {}, tcr_el1", out(reg) tcr, options(nomem, nostack));
        tcr &= !0xFFFF; // T0SZ, EPD0, IRGN0, ORGN0, SH0, TG0
        tcr |= 16 | (1 << 8) | (1 << 10) | (3 << 12); // T0SZ=16, WB/WB, inner shareable, TG0=4K
        asm!("msr tcr_el1, {}", "isb", in(reg) tcr, options(nostack));
        asm!("msr ttbr0_el1, {}", "isb", "tlbi vmalle1is", "dsb ish", "isb", in(reg) root, options(nostack));
    }
    Some(root)
}

fn leaf_bits(f: Flags) -> u64 {
    let mut d = VALID | TABLE | ATTR_NORMAL | SH_INNER | AF;
    if !f.write {
        d |= AP_RO;
    }
    if f.user {
        d |= AP_EL0 | NG | PXN; // the kernel never executes user pages
        if !f.exec {
            d |= UXN;
        }
    } else {
        d |= UXN;
        if !f.exec {
            d |= PXN;
        }
    }
    d
}

/// Walk to the level-3 entry for `va`, creating tables when `frames` is given.
fn walk(root: u64, va: u64, mut frames: Option<&mut BootInfoFrameAllocator>) -> Result<*mut u64, &'static str> {
    let mut t = root;
    for level in 0..3 {
        let e = unsafe { table(t).add(((va >> (39 - 9 * level)) & 0x1FF) as usize) };
        let d = unsafe { e.read_volatile() };
        if d & VALID == 0 {
            let Some(fr) = frames.as_deref_mut() else { return Err("not mapped") };
            let new = fr.alloc().ok_or("out of frames for page tables")?;
            unsafe {
                core::ptr::write_bytes(table(new) as *mut u8, 0, 4096);
                e.write_volatile(new | VALID | TABLE);
            }
            t = new;
        } else if d & TABLE == 0 {
            return Err("covered by a block mapping");
        } else {
            t = d & ADDR;
        }
    }
    Ok(unsafe { table(t).add(((va >> 12) & 0x1FF) as usize) })
}

/// Map the 4 KiB page at `va` to the frame at `pa`.
/// Map `pages` consecutive kernel pages from `va` to the physical range from `pa`.
pub fn map_phys_range(pm: &mut PageMap, frames: &mut BootInfoFrameAllocator, va: u64, pa: u64, pages: u64, f: Flags) -> Result<(), &'static str> {
    for k in 0..pages {
        map(pm, frames, va + k * 4096, pa + k * 4096, f)?;
    }
    Ok(())
}

/// Map `pages` consecutive kernel pages from `va`, each to a fresh frame.
pub fn map_fresh_range(pm: &mut PageMap, frames: &mut BootInfoFrameAllocator, va: u64, pages: u64, f: Flags) -> Result<(), &'static str> {
    for k in 0..pages {
        let phys = frames.alloc().ok_or("out of frames")?;
        map(pm, frames, va + k * 4096, phys, f)?;
    }
    Ok(())
}

pub fn map(map: &mut PageMap, frames: &mut BootInfoFrameAllocator, va: u64, pa: u64, f: Flags) -> Result<(), &'static str> {
    let e = walk(root_for(map, va), va, Some(frames))?;
    unsafe {
        if e.read_volatile() & VALID != 0 {
            return Err("page already mapped");
        }
        e.write_volatile((pa & ADDR) | leaf_bits(f));
    }
    tlbi_page(va);
    Ok(())
}

/// Map a device (MMIO) page: Device-nGnRnE, kernel-only, never executable.
pub fn map_device(map: &mut PageMap, frames: &mut BootInfoFrameAllocator, va: u64, pa: u64) -> Result<(), &'static str> {
    let e = walk(root_for(map, va), va, Some(frames))?;
    unsafe { e.write_volatile((pa & ADDR) | VALID | TABLE | ATTR_DEVICE | AF | PXN | UXN) };
    tlbi_page(va);
    Ok(())
}

/// Unmap the page at `va`; returns the frame it pointed at.
pub fn unmap(map: &mut PageMap, va: u64) -> Option<u64> {
    let e = walk(root_for(map, va), va, None).ok()?;
    let d = unsafe { e.read_volatile() };
    if d & VALID == 0 {
        return None;
    }
    unsafe { e.write_volatile(0) };
    tlbi_page(va);
    Some(d & ADDR)
}

/// True if `va` is mapped (page or block).
pub fn is_mapped(map: &mut PageMap, va: u64) -> bool {
    lookup(root_for(map, va), va).is_some()
}

/// Change the protection of the mapped page at `va`.
pub fn protect(map: &mut PageMap, va: u64, f: Flags) -> bool {
    let Ok(e) = walk(root_for(map, va), va, None) else { return false };
    let d = unsafe { e.read_volatile() };
    if d & VALID == 0 {
        return false;
    }
    unsafe { e.write_volatile((d & ADDR) | leaf_bits(f)) };
    tlbi_page(va);
    true
}

/// The leaf descriptor translating `va` (a level-3 page, or a level-1/2
/// block), if any.
fn lookup(root: u64, va: u64) -> Option<u64> {
    let mut t = root;
    for level in 0..4 {
        let d = unsafe { table(t).add(((va >> (39 - 9 * level)) & 0x1FF) as usize).read_volatile() };
        if d & VALID == 0 {
            return None;
        }
        if level == 3 || d & TABLE == 0 {
            return Some(d);
        }
        t = d & ADDR;
    }
    None
}

/// A fresh lower-half root that identity-maps the 4 KiB page at `phys`
/// (kernel-executable): a secondary CPU switching its MMU on while running at
/// that physical address keeps fetching from it. Returns the root.
pub fn identity_root(frames: &mut BootInfoFrameAllocator, phys: u64) -> Option<u64> {
    let root = frames.alloc()?;
    unsafe { core::ptr::write_bytes(table(root) as *mut u8, 0, 4096) };
    let e = walk(root, phys, Some(frames)).ok()?;
    unsafe { e.write_volatile((phys & ADDR) | VALID | TABLE | ATTR_NORMAL | SH_INNER | AF | UXN) };
    Some(root)
}

/// EL0 access rights of a user address in the active tables:
/// `Some((user_accessible, writable))`, or `None` if unmapped.
pub fn user_access(va: u64) -> Option<(bool, bool)> {
    if va >= USER_TOP {
        return Some((false, false));
    }
    let d = lookup(ttbr0(), va)?;
    Some((d & AP_EL0 != 0, d & AP_RO == 0))
}
