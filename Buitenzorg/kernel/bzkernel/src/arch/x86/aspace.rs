//! Per-process user address spaces (multi-process increment 1).
//!
//! Every ring-3 program links at the same addresses (image at 0x40_0000, mmap
//! arena at 0x2000_0000, stack at 0x7000_0000), so two programs can only be
//! resident at once if each gets its own mapping of the user half.
//!
//! The whole user half (`memory::USER_ADDR_MAX` = 2 GiB) is covered by exactly
//! two PDPT entries under PML4[0]. Rather than giving each process a full page
//! table hierarchy (and having to keep kernel mappings in sync across copies),
//! a process owns just two private page directories; switching processes
//! rewrites those two PDPT entries in the one shared hierarchy and flushes the
//! TLB. Kernel mappings are shared by construction, so anything the kernel maps
//! later is visible to every process.
//!
//! Space id 0 is the kernel's own ("base") space, used by kernel tasks.
//! Process space ids double as process ids (see `process.rs`).
//!
//! When a space dies (`destroy`), every frame it owns goes back to the frame
//! allocator: user pages, the page tables under its two directories, and the
//! directories themselves.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::memory;

const PTE_PRESENT: u64 = 1 << 0;
const PTE_WRITABLE: u64 = 1 << 1;
const PTE_USER: u64 = 1 << 2;
const PTE_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// A process's private page directories for the two user PDPT slots.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Space {
    pub id: u64,
    pub pd: [u64; 2], // physical addresses
}

/// Kernel virtual address of the PDPT under PML4[0] (shared by everyone).
static PDPT_VIRT: AtomicU64 = AtomicU64::new(0);
/// The kernel's own PDPT[0]/[1] entries, restored for space 0.
static BASE_PD: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
/// Id of the space currently installed in the PDPT.
static CURRENT: AtomicU64 = AtomicU64::new(0);

/// The kernel's base space.
pub fn kernel() -> Space {
    Space {
        id: 0,
        pd: [
            BASE_PD[0].load(Ordering::Relaxed) & PTE_ADDR_MASK,
            BASE_PD[1].load(Ordering::Relaxed) & PTE_ADDR_MASK,
        ],
    }
}

/// Id of the space currently installed.
pub fn current_id() -> u64 {
    CURRENT.load(Ordering::SeqCst)
}

fn ready() -> bool {
    PDPT_VIRT.load(Ordering::Relaxed) != 0
}

/// Locate the shared PDPT and record the kernel's base entries. Must run after
/// `memory::install_ctx`. Creates empty base page directories if the bootloader
/// left the user slots unpopulated, so the base never changes afterwards.
pub fn init() {
    use x86_64::registers::control::Cr3;
    let (l4, _) = Cr3::read();
    let l4_virt = memory::phys_to_virt(l4.start_address().as_u64()) as *mut u64;
    let pml4e = unsafe { core::ptr::read_volatile(l4_virt) };
    assert!(pml4e & PTE_PRESENT != 0, "aspace: PML4[0] not present");
    let pdpt = memory::phys_to_virt(pml4e & PTE_ADDR_MASK) as *mut u64;
    PDPT_VIRT.store(pdpt as u64, Ordering::SeqCst);

    for slot in 0..2 {
        let mut e = unsafe { core::ptr::read_volatile(pdpt.add(slot)) };
        if e & PTE_PRESENT == 0 {
            let phys = alloc_zeroed_frame().expect("aspace: out of frames");
            e = phys | PTE_PRESENT | PTE_WRITABLE | PTE_USER;
            unsafe { core::ptr::write_volatile(pdpt.add(slot), e) };
        }
        assert!(e & (1 << 7) == 0, "aspace: 1 GiB page in the user half");
        BASE_PD[slot].store(e, Ordering::SeqCst);
        // Report kernel-owned 2 MiB slots inside the user half: a process shares
        // these (see `create`), so a user mapping must never land in one.
        let pd = memory::phys_to_virt(e & PTE_ADDR_MASK) as *const u64;
        for i in 0..512 {
            let pde = unsafe { core::ptr::read_volatile(pd.add(i)) };
            if pde & PTE_PRESENT != 0 && pde & PTE_USER == 0 {
                let va = (slot as u64) << 30 | (i as u64) << 21;
                crate::println!("[aspace] kernel mapping in user half at {:#x} (shared)", va);
            }
        }
    }
    crate::println!("[aspace] per-process address spaces ready (2 private PDs each)");
}

fn alloc_zeroed_frame() -> Option<u64> {
    memory::with_ctx(|ctx| {
        let phys = ctx.frames.alloc()?;
        unsafe { core::ptr::write_bytes(memory::phys_to_virt(phys) as *mut u8, 0, 4096) };
        Some(phys)
    })
}

/// Create a fresh, empty user address space with id `id`. Kernel-owned entries
/// of the base directories (no USER bit — e.g. bootloader identity mappings)
/// are shared; everything a previous user program mapped is not.
pub fn create(id: u64) -> Result<Space, &'static str> {
    if !ready() {
        return Err("address spaces not initialized");
    }
    let mut pd = [0u64; 2];
    for slot in 0..2 {
        let phys = alloc_zeroed_frame().ok_or("out of frames")?;
        let base = memory::phys_to_virt(BASE_PD[slot].load(Ordering::Relaxed) & PTE_ADDR_MASK)
            as *const u64;
        let dst = memory::phys_to_virt(phys) as *mut u64;
        for i in 0..512 {
            let pde = unsafe { core::ptr::read_volatile(base.add(i)) };
            if pde & PTE_PRESENT != 0 && pde & PTE_USER == 0 {
                unsafe { core::ptr::write_volatile(dst.add(i), pde) };
            }
        }
        pd[slot] = phys;
    }
    memory::mmap_space_create(id);
    Ok(Space { id, pd })
}

/// Tear down a dead space: free every user page, every page table under its
/// two directories, and the directories, then drop its mmap arena. It must
/// not be installed (callers switch to another space first) and none of its
/// tasks may ever run again. Returns the number of frames freed.
///
/// Directory entries shared from the kernel's base directories (bootloader
/// identity maps such as 0x0 and 0x1200000) are skipped: those frames are not
/// the process's. An entry is shared if it equals the base entry at the same
/// index, *or* lacks the USER bit — either test alone would already do.
pub fn destroy(space: Space) -> u64 {
    if space.id == 0 {
        return 0;
    }
    assert!(CURRENT.load(Ordering::SeqCst) != space.id, "aspace: destroying the installed space");
    let mut freed = 0;
    for slot in 0..2 {
        let pd_phys = space.pd[slot];
        if pd_phys == 0 {
            continue;
        }
        let pd = memory::phys_to_virt(pd_phys) as *const u64;
        let base = memory::phys_to_virt(BASE_PD[slot].load(Ordering::Relaxed) & PTE_ADDR_MASK)
            as *const u64;
        for i in 0..512 {
            let pde = unsafe { core::ptr::read_volatile(pd.add(i)) };
            let shared = pde == unsafe { core::ptr::read_volatile(base.add(i)) };
            if pde & PTE_PRESENT == 0 || pde & PTE_USER == 0 || shared || pde & (1 << 7) != 0 {
                continue;
            }
            let pt_phys = pde & PTE_ADDR_MASK;
            let pt = memory::phys_to_virt(pt_phys) as *const u64;
            for j in 0..512 {
                let pte = unsafe { core::ptr::read_volatile(pt.add(j)) };
                if pte & PTE_PRESENT != 0 && pte & PTE_USER != 0 {
                    memory::free_frame(pte & PTE_ADDR_MASK);
                    freed += 1;
                }
            }
            memory::free_frame(pt_phys);
            freed += 1;
        }
        memory::free_frame(pd_phys);
        freed += 1;
    }
    memory::mmap_space_destroy(space.id);
    freed
}

/// Install `space` into the shared PDPT (no-op if already current) and flush
/// the TLB. Call with interrupts disabled.
pub fn install(space: Space) {
    if !ready() || CURRENT.load(Ordering::SeqCst) == space.id {
        return;
    }
    let pdpt = PDPT_VIRT.load(Ordering::Relaxed) as *mut u64;
    for slot in 0..2 {
        // The kernel space always gets its recorded entries back (tasks created
        // before `init` carry a placeholder copy of it).
        let entry = if space.id == 0 {
            BASE_PD[slot].load(Ordering::Relaxed)
        } else {
            space.pd[slot] | PTE_PRESENT | PTE_WRITABLE | PTE_USER
        };
        unsafe { core::ptr::write_volatile(pdpt.add(slot), entry) };
    }
    CURRENT.store(space.id, Ordering::SeqCst);
    crate::arch::cpu::flush_tlb();
}
