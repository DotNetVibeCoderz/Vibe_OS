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
const BASE_HEAP: u64 = 32 * 1024 * 1024;
/// The base heap was sized on screens up to this (bytes of 1280x800x4).
const BASE_SCREEN: u64 = 1280 * 800 * 4;
/// Full-screen buffers the desktop keeps per screen: back buffer, compose
/// buffer, the wallpaper at screen size, ... -- on a 2560x1440 monitor the
/// base heap ran out right after the first frame.
const SCREEN_BUFFERS: u64 = 6;

/// Bytes of kernel heap: the base, more on a larger screen, at most half of RAM
/// (and never less than the base: the kernel does not run on less).
pub fn heap_size() -> u64 {
    let screen = crate::boot::info().framebuffer.map_or(0, |f| (f.width * f.height * 4) as u64);
    let wanted = BASE_HEAP + SCREEN_BUFFERS * screen.saturating_sub(BASE_SCREEN);
    let half_ram = memory::total_usable_mib() * 1024 * 1024 / 2;
    wanted.min(half_ram).max(BASE_HEAP) & !0xFFF
}

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Map the heap's pages (kernel-only, read/write, not executable) through the
/// paging context and hand the range to the allocator.
pub fn init() -> Result<(), &'static str> {
    const KERNEL_DATA: Flags = Flags { write: true, user: false, exec: false };
    let size = heap_size();
    memory::with_ctx(|ctx| {
        paging::map_fresh_range(&mut ctx.mapper, &mut ctx.frames, HEAP_START, size / 4096, KERNEL_DATA)
            .map_err(|_| "out of frames for the kernel heap")
    })?;

    unsafe {
        ALLOCATOR
            .lock()
            .init(HEAP_START as *mut u8, size as usize);
    }
    Ok(())
}

/// (bytes used, bytes total) of the kernel heap, for SYS_STAT / Task Manager.
pub fn stats() -> (usize, usize) {
    let heap = ALLOCATOR.lock();
    (heap.used(), heap.size())
}
