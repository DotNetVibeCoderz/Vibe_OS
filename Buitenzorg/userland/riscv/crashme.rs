//! Crashes on purpose, three calls deep: the kernel must end it and write a
//! crash report whose backtrace names level3 <- level2 <- level1 (the boot's
//! `CRASH REPORT OK` check). Built with its symbol table (strip=debuginfo).
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

fn main(_arg: u64) -> u64 {
    rt::print("crashme: about to read address 0, three calls deep\n");
    let null = core::hint::black_box(0usize) as *const u64;
    level1(null)
}
