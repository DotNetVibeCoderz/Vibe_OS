//! x86-64 four-level paging (the `arch::paging` HAL).
//!
//! Generic code describes a mapping with [`Flags`] (write / user / exec) and
//! physical frames as plain `u64` addresses; this module turns that into x86
//! page-table entries through the `x86_64` crate's `OffsetPageTable` (the
//! bootloader maps all physical memory at a fixed offset, see
//! `memory::phys_to_virt`). It also adapts the generic frame allocator
//! (`memory::BootInfoFrameAllocator`) to the crate's allocator traits.

use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{
    FrameAllocator, FrameDeallocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags,
    PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

use crate::memory::{phys_to_virt, BootInfoFrameAllocator};

/// The kernel's handle on the active page tables.
pub type PageMap = OffsetPageTable<'static>;

/// ELF `e_machine` of executables for this architecture (EM_X86_64).
pub const ELF_MACHINE: u16 = 0x3E;

const PTE_PRESENT: u64 = 1 << 0;
const PTE_WRITABLE: u64 = 1 << 1;
const PTE_USER: u64 = 1 << 2;
const PTE_HUGE: u64 = 1 << 7;
const PTE_NX: u64 = 1 << 63;
const PTE_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// Architecture-neutral protection of one page.
#[derive(Clone, Copy)]
pub struct Flags {
    pub write: bool,
    pub user: bool,
    pub exec: bool,
}

fn pte_flags(f: Flags) -> PageTableFlags {
    let mut p = PageTableFlags::PRESENT;
    if f.write {
        p |= PageTableFlags::WRITABLE;
    }
    if f.user {
        p |= PageTableFlags::USER_ACCESSIBLE;
    }
    if !f.exec {
        p |= PageTableFlags::NO_EXECUTE;
    }
    p
}

fn page(va: u64) -> Page<Size4KiB> {
    Page::containing_address(VirtAddr::new(va))
}

fn frame(pa: u64) -> PhysFrame<Size4KiB> {
    PhysFrame::containing_address(PhysAddr::new(pa))
}

// The generic frame allocator, seen through the x86_64 crate's traits (the
// mapper allocates intermediate page tables through them).
unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        self.alloc().map(frame)
    }
}

impl FrameDeallocator<Size4KiB> for BootInfoFrameAllocator {
    unsafe fn deallocate_frame(&mut self, f: PhysFrame) {
        self.free(f.start_address().as_u64());
    }
}

/// Wrap the active page tables. `memory::init` records the physical-memory
/// offset first.
///
/// # Safety
/// All physical memory must be mapped at `phys_offset`, and this may be called
/// only once (it hands out a `&'static mut` to the live top-level table).
pub unsafe fn init(phys_offset: u64) -> PageMap {
    let (l4, _) = Cr3::read();
    let table = (phys_offset + l4.start_address().as_u64()) as *mut PageTable;
    unsafe { OffsetPageTable::new(&mut *table, VirtAddr::new(phys_offset)) }
}

/// Ring-3 addresses (the per-process half below 2 GiB) are mapped through the
/// page tables *this CPU* is running on: each CPU installs the running
/// process's directories in its own root, so the kernel's stored master
/// `PageMap` would reach the wrong process on an application processor.
/// Kernel addresses always go through the master, whose upper levels every
/// CPU shares.
const USER_TOP: u64 = 0x8000_0000;

fn active() -> PageMap {
    let (l4, _) = Cr3::read();
    let offset = phys_to_virt(0);
    let table = (offset + l4.start_address().as_u64()) as *mut PageTable;
    unsafe { OffsetPageTable::new(&mut *table, VirtAddr::new(offset)) }
}

/// Map the 4 KiB page at `va` to the frame at `pa`. Intermediate tables are
/// created as needed; for user pages every level of the walk is opened to
/// ring 3 (see [`make_user_path`]).
pub fn map(map: &mut PageMap, frames: &mut BootInfoFrameAllocator, va: u64, pa: u64, f: Flags) -> Result<(), &'static str> {
    let mut local;
    let map = if va < USER_TOP {
        local = active();
        &mut local
    } else {
        map
    };
    let flags = pte_flags(f);
    let result = if f.user {
        let parent = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
        unsafe { map.map_to_with_table_flags(page(va), frame(pa), flags, parent, frames) }
    } else {
        unsafe { map.map_to(page(va), frame(pa), flags, frames) }
    };
    result.map_err(|_| "page mapping failed")?.flush();
    if f.user {
        make_user_path(va);
    }
    Ok(())
}

/// Map the kernel page at `va` to device memory (MMIO) at `pa`: writable,
/// never executable, uncached (PCD + PWT), so register reads and writes reach
/// the device in program order.
pub fn map_device(map: &mut PageMap, frames: &mut BootInfoFrameAllocator, va: u64, pa: u64) -> Result<(), &'static str> {
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_CACHE
        | PageTableFlags::WRITE_THROUGH
        | PageTableFlags::NO_EXECUTE;
    unsafe { map.map_to(page(va), frame(pa), flags, frames) }
        .map_err(|_| "device mapping failed")?
        .flush();
    Ok(())
}

/// Unmap the page at `va`; returns the frame it pointed at.
pub fn unmap(map: &mut PageMap, va: u64) -> Option<u64> {
    let mut local;
    let map = if va < USER_TOP {
        local = active();
        &mut local
    } else {
        map
    };
    let (f, flush) = map.unmap(page(va)).ok()?;
    flush.flush();
    Some(f.start_address().as_u64())
}

/// True if `va` is mapped.
pub fn is_mapped(map: &mut PageMap, va: u64) -> bool {
    if va < USER_TOP {
        return active().translate_page(page(va)).is_ok();
    }
    map.translate_page(page(va)).is_ok()
}

/// Change the protection of the mapped page at `va`.
pub fn protect(map: &mut PageMap, va: u64, f: Flags) -> bool {
    let mut local;
    let map = if va < USER_TOP {
        local = active();
        &mut local
    } else {
        map
    };
    match unsafe { map.update_flags(page(va), pte_flags(f)) } {
        Ok(flush) => {
            flush.flush();
            true
        }
        Err(_) => false,
    }
}

/// Prepare the PML4/PDPT/PD entries on the walk to `vaddr` for ring-3 use:
/// set `USER_ACCESSIBLE` and clear `NX` on each intermediate entry.
///
/// Both are AND-ed across the walk: a ring-3 access needs the user bit on
/// every level, and an instruction fetch faults if NX is set at any level.
/// The upper tables are shared with kernel mappings created without user and
/// (for the low PML4 entry) with NX set, so we adjust them here. Only the
/// intermediate entries are widened; the actual leaves keep their own user/NX
/// bits, so kernel pages under the same subtree stay protected and
/// non-executable.
pub fn make_user_path(vaddr: u64) {
    let (l4_frame, _) = Cr3::read();
    let mut table_phys = l4_frame.start_address().as_u64();
    for level in [39u64, 30, 21] {
        let idx = ((vaddr >> level) & 0x1FF) as usize;
        let entry = (phys_to_virt(table_phys) as *mut u64).wrapping_add(idx);
        let val = unsafe { core::ptr::read_volatile(entry) };
        if val & PTE_PRESENT == 0 {
            return; // not present (shouldn't happen after map_to)
        }
        unsafe { core::ptr::write_volatile(entry, (val | PTE_USER) & !PTE_NX) };
        table_phys = val & PTE_ADDR_MASK;
    }
}

/// Ring-3 access rights of `vaddr` in the active tables, read from its leaf
/// entry: `Some((user_accessible, writable))`, or `None` if unmapped. Never
/// changes anything.
pub fn user_access(vaddr: u64) -> Option<(bool, bool)> {
    let (l4_frame, _) = Cr3::read();
    let mut table_phys = l4_frame.start_address().as_u64();
    for level in [39u64, 30, 21, 12] {
        let idx = ((vaddr >> level) & 0x1FF) as usize;
        let entry = (phys_to_virt(table_phys) as *const u64).wrapping_add(idx);
        let val = unsafe { core::ptr::read_volatile(entry) };
        if val & PTE_PRESENT == 0 {
            return None;
        }
        if level == 12 || val & PTE_HUGE != 0 {
            return Some((val & PTE_USER != 0, val & PTE_WRITABLE != 0));
        }
        table_phys = val & PTE_ADDR_MASK;
    }
    None
}
