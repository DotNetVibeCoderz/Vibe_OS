//! Real managed threads: RustCLR's `threads` feature without `std` runs on the
//! hooks of `rustclr-thread`, which this module implements with Buitenzorg
//! syscalls -- THREAD_CREATE / THREAD_JOIN / THREAD_EXIT for threads (each on
//! its own committed mmap stack), FUTEX_WAIT_TIMEOUT / FUTEX_WAKE for the
//! locks, condition variables and sleeps, CLOCK_NANOS for the clock. Threads
//! of one process run in parallel on several CPUs (the kernel's
//! `PARALLEL_THREADS`).

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use core::arch::global_asm;
use core::sync::atomic::AtomicU32;

use rustclr_thread::sync::Mutex;
use rustclr_thread::Hooks;

use crate::sys;

pub const YIELD: u64 = 3;
pub const MUNMAP: u64 = 15;
pub const THREAD_CREATE: u64 = 16;
pub const THREAD_JOIN: u64 = 17;
pub const THREAD_EXIT: u64 = 18;
pub const FUTEX_WAKE: u64 = 20;
pub const THREAD_SELF: u64 = 21;
pub const FUTEX_WAIT_TIMEOUT: u64 = 43;
pub const CLOCK_NANOS: u64 = 44;

/// What a new thread runs.
struct Start {
    entry: extern "C" fn(usize),
    arg: usize,
}

/// Stacks of live threads, unmapped when they are joined: token -> (base, len).
static STACKS: Mutex<BTreeMap<u64, (u64, u64)>> = Mutex::new(BTreeMap::new());

// The kernel enters a thread at its entry with the argument in the first
// argument register and the stack we gave it; align it and call Rust.
#[cfg(target_arch = "x86_64")]
global_asm!(".global bz_clr_thread", "bz_clr_thread:", "and rsp, -16", "call bz_clr_thread_main", "ud2");
#[cfg(target_arch = "aarch64")]
global_asm!(".global bz_clr_thread", "bz_clr_thread:", "bl bz_clr_thread_main", "brk #0");
#[cfg(target_arch = "riscv64")]
global_asm!(".global bz_clr_thread", "bz_clr_thread:", "call bz_clr_thread_main", "unimp");

extern "C" {
    fn bz_clr_thread();
}

#[no_mangle]
extern "C" fn bz_clr_thread_main(start: usize) -> ! {
    let start = unsafe { Box::from_raw(start as *mut Start) };
    (start.entry)(start.arg);
    sys::syscall(THREAD_EXIT, 0, 0, 0);
    loop {}
}

fn spawn(entry: extern "C" fn(usize), arg: usize, stack_size: usize) -> u64 {
    // Committed up front (the kernel does not grow stacks): honour the size
    // asked for -- RustCLR asks for std's 2 MiB default -- with a 1 MiB floor.
    let len = (stack_size as u64).max(1 << 20).next_multiple_of(4096);
    let base = sys::syscall(sys::MMAP, len, sys::PROT_RW, 0);
    if sys::is_err(base) || base == 0 {
        return 0;
    }
    let start = Box::into_raw(Box::new(Start { entry, arg })) as u64;
    let tid = sys::syscall(THREAD_CREATE, bz_clr_thread as *const () as u64, start, (base + len) & !0xF);
    if sys::is_err(tid) || tid == 0 {
        drop(unsafe { Box::from_raw(start as *mut Start) });
        sys::syscall(MUNMAP, base, len, 0);
        return 0;
    }
    STACKS.lock().unwrap().insert(tid, (base, len));
    tid
}

fn join(token: u64) {
    sys::syscall(THREAD_JOIN, token, 0, 0);
    if let Some((base, len)) = STACKS.lock().unwrap().remove(&token) {
        sys::syscall(MUNMAP, base, len, 0);
    }
}

fn wait(word: &AtomicU32, expected: u32, timeout_ns: Option<u64>) {
    let t = match timeout_ns {
        Some(0) => return,
        Some(ns) => ns,
        None => 0,
    };
    sys::syscall(FUTEX_WAIT_TIMEOUT, word.as_ptr() as u64, expected as u64, t);
}

fn wake(word: &AtomicU32, count: u32) {
    let woken = sys::syscall(FUTEX_WAKE, word.as_ptr() as u64, count as u64, 0);
    // Hand the processor over: a woken thread otherwise waits for this CPU's
    // next timer tick (55 ms on the x86 boot CPU, whose clock is the PIT),
    // which turns every Monitor.Pulse / lock hand-off into a tick of latency.
    if woken > 0 && !sys::is_err(woken) {
        yield_now();
    }
}

fn now_ns() -> u64 {
    sys::syscall(CLOCK_NANOS, 0, 0, 0)
}

fn current_id() -> u64 {
    sys::syscall(THREAD_SELF, 0, 0, 0)
}

fn yield_now() {
    sys::syscall(YIELD, 0, 0, 0);
}

fn parallelism() -> usize {
    2
}

static HOOKS: Hooks = Hooks { spawn, join, wait, wake, now_ns, current_id, yield_now, parallelism };

/// Give RustCLR its threads. Before anything starts a thread.
pub fn install() {
    rustclr_thread::install(&HOOKS);
}
