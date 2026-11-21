//! The Rust heap the runtime allocates from (type registry, IL bodies, the
//! managed objects' storage), over an address range reserved once with
//! `MMAP(PROT_NONE)` -- no frames yet -- and committed in chunks with
//! `MPROTECT` as it fills (the kernel's reserve-then-commit model, the one
//! the .NET GC uses). Freed memory is reused.
//!
//! Two layers. Blocks up to 4 KiB (nearly every allocation the interpreter
//! makes) come from **size classes**: one intrusive free list per class, fed
//! by 64 KiB slabs carved front to back, so allocating and freeing a small
//! block is O(1). Larger blocks (and the slabs) come from a first-fit
//! linked-list allocator. That list used to serve everything: each alloc and
//! free walks it, it grows with fragmentation, and a fragmented heap made a
//! workload of 160 000 small strings take 265 s instead of 0.6 s -- and a
//! managed thread's exit (it frees its whole copy of the interpreter) stall a
//! `Thread.Join` for half a minute.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use linked_list_allocator::Heap;

use crate::sys;

/// Address space reserved for the heap (frames are committed on demand).
const RESERVE: u64 = 768 << 20;
/// Committed up front and per growth step.
const CHUNK: u64 = 8 << 20;

/// Block sizes of the size classes (multiples of 16, so every block is
/// 16-byte aligned inside a 16-byte aligned slab).
const CLASSES: [usize; 28] = [
    16, 32, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768, 896, 1024, 1280,
    1536, 1792, 2048, 2560, 3072, 3584, 4096,
];
const SMALL_MAX: usize = 4096;
/// Bytes taken from the large heap per refill.
const SLAB: usize = 64 << 10;

/// Class index for every size in 16-byte steps up to `SMALL_MAX`.
const CLASS_OF: [u8; SMALL_MAX / 16 + 1] = {
    let mut t = [0u8; SMALL_MAX / 16 + 1];
    let mut step = 0;
    while step <= SMALL_MAX / 16 {
        let size = step * 16;
        let mut c = 0;
        while CLASSES[c] < size {
            c += 1;
        }
        t[step] = c as u8;
        step += 1;
    }
    t
};

/// The size class of a layout, or None for the large heap.
fn class(layout: Layout) -> Option<usize> {
    if layout.size() > SMALL_MAX || layout.align() > 16 {
        return None;
    }
    Some(CLASS_OF[layout.size().div_ceil(16)] as usize)
}

/// One free list per class plus the unused tail of the current slab.
struct Classes {
    free: [*mut u8; CLASSES.len()],
    bump: [usize; CLASSES.len()],
    end: [usize; CLASSES.len()],
}

struct Inner {
    large: Heap,
    small: Classes,
    base: u64,
    committed: u64,
}

pub struct BzHeap {
    lock: AtomicBool,
    inner: UnsafeCell<Inner>,
}

unsafe impl Sync for BzHeap {}

#[global_allocator]
pub static HEAP: BzHeap = BzHeap {
    lock: AtomicBool::new(false),
    inner: UnsafeCell::new(Inner {
        large: Heap::empty(),
        small: Classes { free: [null_mut(); CLASSES.len()], bump: [0; CLASSES.len()], end: [0; CLASSES.len()] },
        base: 0,
        committed: 0,
    }),
};

impl BzHeap {
    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        // Threads allocate concurrently (several CPUs); a holder preempted
        // on this CPU would keep a spinner waiting a whole slice, so yield
        // after a short spin.
        let mut spins = 0u32;
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            spins += 1;
            if spins % 64 == 0 {
                sys::syscall(3, 0, 0, 0); // YIELD
            }
            core::hint::spin_loop();
        }
        let r = unsafe { f(&mut *self.inner.get()) };
        self.lock.store(false, Ordering::Release);
        r
    }

    /// (bytes committed, bytes in use).
    pub fn stats(&self) -> (u64, u64) {
        self.with(|h| (h.committed, h.large.used() as u64))
    }
}

impl Inner {
    /// Commit the next chunk (at least `need` bytes) and hand it to the
    /// large allocator. False when the reservation is used up.
    fn grow(&mut self, need: u64) -> bool {
        let step = need.div_ceil(CHUNK).max(1) * CHUNK;
        if self.committed + step > RESERVE {
            return false;
        }
        if sys::syscall(sys::MPROTECT, self.base + self.committed, step, sys::PROT_RW) != 0 {
            return false;
        }
        unsafe { self.large.extend(step as usize) };
        self.committed += step;
        true
    }

    fn alloc_large(&mut self, layout: Layout) -> *mut u8 {
        if let Ok(p) = self.large.allocate_first_fit(layout) {
            return p.as_ptr();
        }
        // Committed space is full (or too fragmented): commit more and
        // retry. `extend` appends to the top hole, which is contiguous with
        // the reservation.
        if self.grow((layout.size() + layout.align()) as u64) {
            if let Ok(p) = self.large.allocate_first_fit(layout) {
                return p.as_ptr();
            }
        }
        null_mut()
    }

    fn alloc_small(&mut self, c: usize) -> *mut u8 {
        let head = self.small.free[c];
        if !head.is_null() {
            self.small.free[c] = unsafe { *(head as *mut *mut u8) };
            return head;
        }
        let size = CLASSES[c];
        if self.small.bump[c] + size > self.small.end[c] {
            let slab = self.alloc_large(Layout::from_size_align(SLAB, 16).unwrap());
            if slab.is_null() {
                return null_mut();
            }
            self.small.bump[c] = slab as usize;
            self.small.end[c] = slab as usize + SLAB;
        }
        let p = self.small.bump[c];
        self.small.bump[c] += size;
        p as *mut u8
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
    HEAP.with(|h| {
        unsafe { h.large.init(base as *mut u8, CHUNK as usize) };
        h.base = base;
        h.committed = CHUNK;
    });
    true
}

unsafe impl GlobalAlloc for BzHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.with(|h| match class(layout) {
            Some(c) => h.alloc_small(c),
            None => h.alloc_large(layout),
        })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.with(|h| match class(layout) {
            Some(c) => {
                unsafe { *(ptr as *mut *mut u8) = h.small.free[c] };
                h.small.free[c] = ptr;
            }
            None => unsafe { h.large.deallocate(core::ptr::NonNull::new_unchecked(ptr), layout) },
        })
    }
}
