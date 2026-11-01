//! Per-process address spaces on RISC-V: each process owns a root page table
//! (`satp`) whose lower half is private and whose upper half repeats the
//! kernel's top-level entries (shared tables, see `paging`). Switching =
//! write `satp` + `sfence.vma`. Space id 0 is the kernel's (empty lower half).

use core::sync::atomic::{AtomicU64, Ordering};

use crate::memory;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Space {
    pub id: u64,
    pub root: u64,
}

static KERNEL_ROOT: AtomicU64 = AtomicU64::new(0);
/// Space installed on each CPU (u64::MAX = none yet).
static CURRENT: [AtomicU64; crate::arch::cpu::MAX_CPUS] =
    [const { AtomicU64::new(u64::MAX) }; crate::arch::cpu::MAX_CPUS];

fn me() -> usize {
    crate::arch::cpu::id()
}

/// Record the kernel's root (already active), created at boot.
pub fn init(kernel_root: u64) {
    KERNEL_ROOT.store(kernel_root, Ordering::SeqCst);
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

/// A fresh user address space (kernel half shared).
pub fn create(id: u64) -> Result<Space, &'static str> {
    let root = memory::with_ctx(|ctx| super::paging::new_root(&mut ctx.frames)).ok_or("out of frames")?;
    memory::mmap_space_create(id);
    Ok(Space { id, root })
}

/// Make `space` active on this hart.
pub fn install(space: Space) {
    let c = me();
    if CURRENT[c].load(Ordering::SeqCst) == space.id {
        return;
    }
    super::paging::switch_root(space.root);
    CURRENT[c].store(space.id, Ordering::SeqCst);
}

/// Free every user page and table of a dead space (not installed anywhere).
/// Returns the number of frames freed.
pub fn destroy(space: Space) -> u64 {
    if space.id == 0 {
        return 0;
    }
    assert!(
        CURRENT.iter().all(|c| c.load(Ordering::SeqCst) != space.id),
        "aspace: destroying a space installed on a CPU"
    );
    let freed = super::paging::free_user_half(space.root, &mut |f| memory::free_frame(f));
    memory::mmap_space_destroy(space.id);
    freed
}
