//! Freestanding Buitenzorg startup + PAL shim for bflat/zerolib C# programs
//! (v0.4 "Tunas"). Compiled to an object for `x86_64-unknown-none` and linked
//! with the bflat `-c` output to produce a static ELF that runs in ring 3.
//!
//! It replaces glibc + libSystem.Native with three raw-syscall functions and
//! an `_start` that calls the NativeAOT managed entry. The ring-3 syscall ABI
//! matches the kernel dispatcher (kernel/bzkernel/src/syscall.rs):
//!   rax = syscall number, rdi/rsi/rdx = args, result in rax.
#![no_std]
#![no_main]
#![allow(internal_features)]

use core::panic::PanicInfo;

// Buitenzorg syscall numbers (mirror of bz_abi::sysno).
const SYS_DEBUG_WRITE: u64 = 1;
const SYS_EXIT: u64 = 2;
const SYS_TICKS: u64 = 4;
const SYS_WIN_CREATE: u64 = 6;
const SYS_WIN_CMD: u64 = 7;
const SYS_WIN_PRESENT: u64 = 8;
const SYS_KEY_READ: u64 = 9;
const SYS_PROC_LIST: u64 = 10;
const SYS_PROC_KILL: u64 = 11;
const SYS_SYS_STAT: u64 = 12;
const SYS_YIELD: u64 = 3;
const SYS_MMAP: u64 = 13;
const SYS_MPROTECT: u64 = 14;
const SYS_MUNMAP: u64 = 15;
const SYS_THREAD_CREATE: u64 = 16;
const SYS_THREAD_JOIN: u64 = 17;
const SYS_THREAD_EXIT: u64 = 18;
const SYS_FUTEX_WAIT: u64 = 19;
const SYS_FUTEX_WAKE: u64 = 20;
const SYS_THREAD_SELF: u64 = 21;
const SYS_CLOCK_MONO: u64 = 22;
const SYS_AUDIO_STAT: u64 = 23;
const SYS_AUDIO_SET_VOLUME: u64 = 24;
const SYS_AUDIO_TONE: u64 = 25;
const SYS_AUDIO_PLAY: u64 = 26;
const SYS_PKG_LIST: u64 = 27;
const SYS_PKG_SET: u64 = 28;
const SYS_FS_LIST: u64 = 29;
const SYS_FS_READ: u64 = 30;
const SYS_IS_INTERACTIVE: u64 = 31;
const SYS_FS_WRITE: u64 = 32;
const SYS_CLOCK_RTC: u64 = 33;
const SYS_NET_SOCKET: u64 = 34;
const SYS_NET_BIND: u64 = 35;
const SYS_NET_SEND: u64 = 36;
const SYS_NET_RECV: u64 = 37;
const SYS_NET_CLOSE: u64 = 38;
const SYS_NET_INFO: u64 = 39;

#[inline(always)]
unsafe fn syscall3(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let ret: u64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") nr => ret,
        // The kernel entry hands the args to a C dispatcher and restores ONLY
        // rcx/r11/rsp before sysretq — every other caller-saved register comes
        // back holding whatever the kernel left in it. So the argument
        // registers rdi/rsi/rdx MUST be declared clobbered (inlateout => _),
        // not plain `in` (which promises the asm preserves them). With plain
        // `in`, LLVM kept `chunk` in rdi across the mmap syscall in
        // SystemNative_Malloc and stored the kernel's garbage into HEAP_CAP —
        // the grow check then never fired and the next big allocation ran off
        // the mapped chunk (USER page fault; surfaced by the first app to need
        // a second heap growth). Same bug class as the r8/r9/r10 note below.
        inlateout("rdi") a0 => _,
        inlateout("rsi") a1 => _,
        inlateout("rdx") a2 => _,
        lateout("rcx") _, // clobbered by the syscall instruction (return rip)
        lateout("r11") _, // clobbered by the syscall instruction (rflags)
        // The kernel entry marshals args through r8/r9/r10 and the C dispatcher
        // clobbers all caller-saved registers, so declare them clobbered too —
        // otherwise the compiler may keep a live value in r8/r9/r10 across a
        // syscall and get a garbage value back (this bit the thread arg).
        lateout("r8") _,
        lateout("r9") _,
        lateout("r10") _,
        options(nostack),
    );
    ret
}

#[inline(always)]
unsafe fn syscall2(nr: u64, a0: u64, a1: u64) -> u64 {
    syscall3(nr, a0, a1, 0)
}

// NativeAOT managed entry emitted by bflat: `int __managed__Main(int, char**)`.
extern "C" {
    fn __managed__Main(argc: i32, argv: *const *const u8) -> i32;
}

// Freestanding C runtime intrinsics the compiler emits for struct/buffer
// zeroing and copies. Needed once apps use larger stackalloc/structs.
#[no_mangle]
pub unsafe extern "C" fn memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
    let byte = c as u8;
    let mut i = 0;
    while i < n {
        *dest.add(i) = byte;
        i += 1;
    }
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        *dest.add(i) = *src.add(i);
        i += 1;
    }
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if (dest as usize) < (src as usize) {
        memcpy(dest, src, n)
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            *dest.add(i) = *src.add(i);
        }
        dest
    }
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    let mut i = 0;
    while i < n {
        let (x, y) = (*a.add(i), *b.add(i));
        if x != y {
            return x as i32 - y as i32;
        }
        i += 1;
    }
    0
}

/// zerolib Console.Write → SystemNative_Log(buffer, length).
#[no_mangle]
pub unsafe extern "C" fn SystemNative_Log(buffer: *const u8, length: i32) {
    if !buffer.is_null() && length > 0 {
        syscall2(SYS_DEBUG_WRITE, buffer as u64, length as u64);
    }
}

/// Write raw bytes to the kernel console (used by Buitenzorg.Bcl for dynamic
/// text output, since zerolib can't build new strings).
#[no_mangle]
pub unsafe extern "C" fn bz_write(buffer: *const u8, length: u64) {
    if !buffer.is_null() && length > 0 {
        syscall2(SYS_DEBUG_WRITE, buffer as u64, length);
    }
}

/// Growable bump heap (v0.15 "Matang" increment 4). zerolib's `new` /
/// `RhpNewArray` route through `SystemNative_Malloc` (verified by disasm), so a
/// sound, zeroed, growable allocator makes heap objects, arrays, and generic
/// instances work in ring-3 C#. Chunks come from `mmap` (>= 1 MiB each); the
/// first allocation maps the first chunk. Never frees (no GC yet), so every
/// byte handed out is pristine zero: `mmap` zeroes its frames.
// The bump-heap cursor is three words: base, offset, capacity. The first
// allocation maps the first chunk on demand (no .bss arena — one less moving
// part, and mmap frames come back already zeroed). They are `AtomicUsize`
// (Relaxed), which is correct for the cooperative ring-3 threads that share
// this heap.
//
// History: the heap used to fault on the SECOND growth of a process (the first
// apps to need >1 MiB of managed heap — imgview, a multi-array test — hit it).
// The root cause was NOT here but in `syscall3`: rdi/rsi/rdx were declared
// `in` instead of clobbered, and LLVM kept `chunk` in rdi across the mmap
// syscall — see the comment in `syscall3` before suspecting this code again.
use core::sync::atomic::{AtomicUsize, Ordering as HeapOrd};
static HEAP_BASE: AtomicUsize = AtomicUsize::new(0); // 0 => uninitialized
static HEAP_OFF: AtomicUsize = AtomicUsize::new(0);
static HEAP_CAP: AtomicUsize = AtomicUsize::new(0);
// Accounting for Buitenzorg.Bcl's `BzGC` (there is no reclaiming collector yet,
// so these are the honest numbers: bytes handed out, bytes mapped, chunk count).
static HEAP_ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static HEAP_COMMITTED: AtomicUsize = AtomicUsize::new(0);
static HEAP_CHUNKS: AtomicUsize = AtomicUsize::new(0);
static HEAP_ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Report managed-heap statistics into `out` (5 x u64):
/// [allocated, committed, chunks, allocation count, current chunk free bytes].
#[no_mangle]
pub unsafe extern "C" fn bz_heap_stats(out: *mut u64) {
    if out.is_null() {
        return;
    }
    let cap = HEAP_CAP.load(HeapOrd::Relaxed);
    let off = HEAP_OFF.load(HeapOrd::Relaxed);
    out.write_volatile(HEAP_ALLOCATED.load(HeapOrd::Relaxed) as u64);
    out.add(1).write_volatile(HEAP_COMMITTED.load(HeapOrd::Relaxed) as u64);
    out.add(2).write_volatile(HEAP_CHUNKS.load(HeapOrd::Relaxed) as u64);
    out.add(3).write_volatile(HEAP_ALLOC_COUNT.load(HeapOrd::Relaxed) as u64);
    out.add(4).write_volatile(cap.saturating_sub(off) as u64);
}

#[no_mangle]
pub unsafe extern "C" fn SystemNative_Malloc(size: usize) -> *mut u8 {
    let align = 16;
    let size = if size == 0 { 1 } else { size };
    let mut off = (HEAP_OFF.load(HeapOrd::Relaxed) + align - 1) & !(align - 1);
    // Grow when uninitialized (BASE == 0) or the request won't fit the chunk.
    if HEAP_BASE.load(HeapOrd::Relaxed) == 0 || off + size > HEAP_CAP.load(HeapOrd::Relaxed) {
        // Map a fresh chunk (>= 1 MiB) via the memory PAL.
        let want = size + align;
        let chunk = if want > 1024 * 1024 { (want + 4095) & !4095 } else { 1024 * 1024 };
        let base = bz_mmap(chunk as u64, 1 | 2); // READ | WRITE, zeroed
        if base >= u64::MAX - 1 || base == 0 {
            return core::ptr::null_mut();
        }
        HEAP_BASE.store(base as usize, HeapOrd::Relaxed);
        HEAP_CAP.store(chunk, HeapOrd::Relaxed);
        HEAP_OFF.store(0, HeapOrd::Relaxed);
        HEAP_COMMITTED.fetch_add(chunk, HeapOrd::Relaxed);
        HEAP_CHUNKS.fetch_add(1, HeapOrd::Relaxed);
        off = 0;
    }
    HEAP_ALLOCATED.fetch_add(size, HeapOrd::Relaxed);
    HEAP_ALLOC_COUNT.fetch_add(1, HeapOrd::Relaxed);
    HEAP_OFF.store(off + size, HeapOrd::Relaxed);
    (HEAP_BASE.load(HeapOrd::Relaxed) + off) as *mut u8
}

// libc-style allocator (for the future --stdlib:dotnet CoreLib PAL). Uses a
// 16-byte size header so free/realloc know the size. `free` is a no-op until a
// real GC/heap lands; `calloc` returns zeroed memory (the arena is pristine).
#[no_mangle]
pub unsafe extern "C" fn malloc(size: usize) -> *mut u8 {
    let p = SystemNative_Malloc(size + 16);
    if p.is_null() {
        return p;
    }
    *(p as *mut usize) = size;
    p.add(16)
}

#[no_mangle]
pub unsafe extern "C" fn free(_p: *mut u8) {
    // No reclaim yet (bump allocator).
}

#[no_mangle]
pub unsafe extern "C" fn calloc(n: usize, sz: usize) -> *mut u8 {
    malloc(n.wrapping_mul(sz)) // arena is already zeroed
}

#[no_mangle]
pub unsafe extern "C" fn realloc(p: *mut u8, newsize: usize) -> *mut u8 {
    if p.is_null() {
        return malloc(newsize);
    }
    let oldsize = *(p.sub(16) as *const usize);
    let np = malloc(newsize);
    if !np.is_null() {
        let n = if oldsize < newsize { oldsize } else { newsize };
        core::ptr::copy_nonoverlapping(p, np, n);
    }
    np
}

#[no_mangle]
pub unsafe extern "C" fn SystemNative_Abort() -> ! {
    syscall2(SYS_EXIT, 134, 0);
    loop {}
}

// --- Buitenzorg UI/app ABI (v0.8), called from C# apps via DllImport --------

/// Create an app window; returns its id. `dims = (w << 32) | h`.
#[no_mangle]
pub unsafe extern "C" fn bz_win_create(title_ptr: *const u8, title_len: u64, dims: u64) -> u32 {
    syscall3(SYS_WIN_CREATE, title_ptr as u64, title_len, dims) as u32
}

/// Submit a draw command (`cmd` points to a DrawCmd) to a window.
#[no_mangle]
pub unsafe extern "C" fn bz_win_cmd(window: u32, cmd: *const u8) -> u64 {
    syscall2(SYS_WIN_CMD, window as u64, cmd as u64)
}

/// Recompose the desktop so the window's canvas is shown.
#[no_mangle]
pub unsafe extern "C" fn bz_win_present(window: u32) {
    syscall2(SYS_WIN_PRESENT, window as u64, 0);
}

/// Read one keyboard character (0 when none).
#[no_mangle]
pub unsafe extern "C" fn bz_key_read() -> u32 {
    syscall2(SYS_KEY_READ, 0, 0) as u32
}

/// Monotonic timer ticks since boot (for simple app timing).
#[no_mangle]
pub unsafe extern "C" fn bz_ticks() -> u64 {
    syscall2(SYS_TICKS, 0, 0)
}

/// Fill a ProcInfo array; returns the count written.
#[no_mangle]
pub unsafe extern "C" fn bz_proc_list(buf: *mut u8, max: u64) -> u64 {
    syscall2(SYS_PROC_LIST, buf as u64, max)
}

/// Terminate a process by pid; 0 on success.
#[no_mangle]
pub unsafe extern "C" fn bz_proc_kill(pid: u64) -> u64 {
    syscall2(SYS_PROC_KILL, pid, 0)
}

/// Fill a SysStat pointed to by `out`; 0 on success.
#[no_mangle]
pub unsafe extern "C" fn bz_sys_stat(out: *mut u8) -> u64 {
    syscall2(SYS_SYS_STAT, out as u64, 0)
}

// --- v0.15 "Matang" managed-runtime PAL: user-space memory syscalls ----------

/// Map `size` bytes of anonymous user pages with protection `prot`
/// (1=R, 2=W, 4=X, OR-combined). Returns the base address, or a value in the
/// high error range (>= u64::MAX-1) on failure.
#[no_mangle]
pub unsafe extern "C" fn bz_mmap(size: u64, prot: u64) -> u64 {
    syscall2(SYS_MMAP, size, prot)
}

/// Change protection on a mapped range; 0 on success.
#[no_mangle]
pub unsafe extern "C" fn bz_mprotect(addr: u64, size: u64, prot: u64) -> u64 {
    syscall3(SYS_MPROTECT, addr, size, prot)
}

/// Unmap a range; 0 on success.
#[no_mangle]
pub unsafe extern "C" fn bz_munmap(addr: u64, size: u64) -> u64 {
    syscall2(SYS_MUNMAP, addr, size)
}

// --- v0.15 "Matang" increment 2: cooperative ring-3 threads ------------------

/// Cooperatively yield the CPU (lets other threads of this process run).
#[no_mangle]
pub unsafe extern "C" fn bz_yield() {
    syscall2(SYS_YIELD, 0, 0);
}

/// Rust body of a spawned thread: reads (entry, arg) from the ctx the kernel
/// passed in rdi, calls `entry(arg)`, then exits the thread.
extern "C" fn bz_thread_body(ctx: *const u64) -> ! {
    unsafe {
        let entry = *ctx;
        let arg = *ctx.add(1);
        let f: extern "C" fn(u64) = core::mem::transmute(entry);
        f(arg);
        syscall2(SYS_THREAD_EXIT, 0, 0);
    }
    loop {}
}

/// Ring-3 entry stub the kernel jumps to for a new thread (rdi = ctx). Aligns
/// the stack, runs the body, and exits the thread if the body ever returns.
#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn bz_thread_trampoline() -> ! {
    core::arch::naked_asm!(
        "and rsp, -16",          // 16-align; +8 after the call = SysV entry
        "call {body}",           // body(ctx) — rdi already holds ctx
        "mov rax, {texit}",      // body shouldn't return, but exit if it does
        "xor edi, edi",
        "syscall",
        "2: hlt",
        "jmp 2b",
        body = sym bz_thread_body,
        texit = const SYS_THREAD_EXIT,
    )
}

/// Spawn a cooperative thread running `entry(arg)` (both raw values from C#).
/// Allocates the thread's user stack + a small ctx via mmap. Returns the
/// thread id, or 0 on failure.
#[no_mangle]
pub unsafe extern "C" fn bz_thread_create(entry: u64, arg: u64) -> u64 {
    const STACK: u64 = 64 * 1024;
    let base = bz_mmap(STACK, 1 | 2); // READ | WRITE
    if base >= u64::MAX - 1 || base == 0 {
        return 0;
    }
    // ctx at the low end of the region; stack grows down from the top.
    let ctx = base as *mut u64;
    *ctx = entry;
    *ctx.add(1) = arg;
    let stack_top = base + STACK;
    let tid = syscall3(
        SYS_THREAD_CREATE,
        bz_thread_trampoline as *const () as u64,
        ctx as u64,
        stack_top,
    );
    if tid >= u64::MAX - 1 {
        0
    } else {
        tid
    }
}

/// Wait for a thread to finish; 0 on success.
#[no_mangle]
pub unsafe extern "C" fn bz_thread_join(tid: u64) -> u64 {
    syscall2(SYS_THREAD_JOIN, tid, 0)
}

/// Demo worker: bump the shared i64 counter at `arg` a fixed number of times,
/// yielding between bumps so it interleaves with the main thread. Provided by
/// the shim (a valid native entry) so the thread demo doesn't depend on C#
/// UnmanagedCallersOnly marshaling. `bz_thread_create` still accepts arbitrary
/// entries for the future.
const DEMO_WORKER_ITERS: u64 = 1000;

extern "C" fn bz_demo_worker(arg: u64) {
    let counter = arg as *mut i64;
    let mut i = 0u64;
    while i < DEMO_WORKER_ITERS {
        unsafe {
            *counter += 1;
            syscall2(SYS_YIELD, 0, 0);
        }
        i += 1;
    }
}

/// Spawn the demo worker on the counter at `counter`. Returns the thread id.
/// The worker performs `DEMO_WORKER_ITERS` (1000) bumps.
#[no_mangle]
pub unsafe extern "C" fn bz_spawn_worker(counter: u64) -> u64 {
    bz_thread_create(bz_demo_worker as *const () as u64, counter)
}

// --- v0.15 "Matang" increment 3: sync (futex/mutex), TLS, monotonic clock ----

/// Futex wait: if *addr == expected, block until woken. Returns 0.
#[no_mangle]
pub unsafe extern "C" fn bz_futex_wait(addr: u64, expected: u64) -> u64 {
    syscall2(SYS_FUTEX_WAIT, addr, expected)
}

/// Futex wake up to `count` waiters on `addr`. Returns the number woken.
#[no_mangle]
pub unsafe extern "C" fn bz_futex_wake(addr: u64, count: u64) -> u64 {
    syscall2(SYS_FUTEX_WAKE, addr, count)
}

/// The calling thread's id (pthread_self / TLS foundation).
#[no_mangle]
pub unsafe extern "C" fn bz_thread_self() -> u64 {
    syscall2(SYS_THREAD_SELF, 0, 0)
}

/// Monotonic high-resolution counter (CPU timestamp counter cycles).
#[no_mangle]
pub unsafe extern "C" fn bz_clock_mono() -> u64 {
    syscall2(SYS_CLOCK_MONO, 0, 0)
}

/// Fill an AudioInfo (6×u64) at `out`. Returns 0 on success.
#[no_mangle]
pub unsafe extern "C" fn bz_audio_stat(out: *mut u8) -> u64 {
    syscall2(SYS_AUDIO_STAT, out as u64, 0)
}

/// Set the master output volume (0..=100 percent). Returns 0 on success.
#[no_mangle]
pub unsafe extern "C" fn bz_audio_set_volume(pct: u64) -> u64 {
    syscall2(SYS_AUDIO_SET_VOLUME, pct, 0)
}

/// Play a generated sine tone: `freq` Hz for `ms` milliseconds.
#[no_mangle]
pub unsafe extern "C" fn bz_audio_tone(freq: u64, ms: u64) -> u64 {
    syscall2(SYS_AUDIO_TONE, freq, ms)
}

/// Play interleaved 16-bit stereo PCM (`ptr`, `len` bytes) at 48 kHz.
#[no_mangle]
pub unsafe extern "C" fn bz_audio_play(ptr: *const u8, len: u64) -> u64 {
    syscall2(SYS_AUDIO_PLAY, ptr as u64, len)
}

/// Fill an array of PkgInfo (48 bytes each) at `buf`; returns the count.
#[no_mangle]
pub unsafe extern "C" fn bz_pkg_list(buf: *mut u8, max: u64) -> u64 {
    syscall2(SYS_PKG_LIST, buf as u64, max)
}

/// Install (action=1) or remove (action=0) a package by name. Returns 0 on ok.
#[no_mangle]
pub unsafe extern "C" fn bz_pkg_set(name: *const u8, len: u64, action: u64) -> u64 {
    syscall3(SYS_PKG_SET, name as u64, len, action)
}

/// List a VFS directory into an FsEntry array (32 bytes each). `path` is a
/// NUL-terminated string ("" = list mounts). Returns the entry count.
#[no_mangle]
pub unsafe extern "C" fn bz_fs_list(path: *const u8, buf: *mut u8, max: u64) -> u64 {
    syscall3(SYS_FS_LIST, path as u64, buf as u64, max)
}

/// Read a file's bytes from the VFS into `buf` (up to `max` bytes). `path` is a
/// NUL-terminated string. Returns the number of bytes read (0 if missing).
#[no_mangle]
pub unsafe extern "C" fn bz_fs_read(path: *const u8, buf: *mut u8, max: u64) -> u64 {
    syscall3(SYS_FS_READ, path as u64, buf as u64, max)
}

/// 1 if the app runs in an interactive session (desktop up, keyboard readable),
/// 0 during the headless boot-demo runs. Interactive apps loop on the keyboard
/// only when this is 1; otherwise they exit after their scripted demo.
#[no_mangle]
pub unsafe extern "C" fn bz_is_interactive() -> u64 {
    syscall2(SYS_IS_INTERACTIVE, 0, 0)
}

// ---- pre-v1.0 Buitenzorg.Bcl: System.IO / Globalization / Net ------------

/// Write `len` bytes to a VFS file (create/truncate). Returns bytes written.
#[no_mangle]
pub unsafe extern "C" fn bz_fs_write(path: *const u8, buf: *const u8, len: u64) -> u64 {
    let r = syscall3(SYS_FS_WRITE, path as u64, buf as u64, len);
    if r >= u64::MAX - 1 { 0 } else { r }
}

/// Read the CMOS real-time clock into 6 u64s: year, month, day, hour, min, sec.
#[no_mangle]
pub unsafe extern "C" fn bz_clock_rtc(out: *mut u64) -> u64 {
    syscall2(SYS_CLOCK_RTC, out as u64, 0)
}

/// Create a socket (kind 0 = UDP). Returns a handle, or 0 on failure.
#[no_mangle]
pub unsafe extern "C" fn bz_net_socket(kind: u64) -> u64 {
    let r = syscall2(SYS_NET_SOCKET, kind, 0);
    if r >= u64::MAX - 1 { 0 } else { r }
}

/// Bind a socket to a local port. Returns 0 on success.
#[no_mangle]
pub unsafe extern "C" fn bz_net_bind(handle: u64, port: u64) -> u64 {
    syscall2(SYS_NET_BIND, handle, port)
}

/// Send a datagram: `buf` is a 16-byte header (addr[4], port u32, len u64)
/// followed by the payload. Returns payload bytes sent.
#[no_mangle]
pub unsafe extern "C" fn bz_net_send(handle: u64, buf: *const u8, len: u64) -> u64 {
    let r = syscall3(SYS_NET_SEND, handle, buf as u64, len);
    if r >= u64::MAX - 1 { 0 } else { r }
}

/// Receive a datagram into the same header+payload layout. Returns the payload
/// length, or 0 when nothing is queued (non-blocking).
#[no_mangle]
pub unsafe extern "C" fn bz_net_recv(handle: u64, buf: *mut u8, max: u64) -> u64 {
    let r = syscall3(SYS_NET_RECV, handle, buf as u64, max);
    if r >= u64::MAX - 1 { 0 } else { r }
}

/// Close a socket. Returns 0 on success.
#[no_mangle]
pub unsafe extern "C" fn bz_net_close(handle: u64) -> u64 {
    syscall2(SYS_NET_CLOSE, handle, 0)
}

/// Interface info + counters into an `abi::NetInfo` (56 bytes).
#[no_mangle]
pub unsafe extern "C" fn bz_net_info(out: *mut u8) -> u64 {
    syscall2(SYS_NET_INFO, out as u64, 0)
}

/// Lock a futex-backed mutex (0 = unlocked, 1 = locked). Single-core
/// cooperative, so the compare-and-set is a plain read/write; contention blocks
/// on the futex instead of spinning.
#[no_mangle]
pub unsafe extern "C" fn bz_mutex_lock(m: *mut i32) {
    loop {
        if *m == 0 {
            *m = 1;
            return;
        }
        bz_futex_wait(m as u64, 1);
    }
}

/// Unlock a mutex and wake one waiter.
#[no_mangle]
pub unsafe extern "C" fn bz_mutex_unlock(m: *mut i32) {
    *m = 0;
    bz_futex_wake(m as u64, 1);
}

/// Context for the mutual-exclusion demo worker (shared by both workers).
#[repr(C)]
struct MutexCtx {
    mutex: *mut i32,
    counter: *mut i64,
    token: *mut i64, // the id of the thread currently in the critical section
    error: *mut i32, // set to 1 if mutual exclusion is ever violated
    iters: u64,
    ids: *mut i64,  // [2]: each worker records its own thread-self id here
    slot: *mut i32, // claim counter so each worker picks a distinct ids[] slot
}

/// Demo worker proving mutual exclusion: it records its own thread id (so the
/// caller can check THREAD_SELF gives distinct, correct ids), then repeatedly
/// enters the critical section (holding the mutex), stamps its id, yields
/// *inside* the CS, verifies the stamp is unchanged (no other thread entered),
/// bumps the counter, and leaves.
extern "C" fn bz_mutex_worker(arg: u64) {
    let c = arg as *const MutexCtx;
    unsafe {
        let me = bz_thread_self() as i64;
        // Claim a slot (cooperative single-core: no yield between read/write).
        let s = *(*c).slot;
        *(*c).slot = s + 1;
        if s >= 0 && s < 2 {
            *(*c).ids.offset(s as isize) = me;
        }
        let mut i = 0u64;
        while i < (*c).iters {
            bz_mutex_lock((*c).mutex);
            *(*c).token = me;
            syscall2(SYS_YIELD, 0, 0); // yield while holding the lock
            if *(*c).token != me {
                *(*c).error = 1; // another thread entered the CS -> broken
            }
            *(*c).counter += 1;
            bz_mutex_unlock((*c).mutex);
            syscall2(SYS_YIELD, 0, 0);
            i += 1;
        }
    }
}

/// Spawn a mutual-exclusion demo worker from a `MutexCtx` pointer.
#[no_mangle]
pub unsafe extern "C" fn bz_spawn_mutex_worker(ctx: u64) -> u64 {
    bz_thread_create(bz_mutex_worker as *const () as u64, ctx)
}

/// Exit the calling thread (does not return).
#[no_mangle]
pub unsafe extern "C" fn bz_thread_exit(code: u64) -> ! {
    syscall2(SYS_THREAD_EXIT, code, 0);
    loop {}
}

/// ELF entry point. zerolib's Main needs no OS-level init, so we call the
/// managed entry directly with a minimal argv, then exit with its return code.
#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "xor ebp, ebp",
        "lea rdi, [rip + PROG_NAME]",  // argv[0]
        "push 0",                       // argv[1] = null
        "push rdi",                     // argv[0]
        "mov rsi, rsp",                 // argv
        "mov edi, 1",                   // argc = 1
        "and rsp, -16",                 // 16-byte align for the call
        "call {main}",
        "mov edi, eax",                 // exit code
        "mov rax, {exit}",
        "mov esi, 0",
        "syscall",
        "2: hlt",
        "jmp 2b",
        main = sym __managed__Main,
        exit = const SYS_EXIT,
    )
}

#[no_mangle]
static PROG_NAME: [u8; 6] = *b"hello\0";

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    unsafe { SystemNative_Abort() }
}
