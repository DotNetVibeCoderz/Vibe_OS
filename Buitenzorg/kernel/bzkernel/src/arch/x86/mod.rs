//! x86-64 implementation of the HAL: segmentation + TSS (`gdt`), the IDT and
//! legacy PIC interrupts (`interrupts`), ring-3 entry and the SYSCALL/SYSRET
//! path (`usermode`), Local APIC + AP bring-up (`smp`), and VT-x/AMD-V
//! detection (`vmx`).

pub mod aspace;
pub mod context;
pub mod gdt;
pub mod interrupts;
pub mod paging;
pub mod smp;
pub mod usermode;
pub mod vmx;
