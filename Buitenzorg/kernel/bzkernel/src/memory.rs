//! Physical frame allocator + memory policy (v0.2 "Akar": memory manager).
//!
//! Architecture-neutral: page tables are reached through the `arch::paging`
//! HAL (map / unmap / protect with neutral [`Flags`], plus the ring-3 access
//! check), and physical frames are plain `u64` addresses.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::boot::MemRegion;

use crate::arch::paging::{self, Flags, PageMap};

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

/// Upper bound of the ring-3 address space. Everything a user app can legally
/// touch lives below this: the app image at 0x40_0000, the mmap arena
/// (0x2000_0000..0x6000_0000) and the user stack at 0x7000_0000. Every kernel
/// mapping — the heap at 0x4444_4444_0000, the complete-physical-memory window
/// and the kernel image itself — is above it.
pub const USER_ADDR_MAX: u64 = 0x8000_0000; // 2 GiB

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
        match paging::user_access(page) {
            Some((user, writable)) => {
                if !user || (need_write && !writable) {
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

/// Record the bootloader's physical-memory offset and wrap the active page
/// tables.
///
/// # Safety
/// The full physical memory must be mapped at `physical_memory_offset`, and
/// this may be called only once.
pub unsafe fn init(physical_memory_offset: u64) -> PageMap {
    PHYS_OFFSET.store(physical_memory_offset, Ordering::Relaxed);
    unsafe { paging::init(physical_memory_offset) }
}

/// Protection of a user data page (stack, heap): read/write, not executable.
const USER_DATA: Flags = Flags { write: true, user: true, exec: false };

/// Map `count` writable, user-accessible pages at `base`. Returns the top
/// address (for a user stack) and the list of mapped page addresses (for
/// teardown). Frames are zeroed.
pub fn map_user_region(
    base: u64,
    count: u64,
    ctx: &mut MemCtx,
) -> Result<(u64, alloc::vec::Vec<u64>), &'static str> {
    let pages = map_user_region_flags(base, count, USER_DATA, ctx)?;
    Ok((base + count * 4096, pages))
}

/// Unmap a set of user pages (this CPU's TLB is flushed); returns their
/// frames, which the caller hands to [`release_frames`] once it has let go of
/// the paging context.
pub fn unmap_user_pages(pages: &[u64], ctx: &mut MemCtx) -> alloc::vec::Vec<u64> {
    let mut frames = alloc::vec::Vec::new();
    for &addr in pages {
        if let Some(phys) = paging::unmap(&mut ctx.mapper, addr) {
            frames.push(phys);
        }
    }
    crate::arch::cpu::flush_tlb();
    frames
}

/// Free frames that were just unmapped: first make every other CPU drop stale
/// translations (TLB shootdown), then give the frames back — never the other
/// way round, or a CPU could still reach a frame that was handed out again.
/// Must be called with no lock held: the shootdown waits for other CPUs, and
/// one of them may be spinning on that lock with interrupts off.
pub fn release_frames(frames: alloc::vec::Vec<u64>) {
    if frames.is_empty() {
        return;
    }
    crate::arch::cpu::flush_tlb_everywhere();
    with_ctx(|ctx| {
        for f in frames {
            ctx.frames.free(f);
        }
    });
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

fn flags_from_prot(prot: u64) -> Flags {
    Flags {
        write: prot & bz_abi::mmap_prot::WRITE != 0,
        user: true,
        exec: prot & bz_abi::mmap_prot::EXEC != 0,
    }
}

/// Map `count` fresh zeroed user pages at `base` with protection `flags`.
fn map_user_region_flags(
    base: u64,
    count: u64,
    flags: Flags,
    ctx: &mut MemCtx,
) -> Result<alloc::vec::Vec<u64>, &'static str> {
    let mut pages = alloc::vec::Vec::new();
    for i in 0..count {
        let addr = base + i * 4096;
        let phys = ctx.frames.alloc().ok_or("out of frames")?;
        unsafe { core::ptr::write_bytes(phys_to_virt(phys) as *mut u8, 0, 4096) };
        paging::map(&mut ctx.mapper, &mut ctx.frames, addr, phys, flags)?;
        pages.push(addr);
    }
    crate::arch::cpu::flush_tlb();
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
    match with_ctx(|ctx| map_user_region_flags(base, count, flags, ctx)) {
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
    if size == 0 || addr % 4096 != 0 {
        return bz_abi::syserr::INVAL;
    }
    let count = (size + 4095) / 4096;
    let flags = flags_from_prot(prot);
    let has_access = prot_has_access(prot);
    let r = with_ctx(|ctx| {
        for i in 0..count {
            let va = addr + i * 4096;
            if paging::is_mapped(&mut ctx.mapper, va) {
                // Already committed: just re-flag.
                if !paging::protect(&mut ctx.mapper, va, flags) {
                    return bz_abi::syserr::INVAL;
                }
            } else if has_access {
                // Reserved but not committed: map a fresh zeroed frame.
                match map_user_region_flags(va, 1, flags, ctx) {
                    Ok(mut pages) => {
                        with_arena(|a| a.pages.append(&mut pages));
                    }
                    Err(_) => return bz_abi::syserr::INVAL,
                }
            }
            // else: prot NONE on an uncommitted page — nothing to do.
        }
        0
    });
    // Downgraded permissions must not survive in another CPU's TLB (shootdown
    // with the paging context released, see `release_frames`).
    crate::arch::cpu::flush_tlb_everywhere();
    r
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
    let frames = with_ctx(|ctx| unmap_user_pages(&pages, ctx));
    with_arena(|a| a.pages.retain(|p| !pages.contains(p)));
    release_frames(frames);
    0
}

/// Global paging context so subsystems (app launcher, shell) can map user
/// memory after boot without threading `&mut` references through every call.
pub struct MemCtx {
    pub mapper: PageMap,
    pub frames: BootInfoFrameAllocator,
}

static CTX: spin::Mutex<Option<MemCtx>> = spin::Mutex::new(None);

/// Store the boot-created mapper + frame allocator as the global context.
/// Needs no heap, so it runs right after `init` (the heap is mapped through it).
pub fn install_ctx(mapper: PageMap, frames: BootInfoFrameAllocator) {
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
        let phys = ctx.frames.alloc()?;
        let virt = phys_to_virt(phys) as *mut u8;
        unsafe { core::ptr::write_bytes(virt, 0, 4096) };
        Some((phys, virt))
    })
}

/// Frame allocator over the bootloader-provided memory map. Hands out the
/// usable frames in map order, then recycles returned ones. It keeps a cursor
/// (region index + next address) — it used to re-walk the whole memory map
/// from the start for every frame (`usable_frames().nth(next)`), making each
/// allocation O(frames already handed out), which slowed every ELF load as
/// boot went on. The architecture's paging code adapts it to its own
/// allocator interface (`arch/x86/paging.rs`).
pub struct BootInfoFrameAllocator {
    regions: &'static [MemRegion],
    region: usize,
    addr: u64,
    /// Intrusive free list of returned frames: each free frame stores the
    /// physical address of the next one in its first 8 bytes (FREE_END ends
    /// it). Costs no heap and is O(1) both ways.
    free_head: u64,
    allocated: u64,
    freed: u64,
}

/// Physical memory below this is never allocated (see `alloc`).
const LOW_MEMORY_END: u64 = 0x10_0000;

/// End of the free list (phys 0 can be a usable frame, so it is no sentinel).
const FREE_END: u64 = u64::MAX;

impl BootInfoFrameAllocator {
    /// # Safety
    /// The memory map must be valid and its `Usable` regions really unused.
    pub unsafe fn init(regions: &'static [MemRegion]) -> Self {
        Self { regions, region: 0, addr: 0, free_head: FREE_END, allocated: 0, freed: 0 }
    }

    /// Number of usable 4 KiB frames reported by the bootloader.
    pub fn usable_frame_count(&self) -> usize {
        self.regions
            .iter()
            .filter(|r| r.usable)
            .map(|r| ((r.end - r.start) / 4096) as usize)
            .sum()
    }

    /// Hand out one 4 KiB frame (its physical address). Not zeroed: callers
    /// zero what they need (the ELF loader, user mappings, page-table creation
    /// and aspace::create all do).
    pub fn alloc(&mut self) -> Option<u64> {
        // Recycled frames first.
        if self.free_head != FREE_END {
            let head = self.free_head;
            self.free_head = unsafe { core::ptr::read_volatile(phys_to_virt(head) as *const u64) };
            self.allocated += 1;
            return Some(head);
        }
        // Then fresh frames, in memory-map order.
        loop {
            let r = self.regions.get(self.region)?;
            if r.usable {
                // Never hand out memory below 1 MiB: it holds the SMP
                // trampoline (0x8000, smp.rs) and must stay free for it.
                let floor = r.start.max(LOW_MEMORY_END);
                if self.addr < floor {
                    self.addr = floor;
                }
                if self.addr < r.end {
                    let frame = self.addr;
                    self.addr += 4096;
                    self.allocated += 1;
                    return Some(frame);
                }
            }
            self.region += 1;
            self.addr = 0;
        }
    }

    /// Return the frame at `phys` to the free list. The caller guarantees
    /// nothing maps or references it any more.
    pub fn free(&mut self, phys: u64) {
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
            .any(|r| r.usable && r.start <= phys && phys + 4096 <= r.end)
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
    with_ctx(|ctx| ctx.frames.free(phys));
}
