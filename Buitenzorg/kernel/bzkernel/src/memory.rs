//! Physical frame allocator + page table access (v0.2 "Akar": memory manager).

use core::sync::atomic::{AtomicU64, Ordering};

use bootloader_api::info::{MemoryRegionKind, MemoryRegions};
use x86_64::structures::paging::{
    FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

/// The bootloader's complete-physical-memory mapping offset, stored so other
/// modules (e.g. the ELF loader) can convert a physical frame to a writable
/// kernel virtual address.
static PHYS_OFFSET: AtomicU64 = AtomicU64::new(0);

/// Virtual address at which physical address `phys` is mapped by the kernel.
pub fn phys_to_virt(phys: u64) -> u64 {
    PHYS_OFFSET.load(Ordering::Relaxed) + phys
}

static TOTAL_USABLE_MIB: AtomicU64 = AtomicU64::new(0);

/// Record total usable physical memory (MiB) for SYS_STAT.
pub fn set_total_usable_mib(mib: u64) {
    TOTAL_USABLE_MIB.store(mib, Ordering::Relaxed);
}

/// Total usable physical memory in MiB.
pub fn total_usable_mib() -> u64 {
    TOTAL_USABLE_MIB.load(Ordering::Relaxed)
}

const PTE_USER: u64 = 1 << 2;
const PTE_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

const PTE_NX: u64 = 1 << 63;

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
    use x86_64::registers::control::Cr3;
    let (l4_frame, _) = Cr3::read();
    let mut table_phys = l4_frame.start_address().as_u64();
    for level in [39u64, 30, 21] {
        let idx = ((vaddr >> level) & 0x1FF) as usize;
        let entry = (phys_to_virt(table_phys) as *mut u64).wrapping_add(idx);
        let val = unsafe { core::ptr::read_volatile(entry) };
        if val & 1 == 0 {
            return; // not present (shouldn't happen after map_to)
        }
        unsafe { core::ptr::write_volatile(entry, (val | PTE_USER) & !PTE_NX) };
        table_phys = val & PTE_ADDR_MASK;
    }
}

/// Initialize an [`OffsetPageTable`] using the complete physical memory
/// mapping installed by the bootloader at `physical_memory_offset`.
///
/// # Safety
/// Caller must guarantee the full physical memory is mapped at the given
/// offset and that this is called only once (aliasing `&mut PageTable`).
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    PHYS_OFFSET.store(physical_memory_offset.as_u64(), Ordering::Relaxed);
    let level_4_table = active_level_4_table(physical_memory_offset);
    OffsetPageTable::new(level_4_table, physical_memory_offset)
}

/// Map `count` writable, user-accessible pages at `base`. Returns the top
/// address (for a user stack) and the list of mapped page addresses (for
/// teardown). Frames are zeroed.
pub fn map_user_region(
    base: u64,
    count: u64,
    mapper: &mut impl x86_64::structures::paging::Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(u64, alloc::vec::Vec<u64>), &'static str> {
    use x86_64::structures::paging::{Page, PageTableFlags};
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;
    let mut pages = alloc::vec::Vec::new();
    for i in 0..count {
        let addr = base + i * 4096;
        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(addr));
        let frame = frame_allocator.allocate_frame().ok_or("out of frames")?;
        let dst = phys_to_virt(frame.start_address().as_u64()) as *mut u8;
        unsafe { core::ptr::write_bytes(dst, 0, 4096) };
        unsafe {
            mapper
                .map_to_with_table_flags(page, frame, flags, flags, frame_allocator)
                .map_err(|_| "map_to failed for user region")?
                .flush();
        }
        make_user_path(addr);
        pages.push(addr);
    }
    x86_64::instructions::tlb::flush_all();
    Ok((base + count * 4096, pages))
}

/// Unmap a set of user pages (on process exit). Frames are not returned to the
/// bump allocator (it does not reclaim); this frees the virtual mappings so a
/// later program can reuse the same addresses.
pub fn unmap_user_pages(
    pages: &[u64],
    mapper: &mut impl x86_64::structures::paging::Mapper<Size4KiB>,
) {
    use x86_64::structures::paging::{Page, Size4KiB};
    for &addr in pages {
        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(addr));
        if let Ok((_frame, flush)) = mapper.unmap(page) {
            flush.flush();
        }
    }
    x86_64::instructions::tlb::flush_all();
}

unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (frame, _) = Cr3::read();
    let phys = frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let ptr: *mut PageTable = virt.as_mut_ptr();
    &mut *ptr
}

/// Global paging context so subsystems (app launcher, shell) can map user
/// memory after boot without threading `&mut` references through every call.
pub struct MemCtx {
    pub mapper: OffsetPageTable<'static>,
    pub frames: BootInfoFrameAllocator,
}

static CTX: spin::Mutex<Option<MemCtx>> = spin::Mutex::new(None);

/// Store the boot-created mapper + frame allocator as the global context.
pub fn install_ctx(mapper: OffsetPageTable<'static>, frames: BootInfoFrameAllocator) {
    *CTX.lock() = Some(MemCtx { mapper, frames });
}

/// Run `f` with the global paging context. Panics if called before install.
pub fn with_ctx<R>(f: impl FnOnce(&mut MemCtx) -> R) -> R {
    let mut guard = CTX.lock();
    f(guard.as_mut().expect("memory context not installed"))
}

/// Frame allocator over the bootloader-provided memory map.
pub struct BootInfoFrameAllocator {
    regions: &'static MemoryRegions,
    next: usize,
}

impl BootInfoFrameAllocator {
    /// # Safety
    /// The memory map must be valid and its `Usable` regions really unused.
    pub unsafe fn init(regions: &'static MemoryRegions) -> Self {
        Self { regions, next: 0 }
    }

    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> + '_ {
        self.regions
            .iter()
            .filter(|r| r.kind == MemoryRegionKind::Usable)
            .map(|r| r.start..r.end)
            .flat_map(|r| r.step_by(4096))
            .map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }

    /// Number of usable 4 KiB frames reported by the bootloader.
    pub fn usable_frame_count(&self) -> usize {
        self.usable_frames().count()
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.usable_frames().nth(self.next);
        self.next += 1;
        frame
    }
}
