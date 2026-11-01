//! Platform devices — the machine around the CPU. `pc` is the IBM-PC
//! compatible platform reached through x86 port I/O; ARM64/RISC-V boards
//! (QEMU `virt`) will add their own (PL011/16550-MMIO, virtio, PL031, …).

#[cfg(target_arch = "x86_64")]
pub mod pc;

/// QEMU `virt` / Device-Tree boards (ARM64 port): MMIO devices found via the DTB.
#[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
pub mod virt;
