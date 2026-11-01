//! ARM64 (AArch64) implementation of the HAL — multi-arch M3, increment 1:
//! the kernel runs at EL1 on QEMU `virt`, loaded by Limine.
//!
//! - `cpu`: interrupt masking (DAIF), `wfi`, the virtual counter, TLB flush;
//! - `exceptions`: the EL1 vector table (VBAR_EL1) and its Rust dispatcher;
//! - `paging`: the `arch::paging` HAL (TTBR1 kernel half, TTBR0 user half);
//! - `aspace`: per-process address spaces (one TTBR0 root each);
//! - `gic`: GICv3 distributor + redistributor + system-register CPU interface;
//! - `timer`: the EL1 virtual generic timer (PPI 27) as the periodic tick.

pub mod aspace;
pub mod cpu;
pub mod exceptions;
pub mod gic;
pub mod paging;
pub mod timer;

/// ELF entry symbol. Limine enters through the entry-point request
/// (`limine::limine_entry`) instead; this only satisfies the linker and parks
/// a core that would ever arrive here.
#[no_mangle]
extern "C" fn bz_arm_start() -> ! {
    loop {
        cpu::halt();
    }
}
