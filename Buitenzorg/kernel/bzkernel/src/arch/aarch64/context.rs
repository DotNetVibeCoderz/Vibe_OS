//! Kernel-thread context switching on AArch64 (the `arch::context` HAL,
//! same API as x86): a switch saves the callee-saved registers x19..x30, the
//! interrupt mask (DAIF) and SP on the old stack, records SP, and resumes the
//! new stack. The kernel itself is soft-float; FP/SIMD state (q0..q31, FPCR,
//! FPSR) belongs to user code and is saved/restored per task like FXSAVE on
//! x86 — CPACR_EL1.FPEN is opened at boot so EL0 can use it.

use alloc::boxed::Box;
use core::arch::{asm, global_asm};

global_asm!(
    r#"
.arch armv8-a+fp+simd
.section .text.bz_context, "ax"

// bz_switch_stacks(prev_sp: *mut u64, next_sp: u64)
.global bz_switch_stacks
bz_switch_stacks:
    sub sp, sp, #112
    stp x19, x20, [sp, #0]
    stp x21, x22, [sp, #16]
    stp x23, x24, [sp, #32]
    stp x25, x26, [sp, #48]
    stp x27, x28, [sp, #64]
    stp x29, x30, [sp, #80]
    mrs x9, daif
    str x9, [sp, #96]
    mov x9, sp
    str x9, [x0]
    mov sp, x1
    ldr x9, [sp, #96]
    ldp x19, x20, [sp, #0]
    ldp x21, x22, [sp, #16]
    ldp x23, x24, [sp, #32]
    ldp x25, x26, [sp, #48]
    ldp x27, x28, [sp, #64]
    ldp x29, x30, [sp, #80]
    add sp, sp, #112
    msr daif, x9
    ret

// bz_fp_save(area: *mut FpuState) / bz_fp_restore(area: *const FpuState)
.global bz_fp_save
bz_fp_save:
    stp q0, q1, [x0, #0]
    stp q2, q3, [x0, #32]
    stp q4, q5, [x0, #64]
    stp q6, q7, [x0, #96]
    stp q8, q9, [x0, #128]
    stp q10, q11, [x0, #160]
    stp q12, q13, [x0, #192]
    stp q14, q15, [x0, #224]
    stp q16, q17, [x0, #256]
    stp q18, q19, [x0, #288]
    stp q20, q21, [x0, #320]
    stp q22, q23, [x0, #352]
    stp q24, q25, [x0, #384]
    stp q26, q27, [x0, #416]
    stp q28, q29, [x0, #448]
    stp q30, q31, [x0, #480]
    mrs x9, fpcr
    mrs x10, fpsr
    str x9, [x0, #512]
    str x10, [x0, #520]
    ret

.global bz_fp_restore
bz_fp_restore:
    ldp q0, q1, [x0, #0]
    ldp q2, q3, [x0, #32]
    ldp q4, q5, [x0, #64]
    ldp q6, q7, [x0, #96]
    ldp q8, q9, [x0, #128]
    ldp q10, q11, [x0, #160]
    ldp q12, q13, [x0, #192]
    ldp q14, q15, [x0, #224]
    ldp q16, q17, [x0, #256]
    ldp q18, q19, [x0, #288]
    ldp q20, q21, [x0, #320]
    ldp q22, q23, [x0, #352]
    ldp q24, q25, [x0, #384]
    ldp q26, q27, [x0, #416]
    ldp q28, q29, [x0, #448]
    ldp q30, q31, [x0, #480]
    ldr x9, [x0, #512]
    ldr x10, [x0, #520]
    msr fpcr, x9
    msr fpsr, x10
    ret

// Test helpers: write / read d0 (the low half of q0).
.global bz_fp_set_d0
bz_fp_set_d0:
    fmov d0, x0
    ret
.global bz_fp_get_d0
bz_fp_get_d0:
    fmov x0, d0
    ret
"#
);

extern "C" {
    fn bz_switch_stacks(prev_sp: *mut u64, next_sp: u64);
    fn bz_fp_save(area: *mut FpuState);
    fn bz_fp_restore(area: *const FpuState);
    fn bz_fp_set_d0(v: u64);
    fn bz_fp_get_d0() -> u64;
}

/// FP/SIMD state of a task: q0..q31 then FPCR, FPSR.
#[repr(C, align(16))]
pub struct FpuState([u8; 528]);

impl FpuState {
    pub fn clean() -> Box<FpuState> {
        Box::new(FpuState([0; 528]))
    }
}

/// Let EL0 and EL1 execute FP/SIMD instructions (CPACR_EL1.FPEN = 0b11).
pub fn enable_fp() {
    unsafe {
        let mut v: u64;
        asm!("mrs {}, cpacr_el1", out(reg) v, options(nomem, nostack));
        v |= 3 << 20;
        asm!("msr cpacr_el1, {}", "isb", in(reg) v, options(nostack));
    }
}

/// Build the first frame of a new kernel thread on `stack`: `ret` lands in
/// `entry` with interrupts masked (the scheduler's trampoline enables them,
/// as on x86). Returns the initial stack pointer.
pub fn prepare_stack(stack: &[u8], entry: extern "C" fn() -> !) -> u64 {
    let top = (stack.as_ptr() as u64 + stack.len() as u64) & !0xF;
    let sp = top - 112;
    unsafe {
        let f = sp as *mut u64;
        for i in 0..14 {
            f.add(i).write(0);
        }
        f.add(11).write(entry as *const () as u64); // x30 (lr)
        f.add(12).write(0x3C0); // DAIF: everything masked
    }
    sp
}

/// Save this task's FP state into `prev_fpu`, load `next_fpu`, and switch
/// stacks. Returns when the old task is resumed.
///
/// # Safety
/// Both stack pointers and FP areas must belong to live tasks.
pub unsafe fn switch(prev_sp: *mut u64, next_sp: u64, prev_fpu: *mut FpuState, next_fpu: *const FpuState) {
    unsafe {
        bz_fp_save(prev_fpu);
        bz_fp_restore(next_fpu);
        bz_switch_stacks(prev_sp, next_sp);
    }
}

/// Test helpers for the context self-test (FP state must follow the task).
pub fn fp_set_d0(v: u64) {
    unsafe { bz_fp_set_d0(v) }
}
pub fn fp_get_d0() -> u64 {
    unsafe { bz_fp_get_d0() }
}
