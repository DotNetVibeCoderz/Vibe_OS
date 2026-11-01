//! Ring-3 execution + SYSCALL/SYSRET entry (v0.4 "Tunas": the Layer-4 bridge
//! that lets a NativeAOT-compiled C# program run in user space and call the
//! kernel through the syscall ABI).
//!
//! Ring-3 syscall convention (matches userland/hello-csharp/bzstart.rs):
//!   rax = syscall number, rdi/rsi/rdx = args, result returned in rax.
//! SFMASK clears IF on entry, so a syscall runs to completion without
//! interruption; the timer still preempts *user* code normally via the TSS
//! ring-0 stack.
//!
//! SMP (increment 3): SYSCALL does not switch stacks, so the entry needs a
//! per-CPU place for the user rsp and the kernel stack. LSTAR is a per-CPU MSR,
//! so every application processor gets its *own* entry stub
//! (`ap_syscall_entry_<n>`) that uses slot `n` of [`AP_USER_RSP`] /
//! [`AP_KSTACK`] — no GS base and no `swapgs` involved. Each stub immediately
//! pushes the user rsp onto the task's kernel stack, so the return path
//! (`syscall_exit_common`: pop rflags, rip, rsp; `sysretq`) is CPU-independent:
//! a syscall entered on an AP may finish on the BSP or on another AP.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use x86_64::registers::model_specific::{Efer, EferFlags, LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

use crate::gdt;

/// Kernel stack pointer used by the BSP's SYSCALL entry (SYSCALL does not
/// switch stacks itself).
static SYSCALL_KSTACK_TOP: AtomicU64 = AtomicU64::new(0);
/// Scratch slot for the user's rsp during the BSP's syscall entry (only valid
/// between the entry's first instructions; the entry then pushes it).
static USER_RSP: AtomicU64 = AtomicU64::new(0);

const MAX_CPUS: usize = crate::arch::cpu::MAX_CPUS;
/// Per-AP user-rsp scratch for the entry stubs (index = CPU index).
static mut AP_USER_RSP: [u64; MAX_CPUS] = [0; MAX_CPUS];
/// Per-AP SYSCALL kernel stack: the running task's syscall stack top.
static mut AP_KSTACK: [u64; MAX_CPUS] = [0; MAX_CPUS];

/// Point AP `cpu`'s SYSCALL entry at kernel stack `top` (the task it is about
/// to run). Called on that CPU with interrupts disabled.
pub fn set_ap_syscall_kstack(cpu: usize, top: u64) {
    unsafe { (core::ptr::addr_of_mut!(AP_KSTACK) as *mut u64).add(cpu).write_volatile(top) };
}

/// Program this AP's SYSCALL MSRs (they are per CPU): same STAR/SFMASK as the
/// BSP, LSTAR = this CPU's own entry stub.
pub fn init_on_ap(cpu: usize) {
    let sel = gdt::selectors();
    unsafe {
        Efer::update(|f| f.insert(EferFlags::SYSTEM_CALL_EXTENSIONS));
    }
    Star::write(sel.user_code, sel.user_data, sel.kernel_code, sel.kernel_data)
        .expect("STAR segment layout invalid");
    let table = core::ptr::addr_of!(ap_syscall_table) as *const i32;
    let entry = (table as i64 + unsafe { table.add(cpu).read() } as i64) as u64;
    LStar::write(VirtAddr::new(entry));
    SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::DIRECTION_FLAG | RFlags::TRAP_FLAG);
}
/// Saved kernel context for the longjmp back out of [`enter_user`] on exit.
static KCTX_RSP: AtomicU64 = AtomicU64::new(0);
/// True while a ring-3 program is running (routes the EXIT syscall).
static USER_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn user_active() -> bool {
    USER_ACTIVE.load(Ordering::SeqCst)
}
pub fn set_user_active(v: bool) {
    USER_ACTIVE.store(v, Ordering::SeqCst);
}

// --- Per-thread context accessors (v0.15 "Matang" increment 2) ---------------
//
// The SYSCALL entry, longjmp context, user-rsp scratch and user-active flag are
// single globals (single core, one running thread). The scheduler makes them
// per-task: it saves the outgoing task's values and loads the incoming task's
// around every context switch (task.rs `switch_with`), so ring-3 processes and
// threads never share a syscall kernel stack.

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
    crate::arch::cpu::enable_interrupts();
    code
}

/// Called by the EXIT syscall handler when a ring-3 program exits. Never
/// returns to the caller; unwinds back into [`enter_user`] instead.
pub fn exit_user(code: u64) -> ! {
    unsafe { return_to_kernel(code) }
}

/// C entry called from the BSP's SYSCALL trampoline.
#[no_mangle]
extern "C" fn syscall_dispatch_c(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    // Everything arriving here came from ring 3, so its pointer arguments are
    // untrusted and must be validated (v1.0 hardening). Kernel-internal callers
    // of `dispatch` pass their own addresses and are trusted.
    let r = crate::syscall::dispatch_from_user(nr, a0, a1, a2);
    // About to return to ring 3: an idle application processor may take over.
    crate::task::maybe_offload();
    r
}

/// C entry called from an AP's SYSCALL trampoline. Kernel code runs only on
/// the BSP (asymmetric multiprocessing), so the task migrates there for the
/// syscall — except for a few that only read a clock and touch no kernel state.
#[no_mangle]
extern "C" fn ap_syscall_dispatch_c(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    use bz_abi::sysno as n;
    match nr {
        n::TICKS => return crate::interrupts::ticks(),
        n::CLOCK_MONO => return crate::arch::cpu::timestamp(),
        _ => {}
    }
    crate::task::ap_leave_to_bsp();
    let r = crate::syscall::dispatch_from_user(nr, a0, a1, a2);
    crate::task::maybe_offload();
    r
}

core::arch::global_asm!(
    ".global syscall_entry",
    "syscall_entry:",
    // On entry: rcx = user rip, r11 = user rflags, rsp = user rsp.
    "mov [rip + {user_rsp}], rsp",
    "mov rsp, [rip + {kstack}]",
    "push qword ptr [rip + {user_rsp}]", // user rsp, kept with the task
    "push rcx",                 // save user rip
    "push r11",                 // save user rflags
    "sub rsp, 8",               // 16-byte alignment for the call
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
    "add rsp, 8",
    // Shared return path (also used by the AP stubs): CPU-independent.
    ".global syscall_exit_common",
    "syscall_exit_common:",
    "pop r11",                  // user rflags
    "pop rcx",                  // user rip
    "pop rsp",                  // user rsp
    "sysretq",
    user_rsp = sym USER_RSP,
    kstack = sym SYSCALL_KSTACK_TOP,
);

// Common body of the AP entry stubs: the stub already switched to the task's
// kernel stack and pushed the user rsp.
core::arch::global_asm!(
    ".global syscall_body_ap",
    "syscall_body_ap:",
    "push rcx",
    "push r11",
    "sub rsp, 8",
    "mov r8, rax",
    "mov r9, rdi",
    "mov r10, rsi",
    "mov r11, rdx",
    "mov rdi, r8",
    "mov rsi, r9",
    "mov rdx, r10",
    "mov rcx, r11",
    "call ap_syscall_dispatch_c",
    "add rsp, 8",
    "jmp syscall_exit_common",
);

/// One SYSCALL entry stub per AP: per-CPU slot `n` of the scratch/stack arrays.
macro_rules! ap_syscall_stubs {
    ($($n:literal),*) => {
        $(
            core::arch::global_asm!(
                concat!(".global ap_syscall_entry_", stringify!($n)),
                concat!("ap_syscall_entry_", stringify!($n), ":"),
                "mov [rip + {ursp} + {off}], rsp",
                "mov rsp, [rip + {kstack} + {off}]",
                "push qword ptr [rip + {ursp} + {off}]",
                "jmp syscall_body_ap",
                ursp = sym AP_USER_RSP,
                kstack = sym AP_KSTACK,
                off = const $n * 8,
            );
        )*
        core::arch::global_asm!(
            ".pushsection .rodata",
            ".p2align 3",
            ".global ap_syscall_table",
            "ap_syscall_table:",
            // Offsets relative to the table (the kernel is position-independent,
            // so it cannot hold absolute addresses in data).
            ".long 0", // the BSP uses `syscall_entry`
            $( concat!(".long ap_syscall_entry_", stringify!($n), " - ap_syscall_table"), )*
            ".popsection",
        );
    };
}
ap_syscall_stubs!(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);

extern "C" {
    /// Entry stub offset (from the table) per CPU index (0 unused).
    static ap_syscall_table: [i32; MAX_CPUS];
}

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
