//! EL0 (user mode) on AArch64 -- the counterpart of x86 `usermode.rs`, with
//! the same API for the scheduler (`task.rs`) and the kernel:
//!
//! - [`enter_user_thread`] pushes the kernel's callee-saved registers on the
//!   task's own kernel stack, records that stack pointer in the task's `kctx`
//!   (`task::current_kctx_ptr`), points SP_EL0 / ELR_EL1 at the user stack /
//!   entry, puts the argument in x0, clears every other register and `eret`s
//!   to EL0t with interrupts enabled;
//! - [`exit_user`] (EXIT / THREAD_EXIT syscall, or a fatal fault) jumps back to
//!   that saved stack, so `enter_user_thread` returns the exit code.
//!
//! Exceptions from EL0 run on SP_EL1, which is the task's kernel stack below
//! the `enter_user_thread` frame -- so on ARM every task automatically takes
//! its syscalls and interrupts on its own kernel stack, which x86 must arrange
//! through TSS rsp0 + a per-CPU SYSCALL stack ([`set_syscall_kstack`] is a
//! no-op here). Syscalls: `SVC #0`, x8 = number, x0..x2 = arguments, result in
//! x0 (Linux arm64 convention, bz-abi numbers).

use core::arch::{asm, global_asm};

global_asm!(
    r#"
.section .text.bz_user, "ax"

// bz_enter_user_thread(entry, user_sp, arg, kctx_ptr) -> exit code
.global bz_enter_user_thread
bz_enter_user_thread:
    sub sp, sp, #112
    stp x19, x20, [sp, #0]
    stp x21, x22, [sp, #16]
    stp x23, x24, [sp, #32]
    stp x25, x26, [sp, #48]
    stp x27, x28, [sp, #64]
    stp x29, x30, [sp, #80]
    mov x9, sp
    str x9, [x3]
    msr sp_el0, x1
    msr elr_el1, x0
    msr spsr_el1, xzr          // EL0t, D/A/I/F unmasked
    mov x0, x2                 // the thread argument
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

// bz_return_to_kernel(code, kctx) -> ! : back onto the saved kernel stack.
.global bz_return_to_kernel
bz_return_to_kernel:
    mov sp, x1
    ldp x19, x20, [sp, #0]
    ldp x21, x22, [sp, #16]
    ldp x23, x24, [sp, #32]
    ldp x25, x26, [sp, #48]
    ldp x27, x28, [sp, #64]
    ldp x29, x30, [sp, #80]
    add sp, sp, #112
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
    fn bz_enter_user_thread(entry: u64, user_sp: u64, arg: u64, kctx: *mut u64) -> u64;
    fn bz_return_to_kernel(code: u64, kctx: u64) -> !;
    static bz_user_demo_start: u8;
    static bz_user_demo_end: u8;
    static bz_user_fault_start: u8;
    static bz_user_fault_end: u8;
}

/// Nothing per CPU to program: SP_EL1 already is the task's kernel stack.
pub fn set_syscall_kstack(_cpu: usize, _top: u64) {}

/// True while the current task's EL0 program is alive (routes EXIT).
pub fn user_active() -> bool {
    crate::task::user_active()
}

/// Run user code (the current task's program) until it exits; interrupts
/// are enabled again afterwards (the boot flow's direct use).
pub fn enter_user(entry: u64, user_stack_top: u64) -> u64 {
    let code = enter_user_thread(entry, user_stack_top, 0);
    unsafe { asm!("msr daifclr, #2", options(nomem, nostack)) };
    code
}

/// Run a user thread at `entry(arg)` on `user_stack_top`; returns its exit
/// code. Inside the scheduler (which manages the interrupt mask).
pub fn enter_user_thread(entry: u64, user_stack_top: u64, arg: u64) -> u64 {
    crate::task::set_user_active(true);
    let kctx = crate::task::current_kctx_ptr();
    crate::task::kernel_exit(); // EL0 runs without the Big Kernel Lock
    let code = unsafe { bz_enter_user_thread(entry, user_stack_top, arg, kctx) };
    crate::task::set_user_active(false);
    code
}

/// End the current EL0 program with `code` (EXIT / THREAD_EXIT / a fault):
/// resume the kernel where `enter_user_thread` was called.
pub fn exit_user(code: u64) -> ! {
    let kctx = unsafe { crate::task::current_kctx_ptr().read() };
    unsafe { bz_return_to_kernel(code, kctx) }
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
