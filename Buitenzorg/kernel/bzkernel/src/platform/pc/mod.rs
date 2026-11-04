//! PC platform devices (port I/O): 16550 serial, IDE/ATA PIO, PS/2 mouse, PCI
//! configuration space (CF8/CFC), CMOS RTC, ACPI power, AC'97 audio.

pub mod ahci;
pub mod ata;
pub mod audio;
pub mod bochs_vbe;
pub mod e1000;
pub mod hda;
pub mod mouse;
pub mod nvme;
pub mod pci;
pub mod power;
pub mod rtc;
pub mod serial;
