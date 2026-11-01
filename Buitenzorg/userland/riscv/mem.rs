//! The user-memory PAL on RISC-V (the C# runtime's heap path elsewhere):
//! mmap a read/write region, fill + verify it, make it read-only, unmap it;
//! then reserve 64 MiB without committing (prot NONE) and commit two pages
//! with mprotect on demand.
#![no_std]
#![no_main]
mod rt;
use rt::*;

fn main(_arg: u64) -> u64 {
    const N: u64 = 16 * 4096;
    let a = mmap(N, PROT_READ | PROT_WRITE);
    if a >= 1 << 40 {
        print("mem: mmap failed\n");
        return 1;
    }
    let p = a as *mut u64;
    for i in 0..N / 8 {
        unsafe { p.add(i as usize).write_volatile(i * 0x9E37_79B9) };
    }
    let ok_fill = (0..N / 8).all(|i| unsafe { p.add(i as usize).read_volatile() } == i * 0x9E37_79B9);
    let ro = syscall(MPROTECT, a, N, PROT_READ) == 0;
    let still = unsafe { p.read_volatile() } == 0;
    let unmapped = syscall(MUNMAP, a, N, 0) == 0;

    const RESERVE: u64 = 64 << 20;
    let r = mmap(RESERVE, PROT_NONE);
    let reserved = r < 1 << 40;
    let commit = syscall(MPROTECT, r + RESERVE / 2, 2 * 4096, PROT_READ | PROT_WRITE) == 0;
    let q = (r + RESERVE / 2) as *mut u64;
    unsafe { q.add(1000).write_volatile(0xC0FFEE) };
    let committed = unsafe { q.add(1000).read_volatile() } == 0xC0FFEE && unsafe { q.read_volatile() } == 0;

    let ok = ok_fill && ro && still && unmapped && reserved && commit && committed;
    print(if ok {
        "mem: mmap 64 KiB rw + verify, mprotect ro, munmap, reserve 64 MiB + commit on demand: ok\n"
    } else {
        "mem: FAILED\n"
    });
    if ok { 0 } else { 1 }
}
