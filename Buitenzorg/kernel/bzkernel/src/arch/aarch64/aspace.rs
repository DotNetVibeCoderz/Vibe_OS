//! Per-process address spaces on AArch64: each process owns a TTBR0 root
//! (the lower half, user programs), while TTBR1 (the kernel half) is shared by
//! every space by construction — the split the architecture was designed
//! for, so unlike the x86 PDPT-swap model nothing has to be copied or kept in
//! sync. Switching a space = load TTBR0_EL1 + invalidate the TLB (user pages
//! are non-global). Space id 0 is the kernel's (an empty user half).

use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::memory::{self, phys_to_virt};

const VALID: u64 = 1;
const TABLE: u64 = 2;
const ADDR: u64 = 0x0000_FFFF_FFFF_F000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Space {
    pub id: u64,
    pub root: u64,
}

static KERNEL_ROOT: AtomicU64 = AtomicU64::new(0);
/// Space installed on each CPU (u64::MAX = none yet: an AP before its first
/// install).
static CURRENT: [AtomicU64; crate::arch::cpu::MAX_CPUS] =
    [const { AtomicU64::new(u64::MAX) }; crate::arch::cpu::MAX_CPUS];

fn me() -> usize {
    crate::arch::cpu::id()
}

/// Record the kernel's (empty) user-half root, created at boot.
pub fn init(kernel_user_root: u64) {
    KERNEL_ROOT.store(kernel_user_root, Ordering::SeqCst);
    CURRENT[me()].store(0, Ordering::SeqCst);
}

pub fn kernel() -> Space {
    Space { id: 0, root: KERNEL_ROOT.load(Ordering::SeqCst) }
}

/// Id of the installed space.
pub fn current_id() -> u64 {
    match CURRENT[me()].load(Ordering::SeqCst) {
        u64::MAX => 0,
        id => id,
    }
}

/// A fresh, empty user address space.
pub fn create(id: u64) -> Result<Space, &'static str> {
    let root = memory::with_ctx(|ctx| ctx.frames.alloc()).ok_or("out of frames")?;
    unsafe { core::ptr::write_bytes(phys_to_virt(root) as *mut u8, 0, 4096) };
    memory::mmap_space_create(id);
    Ok(Space { id, root })
}

/// Make `space` the active lower half on this CPU. The TLB flush is local:
/// other CPUs keep their own TTBR0 (changes to a shared mapping are
/// broadcast by the paging code's `tlbi ...is`).
pub fn install(space: Space) {
    let c = me();
    if CURRENT[c].load(Ordering::SeqCst) == space.id {
        return;
    }
    unsafe {
        asm!(
            "msr ttbr0_el1, {}",
            "isb",
            "tlbi vmalle1",
            "dsb nsh",
            "isb",
            in(reg) space.root,
            options(nostack)
        );
    }
    CURRENT[c].store(space.id, Ordering::SeqCst);
}

/// Free every page and table of a dead space (it must not be installed).
/// Returns the number of frames freed.
pub fn destroy(space: Space) -> u64 {
    if space.id == 0 {
        return 0;
    }
    assert!(
        CURRENT.iter().all(|c| c.load(Ordering::SeqCst) != space.id),
        "aspace: destroying a space installed on a CPU"
    );
    fn free_table(t: u64, level: u32, freed: &mut u64) {
        let e = phys_to_virt(t) as *const u64;
        for i in 0..512 {
            let d = unsafe { e.add(i).read_volatile() };
            if d & VALID == 0 {
                continue;
            }
            if level < 3 && d & TABLE != 0 {
                free_table(d & ADDR, level + 1, freed);
            } else if level == 3 {
                memory::free_frame(d & ADDR);
                *freed += 1;
            }
        }
        memory::free_frame(t);
        *freed += 1;
    }
    let mut freed = 0;
    free_table(space.root, 0, &mut freed);
    memory::mmap_space_destroy(space.id);
    freed
}
