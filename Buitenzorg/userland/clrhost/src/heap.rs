//! The Rust heap the runtime allocates from (type registry, IL bodies, the
//! managed objects' storage): a linked-list allocator over an address range
//! reserved once with `MMAP(PROT_NONE)` -- no frames yet -- and committed in
//! chunks with `MPROTECT` as it fills (the kernel's reserve-then-commit
//! model, the one the .NET GC uses). Freed memory is reused, unlike the
//! bump heap of the freestanding C# apps.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

use linked_list_allocator::Heap;

use crate::sys;

/// Address space reserved for the heap (frames are committed on demand).
const RESERVE: u64 = 768 << 20;
/// Committed up front and per growth step.
const CHUNK: u64 = 8 << 20;

pub struct BzHeap {
    lock: AtomicBool,
    heap: UnsafeCell<Heap>,
    base: UnsafeCell<u64>,
    committed: UnsafeCell<u64>,
}

unsafe impl Sync for BzHeap {}

#[global_allocator]
pub static HEAP: BzHeap = BzHeap {
    lock: AtomicBool::new(false),
    heap: UnsafeCell::new(Heap::empty()),
    base: UnsafeCell::new(0),
    committed: UnsafeCell::new(0),
};

impl BzHeap {
    fn with<R>(&self, f: impl FnOnce(&mut Heap, &mut u64, &mut u64) -> R) -> R {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        let r = unsafe { f(&mut *self.heap.get(), &mut *self.base.get(), &mut *self.committed.get()) };
        self.lock.store(false, Ordering::Release);
        r
    }

    /// Commit the next chunk (at least `need` bytes) and hand it to the
    /// allocator. False when the reservation is used up.
    fn grow(heap: &mut Heap, base: u64, committed: &mut u64, need: u64) -> bool {
        let step = need.div_ceil(CHUNK).max(1) * CHUNK;
        if *committed + step > RESERVE {
            return false;
        }
        if sys::syscall(sys::MPROTECT, base + *committed, step, sys::PROT_RW) != 0 {
            return false;
        }
        unsafe { heap.extend(step as usize) };
        *committed += step;
        true
    }

    /// (bytes committed, bytes in use).
    pub fn stats(&self) -> (u64, u64) {
        self.with(|h, _, c| (*c, h.used() as u64))
    }
}

/// Reserve the range and commit the first chunk. Must run before anything
/// allocates.
pub fn init() -> bool {
    let base = sys::syscall(sys::MMAP, RESERVE, sys::PROT_NONE, 0);
    if sys::is_err(base) || base == 0 {
        return false;
    }
    if sys::syscall(sys::MPROTECT, base, CHUNK, sys::PROT_RW) != 0 {
        return false;
    }
    HEAP.with(|h, b, c| {
        unsafe { h.init(base as *mut u8, CHUNK as usize) };
        *b = base;
        *c = CHUNK;
    });
    true
}

unsafe impl GlobalAlloc for BzHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.with(|h, base, committed| {
            if let Ok(p) = h.allocate_first_fit(layout) {
                return p.as_ptr();
            }
            // Committed space is full (or too fragmented): commit more and
            // retry. `extend` appends to the top hole, which is contiguous
            // with the reservation.
            let need = (layout.size() + layout.align()) as u64;
            if Self::grow(h, *base, committed, need) {
                if let Ok(p) = h.allocate_first_fit(layout) {
                    return p.as_ptr();
                }
            }
            core::ptr::null_mut()
        })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.with(|h, _, _| unsafe { h.deallocate(core::ptr::NonNull::new_unchecked(ptr), layout) })
    }
}
