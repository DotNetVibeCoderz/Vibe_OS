//! Devices of Device-Tree boards (QEMU `virt` for ARM64 / RISC-V): MMIO
//! peripherals whose addresses the DTB provides.

pub mod audio;
pub mod calendar;
#[cfg(target_arch = "riscv64")]
pub mod goldfish_rtc;
pub mod mouse;
#[cfg(target_arch = "aarch64")]
pub mod pl011;
#[cfg(target_arch = "aarch64")]
pub mod pl031;
pub mod power;
#[cfg(target_arch = "aarch64")]
pub mod sdhci;
#[cfg(target_arch = "aarch64")]
pub mod brcm_pcie;
#[cfg(target_arch = "aarch64")]
pub mod genet;
#[cfg(target_arch = "aarch64")]
pub mod rpi_io;
#[cfg(target_arch = "riscv64")]
pub mod uart16550;
