//! RISC-V paging (the `arch::paging` HAL): Sv48 as Limine sets it up (the
//! level count follows `satp.MODE`, so Sv39 and Sv57 work too), 4 KiB pages.
//!
//! RISC-V has a single root (`satp`) for both halves. A process gets its own
//! root whose **upper half (top-level entries 256..511) is copied from the
//! kernel's**; so that the copy can never go stale, every empty kernel-half
//! top-level entry is given a table at boot ([`init_user_half`]) — later
//! kernel mappings (heap, MMIO window) only change tables below the top
//! level, which all roots share. User programs live below [`USER_TOP`]
//! (2 GiB), like on x86 and ARM64.
//!
//! The supervisor never executes user pages (the ISA forbids it) and reads
//! or writes them only with `sstatus.SUM` set (`cpu::allow_user_access`).

use core::arch::asm;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::memory::{phys_to_virt, BootInfoFrameAllocator};

/// Generic page protection (shared with the x86 HAL).
#[derive(Clone, Copy)]
pub struct Flags {
    pub write: bool,
    pub user: bool,
    pub exec: bool,
}

/// ELF `e_machine` of native executables (EM_RISCV).
pub const ELF_MACHINE: u16 = 243;

/// The kernel heap, in the upper half (the lower half belongs to processes).
pub const KERNEL_HEAP_BASE: u64 = 0xFFFF_D000_0000_0000;
/// The MMIO window for device registers.
pub const MMIO_BASE: u64 = 0xFFFF_C000_0000_0000;

/// User addresses: the per-process lower half used by programs.
pub const USER_TOP: u64 = 0x8000_0000;

const V: u64 = 1 << 0;
const R: u64 = 1 << 1;
const W: u64 = 1 << 2;
const X: u64 = 1 << 3;
const U: u64 = 1 << 4;
const G: u64 = 1 << 5;
const A: u64 = 1 << 6;
const D: u64 = 1 << 7;
const PPN_MASK: u64 = 0x003F_FFFF_FFFF_FC00;

/// Page-table levels of the active mode (3 = Sv39, 4 = Sv48, 5 = Sv57).
static LEVELS: AtomicU32 = AtomicU32::new(4);
/// satp.MODE of the active mode.
static MODE: AtomicU32 = AtomicU32::new(9);

/// Handle on the kernel's page tables.
pub struct PageMap {
    kernel_root: u64,
}

fn pte_addr(pte: u64) -> u64 {
    (pte & PPN_MASK) << 2
}

fn pte_for(pa: u64) -> u64 {
    (pa >> 12) << 10
}

/// Physical address of the active root.
pub fn active_root() -> u64 {
    (super::cpu::satp() & 0xFFF_FFFF_FFFF) << 12
}

/// The `satp` value selecting `root` in the active mode (ASID 0).
pub fn satp_for(root: u64) -> u64 {
    ((MODE.load(Ordering::Relaxed) as u64) << 60) | (root >> 12)
}

/// Switch this hart to `root` and drop its stale translations.
pub fn switch_root(root: u64) {
    unsafe { asm!("csrw satp, {}", "sfence.vma", in(reg) satp_for(root), options(nostack)) };
}

/// Paging mode name (for the boot log).
pub fn mode_name() -> &'static str {
    match LEVELS.load(Ordering::Relaxed) {
        3 => "Sv39",
        4 => "Sv48",
        5 => "Sv57",
        _ => "?",
    }
}

fn levels() -> usize {
    LEVELS.load(Ordering::Relaxed) as usize
}

fn index(va: u64, level: usize) -> usize {
    // level 0 = the root.
    ((va >> (12 + 9 * (levels() - 1 - level))) & 0x1FF) as usize
}

fn table(phys: u64) -> *mut u64 {
    phys_to_virt(phys) as *mut u64
}

fn sfence_page(va: u64) {
    unsafe { asm!("sfence.vma {}, zero", in(reg) va, options(nostack)) }
}

/// Take over the paging Limine left (mode from `satp`).
///
/// # Safety
/// Physical memory must be mapped at `phys_offset`; call once.
pub unsafe fn init(_phys_offset: u64) -> PageMap {
    let satp = super::cpu::satp();
    let mode = (satp >> 60) as u32;
    MODE.store(mode, Ordering::SeqCst);
    LEVELS.store(mode.saturating_sub(5).clamp(3, 5), Ordering::SeqCst);
    PageMap { kernel_root: active_root() }
}

/// Make the kernel half shareable (a table behind every kernel-half top-level
/// entry), then give the kernel its own root with an empty lower half and
/// switch to it. Returns that root (the kernel's address space).
pub fn init_user_half(frames: &mut BootInfoFrameAllocator) -> Option<u64> {
    let old = active_root();
    let top = table(old);
    for i in 256..512 {
        unsafe {
            if top.add(i).read_volatile() & V == 0 {
                let t = frames.alloc()?;
                core::ptr::write_bytes(table(t) as *mut u8, 0, 4096);
                top.add(i).write_volatile(pte_for(t) | V);
            }
        }
    }
    let root = new_root(frames)?;
    switch_root(root);
    Some(root)
}

/// A fresh root: empty lower half, the kernel half copied from the kernel's
/// (active) root.
pub fn new_root(frames: &mut BootInfoFrameAllocator) -> Option<u64> {
    let root = frames.alloc()?;
    let src = table(active_root());
    let dst = table(root);
    unsafe {
        core::ptr::write_bytes(dst as *mut u8, 0, 2048);
        for i in 256..512 {
            dst.add(i).write_volatile(src.add(i).read_volatile());
        }
    }
    Some(root)
}

fn leaf_bits(f: Flags) -> u64 {
    let mut d = V | R | A | D;
    if f.write {
        d |= W;
    }
    if f.exec {
        d |= X;
    }
    if f.user {
        d |= U;
    } else {
        d |= G;
    }
    d
}

/// Walk to the leaf entry for `va`, creating tables when `frames` is given.
fn walk(root: u64, va: u64, mut frames: Option<&mut BootInfoFrameAllocator>) -> Result<*mut u64, &'static str> {
    let mut t = root;
    for level in 0..levels() - 1 {
        let e = unsafe { table(t).add(index(va, level)) };
        let d = unsafe { e.read_volatile() };
        if d & V == 0 {
            let Some(fr) = frames.as_deref_mut() else { return Err("not mapped") };
            let new = fr.alloc().ok_or("out of frames for page tables")?;
            unsafe {
                core::ptr::write_bytes(table(new) as *mut u8, 0, 4096);
                e.write_volatile(pte_for(new) | V);
            }
            t = new;
        } else if d & (R | W | X) != 0 {
            return Err("covered by a superpage");
        } else {
            t = pte_addr(d);
        }
    }
    Ok(unsafe { table(t).add(index(va, levels() - 1)) })
}

fn root_for(map: &PageMap, va: u64) -> u64 {
    if va < USER_TOP {
        active_root()
    } else {
        map.kernel_root
    }
}

/// Map the 4 KiB page at `va` to the frame at `pa`.
pub fn map(map: &mut PageMap, frames: &mut BootInfoFrameAllocator, va: u64, pa: u64, f: Flags) -> Result<(), &'static str> {
    let e = walk(root_for(map, va), va, Some(frames))?;
    unsafe {
        if e.read_volatile() & V != 0 {
            return Err("page already mapped");
        }
        e.write_volatile(pte_for(pa) | leaf_bits(f));
    }
    sfence_page(va);
    Ok(())
}

/// Map a device (MMIO) page: kernel-only, never executable (on QEMU `virt`
/// the physical memory attributes already make device ranges uncached).
pub fn map_device(map: &mut PageMap, frames: &mut BootInfoFrameAllocator, va: u64, pa: u64) -> Result<(), &'static str> {
    let e = walk(root_for(map, va), va, Some(frames))?;
    unsafe { e.write_volatile(pte_for(pa) | V | R | W | A | D | G) };
    sfence_page(va);
    Ok(())
}

/// Unmap the page at `va`; returns the frame it pointed at.
pub fn unmap(map: &mut PageMap, va: u64) -> Option<u64> {
    let e = walk(root_for(map, va), va, None).ok()?;
    let d = unsafe { e.read_volatile() };
    if d & V == 0 {
        return None;
    }
    unsafe { e.write_volatile(0) };
    sfence_page(va);
    Some(pte_addr(d))
}

/// True if `va` is mapped (page or superpage).
pub fn is_mapped(map: &mut PageMap, va: u64) -> bool {
    lookup(root_for(map, va), va).is_some()
}

/// Change the protection of the mapped page at `va`.
pub fn protect(map: &mut PageMap, va: u64, f: Flags) -> bool {
    let Ok(e) = walk(root_for(map, va), va, None) else { return false };
    let d = unsafe { e.read_volatile() };
    if d & V == 0 {
        return false;
    }
    unsafe { e.write_volatile(pte_for(pte_addr(d)) | leaf_bits(f)) };
    sfence_page(va);
    true
}

/// The leaf entry translating `va` (a page or a superpage), if any.
fn lookup(root: u64, va: u64) -> Option<u64> {
    let mut t = root;
    for level in 0..levels() {
        let d = unsafe { table(t).add(index(va, level)).read_volatile() };
        if d & V == 0 {
            return None;
        }
        if d & (R | W | X) != 0 {
            return Some(d);
        }
        t = pte_addr(d);
    }
    None
}

/// U-mode access rights of a user address in the active tables:
/// `Some((user_accessible, writable))`, or `None` if unmapped.
pub fn user_access(va: u64) -> Option<(bool, bool)> {
    if va >= USER_TOP {
        return Some((false, false));
    }
    let d = lookup(active_root(), va)?;
    Some((d & U != 0, d & W != 0))
}

/// Free every page and table of a root's lower (user) half, and the root.
/// Returns the number of frames freed.
pub fn free_user_half(root: u64, free: &mut dyn FnMut(u64)) -> u64 {
    fn walk_free(t: u64, level: usize, last: usize, free: &mut dyn FnMut(u64), n: &mut u64) {
        let e = table(t);
        for i in 0..512 {
            let d = unsafe { e.add(i).read_volatile() };
            if d & V == 0 {
                continue;
            }
            if d & (R | W | X) == 0 && level < last {
                walk_free(pte_addr(d), level + 1, last, free, n);
            } else if level == last {
                free(pte_addr(d));
                *n += 1;
            }
        }
        free(t);
        *n += 1;
    }
    let last = levels() - 1;
    let mut n = 0;
    let top = table(root);
    for i in 0..256 {
        let d = unsafe { top.add(i).read_volatile() };
        if d & V != 0 && d & (R | W | X) == 0 {
            walk_free(pte_addr(d), 1, last, free, &mut n);
        }
    }
    free(root);
    n + 1
}
