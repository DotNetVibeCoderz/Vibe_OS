//! Preemption probe (two copies run at once): spin ~1.5 s without ever
//! yielding, keeping a running sum in an FP register (f8, through 1M-step
//! asm loops) and a stamp in our own mmap page -- both copies use the same
//! virtual address. Exact results prove the timer preempted U-mode code and
//! every switch kept FP state and the address space per process.
#![no_std]
#![no_main]
mod rt;
use core::arch::asm;
use rt::*;

/// Add f9 to f8 `n` times (f8 survives only if the kernel saves FP state).
fn fp_steps(n: u64) {
    unsafe {
        asm!(
            ".option push",
            ".option arch, +d",
            "1: fadd.d f8, f8, f9",
            "addi {n}, {n}, -1",
            "bnez {n}, 1b",
            ".option pop",
            n = inout(reg) n => _,
            options(nostack)
        );
    }
}

fn main(_arg: u64) -> u64 {
    let me = syscall(THREAD_SELF, 0, 0, 0);
    let inc = (me % 7 + 1) as f64;
    let page = mmap(4096, PROT_READ | PROT_WRITE);
    unsafe {
        (page as *mut u64).write_volatile(0x5EED_0000 + me);
        asm!(
            ".option push",
            ".option arch, +d",
            "fmv.d.x f8, zero",
            "fmv.d.x f9, {}",
            ".option pop",
            in(reg) inc.to_bits(),
            options(nostack)
        );
    }
    let t0 = syscall(CLOCK_MONO, 0, 0, 0);
    let mut chunks = 0u64;
    // 10 MHz timebase on QEMU virt: 15M = 1.5 s.
    while syscall(CLOCK_MONO, 0, 0, 0) - t0 < 15_000_000 {
        fp_steps(1_000_000);
        chunks += 1;
    }
    let sum: u64;
    unsafe { asm!(".option push", ".option arch, +d", "fmv.x.d {}, f8", ".option pop", out(reg) sum, options(nostack)) };
    let want = (chunks * 1_000_000) as f64 * inc;
    let exact = f64::from_bits(sum) == want;
    let stamp_ok = unsafe { (page as *const u64).read_volatile() } == 0x5EED_0000 + me;
    print("mproc: ");
    print_u64(chunks);
    print("M FP steps, sum ");
    print(if exact { "exact" } else { "WRONG" });
    print(", page stamp ");
    print(if stamp_ok { "intact\n" } else { "CLOBBERED\n" });
    if exact && stamp_ok && chunks > 0 { 0 } else { 1 }
}
