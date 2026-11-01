//! Devices of Device-Tree boards (QEMU `virt` for ARM64 / RISC-V): MMIO
//! peripherals whose addresses the DTB provides.

#[cfg(target_arch = "aarch64")]
pub mod audio;
#[cfg(target_arch = "aarch64")]
pub mod mouse;
#[cfg(target_arch = "aarch64")]
pub mod pl011;
#[cfg(target_arch = "aarch64")]
pub mod pl031;
#[cfg(target_arch = "aarch64")]
pub mod power;
#[cfg(target_arch = "riscv64")]
pub mod uart16550;
