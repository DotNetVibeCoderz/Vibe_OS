//! Ring-3 execution + SYSCALL/SYSRET entry (v0.4 "Tunas": the Layer-4 bridge
//! that lets a NativeAOT-compiled C# program run in user space and call the
//! kernel through the syscall ABI).
//!
//! Ring-3 syscall convention (matches userland/hello-csharp/bzstart.rs):
//!   rax = syscall number, rdi/rsi/rdx = args, result returned in rax.
//! SFMASK clears IF on entry, so a syscall runs to completion without
//! interruption; the timer still preempts *user* code normally via the TSS
//! ring-0 stack.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use x86_64::registers::model_specific::{Efer, EferFlags, LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

use crate::gdt;

/// Kernel stack pointer used by the SYSCALL entry (SYSCALL does not switch
/// stacks itself). Single-core, so one is enough.
static SYSCALL_KSTACK_TOP: AtomicU64 = AtomicU64::new(0);
/// Scratch slot for the user's rsp across a syscall.
static USER_RSP: AtomicU64 = AtomicU64::new(0);
/// Saved kernel context for the longjmp back out of [`enter_user`] on exit.
static KCTX_RSP: AtomicU64 = AtomicU64::new(0);
/// True while a ring-3 program is running (routes the EXIT syscall).
static USER_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn user_active() -> bool {
    USER_ACTIVE.load(Ordering::SeqCst)
}

// --- Per-thread context accessors (v0.15 "Matang" increment 2) ---------------
//
// The SYSCALL entry, longjmp context, and user-rsp scratch are single globals
// (single core, one running thread). Cooperative user threads make them
// per-thread: the scheduler saves the outgoing thread's KCTX/USER_RSP and loads
// the incoming thread's KSTACK/KCTX/USER_RSP around each context switch, so two
// ring-3 threads never share a syscall kernel stack.

pub fn get_kctx() -> u64 {
    KCTX_RSP.load(Ordering::SeqCst)
}
pub fn set_kctx(v: u64) {
    KCTX_RSP.store(v, Ordering::SeqCst);
}
pub fn get_user_rsp() -> u64 {
    USER_RSP.load(Ordering::SeqCst)
}
pub fn set_user_rsp(v: u64) {
    USER_RSP.store(v, Ordering::SeqCst);
}
pub fn set_syscall_kstack(v: u64) {
    SYSCALL_KSTACK_TOP.store(v, Ordering::SeqCst);
}

/// Enter ring 3 at a thread entry with `arg` in rdi (System V first argument),
/// on the given user stack. Returns the code passed to THREAD_EXIT/EXIT. Used
/// by cooperative user threads; the caller must have set this thread's syscall
/// kernel stack via [`set_syscall_kstack`] first.
pub fn enter_user_thread(entry: u64, user_stack_top: u64, arg: u64) -> u64 {
    USER_ACTIVE.store(true, Ordering::SeqCst);
    let code = unsafe { enter_user_thread_asm(entry, user_stack_top, arg) };
    // Note: unlike enter_user, we do NOT force-enable interrupts here — the
    // thread runs inside the scheduler, which manages IF; the main thread's
    // enter_user still re-enables on the boot path.
    code
}

/// Program the SYSCALL/SYSRET MSRs. Call once, after [`gdt::init`].
pub fn init() {
    let sel = gdt::selectors();
    unsafe {
        Efer::update(|f| f.insert(EferFlags::SYSTEM_CALL_EXTENSIONS));
    }
    Star::write(sel.user_code, sel.user_data, sel.kernel_code, sel.kernel_data)
        .expect("STAR segment layout invalid");
    LStar::write(VirtAddr::new(syscall_entry as *const () as u64));
    // Clear IF (no nested interrupts), DF and TF on entry.
    SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::DIRECTION_FLAG | RFlags::TRAP_FLAG);

    SYSCALL_KSTACK_TOP.store(gdt::privilege_stack_top().as_u64(), Ordering::SeqCst);
}

/// Enter ring 3 at `entry` with the given user stack pointer, and return the
/// exit code the program passes to the EXIT syscall. Implemented as a
/// setjmp/longjmp pair with [`return_to_kernel`].
pub fn enter_user(entry: u64, user_stack_top: u64) -> u64 {
    USER_ACTIVE.store(true, Ordering::SeqCst);
    let code = unsafe { enter_user_asm(entry, user_stack_top) };
    USER_ACTIVE.store(false, Ordering::SeqCst);
    // The app exits via the EXIT syscall, whose entry cleared IF (SFMASK) and
    // whose return path (return_to_kernel) longjmps back here without restoring
    // it. Re-enable interrupts so the kernel keeps receiving the timer/IRQs.
    x86_64::instructions::interrupts::enable();
    code
}

/// Called by the EXIT syscall handler when a ring-3 program exits. Never
/// returns to the caller; unwinds back into [`enter_user`] instead.
pub fn exit_user(code: u64) -> ! {
    unsafe { return_to_kernel(code) }
}

/// C entry called from the assembly SYSCALL trampoline.
#[no_mangle]
extern "C" fn syscall_dispatch_c(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    // Everything arriving here came from ring 3, so its pointer arguments are
    // untrusted and must be validated (v1.0 hardening). Kernel-internal callers
    // of `dispatch` pass their own addresses and are trusted.
    crate::syscall::dispatch_from_user(nr, a0, a1, a2)
}

core::arch::global_asm!(
    ".global syscall_entry",
    "syscall_entry:",
    // On entry: rcx = user rip, r11 = user rflags, rsp = user rsp.
    "mov [rip + {user_rsp}], rsp",
    "mov rsp, [rip + {kstack}]",
    "push rcx",                 // save user rip
    "push r11",                 // save user rflags
    // Marshal (rax=nr, rdi=a0, rsi=a1, rdx=a2) into the C ABI
    // dispatch(nr, a0, a1, a2) = (rdi, rsi, rdx, rcx).
    "mov r8, rax",
    "mov r9, rdi",
    "mov r10, rsi",
    "mov r11, rdx",
    "mov rdi, r8",
    "mov rsi, r9",
    "mov rdx, r10",
    "mov rcx, r11",
    "call syscall_dispatch_c",  // result in rax
    "pop r11",                  // user rflags
    "pop rcx",                  // user rip
    "mov rsp, [rip + {user_rsp}]",
    "sysretq",
    user_rsp = sym USER_RSP,
    kstack = sym SYSCALL_KSTACK_TOP,
);

extern "C" {
    fn syscall_entry();
}

core::arch::global_asm!(
    ".global enter_user_asm",
    "enter_user_asm:",
    // rdi = entry, rsi = user stack top.
    "push rbx",
    "push rbp",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "mov [rip + {kctx}], rsp",   // save kernel rsp for the longjmp
    "mov rcx, rdi",              // SYSRET loads rip from rcx
    "mov r11, 0x202",            // user rflags: IF set
    "mov rsp, rsi",              // user stack
    "sysretq",                   // -> ring 3 at entry
    kctx = sym KCTX_RSP,
);

extern "C" {
    fn enter_user_asm(entry: u64, user_stack_top: u64) -> u64;
}

core::arch::global_asm!(
    ".global enter_user_thread_asm",
    "enter_user_thread_asm:",
    // rdi = entry, rsi = user stack top, rdx = arg.
    "push rbx",
    "push rbp",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "mov [rip + {kctx}], rsp",   // save this thread's kernel rsp for the longjmp
    "mov rcx, rdi",              // SYSRET loads rip from rcx
    "mov r11, 0x202",            // user rflags: IF set
    "mov rsp, rsi",              // user stack
    "mov rdi, rdx",              // arg -> first System V argument
    "sysretq",                   // -> ring 3 at entry(arg)
    kctx = sym KCTX_RSP,
);

extern "C" {
    fn enter_user_thread_asm(entry: u64, user_stack_top: u64, arg: u64) -> u64;
}

core::arch::global_asm!(
    ".global return_to_kernel",
    "return_to_kernel:",
    // rdi = exit code. Restore the kernel stack saved by enter_user_asm and
    // return from it with the code in rax.
    "mov rsp, [rip + {kctx}]",
    "mov rax, rdi",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbp",
    "pop rbx",
    "ret",
    kctx = sym KCTX_RSP,
);

extern "C" {
    fn return_to_kernel(code: u64) -> !;
}
