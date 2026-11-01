//! Kernel-thread context switching on x86-64 (the `arch::context` HAL).
//!
//! A suspended task is just a saved stack pointer: `switch` pushes rflags and
//! the System V callee-saved registers on the outgoing stack, stores its rsp,
//! loads the incoming one and pops the same frame there. The x87/SSE state is
//! saved and restored around it with FXSAVE/FXRSTOR (the kernel is soft-float,
//! so only ring-3 code dirties those registers). `prepare_stack` builds the
//! frame a brand-new task resumes into.

use alloc::boxed::Box;

/// FXSAVE/FXRSTOR area (x87 + SSE state) of one task.
#[repr(C, align(16))]
pub struct FpuState([u8; 512]);

impl FpuState {
    /// Default state for a fresh thread: x87 control word 0x037F, MXCSR 0x1F80
    /// (all exceptions masked), everything else zero — no state leaks in from
    /// whatever ran before.
    pub fn clean() -> Box<FpuState> {
        let mut a = Box::new(FpuState([0; 512]));
        a.0[0] = 0x7F;
        a.0[1] = 0x03;
        a.0[24..28].copy_from_slice(&0x1F80u32.to_le_bytes());
        a
    }
}

/// Build the initial frame `switch` restores into on `stack`: saved rflags +
/// six callee-saved registers, then `entry` as the return address, then a 0
/// sentinel so `rsp % 16 == 8` at entry (SysV ABI) and backtraces terminate.
/// Returns the stack pointer to store for the new task.
pub fn prepare_stack(stack: &[u8], entry: extern "C" fn() -> !) -> u64 {
    let top = (stack.as_ptr() as u64 + stack.len() as u64) & !0xF;
    unsafe {
        let mut sp = top as *mut u64;
        let mut push = |val: u64| {
            sp = sp.sub(1);
            sp.write(val);
        };
        push(0); // sentinel return address
        push(entry as *const () as u64); // `ret` target
        push(0); // rbp
        push(0); // rbx
        push(0); // r12
        push(0); // r13
        push(0); // r14
        push(0); // r15
        push(0x202); // rflags: IF=1 so fresh tasks run with interrupts enabled
        sp as u64
    }
}

/// Save the outgoing task's FPU state to `prev_fpu`, load the incoming one's
/// from `next_fpu`, then switch stacks: the current stack pointer is stored
/// through `prev_rsp` and execution resumes on `next_rsp`. Returns when the
/// outgoing task is switched back in.
///
/// # Safety
/// Interrupts must be off, all pointers valid for the duration, and
/// `next_rsp` a frame produced by `prepare_stack` or an earlier `switch`.
pub unsafe fn switch(prev_rsp: *mut u64, next_rsp: u64, prev_fpu: *mut FpuState, next_fpu: *const FpuState) {
    unsafe {
        core::arch::asm!("fxsave64 [{}]", in(reg) prev_fpu, options(nostack, preserves_flags));
        core::arch::asm!("fxrstor64 [{}]", in(reg) next_fpu, options(nostack, preserves_flags));
        switch_stacks(prev_rsp, next_rsp)
    }
}

/// Save callee-saved state + rflags on the current stack, store rsp through
/// `prev_rsp`, load `next_rsp`, restore, and return on the new stack.
#[unsafe(naked)]
unsafe extern "C" fn switch_stacks(prev_rsp: *mut u64, next_rsp: u64) {
    core::arch::naked_asm!(
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "pushfq",
        "mov [rdi], rsp",
        "mov rsp, rsi",
        "popfq",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "ret",
    )
}
