//! Power management on Device-Tree boards: the firmware's power-off / reset
//! calls (the counterpart of the PC's ACPI PM1 + reset ports) -- **PSCI**
//! SYSTEM_OFF / SYSTEM_RESET on ARM64, the **SBI system-reset extension**
//! (SRST: shutdown / cold reboot) on RISC-V -- and a light sleep that idles
//! until input. Same API as `platform/pc/power.rs`.

/// The firmware interface behind shutdown / restart.
#[cfg(target_arch = "aarch64")]
mod fw {
    use crate::arch::aarch64::psci;

    pub const NAME: &str = "PSCI";

    pub fn init(method: &str) -> bool {
        psci::init(method);
        psci::available()
    }
    pub fn available() -> bool {
        psci::available()
    }
    pub fn off() {
        psci::call(psci::SYSTEM_OFF, 0, 0, 0);
    }
    pub fn reset() {
        psci::call(psci::SYSTEM_RESET, 0, 0, 0);
    }
}

#[cfg(target_arch = "riscv64")]
mod fw {
    use crate::arch::riscv64::sbi;
    use core::sync::atomic::{AtomicBool, Ordering};

    pub const NAME: &str = "SBI SRST";
    static SRST: AtomicBool = AtomicBool::new(false);

    /// `_method` is PSCI's conduit; RISC-V always calls the SBI with `ecall`.
    pub fn init(_method: &str) -> bool {
        SRST.store(sbi::probe(sbi::EXT_SRST), Ordering::SeqCst);
        available()
    }
    pub fn available() -> bool {
        SRST.load(Ordering::SeqCst)
    }
    /// sbi_system_reset(type, reason): type 0 = shutdown, 1 = cold reboot.
    pub fn off() {
        sbi::call(sbi::EXT_SRST, 0, 0, 0, 0);
    }
    pub fn reset() {
        sbi::call(sbi::EXT_SRST, 0, 1, 0, 0);
    }
}

/// Probe the firmware interface (`method` = the DTB `/psci` conduit on ARM64).
pub fn init(method: &str) -> bool {
    fw::init(method)
}

/// Name of the firmware interface used for shutdown / restart.
pub fn firmware() -> &'static str {
    fw::NAME
}

/// No ACPI on these boards.
pub fn acpi_available() -> bool {
    false
}

/// (ACPI available, PM1a, SLP_TYP, reset port) -- the PC's shape; on these
/// boards the ports are 0 and the first field says whether the firmware's
/// power calls are usable.
pub fn summary() -> (bool, u16, u16, u16) {
    (fw::available(), 0, 0, 0)
}

pub fn shutdown() -> ! {
    crate::println!("[power] shutting down ({} power-off)...", fw::NAME);
    crate::arch::cpu::disable_interrupts();
    if fw::available() {
        fw::off();
    }
    loop {
        crate::arch::cpu::halt();
    }
}

pub fn restart() -> ! {
    crate::println!("[power] restarting ({} reset)...", fw::NAME);
    crate::arch::cpu::disable_interrupts();
    if fw::available() {
        fw::reset();
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
