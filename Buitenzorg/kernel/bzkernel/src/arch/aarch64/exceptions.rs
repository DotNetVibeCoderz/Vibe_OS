//! The EL1 exception vector table (VBAR_EL1): 16 entries of 0x80 bytes —
//! {synchronous, IRQ, FIQ, SError} × {current EL with SP_EL0, current EL with
//! SP_ELx, lower EL AArch64, lower EL AArch32}. Each entry reserves a 256-byte
//! frame, saves x0/x1, and jumps to a common path that saves x2..x30 and calls
//! [`bz_exception`] with the entry index; returning restores and `eret`s.

use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicU64, Ordering};

global_asm!(
    r#"
.section .text.bz_vectors, "ax"
.macro BZ_VEC idx
    .balign 0x80
    sub sp, sp, #256
    stp x0, x1, [sp, #0]
    mov x0, #\idx
    b bz_exc_common
.endm

.balign 2048
.global bz_vectors
bz_vectors:
    BZ_VEC 0
    BZ_VEC 1
    BZ_VEC 2
    BZ_VEC 3
    BZ_VEC 4
    BZ_VEC 5
    BZ_VEC 6
    BZ_VEC 7
    BZ_VEC 8
    BZ_VEC 9
    BZ_VEC 10
    BZ_VEC 11
    BZ_VEC 12
    BZ_VEC 13
    BZ_VEC 14
    BZ_VEC 15

bz_exc_common:
    stp x2, x3, [sp, #16]
    stp x4, x5, [sp, #32]
    stp x6, x7, [sp, #48]
    stp x8, x9, [sp, #64]
    stp x10, x11, [sp, #80]
    stp x12, x13, [sp, #96]
    stp x14, x15, [sp, #112]
    stp x16, x17, [sp, #128]
    stp x18, x19, [sp, #144]
    stp x20, x21, [sp, #160]
    stp x22, x23, [sp, #176]
    stp x24, x25, [sp, #192]
    stp x26, x27, [sp, #208]
    stp x28, x29, [sp, #224]
    str x30, [sp, #240]
    mov x1, sp
    bl bz_exception
    ldp x2, x3, [sp, #16]
    ldp x4, x5, [sp, #32]
    ldp x6, x7, [sp, #48]
    ldp x8, x9, [sp, #64]
    ldp x10, x11, [sp, #80]
    ldp x12, x13, [sp, #96]
    ldp x14, x15, [sp, #112]
    ldp x16, x17, [sp, #128]
    ldp x18, x19, [sp, #144]
    ldp x20, x21, [sp, #160]
    ldp x22, x23, [sp, #176]
    ldp x24, x25, [sp, #192]
    ldp x26, x27, [sp, #208]
    ldp x28, x29, [sp, #224]
    ldr x30, [sp, #240]
    ldp x0, x1, [sp, #0]
    add sp, sp, #256
    eret
"#
);

extern "C" {
    static bz_vectors: u8;
}

static IRQS: AtomicU64 = AtomicU64::new(0);
static IRQS_FROM_EL0: AtomicU64 = AtomicU64::new(0);
static SVCS: AtomicU64 = AtomicU64::new(0);

/// Install the vector table on this CPU.
///
/// Limine may enter the kernel in EL1t (running on SP_EL0). An exception
/// taken at EL1 always switches to SP_EL1, which would then be whatever the
/// loader left there — the first IRQ faulted writing its frame past the end
/// of the kernel image. So move onto SP_EL1 first, with the very same stack
/// pointer value (the running Rust frame stays valid).
pub fn init() {
    super::context::enable_fp();
    unsafe {
        asm!(
            "mrs {t}, spsel",
            "cbnz {t}, 1f",
            "mov {t}, sp",
            "msr spsel, #1",
            "mov sp, {t}",
            "1:",
            t = out(reg) _,
            options(nostack)
        );
        let base = &bz_vectors as *const u8 as u64;
        asm!("msr vbar_el1, {}", "isb", in(reg) base, options(nostack));
    }
}

/// Interrupts taken so far.
pub fn irq_count() -> u64 {
    IRQS.load(Ordering::Relaxed)
}

/// (interrupts taken while EL0 code ran, SVC calls served).
pub fn el0_stats() -> (u64, u64) {
    (IRQS_FROM_EL0.load(Ordering::Relaxed), SVCS.load(Ordering::Relaxed))
}

const EC_SVC64: u64 = 0x15;

const KINDS: [&str; 4] = ["synchronous", "IRQ", "FIQ", "SError"];
const ORIGINS: [&str; 4] = ["EL1 (SP_EL0)", "EL1", "EL0 (AArch64)", "EL0 (AArch32)"];

/// `frame` holds x0..x30 of the interrupted code (x0 at index 0); the
/// syscall result is written back into x0.
#[no_mangle]
extern "C" fn bz_exception(index: u64, frame: *mut u64) {
    let kind = (index & 3) as usize;
    let from_el0 = index >= 8;
    if kind == 1 {
        IRQS.fetch_add(1, Ordering::Relaxed);
        if from_el0 {
            IRQS_FROM_EL0.fetch_add(1, Ordering::Relaxed);
        }
        super::gic::handle_irq();
        return;
    }
    let (esr, elr, far): (u64, u64, u64);
    unsafe {
        asm!("mrs {}, esr_el1", out(reg) esr, options(nomem, nostack));
        asm!("mrs {}, elr_el1", out(reg) elr, options(nomem, nostack));
        asm!("mrs {}, far_el1", out(reg) far, options(nomem, nostack));
    }
    if from_el0 && kind == 0 {
        if esr >> 26 == EC_SVC64 {
            SVCS.fetch_add(1, Ordering::Relaxed);
            let r = unsafe {
                let f = frame;
                crate::el0_syscall(f.add(8).read(), f.read(), f.add(1).read(), f.add(2).read())
            };
            unsafe { frame.write(r) };
            return;
        }
        // Any other synchronous exception from user code (bad access,
        // undefined instruction, ...) ends that program, not the kernel.
        crate::el0_fault(esr, far, elr);
    }
    panic!(
        "{} exception from {}: ESR_EL1={:#x} (EC={:#x}) ELR_EL1={:#x} FAR_EL1={:#x}",
        KINDS[kind],
        ORIGINS[(index >> 2) as usize & 3],
        esr,
        esr >> 26,
        elr,
        far
    );
}
