//! The Supervisor Binary Interface: the kernel's calls into the M-mode
//! firmware (OpenSBI on QEMU `virt`). `ecall` with a7 = extension id,
//! a6 = function id, a0..a5 = arguments; the firmware returns (error, value)
//! in a0/a1.

use core::arch::asm;

/// Legacy (v0.1) console putchar -- usable before any device is mapped.
const EXT_LEGACY_PUTCHAR: u64 = 0x01;
/// Base extension (spec / implementation version, probe).
pub const EXT_BASE: u64 = 0x10;
/// Timer extension ("TIME").
pub const EXT_TIME: u64 = 0x5449_4D45;
/// Hart state management ("HSM").
pub const EXT_HSM: u64 = 0x0048_534D;
/// System reset ("SRST").
pub const EXT_SRST: u64 = 0x5352_5354;
/// Remote fence ("RFNC").
pub const EXT_RFENCE: u64 = 0x5246_4E43;
/// Inter-processor interrupts ("sPI").
pub const EXT_IPI: u64 = 0x0073_5049;

/// One SBI call: returns (error, value). Error 0 = success.
pub fn call(ext: u64, fid: u64, a0: u64, a1: u64, a2: u64) -> (i64, u64) {
    let (err, val): (i64, u64);
    unsafe {
        asm!(
            "ecall",
            inlateout("a0") a0 => err,
            inlateout("a1") a1 => val,
            in("a2") a2,
            in("a6") fid,
            in("a7") ext,
            options(nostack)
        );
    }
    (err, val)
}

/// An SBI call with four arguments (a0..a3), e.g. RFENCE.
pub fn call4(ext: u64, fid: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> (i64, u64) {
    let (err, val): (i64, u64);
    unsafe {
        asm!(
            "ecall",
            inlateout("a0") a0 => err,
            inlateout("a1") a1 => val,
            in("a2") a2,
            in("a3") a3,
            in("a6") fid,
            in("a7") ext,
            options(nostack)
        );
    }
    (err, val)
}

/// Write one byte through the firmware's console (legacy extension).
pub fn console_putchar(b: u8) {
    unsafe {
        asm!("ecall", inlateout("a0") b as u64 => _, in("a7") EXT_LEGACY_PUTCHAR, options(nostack));
    }
}

/// Program the next timer interrupt at absolute `time` counter value.
pub fn set_timer(time: u64) {
    call(EXT_TIME, 0, time, 0, 0);
}

/// True if the firmware implements extension `ext`.
pub fn probe(ext: u64) -> bool {
    matches!(call(EXT_BASE, 3, ext, 0, 0), (0, v) if v != 0)
}

/// (spec major, spec minor, implementation id, implementation version).
pub fn version() -> (u64, u64, u64, u64) {
    let spec = call(EXT_BASE, 0, 0, 0, 0).1;
    let imp = call(EXT_BASE, 1, 0, 0, 0).1;
    let ver = call(EXT_BASE, 2, 0, 0, 0).1;
    ((spec >> 24) & 0x7F, spec & 0xFF_FFFF, imp, ver)
}
