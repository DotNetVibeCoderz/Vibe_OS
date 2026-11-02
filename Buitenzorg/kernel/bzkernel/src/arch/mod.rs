//! Hardware abstraction layer (multi-arch milestone M0, see PLAN.md §v1.x).
//!
//! Everything that depends on the CPU architecture lives under `arch/<arch>/`
//! (today: `arch/x86` — GDT/TSS, IDT + PIC, SYSCALL/SYSRET entry, SMP
//! bring-up, VT-x detection); everything that depends on the PC platform's
//! port-I/O devices lives under `platform/pc/` (16550 serial, IDE PIO, PS/2
//! mouse, PCI CF8/CFC, CMOS RTC, ACPI power, AC'97). Generic kernel code calls
//! the facades here instead of the `x86_64` crate or inline assembly, so an
//! ARM64 or RISC-V port provides another `arch/<arch>` behind the same API.
//!
//! `scripts/check-arch-boundary.sh` enforces the boundary: no file outside
//! `arch/` and `platform/` may touch architecture details (the `x86_64` crate,
//! inline assembly, port I/O, MSRs). Per-process address spaces (`aspace`) are
//! x86-specific by design — the PDPT-swap trick — and live in `arch/x86`.

#[cfg(target_arch = "x86_64")]
pub mod x86;

#[cfg(target_arch = "aarch64")]
pub mod aarch64;

#[cfg(target_arch = "riscv64")]
pub mod riscv64;

/// Kernel-thread context switching: per-task FPU state, the initial frame of
/// a new task, and the stack switch itself.
pub mod context {
    #[cfg(target_arch = "x86_64")]
    pub use super::x86::context::{prepare_stack, switch, FpuState};
    #[cfg(target_arch = "aarch64")]
    pub use super::aarch64::context::{prepare_stack, switch, FpuState};
    #[cfg(target_arch = "riscv64")]
    pub use super::riscv64::context::{prepare_stack, switch, FpuState};
}

/// Page tables: map / unmap / protect 4 KiB pages with neutral `Flags`, the
/// ring-3 access check behind syscall pointer validation, the active-table
/// handle (`PageMap`) and the ELF machine id of native executables.
pub mod paging {
    #[cfg(target_arch = "x86_64")]
    pub use super::x86::paging::{
        init, is_mapped, map, map_device, protect, unmap, user_access, Flags, PageMap, ELF_MACHINE, KERNEL_HEAP_BASE, MMIO_BASE,
    };
    #[cfg(target_arch = "aarch64")]
    pub use super::aarch64::paging::{
        init, init_user_half, is_mapped, map, map_device, protect, unmap, user_access, Flags, PageMap, ELF_MACHINE,
        KERNEL_HEAP_BASE, MMIO_BASE,
    };
    #[cfg(target_arch = "riscv64")]
    pub use super::riscv64::paging::{
        init, init_user_half, is_mapped, map, map_device, protect, unmap, user_access, Flags, PageMap, ELF_MACHINE,
        KERNEL_HEAP_BASE, MMIO_BASE,
    };
}

/// CPU primitives every architecture provides.
pub mod cpu {
    /// Upper bound on the CPUs the kernel manages (per-CPU arrays).
    pub const MAX_CPUS: usize = 16;

    /// Invalidate stale translations on every CPU: this one, plus a TLB
    /// shootdown (with acknowledgement) on every other online CPU. Use after
    /// removing or downgrading a mapping that another core may have cached.
    pub fn flush_tlb_everywhere() {
        flush_tlb();
        #[cfg(target_arch = "x86_64")]
        super::x86::smp::tlb_shootdown();
        // RISC-V has no broadcast sfence.vma: ask the SBI firmware for a
        // remote fence on every hart (ARM64's `tlbi ...is` broadcasts).
        #[cfg(target_arch = "riscv64")]
        super::riscv64::smp::remote_sfence_all();
    }

    /// Stop the CPU until the next interrupt.
    #[inline]
    pub fn halt() {
        #[cfg(target_arch = "x86_64")]
        x86_64::instructions::hlt();
        #[cfg(target_arch = "aarch64")]
        super::aarch64::cpu::halt();
        #[cfg(target_arch = "riscv64")]
        super::riscv64::cpu::halt();
    }

    /// Enable interrupts and halt as one step (no interrupt can slip in
    /// between and leave the CPU asleep with work pending).
    #[inline]
    pub fn enable_and_halt() {
        #[cfg(target_arch = "x86_64")]
        x86_64::instructions::interrupts::enable_and_hlt();
        #[cfg(target_arch = "aarch64")]
        super::aarch64::cpu::enable_and_halt();
        #[cfg(target_arch = "riscv64")]
        super::riscv64::cpu::enable_and_halt();
    }

    /// True if maskable interrupts are enabled on this CPU.
    #[inline]
    pub fn interrupts_enabled() -> bool {
        #[cfg(target_arch = "x86_64")]
        return x86_64::instructions::interrupts::are_enabled();
        #[cfg(target_arch = "aarch64")]
        return super::aarch64::cpu::interrupts_enabled();
        #[cfg(target_arch = "riscv64")]
        return super::riscv64::cpu::interrupts_enabled();
    }

    /// Enable maskable interrupts on this CPU.
    #[inline]
    pub fn enable_interrupts() {
        #[cfg(target_arch = "x86_64")]
        x86_64::instructions::interrupts::enable();
        #[cfg(target_arch = "aarch64")]
        super::aarch64::cpu::enable_interrupts();
        #[cfg(target_arch = "riscv64")]
        super::riscv64::cpu::enable_interrupts();
    }

    /// Disable maskable interrupts on this CPU.
    #[inline]
    pub fn disable_interrupts() {
        #[cfg(target_arch = "x86_64")]
        x86_64::instructions::interrupts::disable();
        #[cfg(target_arch = "aarch64")]
        super::aarch64::cpu::disable_interrupts();
        #[cfg(target_arch = "riscv64")]
        super::riscv64::cpu::disable_interrupts();
    }

    /// Run `f` with interrupts disabled, restoring the previous state after.
    #[inline]
    pub fn without_interrupts<R>(f: impl FnOnce() -> R) -> R {
        #[cfg(target_arch = "x86_64")]
        return x86_64::instructions::interrupts::without_interrupts(f);
        #[cfg(target_arch = "aarch64")]
        return super::aarch64::cpu::without_interrupts(f);
        #[cfg(target_arch = "riscv64")]
        return super::riscv64::cpu::without_interrupts(f);
    }

    /// A monotonic, high-resolution cycle counter (TSC on x86; CNTVCT on
    /// ARM64 and `rdtime` on RISC-V in their ports).
    #[inline]
    pub fn timestamp() -> u64 {
        #[cfg(target_arch = "x86_64")]
        return unsafe { core::arch::x86_64::_rdtsc() };
        #[cfg(target_arch = "aarch64")]
        return super::aarch64::cpu::timestamp();
        #[cfg(target_arch = "riscv64")]
        return super::riscv64::cpu::timestamp();
    }

    /// Frequency of [`timestamp`] in Hz (the x86 TSC is calibrated against
    /// the PIT during SMP bring-up; ARM64 and RISC-V know theirs).
    pub fn counter_hz() -> u64 {
        #[cfg(target_arch = "x86_64")]
        let hz = super::x86::smp::tsc_hz();
        #[cfg(target_arch = "aarch64")]
        let hz = super::aarch64::cpu::counter_hz();
        #[cfg(target_arch = "riscv64")]
        let hz = super::riscv64::cpu::counter_hz();
        if hz == 0 { 1_000_000_000 } else { hz }
    }

    /// Monotonic nanoseconds since boot (CLOCK_NANOS).
    pub fn nanos() -> u64 {
        (timestamp() as u128 * 1_000_000_000 / counter_hz() as u128) as u64
    }

    /// Index of the CPU running this code (0 = the bootstrap CPU).
    #[inline]
    pub fn id() -> usize {
        #[cfg(target_arch = "x86_64")]
        return super::x86::smp::cpu_index();
        #[cfg(target_arch = "aarch64")]
        return super::aarch64::cpu::id();
        #[cfg(target_arch = "riscv64")]
        return super::riscv64::cpu::id();
    }

    /// Invalidate every non-global TLB entry on this CPU.
    #[inline]
    pub fn flush_tlb() {
        #[cfg(target_arch = "x86_64")]
        x86_64::instructions::tlb::flush_all();
        #[cfg(target_arch = "aarch64")]
        super::aarch64::cpu::flush_tlb();
        #[cfg(target_arch = "riscv64")]
        super::riscv64::cpu::flush_tlb();
    }
}
