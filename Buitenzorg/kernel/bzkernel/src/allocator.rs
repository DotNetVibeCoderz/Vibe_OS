//! Kernel heap (linked-list allocator) so `alloc` types work in ring 0.

use linked_list_allocator::LockedHeap;

use crate::arch::paging::{self, Flags};
use crate::memory;

/// Per architecture (`arch::paging::KERNEL_HEAP_BASE`): on ARM64 the lower
/// half belongs to processes, so the heap lives in the upper half.
pub const HEAP_START: u64 = paging::KERNEL_HEAP_BASE;
// 32 MiB kernel heap. 16 MiB was exhausted once the desktop is up: the RAM
// disks, ~20 open app-window canvases from the boot demos, the desktop back
// buffer and the persistent full-screen compose buffer (wm::PRESENT_BUF) all
// live here and together crossed 16 MiB.
pub const HEAP_SIZE: u64 = 32 * 1024 * 1024;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Map the heap's pages (kernel-only, read/write, not executable) through the
/// paging context and hand the range to the allocator.
pub fn init() -> Result<(), &'static str> {
    const KERNEL_DATA: Flags = Flags { write: true, user: false, exec: false };
    memory::with_ctx(|ctx| {
        paging::map_fresh_range(&mut ctx.mapper, &mut ctx.frames, HEAP_START, HEAP_SIZE / 4096, KERNEL_DATA)
            .map_err(|_| "out of frames for the kernel heap")
    })?;

    unsafe {
        ALLOCATOR
            .lock()
            .init(HEAP_START as *mut u8, HEAP_SIZE as usize);
    }
    Ok(())
}

/// (bytes used, bytes total) of the kernel heap, for SYS_STAT / Task Manager.
pub fn stats() -> (usize, usize) {
    let heap = ALLOCATOR.lock();
    (heap.used(), heap.size())
}
