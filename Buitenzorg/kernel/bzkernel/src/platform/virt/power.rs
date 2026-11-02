//! Power management on Device-Tree boards: the firmware's power-off / reset
//! calls (the counterpart of the PC's ACPI PM1 + reset ports) -- **PSCI**
//! SYSTEM_OFF / SYSTEM_RESET on ARM64, the **SBI system-reset extension**
//! (SRST: shutdown / cold reboot) on RISC-V, and on a Raspberry Pi booted
//! without PSCI the SoC's **PM watchdog** (`brcm,bcm2835-pm-wdt`) -- and a
//! light sleep that idles until input. Same API as `platform/pc/power.rs`.

/// The firmware interface behind shutdown / restart.
#[cfg(target_arch = "aarch64")]
mod fw {
    use crate::arch::aarch64::psci;
    use core::sync::atomic::{AtomicU64, Ordering};

    /// Mapped Raspberry Pi PM block (watchdog + reset status), 0 = none.
    static PM: AtomicU64 = AtomicU64::new(0);

    const PM_RSTC: u64 = 0x1c;
    const PM_RSTS: u64 = 0x20;
    const PM_WDOG: u64 = 0x24;
    const PM_PASSWORD: u32 = 0x5a00_0000;
    /// The partition number lives in alternating RSTS bits; 63 = "halt".
    const PARTITION_BITS: u32 = 0x555;

    pub fn name() -> &'static str {
        if psci::available() {
            "PSCI"
        } else if PM.load(Ordering::SeqCst) != 0 {
            "BCM2711 PM watchdog"
        } else {
            "PSCI"
        }
    }
    pub fn init(method: &str) -> bool {
        psci::init(method);
        psci::available()
    }
    pub fn set_pm_watchdog(va: u64) {
        PM.store(va, Ordering::SeqCst);
    }
    pub fn available() -> bool {
        psci::available() || PM.load(Ordering::SeqCst) != 0
    }
    /// Reset through the watchdog (as Linux's bcm2835_wdt): `halt` sets the
    /// boot partition to 63, which tells the Pi firmware to stop there.
    fn pm_reset(halt: bool) {
        let pm = PM.load(Ordering::SeqCst);
        if pm == 0 {
            return;
        }
        let r = |o: u64| unsafe { ((pm + o) as *const u32).read_volatile() };
        let w = |o: u64, v: u32| unsafe { ((pm + o) as *mut u32).write_volatile(v) };
        let rsts = r(PM_RSTS) & 0x00FF_FFFF & !PARTITION_BITS;
        w(PM_RSTS, PM_PASSWORD | rsts | if halt { PARTITION_BITS } else { 0 });
        w(PM_WDOG, PM_PASSWORD | 10); // ~150 us
        w(PM_RSTC, PM_PASSWORD | (r(PM_RSTC) & 0x00FF_FFCF) | 0x20); // full reset
    }
    pub fn off() {
        if psci::available() {
            psci::call(psci::SYSTEM_OFF, 0, 0, 0);
        }
        pm_reset(true);
    }
    pub fn reset() {
        if psci::available() {
            psci::call(psci::SYSTEM_RESET, 0, 0, 0);
        }
        pm_reset(false);
    }
}

#[cfg(target_arch = "riscv64")]
mod fw {
    use crate::arch::riscv64::sbi;
    use core::sync::atomic::{AtomicBool, Ordering};

    pub fn name() -> &'static str {
        "SBI SRST"
    }
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
    fw::name()
}

/// A Raspberry Pi without PSCI: shut down / restart through its PM watchdog
/// (the mapped `brcm,bcm2835-pm-wdt` registers).
#[cfg(target_arch = "aarch64")]
pub fn use_pm_watchdog(va: u64) {
    fw::set_pm_watchdog(va);
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
    crate::println!("[power] shutting down ({} power-off)...", fw::name());
    crate::arch::cpu::disable_interrupts();
    if fw::available() {
        fw::off();
    }
    loop {
        crate::arch::cpu::halt();
    }
}

pub fn restart() -> ! {
    crate::println!("[power] restarting ({} reset)...", fw::name());
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
    // A second of grace: the release of the click that chose "sleep" (and any
    // key still held) must not wake it straight away.
    let hz = crate::arch::cpu::counter_hz().max(1);
    let t0 = crate::arch::cpu::timestamp();
    while crate::arch::cpu::timestamp() - t0 < hz {
        crate::arch::cpu::enable_and_halt();
        crate::virtio::input::poll();
        crate::usb::poll();
        while crate::serial::read_byte().is_some() {}
        while crate::keyboard::pop().is_some() {}
    }
    let (_, _, _, p0) = crate::mouse::state();
    loop {
        crate::arch::cpu::enable_and_halt();
        crate::virtio::input::poll();
        crate::usb::poll(); // USB keyboards / mice are polled
        while let Some(b) = crate::serial::read_byte() {
            crate::keyboard::feed_serial(b); // typing on the serial console wakes it too
        }
        let (_, _, _, p1) = crate::mouse::state();
        if crate::keyboard::pop().is_some() || p1 != p0 {
            break;
        }
    }
    crate::println!("[power] bangun");
}
