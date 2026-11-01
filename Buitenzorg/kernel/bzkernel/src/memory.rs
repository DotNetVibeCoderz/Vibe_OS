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

/// Upper bound of the ring-3 address space. Everything a user app can legally
/// touch lives below this: the app image at 0x40_0000, the mmap arena
/// (0x2000_0000..0x6000_0000) and the user stack at 0x7000_0000. Every kernel
/// mapping — the heap at 0x4444_4444_0000, the complete-physical-memory window
/// and the kernel image itself — is above it.
pub const USER_ADDR_MAX: u64 = 0x8000_0000; // 2 GiB

const PTE_WRITABLE: u64 = 1 << 1;

/// Walk the active page tables for `vaddr` and return its leaf entry, or `None`
/// if any level is missing. Read-only: unlike [`make_user_path`] this never
/// widens permissions.
fn leaf_entry(vaddr: u64) -> Option<u64> {
    use x86_64::registers::control::Cr3;
    let (l4_frame, _) = Cr3::read();
    let mut table_phys = l4_frame.start_address().as_u64();
    for level in [39u64, 30, 21, 12] {
        let idx = ((vaddr >> level) & 0x1FF) as usize;
        let entry = (phys_to_virt(table_phys) as *const u64).wrapping_add(idx);
        let val = unsafe { core::ptr::read_volatile(entry) };
        if val & 1 == 0 {
            return None; // not present
        }
        if level > 12 && val & (1 << 7) != 0 {
            return Some(val); // huge page: this entry is the leaf
        }
        if level == 12 {
            return Some(val);
        }
        table_phys = val & PTE_ADDR_MASK;
    }
    None
}

/// Check that `[ptr, ptr+len)` is memory the calling ring-3 app may legitimately
/// hand to the kernel: inside the user half, mapped, and marked
/// `USER_ACCESSIBLE` (plus writable when `need_write`).
///
/// **Why this exists (security hardening for v1.0):** syscalls copy through raw
/// pointers supplied by ring 3. Without this check an app could pass a *kernel*
/// address and have the kernel read it back out (`DEBUG_WRITE` — an information
/// leak) or, far worse, write into it (`FS_READ`, `PROC_LIST`, `PKG_LIST`,
/// `NET_RECV` all copy results to a user buffer) — an arbitrary kernel write,
/// i.e. full privilege escalation from an unprivileged app. An unmapped
/// address was equally fatal in a different way: the kernel would take a page
/// fault inside the syscall and die. Callers must reject the syscall when this
/// returns false.
pub fn validate_user_range(ptr: u64, len: u64, need_write: bool) -> bool {
    if len == 0 {
        return ptr != 0; // a zero-length buffer is fine, a null pointer is not
    }
    if ptr == 0 {
        return false;
    }
    // No wrap-around, and the whole range must stay in the user half.
    let end = match ptr.checked_add(len) {
        Some(e) => e,
        None => return false,
    };
    if end > USER_ADDR_MAX {
        return false;
    }
    // Every page in the range must be present and user-accessible.
    let first = ptr & !0xFFF;
    let last = (end - 1) & !0xFFF;
    let mut page = first;
    loop {
        match leaf_entry(page) {
            Some(e) => {
                if e & PTE_USER == 0 {
                    return false;
                }
                if need_write && e & PTE_WRITABLE == 0 {
                    return false;
                }
            }
            None => return false,
        }
        if page == last {
            break;
        }
        page += 4096;
    }
    true
}

/// Like [`validate_user_range`], but for a NUL-terminated string: checks that
/// the bytes up to the terminator (at most `max`) are readable user memory.
/// Returns the length found, or `None` if the string runs out of valid memory.
pub fn validate_user_cstr(ptr: u64, max: u64) -> Option<u64> {
    if ptr == 0 || ptr >= USER_ADDR_MAX {
        return None;
    }
    let mut n = 0u64;
    while n < max {
        // Re-validate on every page boundary (and for the first byte).
        if n == 0 || (ptr + n) & 0xFFF == 0 {
            if !validate_user_range(ptr + n, 1, false) {
                return None;
            }
        }
        let b = unsafe { core::ptr::read_volatile((ptr + n) as *const u8) };
        if b == 0 {
            return Some(n);
        }
        n += 1;
    }
    None // no terminator within `max`
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

/// Unmap a set of user pages and return their frames to the allocator.
pub fn unmap_user_pages(
    pages: &[u64],
    mapper: &mut impl x86_64::structures::paging::Mapper<Size4KiB>,
    frames: &mut BootInfoFrameAllocator,
) {
    use x86_64::structures::paging::{FrameDeallocator, Page, Size4KiB};
    for &addr in pages {
        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(addr));
        if let Ok((frame, flush)) = mapper.unmap(page) {
            flush.flush();
            unsafe { frames.deallocate_frame(frame) };
        }
    }
    x86_64::instructions::tlb::flush_all();
}

// --- User-space anonymous mmap arena (v0.15 "Matang" managed-runtime PAL) ----
//
// A per-process bump arena the ring-3 C# runtime uses for its managed heap
// (via the MMAP/MPROTECT/MUNMAP syscalls). It sits above the program image
// (0x40_0000) and below the user stack (0x7000_0000). Each address space
// (`aspace.rs`) has its own arena, keyed by space id, so concurrent processes
// hand out the same virtual ranges from their own page directories.

const USER_MMAP_BASE: u64 = 0x2000_0000; // 512 MiB
const USER_MMAP_LIMIT: u64 = 0x6000_0000; // stay well below the stack at 0x7000_0000

/// Per-space arena: next free address + the pages committed so far.
struct MmapArena {
    next: u64,
    pages: alloc::vec::Vec<u64>,
}

static ARENAS: spin::Mutex<alloc::collections::BTreeMap<u64, MmapArena>> =
    spin::Mutex::new(alloc::collections::BTreeMap::new());

/// Give address space `id` a fresh, empty arena.
pub fn mmap_space_create(id: u64) {
    ARENAS.lock().insert(id, MmapArena { next: USER_MMAP_BASE, pages: alloc::vec::Vec::new() });
}

/// Drop address space `id`'s arena (its pages die with the space).
pub fn mmap_space_destroy(id: u64) {
    ARENAS.lock().remove(&id);
}

/// Run `f` on the arena of the address space currently installed.
fn with_arena<R>(f: impl FnOnce(&mut MmapArena) -> R) -> Option<R> {
    let id = crate::aspace::current_id();
    ARENAS.lock().get_mut(&id).map(f)
}

fn flags_from_prot(prot: u64) -> x86_64::structures::paging::PageTableFlags {
    use x86_64::structures::paging::PageTableFlags as F;
    let mut f = F::PRESENT | F::USER_ACCESSIBLE;
    if prot & bz_abi::mmap_prot::WRITE != 0 {
        f |= F::WRITABLE;
    }
    if prot & bz_abi::mmap_prot::EXEC == 0 {
        f |= F::NO_EXECUTE;
    }
    f
}

/// Map `count` user pages at `base` with explicit protection `flags`.
fn map_user_region_flags(
    base: u64,
    count: u64,
    flags: x86_64::structures::paging::PageTableFlags,
    mapper: &mut impl x86_64::structures::paging::Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<alloc::vec::Vec<u64>, &'static str> {
    use x86_64::structures::paging::Page;
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
                .map_err(|_| "map_to failed for mmap region")?
                .flush();
        }
        make_user_path(addr);
        pages.push(addr);
    }
    x86_64::instructions::tlb::flush_all();
    Ok(pages)
}

fn prot_has_access(prot: u64) -> bool {
    use bz_abi::mmap_prot::{EXEC, READ, WRITE};
    prot & (READ | WRITE | EXEC) != 0
}

/// SYS_MMAP handler. With an access protection, commits `ceil(size/4096)` pages
/// immediately. With `NONE`, only **reserves** the address range (no frames) —
/// the .NET GC reserves a large heap up front this way and commits sub-ranges
/// on demand via `mprotect`. Returns the base virtual address.
pub fn user_mmap(size: u64, prot: u64) -> u64 {
    if size == 0 {
        return bz_abi::syserr::INVAL;
    }
    let count = (size + 4095) / 4096;
    let Some(base) = with_arena(|a| {
        let base = a.next;
        a.next += count * 4096;
        base
    }) else {
        return bz_abi::syserr::INVAL; // no arena: not a user process
    };
    if base + count * 4096 > USER_MMAP_LIMIT {
        return bz_abi::syserr::INVAL;
    }
    if !prot_has_access(prot) {
        // Reserve only: no frames are allocated (lazy reservation).
        return base;
    }
    let flags = flags_from_prot(prot);
    let mapped = with_ctx(|ctx| {
        map_user_region_flags(base, count, flags, &mut ctx.mapper, &mut ctx.frames)
    });
    match mapped {
        Ok(pages) => {
            with_arena(|a| a.pages.extend(pages));
            base
        }
        Err(_) => bz_abi::syserr::INVAL,
    }
}

/// SYS_MPROTECT handler. Commits reserved pages on demand: for each page in
/// `[addr, addr+size)`, if it is already mapped its flags are updated; if it is
/// unmapped and `prot` grants access, a fresh zeroed frame is mapped (commit).
/// This makes the GC's reserve-then-commit heap growth work.
pub fn user_mprotect(addr: u64, size: u64, prot: u64) -> u64 {
    use x86_64::structures::paging::{Mapper, Page};
    if size == 0 || addr % 4096 != 0 {
        return bz_abi::syserr::INVAL;
    }
    let count = (size + 4095) / 4096;
    let flags = flags_from_prot(prot);
    let has_access = prot_has_access(prot);
    with_ctx(|ctx| {
        for i in 0..count {
            let va = addr + i * 4096;
            let page = Page::<Size4KiB>::containing_address(VirtAddr::new(va));
            if ctx.mapper.translate_page(page).is_ok() {
                // Already committed: just re-flag.
                unsafe {
                    match ctx.mapper.update_flags(page, flags) {
                        Ok(flush) => flush.flush(),
                        Err(_) => return bz_abi::syserr::INVAL,
                    }
                }
            } else if has_access {
                // Reserved but not committed: map a fresh zeroed frame.
                match map_user_region_flags(va, 1, flags, &mut ctx.mapper, &mut ctx.frames) {
                    Ok(mut pages) => {
                        with_arena(|a| a.pages.append(&mut pages));
                    }
                    Err(_) => return bz_abi::syserr::INVAL,
                }
            }
            // else: prot NONE on an uncommitted page — nothing to do.
        }
        0
    })
}

/// SYS_MUNMAP handler: unmap pages in `[addr, addr+size)`.
pub fn user_munmap(addr: u64, size: u64) -> u64 {
    if size == 0 || addr % 4096 != 0 {
        return bz_abi::syserr::INVAL;
    }
    let count = (size + 4095) / 4096;
    let mut pages = alloc::vec::Vec::new();
    for i in 0..count {
        pages.push(addr + i * 4096);
    }
    with_ctx(|ctx| unmap_user_pages(&pages, &mut ctx.mapper, &mut ctx.frames));
    with_arena(|a| a.pages.retain(|p| !pages.contains(p)));
    0
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

/// Allocate one zeroed 4 KiB physical frame for device DMA, returning
/// `(physical_address, kernel_virtual_pointer)`. The physical address is what a
/// bus-master device (e.g. the AC'97 sound card) programs into its descriptors;
/// the virtual pointer (via the bootloader's complete physical map) is how the
/// CPU fills the buffer. The frame is leaked (never freed) — intended for a few
/// long-lived driver buffers set up once at boot.
pub fn alloc_dma_frame() -> Option<(u64, *mut u8)> {
    with_ctx(|ctx| {
        let frame = ctx.frames.allocate_frame()?;
        let phys = frame.start_address().as_u64();
        let virt = phys_to_virt(phys) as *mut u8;
        unsafe { core::ptr::write_bytes(virt, 0, 4096) };
        Some((phys, virt))
    })
}

/// Frame allocator over the bootloader-provided memory map. Hands out the
/// usable frames in map order and never reclaims them. It keeps a cursor
/// (region index + next address) — it used to re-walk the whole memory map
/// from the start for every frame (`usable_frames().nth(next)`), making each
/// allocation O(frames already handed out), which slowed every ELF load as
/// boot went on.
pub struct BootInfoFrameAllocator {
    regions: &'static MemoryRegions,
    region: usize,
    addr: u64,
    /// Intrusive free list of returned frames: each free frame stores the
    /// physical address of the next one in its first 8 bytes (FREE_END ends
    /// it). Costs no heap and is O(1) both ways.
    free_head: u64,
    allocated: u64,
    freed: u64,
}

/// Physical memory below this is never allocated (see `allocate_frame`).
const LOW_MEMORY_END: u64 = 0x10_0000;

/// End of the free list (phys 0 can be a usable frame, so it is no sentinel).
const FREE_END: u64 = u64::MAX;

impl BootInfoFrameAllocator {
    /// # Safety
    /// The memory map must be valid and its `Usable` regions really unused.
    pub unsafe fn init(regions: &'static MemoryRegions) -> Self {
        Self { regions, region: 0, addr: 0, free_head: FREE_END, allocated: 0, freed: 0 }
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
        // Recycled frames first. Callers zero what they need (the ELF loader,
        // map_user_region, page-table creation and aspace::create all do).
        if self.free_head != FREE_END {
            let head = self.free_head;
            self.free_head = unsafe { core::ptr::read_volatile(phys_to_virt(head) as *const u64) };
            self.allocated += 1;
            return Some(PhysFrame::containing_address(PhysAddr::new(head)));
        }
        // Then fresh frames, in memory-map order.
        loop {
            let r = self.regions.get(self.region)?;
            if r.kind == MemoryRegionKind::Usable {
                // Never hand out memory below 1 MiB: it holds the SMP
                // trampoline (0x8000, smp.rs) and must stay free for it.
                let floor = r.start.max(LOW_MEMORY_END);
                if self.addr < floor {
                    self.addr = floor;
                }
                if self.addr < r.end {
                    let frame = PhysFrame::containing_address(PhysAddr::new(self.addr));
                    self.addr += 4096;
                    self.allocated += 1;
                    return Some(frame);
                }
            }
            self.region += 1;
            self.addr = 0;
        }
    }
}

impl x86_64::structures::paging::FrameDeallocator<Size4KiB> for BootInfoFrameAllocator {
    /// Return a frame to the free list. The caller guarantees nothing maps or
    /// references it any more.
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame) {
        let phys = frame.start_address().as_u64();
        unsafe { core::ptr::write_volatile(phys_to_virt(phys) as *mut u64, self.free_head) };
        self.free_head = phys;
        self.freed += 1;
    }
}

/// True if the 4 KiB page at `phys` lies inside a region the firmware reported
/// as usable RAM (so the kernel may write to it, e.g. the SMP trampoline).
pub fn is_usable_ram(phys: u64) -> bool {
    with_ctx(|ctx| {
        ctx.frames
            .regions
            .iter()
            .any(|r| r.kind == MemoryRegionKind::Usable && r.start <= phys && phys + 4096 <= r.end)
    })
}

/// (frames handed out, frames returned) since boot; in use = the difference.
pub fn frame_stats() -> (u64, u64) {
    with_ctx(|ctx| (ctx.frames.allocated, ctx.frames.freed))
}

/// Frames currently in use (handed out and not yet returned).
pub fn frames_in_use() -> u64 {
    let (a, f) = frame_stats();
    a - f
}

/// Return the physical frame at `phys` to the allocator.
pub fn free_frame(phys: u64) {
    use x86_64::structures::paging::FrameDeallocator;
    with_ctx(|ctx| unsafe {
        ctx.frames.deallocate_frame(PhysFrame::containing_address(PhysAddr::new(phys)))
    });
}
