//! The managed heap of native (bflat / ILC + zerolib) C# programs: a
//! non-moving **mark-sweep garbage collector**. Every managed object comes
//! from here (`RhpNewFast` / `RhpNewArray`; zerolib's own versions are renamed
//! by tools/ZerolibPatch). The libc-style `malloc` family stays a separate,
//! never-collected bump heap (bzstart.rs).
//!
//! Layout: one reserved region (`MMAP` with no access, pages committed with
//! `MPROTECT` when a block is first used) cut into 64 KiB blocks. A small
//! block holds slots of one size class (16 B .. 8 KiB) and keeps two bitmaps
//! at its start (allocated, marked); a large object takes a run of whole
//! blocks. A block table (also mmap'd, so the collector never scans its own
//! bookkeeping) says what each block is.
//!
//! Collection:
//! * **roots, conservatively:** the program's data + bss (`__bz_data_start ..
//!   __bz_bss_end`, which holds the GC static bases) and the stack of the
//!   collecting thread from its stack pointer to the top, with its
//!   callee-saved registers spilled onto it first. Any word that points into
//!   an allocated slot (interior pointers included) keeps that object alive.
//! * **objects:** a class instance is scanned word by word; an array only if
//!   its MethodTable says it holds references (a `byte[]` / `int[]` is never
//!   scanned, so a bitmap cannot pin random objects).
//! * **sweep:** unmarked slots become free (zeroed again when handed out),
//!   empty blocks go back to the free pool.
//!
//! Limit, deliberately: a collection runs only while the program has no
//! other thread (threads made with `bz_thread_create`; their count is kept
//! here). Other threads could hold references in registers the collector
//! cannot see; with threads alive the heap simply grows, as before.

use crate::{bz_mmap, bz_mprotect, syscall2, SYS_CLOCK_NANOS};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const BLOCK_SHIFT: usize = 16;
const BLOCK: usize = 1 << BLOCK_SHIFT;
/// Objects above this size take whole blocks.
const SMALL_MAX: usize = 8192;
const CLASSES: [u32; 33] = [
    16, 32, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768, 896, 1024, 1280, 1536, 1792, 2048, 2560,
    3072, 3584, 4096, 5120, 6144, 7168, 8192, 0,
];
const NCLASS: usize = 32;
/// Block kinds in the block table.
const FREE: u8 = 0;
const SMALL: u8 = 1;
const LARGE: u8 = 2;
const LARGE_TAIL: u8 = 3;
/// The block table entry: kind, size class, committed, large mark; a large
/// head keeps its run length, a tail the index of its head.
#[derive(Clone, Copy)]
#[repr(C)]
struct Meta {
    kind: u8,
    class: u8,
    committed: u8,
    marked: u8,
    n: u32,
}
/// The header at the start of every small block (the bitmaps follow).
#[repr(C)]
struct Small {
    slot: u32,
    nslots: u32,
    free: u32,
    cursor: u32,
    next: u32, // next block of this class with free slots, or NONE
    listed: u32,
    data: u32, // offset of the first slot
    words: u32,
}
const NONE: u32 = u32::MAX;
/// MethodTable flag (high half of the first word): the low half is a
/// component size (arrays, strings).
pub const MT_HAS_COMPONENT_SIZE: u16 = 0x8000;
const HDR: usize = core::mem::size_of::<Small>();

struct Heap {
    base: usize,
    blocks: usize,
    meta: *mut Meta,
    hint: usize,
    cur: [u32; NCLASS],
    list: [u32; NCLASS],
    mark_stack: *mut usize,
    mark_cap: usize,
    mark_len: usize,
    overflow: bool,
    // statistics
    live: usize,
    since: usize,
    threshold: usize,
    total: usize,
    count: usize,
    blocks_used: usize,
    committed: usize,
    collections: usize,
    freed: usize,
    skipped: usize,
    last_us: usize,
}

static mut H: Heap = Heap {
    base: 0,
    blocks: 0,
    meta: core::ptr::null_mut(),
    hint: 0,
    cur: [NONE; NCLASS],
    list: [NONE; NCLASS],
    mark_stack: core::ptr::null_mut(),
    mark_cap: 0,
    mark_len: 0,
    overflow: false,
    live: 0,
    since: 0,
    threshold: 8 << 20,
    total: 0,
    count: 0,
    blocks_used: 0,
    committed: 0,
    collections: 0,
    freed: 0,
    skipped: 0,
    last_us: 0,
};

static LOCK: AtomicBool = AtomicBool::new(false);
/// Threads started with `bz_thread_create` that have not finished.
pub static THREADS: AtomicUsize = AtomicUsize::new(0);
/// The main thread's stack top (recorded by `_start`).
#[no_mangle]
pub static mut BZ_STACK_TOP: usize = 0;
/// 1 = never collect (an escape hatch for a program that hides references).
static DISABLED: AtomicBool = AtomicBool::new(false);

fn lock() {
    let mut spins = 0u32;
    while LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        spins += 1;
        if spins > 64 {
            unsafe { syscall2(crate::SYS_YIELD, 0, 0) };
            spins = 0;
        } else {
            core::hint::spin_loop();
        }
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
}

#[allow(static_mut_refs)]
unsafe fn heap() -> &'static mut Heap {
    &mut *core::ptr::addr_of_mut!(H)
}

unsafe fn now_us() -> usize {
    (syscall2(SYS_CLOCK_NANOS, 0, 0) / 1000) as usize
}

fn ok(r: u64) -> bool {
    r != 0 && r < u64::MAX - 4095
}

/// Reserve the region and the block table (first allocation).
unsafe fn init(h: &mut Heap) -> bool {
    if h.base != 0 {
        return true;
    }
    for mb in [512usize, 256, 128, 64] {
        let r = bz_mmap((mb << 20) as u64, 0);
        if ok(r) {
            // align the start to a block
            let start = (r as usize + BLOCK - 1) & !(BLOCK - 1);
            let blocks = ((r as usize + (mb << 20)) - start) >> BLOCK_SHIFT;
            let mbytes = (blocks * core::mem::size_of::<Meta>() + 4095) & !4095;
            let m = bz_mmap(mbytes as u64, 3);
            let ms = bz_mmap(1 << 20, 3);
            if !ok(m) || !ok(ms) {
                return false;
            }
            h.base = start;
            h.blocks = blocks;
            h.meta = m as *mut Meta;
            h.mark_stack = ms as *mut usize;
            h.mark_cap = (1 << 20) / 8;
            return true;
        }
    }
    false
}

unsafe fn meta(h: &Heap, i: usize) -> &'static mut Meta {
    &mut *h.meta.add(i)
}

unsafe fn block_addr(h: &Heap, i: usize) -> usize {
    h.base + (i << BLOCK_SHIFT)
}

/// `n` free consecutive blocks, committed and zeroed; returns the first index.
unsafe fn take_blocks(h: &mut Heap, n: usize) -> Option<usize> {
    let mut start = h.hint;
    for pass in 0..2 {
        let mut i = start;
        let mut run = 0;
        while i < h.blocks {
            if meta(h, i).kind == FREE {
                run += 1;
                if run == n {
                    let first = i + 1 - n;
                    for b in first..=i {
                        let m = meta(h, b);
                        if m.committed == 0 {
                            // commit on first use (returns 0, or an error code)
                            if bz_mprotect(block_addr(h, b) as u64, BLOCK as u64, 3) >= u64::MAX - 4095 {
                                return None;
                            }
                            m.committed = 1;
                            h.committed += BLOCK;
                        } else {
                            // a recycled block: clear it
                            core::ptr::write_bytes(block_addr(h, b) as *mut u8, 0, BLOCK);
                        }
                    }
                    h.hint = i + 1;
                    h.blocks_used += n;
                    return Some(first);
                }
            } else {
                run = 0;
            }
            i += 1;
        }
        if pass == 0 {
            start = 0;
        }
    }
    None
}

unsafe fn small_hdr(h: &Heap, i: usize) -> *mut Small {
    block_addr(h, i) as *mut Small
}

unsafe fn bitmap(s: *mut Small, which: usize) -> *mut u64 {
    (s as *mut u8).add(HDR + which * (*s).words as usize * 8) as *mut u64
}

unsafe fn new_small_block(h: &mut Heap, class: usize) -> Option<u32> {
    let i = take_blocks(h, 1)?;
    let m = meta(h, i);
    m.kind = SMALL;
    m.class = class as u8;
    let slot = (*CLASSES.get_unchecked(class)) as usize;
    // bitmaps sized for the most slots that could fit
    let words = (BLOCK / slot.max(1)).div_ceil(64);
    let data = (HDR + 2 * words * 8 + 15) & !15;
    let nslots = (BLOCK - data) / slot.max(1);
    let s = small_hdr(h, i);
    *s = Small { slot: slot as u32, nslots: nslots as u32, free: nslots as u32, cursor: 0, next: NONE, listed: 0, data: data as u32, words: words as u32 };
    Some(i as u32)
}

unsafe fn class_of(size: usize) -> usize {
    let mut c = 0;
    while ((*CLASSES.get_unchecked(c)) as usize) < size {
        c += 1;
    }
    c
}

/// A free slot of `class`, or None when every block of it is full.
unsafe fn alloc_small(h: &mut Heap, class: usize) -> Option<usize> {
    loop {
        let mut b = (*h.cur.get_unchecked_mut(class));
        if b == NONE || (*small_hdr(h, b as usize)).free == 0 {
            // the next block with free slots
            b = (*h.list.get_unchecked_mut(class));
            if b == NONE {
                return None;
            }
            let s = small_hdr(h, b as usize);
            (*h.list.get_unchecked_mut(class)) = (*s).next;
            (*s).listed = 0;
            (*s).cursor = 0;
            (*h.cur.get_unchecked_mut(class)) = b;
            continue;
        }
        let s = small_hdr(h, b as usize);
        let am = bitmap(s, 0);
        let words = (*s).words as usize;
        let n = (*s).nslots as usize;
        let mut w = (*s).cursor as usize / 64;
        while w < words {
            let v = *am.add(w);
            if v != u64::MAX {
                let bit = (!v).trailing_zeros() as usize;
                let idx = w * 64 + bit;
                if idx >= n {
                    break;
                }
                *am.add(w) = v | (1 << bit);
                (*s).free -= 1;
                (*s).cursor = idx as u32;
                let addr = block_addr(h, b as usize) + (*s).data as usize + idx * (*s).slot as usize;
                core::ptr::write_bytes(addr as *mut u8, 0, (*s).slot as usize);
                return Some(addr);
            }
            w += 1;
        }
        // the cursor said otherwise: the block is full after all
        (*s).free = 0;
    }
}

unsafe fn alloc_large(h: &mut Heap, size: usize) -> Option<usize> {
    let n = size.div_ceil(BLOCK);
    let i = take_blocks(h, n)?;
    let m = meta(h, i);
    m.kind = LARGE;
    m.n = n as u32;
    m.marked = 0;
    for t in 1..n {
        let mt = meta(h, i + t);
        mt.kind = LARGE_TAIL;
        mt.n = i as u32;
    }
    Some(block_addr(h, i))
}

unsafe fn try_alloc(h: &mut Heap, size: usize) -> Option<usize> {
    if size <= SMALL_MAX {
        let c = class_of(size);
        if let Some(a) = alloc_small(h, c) {
            return Some(a);
        }
        let b = new_small_block(h, c)?;
        (*h.cur.get_unchecked_mut(c)) = b;
        alloc_small(h, c)
    } else {
        alloc_large(h, size)
    }
}

/// Allocate `size` zeroed bytes for an object (collecting first when enough
/// was allocated since the last collection).
pub unsafe fn alloc(size: usize) -> *mut u8 {
    let size = (size.max(16) + 15) & !15;
    lock();
    let h = heap();
    if !init(h) {
        unlock();
        return core::ptr::null_mut();
    }
    if h.since >= h.threshold {
        collect_locked(h, false);
    }
    let mut r = try_alloc(h, size);
    if r.is_none() {
        // out of blocks: collect now, even below the threshold
        collect_locked(h, true);
        r = try_alloc(h, size);
    }
    let p = match r {
        Some(a) => {
            h.since += size;
            h.live += size;
            h.total += size;
            h.count += 1;
            a as *mut u8
        }
        None => core::ptr::null_mut(),
    };
    unlock();
    p
}

// ---- marking ------------------------------------------------------------------

/// The object `addr` points into (start of an allocated slot), if any.
unsafe fn object_at(h: &Heap, addr: usize) -> Option<usize> {
    if addr < h.base || addr >= h.base + (h.blocks << BLOCK_SHIFT) {
        return None;
    }
    let mut i = (addr - h.base) >> BLOCK_SHIFT;
    let m = *meta(h, i);
    match m.kind {
        SMALL => {
            let s = small_hdr(h, i);
            let start = block_addr(h, i) + (*s).data as usize;
            if addr < start {
                return None;
            }
            let idx = (addr - start) / ((*s).slot as usize).max(1);
            if idx >= (*s).nslots as usize {
                return None;
            }
            let am = bitmap(s, 0);
            if *am.add(idx / 64) & (1 << (idx % 64)) == 0 {
                return None;
            }
            Some(start + idx * (*s).slot as usize)
        }
        LARGE | LARGE_TAIL => {
            if m.kind == LARGE_TAIL {
                i = m.n as usize;
            }
            Some(block_addr(h, i))
        }
        _ => None,
    }
}

/// Mark `obj` (an object start); true if it was not marked before.
unsafe fn set_mark(h: &Heap, obj: usize) -> bool {
    let i = (obj - h.base) >> BLOCK_SHIFT;
    let m = meta(h, i);
    if m.kind == LARGE {
        let was = m.marked != 0;
        m.marked = 1;
        return !was;
    }
    let s = small_hdr(h, i);
    let idx = (obj - (block_addr(h, i) + (*s).data as usize)) / ((*s).slot as usize).max(1);
    let mm = bitmap(s, 1);
    let bit = 1u64 << (idx % 64);
    let w = mm.add(idx / 64);
    let was = *w & bit != 0;
    *w |= bit;
    !was
}

unsafe fn consider(h: &mut Heap, word: usize) {
    if let Some(obj) = object_at(h, word) {
        if set_mark(h, obj) {
            if h.mark_len < h.mark_cap {
                *h.mark_stack.add(h.mark_len) = obj;
                h.mark_len += 1;
            } else {
                h.overflow = true;
            }
        }
    }
}

unsafe fn scan_range(h: &mut Heap, from: usize, to: usize) {
    let mut p = (from + 7) & !7;
    while p + 8 <= to {
        consider(h, *(p as *const usize));
        p += 8;
    }
}

/// The slot / block size an object occupies (its scan limit).
unsafe fn extent(h: &Heap, obj: usize) -> usize {
    let i = (obj - h.base) >> BLOCK_SHIFT;
    let m = *meta(h, i);
    if m.kind == LARGE {
        (m.n as usize) << BLOCK_SHIFT
    } else {
        (*small_hdr(h, i)).slot as usize
    }
}

unsafe fn scan_object(h: &mut Heap, obj: usize) {
    let mt = *(obj as *const *const crate::MethodTable);
    if mt.is_null() {
        return; // allocated, its MethodTable not written yet
    }
    // the low 16 bits are the component size only for arrays and strings
    let comp = if (*mt).flags & MT_HAS_COMPONENT_SIZE != 0 { (*mt).component_size as usize } else { 0 };
    let room = extent(h, obj);
    let size = if comp != 0 {
        if (*mt).flags & crate::MT_HAS_POINTERS == 0 {
            return; // an array of plain values
        }
        let len = *((obj + 8) as *const u32) as usize;
        ((*mt).base_size as usize).saturating_add(comp.saturating_mul(len))
    } else {
        (*mt).base_size as usize
    };
    // never read past what the allocator gave the object
    let end = obj + size.min(room);
    scan_range(h, obj + 8, end);
}

unsafe fn drain(h: &mut Heap) {
    while h.mark_len > 0 {
        h.mark_len -= 1;
        let obj = *h.mark_stack.add(h.mark_len);
        scan_object(h, obj);
    }
}

/// After a mark-stack overflow: scan every marked object again until nothing
/// new gets marked.
unsafe fn rescan_marked(h: &mut Heap) {
    while h.overflow {
        h.overflow = false;
        for i in 0..h.blocks {
            let m = *meta(h, i);
            match m.kind {
                SMALL => {
                    let s = small_hdr(h, i);
                    let mm = bitmap(s, 1);
                    for idx in 0..(*s).nslots as usize {
                        if *mm.add(idx / 64) & (1 << (idx % 64)) != 0 {
                            scan_object(h, block_addr(h, i) + (*s).data as usize + idx * (*s).slot as usize);
                            drain(h);
                        }
                    }
                }
                LARGE if m.marked != 0 => {
                    scan_object(h, block_addr(h, i));
                    drain(h);
                }
                _ => {}
            }
        }
    }
}

extern "C" {
    static __bz_data_start: u8;
    static __bz_bss_end: u8;
}

/// Free every unmarked object; clear the marks. Returns bytes freed.
unsafe fn sweep(h: &mut Heap) -> usize {
    let mut freed = 0;
    let mut live = 0;
    h.cur = [NONE; NCLASS];
    h.list = [NONE; NCLASS];
    let mut i = 0;
    while i < h.blocks {
        let m = *meta(h, i);
        match m.kind {
            SMALL => {
                let s = small_hdr(h, i);
                let (am, mm) = (bitmap(s, 0), bitmap(s, 1));
                let mut used = 0usize;
                for w in 0..(*s).words as usize {
                    let a = *am.add(w);
                    let k = *mm.add(w);
                    let keep = a & k;
                    freed += (a & !k).count_ones() as usize * (*s).slot as usize;
                    used += keep.count_ones() as usize;
                    *am.add(w) = keep;
                    *mm.add(w) = 0;
                }
                live += used * (*s).slot as usize;
                (*s).free = (*s).nslots - used as u32;
                (*s).cursor = 0;
                (*s).listed = 0;
                if used == 0 {
                    meta(h, i).kind = FREE;
                    h.blocks_used -= 1;
                } else if (*s).free > 0 {
                    let c = m.class as usize;
                    (*s).next = (*h.list.get_unchecked_mut(c));
                    (*s).listed = 1;
                    (*h.list.get_unchecked_mut(c)) = i as u32;
                }
                i += 1;
            }
            LARGE => {
                let n = m.n as usize;
                if m.marked == 0 {
                    for b in i..i + n {
                        meta(h, b).kind = FREE;
                    }
                    h.blocks_used -= n;
                    freed += n << BLOCK_SHIFT;
                } else {
                    meta(h, i).marked = 0;
                    live += n << BLOCK_SHIFT;
                }
                i += n;
            }
            _ => i += 1,
        }
    }
    h.live = live;
    h.hint = 0;
    freed
}

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    ".global bz_gc_spill_and_mark",
    "bz_gc_spill_and_mark:",
    // rdi = heap; spill callee-saved registers, pass the stack pointer
    "    push rbx",
    "    push rbp",
    "    push r12",
    "    push r13",
    "    push r14",
    "    push r15",
    "    mov rsi, rsp",
    "    sub rsp, 8",
    "    call {mark}",
    "    add rsp, 8",
    "    pop r15",
    "    pop r14",
    "    pop r13",
    "    pop r12",
    "    pop rbp",
    "    pop rbx",
    "    ret",
    mark = sym mark_roots,
);

#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    ".global bz_gc_spill_and_mark",
    "bz_gc_spill_and_mark:",
    "    stp x29, x30, [sp, #-96]!",
    "    stp x19, x20, [sp, #16]",
    "    stp x21, x22, [sp, #32]",
    "    stp x23, x24, [sp, #48]",
    "    stp x25, x26, [sp, #64]",
    "    stp x27, x28, [sp, #80]",
    "    mov x1, sp",
    "    bl {mark}",
    "    ldp x27, x28, [sp, #80]",
    "    ldp x25, x26, [sp, #64]",
    "    ldp x23, x24, [sp, #48]",
    "    ldp x21, x22, [sp, #32]",
    "    ldp x19, x20, [sp, #16]",
    "    ldp x29, x30, [sp], #96",
    "    ret",
    mark = sym mark_roots,
);

extern "C" {
    fn bz_gc_spill_and_mark(h: *mut Heap);
}

extern "C" fn mark_roots(h: *mut Heap, sp: usize) {
    unsafe {
        let h = &mut *h;
        h.mark_len = 0;
        h.overflow = false;
        let (ds, de) = (core::ptr::addr_of!(__bz_data_start) as usize, core::ptr::addr_of!(__bz_bss_end) as usize);
        scan_range(h, ds, de);
        drain(h);
        let top = BZ_STACK_TOP;
        if top > sp {
            scan_range(h, sp, top);
        }
        drain(h);
        rescan_marked(h);
    }
}

/// Collect (heap lock held). `force`: also below the threshold.
unsafe fn collect_locked(h: &mut Heap, force: bool) -> bool {
    let _ = force;
    if h.base == 0 || DISABLED.load(Ordering::Relaxed) || BZ_STACK_TOP == 0 {
        return false;
    }
    if THREADS.load(Ordering::Acquire) != 0 {
        // other threads may hold references in registers: grow instead
        h.skipped += 1;
        h.since = 0;
        return false;
    }
    let t0 = now_us();
    bz_gc_spill_and_mark(h);
    let freed = sweep(h);
    h.freed += freed;
    h.collections += 1;
    h.since = 0;
    // collect again after allocating as much as survived (at least 8 MiB)
    h.threshold = h.live.max(8 << 20);
    h.last_us = now_us().saturating_sub(t0);
    true
}

/// Run a collection now; 1 if it ran, 0 if it was skipped (threads alive).
#[no_mangle]
pub unsafe extern "C" fn bz_gc_collect() -> u64 {
    lock();
    let h = heap();
    let r = collect_locked(h, true);
    unlock();
    r as u64
}

/// 1 = stop collecting (the heap only grows), 0 = collect again.
#[no_mangle]
pub unsafe extern "C" fn bz_gc_disable(on: u64) {
    DISABLED.store(on != 0, Ordering::Relaxed);
}

/// Collector statistics, 8 x u64: collections, live bytes, bytes freed in
/// total, last pause (us), blocks in use, collections skipped (threads),
/// committed bytes, next collection after this many bytes.
#[no_mangle]
pub unsafe extern "C" fn bz_gc_stats(out: *mut u64) {
    if out.is_null() {
        return;
    }
    lock();
    let h = heap();
    let v = [h.collections, h.live, h.freed, h.last_us, h.blocks_used, h.skipped, h.committed, h.threshold];
    unlock();
    for (i, x) in v.iter().enumerate() {
        out.add(i).write_volatile(*x as u64);
    }
}

/// (allocated in total, committed, blocks in use, allocation count, free
/// committed bytes) -- the figures `bz_heap_stats` reports.
pub unsafe fn summary() -> [usize; 5] {
    lock();
    let h = heap();
    let free = h.committed.saturating_sub(h.live);
    let v = [h.total, h.committed, h.blocks_used, h.count, free];
    unlock();
    v
}
