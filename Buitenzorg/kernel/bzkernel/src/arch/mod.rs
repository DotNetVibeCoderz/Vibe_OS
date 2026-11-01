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
//! `scripts/check-arch-boundary.sh` enforces the boundary: outside `arch/` and
//! `platform/`, only the files on its allowlist (paging, the context switch
//! and boot glue — the next M0 steps) may still touch architecture details.

#[cfg(target_arch = "x86_64")]
pub mod x86;

/// CPU primitives every architecture provides.
pub mod cpu {
    /// Stop the CPU until the next interrupt.
    #[inline]
    pub fn halt() {
        #[cfg(target_arch = "x86_64")]
        x86_64::instructions::hlt();
    }

    /// Enable maskable interrupts on this CPU.
    #[inline]
    pub fn enable_interrupts() {
        #[cfg(target_arch = "x86_64")]
        x86_64::instructions::interrupts::enable();
    }

    /// Disable maskable interrupts on this CPU.
    #[inline]
    pub fn disable_interrupts() {
        #[cfg(target_arch = "x86_64")]
        x86_64::instructions::interrupts::disable();
    }

    /// Run `f` with interrupts disabled, restoring the previous state after.
    #[inline]
    pub fn without_interrupts<R>(f: impl FnOnce() -> R) -> R {
        #[cfg(target_arch = "x86_64")]
        return x86_64::instructions::interrupts::without_interrupts(f);
    }

    /// A monotonic, high-resolution cycle counter (TSC on x86; CNTVCT on
    /// ARM64 and `rdtime` on RISC-V in their ports).
    #[inline]
    pub fn timestamp() -> u64 {
        #[cfg(target_arch = "x86_64")]
        return unsafe { core::arch::x86_64::_rdtsc() };
    }

    /// Invalidate every non-global TLB entry on this CPU.
    #[inline]
    pub fn flush_tlb() {
        #[cfg(target_arch = "x86_64")]
        x86_64::instructions::tlb::flush_all();
    }
}
