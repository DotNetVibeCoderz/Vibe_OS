//! PC platform devices (port I/O): 16550 serial, IDE/ATA PIO, PS/2 mouse, PCI
//! configuration space (CF8/CFC), CMOS RTC, ACPI power, AC'97 audio.

pub mod ata;
pub mod audio;
pub mod mouse;
pub mod pci;
pub mod power;
pub mod rtc;
pub mod serial;
