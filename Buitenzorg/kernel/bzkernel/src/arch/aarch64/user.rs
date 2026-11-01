//! EL0 (user mode) on AArch64.
//!
//! [`enter_user`] saves the kernel's callee-saved registers and SP in a
//! [`KernelCtx`] (setjmp), points SP_EL0 at the user stack and ELR_EL1 at the
//! entry, clears every general register (no kernel values leak to user
//! code), and `eret`s to EL0t with interrupts enabled. The program comes back
//! through an exception: `SVC #0` with the syscall number in x8 and arguments
//! in x0..x2 (result in x0) — the Linux arm64 convention — and the EXIT call
//! (or a fatal fault) uses [`return_to_kernel`] (longjmp), which makes
//! `enter_user` return the exit code. Exceptions from EL0 run on SP_EL1, the
//! kernel stack below the `enter_user` frame.
//!
//! Also here: two tiny EL0 programs in assembly for the self-test (position
//! independent; the kernel copies them into a user page).

use core::arch::{asm, global_asm};

global_asm!(
    r#"
.section .text.bz_user, "ax"

// bz_enter_user(entry: u64, user_sp: u64, ctx: *mut KernelCtx) -> u64
.global bz_enter_user
bz_enter_user:
    stp x19, x20, [x2, #0]
    stp x21, x22, [x2, #16]
    stp x23, x24, [x2, #32]
    stp x25, x26, [x2, #48]
    stp x27, x28, [x2, #64]
    stp x29, x30, [x2, #80]
    mov x9, sp
    str x9, [x2, #96]
    msr sp_el0, x1
    msr elr_el1, x0
    msr spsr_el1, xzr          // EL0t, D/A/I/F unmasked
    mov x0, #0
    mov x1, #0
    mov x2, #0
    mov x3, #0
    mov x4, #0
    mov x5, #0
    mov x6, #0
    mov x7, #0
    mov x8, #0
    mov x9, #0
    mov x10, #0
    mov x11, #0
    mov x12, #0
    mov x13, #0
    mov x14, #0
    mov x15, #0
    mov x16, #0
    mov x17, #0
    mov x18, #0
    mov x19, #0
    mov x20, #0
    mov x21, #0
    mov x22, #0
    mov x23, #0
    mov x24, #0
    mov x25, #0
    mov x26, #0
    mov x27, #0
    mov x28, #0
    mov x29, #0
    mov x30, #0
    eret

// bz_return_to_kernel(ctx: *const KernelCtx, code: u64) -> !
.global bz_return_to_kernel
bz_return_to_kernel:
    ldp x19, x20, [x0, #0]
    ldp x21, x22, [x0, #16]
    ldp x23, x24, [x0, #32]
    ldp x25, x26, [x0, #48]
    ldp x27, x28, [x0, #64]
    ldp x29, x30, [x0, #80]
    ldr x9, [x0, #96]
    mov sp, x9
    mov x0, x1
    ret

// ---- EL0 demo program 1: print, query the ABI, spin (timer IRQs arrive in
// EL0), compute 1+..+100, exit with (abi << 16) + 5050.
.balign 16
.global bz_user_demo_start
bz_user_demo_start:
    adr x0, 1f
    adr x1, 2f
    sub x1, x1, x0
    mov x8, #1                  // DEBUG_WRITE
    svc #0
    mov x8, #0                  // ABI_VERSION
    svc #0
    mov x19, x0
    movz x9, #0x200, lsl #16    // ~32M iterations of busy work (several timer ticks)
3:  subs x9, x9, #1
    b.ne 3b
    mov x10, #0
    mov x11, #100
4:  add x10, x10, x11
    subs x11, x11, #1
    b.ne 4b
    add x0, x10, x19, lsl #16
    mov x8, #2                  // EXIT
    svc #0
5:  b 5b
1:  .ascii "Hello from EL0 (ARM64 user mode)!\n"
2:
.balign 16
.global bz_user_demo_end
bz_user_demo_end:

// ---- EL0 demo program 2: read a kernel address (must fault, not succeed).
.global bz_user_fault_start
bz_user_fault_start:
    movz x0, #0xffff, lsl #48
    movk x0, #0xd000, lsl #32   // the kernel heap (TTBR1)
    ldr x1, [x0]
    mov x0, #0                  // reached only if the read succeeded
    mov x8, #2
    svc #0
6:  b 6b
.balign 16
.global bz_user_fault_end
bz_user_fault_end:
"#
);

extern "C" {
    fn bz_enter_user(entry: u64, user_sp: u64, ctx: *mut KernelCtx) -> u64;
    fn bz_return_to_kernel(ctx: *const KernelCtx, code: u64) -> !;
    static bz_user_demo_start: u8;
    static bz_user_demo_end: u8;
    static bz_user_fault_start: u8;
    static bz_user_fault_end: u8;
}

/// Kernel registers saved by `enter_user` (x19..x30, SP).
#[repr(C)]
pub struct KernelCtx([u64; 14]);

impl KernelCtx {
    pub const fn new() -> KernelCtx {
        KernelCtx([0; 14])
    }
}

/// Run user code at `entry` with stack `user_sp` until it exits; returns the
/// exit code. Interrupts are enabled again on return.
pub fn enter_user(entry: u64, user_sp: u64, ctx: &mut KernelCtx) -> u64 {
    let code = unsafe { bz_enter_user(entry, user_sp, ctx) };
    unsafe { asm!("msr daifclr, #2", options(nomem, nostack)) };
    code
}

/// Leave the user program (EXIT syscall or a fatal fault): resume the kernel
/// where `enter_user` was called, which returns `code`.
pub fn return_to_kernel(ctx: &KernelCtx, code: u64) -> ! {
    unsafe { bz_return_to_kernel(ctx, code) }
}

fn bytes(start: &u8, end: &u8) -> &'static [u8] {
    let (s, e) = (start as *const u8 as usize, end as *const u8 as usize);
    unsafe { core::slice::from_raw_parts(s as *const u8, e - s) }
}

/// Machine code of the self-test's EL0 programs: (demo, fault).
pub fn demo_programs() -> (&'static [u8], &'static [u8]) {
    unsafe {
        (
            bytes(&bz_user_demo_start, &bz_user_demo_end),
            bytes(&bz_user_fault_start, &bz_user_fault_end),
        )
    }
}

/// Make freshly written code visible to instruction fetch.
pub fn sync_icache() {
    unsafe { asm!("dsb ish", "ic ialluis", "dsb ish", "isb", options(nostack)) }
}
