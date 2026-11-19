//! Trying a new kernel on a Raspberry Pi without touching the SD card (R6:
//! automatic tests on the board).
//!
//! The Pi firmware's **tryboot** mechanism: a reboot with the reboot flag
//! "tryboot" set (mailbox tag SET_REBOOT_FLAGS) makes the next boot -- that
//! one only -- read `tryboot.txt` instead of `config.txt`. The flow:
//!
//! 1. The PC uploads the new kernel as `/disk/KERNTRY.IMG` (bz-link,
//!    `bz pi update`).
//! 2. `pi tryboot` writes `TRYBOOT.TXT` (config.txt + `kernel=KERNTRY.IMG` +
//!    `cmdline=CMDTRY.TXT`) and `CMDTRY.TXT` (`bz.tryboot`), sets the flag
//!    and restarts.
//! 3. The new kernel sees `bz.tryboot` in its boot arguments, arms the PM
//!    watchdog (15 s) right away and feeds it from the desktop loop. If it
//!    hangs anywhere -- before the desktop or in it -- the watchdog resets
//!    the board, the flag is gone, and the old kernel boots again.
//! 4. The PC checks the new kernel over bz-link and runs `pi promote`: the
//!    watchdog stops and `KERNTRY.IMG` becomes `KERNEL8.IMG`. Without it, a
//!    `restart` (or a power cut) goes back to the old kernel.
//!
//! So a broken kernel can never leave the board unreachable. The flag needs a
//! Pi 4 bootloader EEPROM from 2021 or later.

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The boot argument the trial boot carries (CMDTRY.TXT).
pub const BOOTARG: &str = "bz.tryboot";
const SET_REBOOT_FLAGS: u32 = 0x0003_8064;
const WATCHDOG_S: u32 = 15;

static TRIAL: AtomicBool = AtomicBool::new(false);
static GUARDED: AtomicBool = AtomicBool::new(false);
static LAST_FEED: AtomicU64 = AtomicU64::new(0);

/// Whether this boot is a trial of a new kernel.
pub fn trial() -> bool {
    TRIAL.load(Ordering::Relaxed)
}

/// Early in the boot (right after the Device Tree is parsed): on a trial
/// boot, map the PM block and arm the watchdog before anything that could
/// hang.
pub fn early(f: Option<&crate::dtb::Fdt>) {
    let Some(f) = f else { return };
    let Some(chosen) = f.nodes.iter().position(|n| n.name == "chosen") else { return };
    let has = |arg: &str| f.nodes[chosen].strings("bootargs").any(|s| s.split_whitespace().any(|a| a == arg));
    if !has(BOOTARG) {
        return;
    }
    // QEMU raspi4b's PM model resets the moment the watchdog is started (it
    // ignores the timeout): `bz.nowatchdog` lets the trial flow run there
    let no_watchdog = has("bz.nowatchdog");
    TRIAL.store(true, Ordering::Relaxed);
    let pm = f.compatible("brcm,bcm2835-pm-wdt").next().and_then(|i| f.reg(i).first().copied());
    if let Some(va) = pm.and_then(|(pa, _)| crate::memory::map_mmio(pa, 0x1000)) {
        crate::power::use_pm_watchdog(va);
        if !no_watchdog && crate::power::watchdog_arm(WATCHDOG_S) {
            GUARDED.store(true, Ordering::Relaxed);
            LAST_FEED.store(crate::arch::cpu::timestamp(), Ordering::Relaxed);
        }
    }
    crate::println!(
        "[tryboot] trial boot of a new kernel: watchdog {} -- 'pi promote' keeps this kernel, a hang or a restart goes back to the old one",
        if GUARDED.load(Ordering::Relaxed) { "armed (15 s)" } else { "NOT available" }
    );
}

/// Keep the watchdog quiet while the system makes progress (the desktop loop
/// and the boot's long steps call this). Cheap: feeds at most every second.
pub fn feed() {
    if !GUARDED.load(Ordering::Relaxed) {
        return;
    }
    let now = crate::arch::cpu::timestamp();
    if now - LAST_FEED.load(Ordering::Relaxed) > crate::arch::cpu::counter_hz() {
        LAST_FEED.store(now, Ordering::Relaxed);
        crate::power::watchdog_arm(WATCHDOG_S);
    }
}

fn config_text() -> Result<String, &'static str> {
    let c = crate::vfs::read("/disk/CONFIG.TXT").map_err(|_| "no /disk/CONFIG.TXT (is /disk the Pi's SD card?)")?;
    String::from_utf8(c).map_err(|_| "CONFIG.TXT is not text")
}

/// Shell `pi ...`. The answer also goes to the serial log: a board under test
/// is often watched through its serial cable only.
pub fn command(args: &[&str]) -> Vec<String> {
    let out = run(args);
    for l in &out {
        crate::println!("[pi] {}", l);
    }
    out
}

fn run(args: &[&str]) -> Vec<String> {
    match args.first().copied() {
        None | Some("status") => status(),
        Some("tryboot") => match prepare() {
            Ok(()) => {
                crate::println!("[tryboot] restarting into KERNTRY.IMG (one trial boot)");
                let mut flag = [1u32];
                if !crate::arch::aarch64::rpi::firmware_property(SET_REBOOT_FLAGS, &mut flag) {
                    return alloc::vec![String::from("pi tryboot: the firmware did not take the tryboot flag (an older bootloader?)")];
                }
                crate::power::restart()
            }
            Err(e) => alloc::vec![format!("pi tryboot: {}", e)],
        },
        Some("promote") => promote(),
        Some("watchdog-test") => {
            // proves the reset path: arm for 3 s, then hang with interrupts off
            crate::println!("[tryboot] watchdog test: arming 3 s, then hanging with interrupts off -- the board must reset");
            if !crate::power::watchdog_arm(3) {
                return alloc::vec![String::from("pi watchdog-test: no PM watchdog on this machine")];
            }
            crate::arch::cpu::disable_interrupts();
            let hz = crate::arch::cpu::counter_hz();
            let t0 = crate::arch::cpu::timestamp();
            while crate::arch::cpu::timestamp() - t0 < 10 * hz {
                core::hint::spin_loop();
            }
            crate::arch::cpu::enable_interrupts();
            crate::power::watchdog_stop();
            alloc::vec![String::from("pi watchdog-test: FAILED -- still running 10 s after the watchdog was due")]
        }
        Some(_) => alloc::vec![String::from("pi [status | tryboot | promote | watchdog-test]  -- try a new kernel (/disk/KERNTRY.IMG) once, keep it if it works")],
    }
}

fn status() -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!(
        "boot: {}",
        if trial() {
            if GUARDED.load(Ordering::Relaxed) {
                "TRIAL of a new kernel (watchdog armed) -- 'pi promote' keeps it"
            } else {
                "TRIAL of a new kernel (no watchdog)"
            }
        } else {
            "normal (config.txt)"
        }
    ));
    for name in ["KERNEL8.IMG", "KERNTRY.IMG", "TRYBOOT.TXT"] {
        match crate::vfs::stat(&format!("/disk/{}", name)) {
            Ok(e) => out.push(format!("  /disk/{:<12} {} bytes", name, e.size)),
            Err(_) => out.push(format!("  /disk/{:<12} -", name)),
        }
    }
    out
}

/// TRYBOOT.TXT = config.txt + the trial kernel and its boot arguments (the
/// last `kernel=` wins), CMDTRY.TXT = the boot argument.
fn prepare() -> Result<(), &'static str> {
    let k = crate::vfs::stat("/disk/KERNTRY.IMG").map_err(|_| "no /disk/KERNTRY.IMG (upload the new kernel first: bz pi update)")?;
    if k.size < 64 * 1024 {
        return Err("KERNTRY.IMG is too small to be a kernel");
    }
    let mut t = config_text()?;
    t.push_str("\n[all]\n# trial boot (pi tryboot): used once, then config.txt again\nkernel=KERNTRY.IMG\ncmdline=CMDTRY.TXT\n");
    crate::vfs::write("/disk/TRYBOOT.TXT", t.as_bytes())?;
    crate::vfs::write("/disk/CMDTRY.TXT", BOOTARG.as_bytes())?;
    Ok(())
}

/// Keep the trial kernel: stop the watchdog, copy it over KERNEL8.IMG.
fn promote() -> Vec<String> {
    if !trial() {
        return alloc::vec![String::from("pi promote: this is not a trial boot ('pi tryboot' first)")];
    }
    crate::power::watchdog_stop();
    GUARDED.store(false, Ordering::Relaxed);
    let data = match crate::vfs::read("/disk/KERNTRY.IMG") {
        Ok(d) => d,
        Err(e) => return alloc::vec![format!("pi promote: {}", e)],
    };
    if let Err(e) = crate::vfs::write("/disk/KERNEL8.IMG", &data) {
        return alloc::vec![format!("pi promote: writing KERNEL8.IMG failed: {} -- the old kernel stays", e)];
    }
    // read back: a short write must not become the next normal boot
    match crate::vfs::read("/disk/KERNEL8.IMG") {
        Ok(back) if back == data => {
            let _ = crate::vfs::remove("/disk/KERNTRY.IMG");
            crate::println!("[tryboot] promoted: KERNEL8.IMG is the new kernel ({} bytes)", data.len());
            alloc::vec![format!("pi promote: KERNEL8.IMG is now this kernel ({} bytes); the watchdog is off", data.len())]
        }
        _ => alloc::vec![String::from("pi promote: KERNEL8.IMG did not read back correctly -- restore it from the PC before restarting")],
    }
}
