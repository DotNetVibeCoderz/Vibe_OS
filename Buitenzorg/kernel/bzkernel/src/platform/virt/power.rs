//! Power management on Device-Tree boards: PSCI SYSTEM_OFF / SYSTEM_RESET
//! (the ARM counterpart of the PC's ACPI PM1 + reset ports), and a light
//! sleep that idles until input. Same API as `platform/pc/power.rs`.

use crate::arch::aarch64::psci;

/// Pick the PSCI conduit (`/psci` `method` in the DTB).
pub fn init(method: &str) -> bool {
    psci::init(method);
    psci::available()
}

/// No ACPI on these boards.
pub fn acpi_available() -> bool {
    false
}

/// (ACPI available, PM1a, SLP_TYP, reset port) -- the PC's shape; on PSCI
/// boards the ports are 0 and the first field says whether PSCI is usable.
pub fn summary() -> (bool, u16, u16, u16) {
    (psci::available(), 0, 0, 0)
}

pub fn shutdown() -> ! {
    crate::println!("[power] shutting down (PSCI SYSTEM_OFF)...");
    crate::arch::cpu::disable_interrupts();
    if psci::available() {
        psci::call(psci::SYSTEM_OFF, 0, 0, 0);
    }
    loop {
        crate::arch::cpu::halt();
    }
}

pub fn restart() -> ! {
    crate::println!("[power] restarting (PSCI SYSTEM_RESET)...");
    crate::arch::cpu::disable_interrupts();
    if psci::available() {
        psci::call(psci::SYSTEM_RESET, 0, 0, 0);
    }
    loop {
        crate::arch::cpu::halt();
    }
}

/// Light sleep: blank the screen and idle until a key or pointer event.
pub fn sleep() {
    crate::println!("[power] sleeping (tekan tombol / gerakkan mouse untuk bangun)...");
    if let Some((w, h)) = crate::framebuffer::dimensions() {
        let black = alloc::vec![0u32; w * h];
        crate::framebuffer::present(&black);
    }
    let (_, _, _, p0) = crate::mouse::state();
    loop {
        crate::arch::cpu::enable_and_halt();
        crate::virtio::input::poll();
        let (_, _, _, p1) = crate::mouse::state();
        if crate::keyboard::pop().is_some() || p1 != p0 {
            break;
        }
    }
    crate::println!("[power] bangun");
}
