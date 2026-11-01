//! Devices of Device-Tree boards (QEMU `virt` for ARM64 / RISC-V): MMIO
//! peripherals whose addresses the DTB provides.

pub mod audio;
pub mod mouse;
pub mod pl011;
pub mod pl031;
pub mod power;
