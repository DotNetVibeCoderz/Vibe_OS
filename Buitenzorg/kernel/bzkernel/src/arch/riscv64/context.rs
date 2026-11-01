//! Kernel-thread context switching on RISC-V (the `arch::context` HAL, same
//! API as x86 and ARM64): a switch saves `ra`, `s0`..`s11` and the interrupt
//! enable bit (`sstatus.SIE`) on the old stack, records `sp`, and resumes the
//! new stack. The kernel is soft-float (`riscv64imac`); the F/D registers
//! (f0..f31 + fcsr) belong to user code and are saved/restored per task.
//! `sstatus.FS` is switched on at boot ([`enable_fp`]) so those instructions
//! (and U-mode FP code) do not trap.

use alloc::boxed::Box;
use core::arch::{asm, global_asm};

global_asm!(
    r#"
.section .text.bz_context, "ax"
.option push
.option arch, +f, +d

// bz_switch_stacks(prev_sp: *mut u64, next_sp: u64)
.global bz_switch_stacks
bz_switch_stacks:
    addi sp, sp, -112
    sd ra, 0(sp)
    sd s0, 8(sp)
    sd s1, 16(sp)
    sd s2, 24(sp)
    sd s3, 32(sp)
    sd s4, 40(sp)
    sd s5, 48(sp)
    sd s6, 56(sp)
    sd s7, 64(sp)
    sd s8, 72(sp)
    sd s9, 80(sp)
    sd s10, 88(sp)
    sd s11, 96(sp)
    csrr t0, sstatus
    andi t0, t0, 2
    sd t0, 104(sp)
    sd sp, 0(a0)
    mv sp, a1
    ld ra, 0(sp)
    ld s0, 8(sp)
    ld s1, 16(sp)
    ld s2, 24(sp)
    ld s3, 32(sp)
    ld s4, 40(sp)
    ld s5, 48(sp)
    ld s6, 56(sp)
    ld s7, 64(sp)
    ld s8, 72(sp)
    ld s9, 80(sp)
    ld s10, 88(sp)
    ld s11, 96(sp)
    ld t0, 104(sp)
    addi sp, sp, 112
    csrci sstatus, 2
    csrs sstatus, t0
    ret

// bz_fp_save(area) / bz_fp_restore(area): f0..f31 then fcsr.
.global bz_fp_save
bz_fp_save:
    .irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
    fsd f\n, (\n*8)(a0)
    .endr
    frcsr t0
    sd t0, 256(a0)
    ret

.global bz_fp_restore
bz_fp_restore:
    .irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
    fld f\n, (\n*8)(a0)
    .endr
    ld t0, 256(a0)
    fscsr t0
    ret

// Test helpers: write / read f0 as raw bits.
.global bz_fp_set_f0
bz_fp_set_f0:
    fmv.d.x f0, a0
    ret
.global bz_fp_get_f0
bz_fp_get_f0:
    fmv.x.d a0, f0
    ret
.option pop
"#
);

extern "C" {
    fn bz_switch_stacks(prev_sp: *mut u64, next_sp: u64);
    fn bz_fp_save(area: *mut FpuState);
    fn bz_fp_restore(area: *const FpuState);
    fn bz_fp_set_f0(v: u64);
    fn bz_fp_get_f0() -> u64;
}

/// FP state of a task: f0..f31 then fcsr.
#[repr(C, align(16))]
pub struct FpuState([u8; 272]);

impl FpuState {
    pub fn clean() -> Box<FpuState> {
        Box::new(FpuState([0; 272]))
    }
}

/// Switch the FP unit on (`sstatus.FS` = Initial) for this hart.
pub fn enable_fp() {
    unsafe { asm!("csrs sstatus, {}", in(reg) 1u64 << 13, options(nomem, nostack)) };
}

/// Build the first frame of a new kernel thread on `stack`: `ret` lands in
/// `entry` with interrupts disabled (the scheduler's trampoline enables them,
/// as on x86). Returns the initial stack pointer.
pub fn prepare_stack(stack: &[u8], entry: extern "C" fn() -> !) -> u64 {
    let top = (stack.as_ptr() as u64 + stack.len() as u64) & !0xF;
    let sp = top - 112;
    unsafe {
        let f = sp as *mut u64;
        for i in 0..14 {
            f.add(i).write(0);
        }
        f.write(entry as *const () as u64); // ra
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
pub fn fp_set_f0(v: u64) {
    unsafe { bz_fp_set_f0(v) }
}
pub fn fp_get_f0() -> u64 {
    unsafe { bz_fp_get_f0() }
}
