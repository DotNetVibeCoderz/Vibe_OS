//! Crashes on purpose, three calls deep: the kernel must end it and write a
//! crash report whose backtrace names level3 <- level2 <- level1 (the boot's
//! `CRASH REPORT OK` check). Built with its symbol table (strip=debuginfo).
//! With the argument `spin` it does not crash: ~1.2 s in `hot`, ~0.3 s in
//! `cold`, for the per-app profiler's check (`APP PROFILER OK`).
#![no_std]
#![no_main]
mod rt;

#[inline(never)]
fn level3(p: *const u64) -> u64 {
    // a load from address 0: a load page fault in U-mode
    unsafe { core::ptr::read_volatile(p) }
}

#[inline(never)]
fn level2(p: *const u64) -> u64 {
    level3(p).wrapping_add(rt::syscall(rt::TICKS, 0, 0, 0))
}

#[inline(never)]
fn level1(p: *const u64) -> u64 {
    level2(p).wrapping_mul(3).wrapping_add(rt::syscall(rt::TICKS, 0, 0, 0))
}

const PROC_ARGS: u64 = 42;
const CLOCK_NANOS: u64 = 44;

fn now() -> u64 {
    rt::syscall(CLOCK_NANOS, 0, 0, 0)
}

#[inline(never)]
fn hot(until: u64) -> u64 {
    let mut x = 1u64;
    while now() < until {
        for _ in 0..200000 {
            x = core::hint::black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407));
        }
    }
    x
}

#[inline(never)]
fn cold(until: u64) -> u64 {
    let mut x = 7u64;
    while now() < until {
        for _ in 0..200000 {
            x = core::hint::black_box(x.wrapping_mul(2862933555777941757).wrapping_add(3037000493));
        }
    }
    x
}

fn main(_arg: u64) -> u64 {
    let mut a = [0u8; 16];
    let n = rt::syscall(PROC_ARGS, a.as_mut_ptr() as u64, 16, 0);
    if n >= 4 && &a[..4] == b"spin" {
        let t = now();
        let r = hot(t + 1_200_000_000) ^ cold(t + 1_500_000_000);
        return if r == 42 { 1 } else { 0 };
    }
    rt::print("crashme: about to read address 0, three calls deep\n");
    let null = core::hint::black_box(0usize) as *const u64;
    level1(null)
}
