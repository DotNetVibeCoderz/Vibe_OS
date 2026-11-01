//! U-mode (user mode) on RISC-V -- the counterpart of x86 `usermode.rs` and
//! the ARM64 EL0 code:
//!
//! - [`enter_user_with`] pushes the kernel's callee-saved registers on the
//!   current kernel stack and records that stack pointer in `kctx`; the 16
//!   bytes just below it become the **trap stack top** (`sscratch`, holding
//!   the CPU index for `tp`), so traps from this program land right under the
//!   saved frame. Then `sepc` = entry, `sstatus.SPP` = U, `SPIE` = 1, every
//!   other register cleared, `sret`;
//! - [`return_to_kernel`] (EXIT, or a fatal fault) jumps back to that saved
//!   stack, so `enter_user_with` returns the exit code.
//!
//! Syscalls: `ECALL`, a7 = number, a0..a2 = arguments, result in a0 (the
//! Linux RISC-V convention, bz-abi numbers).

use core::arch::{asm, global_asm};

global_asm!(
    r#"
.section .text.bz_user, "ax"

// bz_enter_user(entry, user_sp, arg, kctx_ptr) -> exit code
.global bz_enter_user
bz_enter_user:
    csrci sstatus, 2             // no trap until sret: sscratch is armed below
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
    sd sp, 0(a3)
    addi t0, sp, -16             // trap stack top; [t0] = CPU index
    sd tp, 0(t0)
    csrw sscratch, t0
    csrw sepc, a0
    li t1, 0x100                 // SPP = 0: return to U-mode
    csrc sstatus, t1
    li t1, 0x20                  // SPIE: SIE after sret (S-mode interrupts are
                                 // always taken while the hart is in U-mode)
    csrs sstatus, t1
    mv sp, a1
    mv a0, a2
    li ra, 0
    li gp, 0
    li tp, 0
    li t0, 0
    li t1, 0
    li t2, 0
    li s0, 0
    li s1, 0
    li a1, 0
    li a2, 0
    li a3, 0
    li a4, 0
    li a5, 0
    li a6, 0
    li a7, 0
    li s2, 0
    li s3, 0
    li s4, 0
    li s5, 0
    li s6, 0
    li s7, 0
    li s8, 0
    li s9, 0
    li s10, 0
    li s11, 0
    li t3, 0
    li t4, 0
    li t5, 0
    li t6, 0
    sret

// bz_return_to_kernel(code, kctx) -> ! : back onto the saved kernel stack.
.global bz_return_to_kernel
bz_return_to_kernel:
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
    addi sp, sp, 112
    ret

// ---- U-mode demo program 1: print, query the ABI, spin (timer interrupts
// arrive in U-mode), compute 1+..+100 (passed through the FP unit), exit
// with (abi << 16) + 5050. Position-independent: copied to a user page.
.option push
.option norelax
.option arch, +f, +d
.balign 16
.global bz_user_demo_start
bz_user_demo_start:
    lla a0, 1f
    lla a1, 2f
    sub a1, a1, a0
    li a7, 1                     // DEBUG_WRITE
    ecall
    li a7, 0                     // ABI_VERSION
    ecall
    mv s0, a0
    li t0, 0x2000000             // ~32M iterations of busy work
3:  addi t0, t0, -1
    bnez t0, 3b
    li t1, 0
    li t2, 100
4:  add t1, t1, t2
    addi t2, t2, -1
    bnez t2, 4b
    fcvt.d.l ft0, t1             // through the FP unit and back
    fadd.d ft0, ft0, ft0
    fcvt.l.d t1, ft0
    srli t1, t1, 1
    slli a0, s0, 16
    add a0, a0, t1
    li a7, 2                     // EXIT
    ecall
5:  j 5b
1:  .ascii "Hello from U-mode (RISC-V user mode)!\n"
2:
.balign 16
.global bz_user_demo_end
bz_user_demo_end:

// ---- U-mode demo program 2: read a kernel address (must fault).
.global bz_user_fault_start
bz_user_fault_start:
    li a0, 0xffffd00000000000    // the kernel heap
    ld a1, 0(a0)
    li a0, 0                     // reached only if the read succeeded
    li a7, 2
    ecall
6:  j 6b
.balign 16
.global bz_user_fault_end
bz_user_fault_end:
.option pop
"#
);

extern "C" {
    fn bz_enter_user(entry: u64, user_sp: u64, arg: u64, kctx: *mut u64) -> u64;
    fn bz_return_to_kernel(code: u64, kctx: u64) -> !;
    static bz_user_demo_start: u8;
    static bz_user_demo_end: u8;
    static bz_user_fault_start: u8;
    static bz_user_fault_end: u8;
}

/// Run user code at `entry(arg)` on `user_stack_top` until it exits; `kctx`
/// receives the kernel stack pointer [`return_to_kernel`] resumes. Returns the
/// exit code (interrupts are left disabled).
pub fn enter_user_with(entry: u64, user_stack_top: u64, arg: u64, kctx: *mut u64) -> u64 {
    unsafe { bz_enter_user(entry, user_stack_top, arg, kctx) }
}

/// Resume the kernel where `enter_user_with` saved `kctx`, returning `code`.
pub fn return_to_kernel(code: u64, kctx: u64) -> ! {
    unsafe { bz_return_to_kernel(code, kctx) }
}

fn bytes(start: &u8, end: &u8) -> &'static [u8] {
    let (s, e) = (start as *const u8 as usize, end as *const u8 as usize);
    unsafe { core::slice::from_raw_parts(s as *const u8, e - s) }
}

/// Machine code of the self-test's U-mode programs: (demo, fault).
pub fn demo_programs() -> (&'static [u8], &'static [u8]) {
    unsafe {
        (
            bytes(&bz_user_demo_start, &bz_user_demo_end),
            bytes(&bz_user_fault_start, &bz_user_fault_end),
        )
    }
}

/// Make freshly written code visible to instruction fetch on this hart.
pub fn sync_icache() {
    unsafe { asm!("fence.i", options(nostack)) }
}
