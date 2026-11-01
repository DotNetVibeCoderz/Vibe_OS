//! RISC-V (RV64) implementation of the HAL — multi-arch M4 (increments 1-4): the
//! kernel runs in S-mode on QEMU `virt` under OpenSBI, loaded by Limine
//! (through edk2).
//!
//! - `sbi`: calls into the SBI firmware (`ecall`): console, timer, reset;
//! - `cpu`: interrupt masking (`sstatus.SIE`), `wfi`, the `time` counter,
//!   `sfence.vma`, the CPU index (kept in `tp` while in the kernel);
//! - `trap`: the S-mode trap vector (`stvec`) and its Rust dispatcher;
//! - `timer`: the SBI timer as the periodic tick;
//! - `plic`: the platform interrupt controller (device interrupts);
//! - `paging`: the `arch::paging` HAL over Sv48 (Sv39/Sv57 tolerated);
//! - `aspace`: per-process address spaces (one root per process, the kernel
//!   half shared by copying its top-level entries);
//! - `context`: kernel-thread switch + per-task F/D register state;
//! - `usermode`: entering U-mode (`sret`) and coming back (EXIT / faults),
//!   with the per-task API the shared scheduler (`task.rs`) uses;
//! - `gdt`, `smp`: the no-op / single-hart pieces `task.rs` calls.

pub mod aspace;
pub mod context;
pub mod cpu;
pub mod gdt;
pub mod interrupts;
pub mod paging;
pub mod plic;
pub mod sbi;
pub mod smp;
pub mod timer;
pub mod trap;
pub mod usermode;

/// ELF entry symbol. Limine enters through the entry-point request
/// (`limine::limine_entry`) instead; this only satisfies the linker and parks
/// a hart that would ever arrive here.
#[no_mangle]
extern "C" fn bz_riscv_start() -> ! {
    loop {
        cpu::halt();
    }
}
