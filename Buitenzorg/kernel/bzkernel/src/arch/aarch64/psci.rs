//! PSCI (Arm Power State Coordination Interface): the firmware call that
//! powers the machine off or resets it (and, later, starts secondary cores).
//! The Device Tree's `/psci` node says whether calls go through `hvc` (to
//! EL2, QEMU `virt` without EL3) or `smc` (to EL3 firmware).

use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

pub const SYSTEM_OFF: u32 = 0x8400_0008;
pub const SYSTEM_RESET: u32 = 0x8400_0009;
pub const PSCI_VERSION: u32 = 0x8400_0000;

static USE_SMC: AtomicBool = AtomicBool::new(false);
static KNOWN: AtomicBool = AtomicBool::new(false);

/// Record the conduit from the DTB (`method = "hvc"` or `"smc"`).
pub fn init(method: &str) {
    USE_SMC.store(method == "smc", Ordering::SeqCst);
    KNOWN.store(method == "smc" || method == "hvc", Ordering::SeqCst);
}

pub fn available() -> bool {
    KNOWN.load(Ordering::SeqCst)
}

/// Call PSCI function `fid` with up to three arguments; returns x0.
pub fn call(fid: u32, a1: u64, a2: u64, a3: u64) -> u64 {
    let mut x0 = fid as u64;
    unsafe {
        if USE_SMC.load(Ordering::SeqCst) {
            asm!("smc #0", inout("x0") x0, in("x1") a1, in("x2") a2, in("x3") a3, options(nostack));
        } else {
            asm!("hvc #0", inout("x0") x0, in("x1") a1, in("x2") a2, in("x3") a3, options(nostack));
        }
    }
    x0
}
