//! Ring-3 execution + SYSCALL/SYSRET entry (v0.4 "Tunas": the Layer-4 bridge
//! that lets a NativeAOT-compiled C# program run in user space and call the
//! kernel through the syscall ABI).
//!
//! Ring-3 syscall convention (matches userland/hello-csharp/bzstart.rs):
//!   rax = syscall number, rdi/rsi/rdx = args, result returned in rax.
//! SFMASK clears IF on entry, so a syscall runs without interruption (except
//! while it waits for the Big Kernel Lock, see task.rs); the timer still
//! preempts *user* code normally via the TSS ring-0 stack.
//!
//! SMP: SYSCALL does not switch stacks, so the entry needs a per-CPU place for
//! the user rsp and the kernel stack. LSTAR is a per-CPU MSR, so every CPU
//! gets its *own* entry stub (`syscall_entry_<n>`) that uses slot `n` of
//! [`CPU_USER_RSP`] / [`CPU_KSTACK`] — no GS base and no `swapgs` involved.
//! Each stub immediately pushes the user rsp onto the task's kernel stack, so
//! the return path (`syscall_exit_common`: pop rflags, rip, rsp; `sysretq`) is
//! CPU-independent. The longjmp context used when a program exits is saved in
//! the *task* (task.rs `current_kctx_ptr`), not in a global, so it is correct
//! on whichever CPU the task runs.

use x86_64::registers::model_specific::{Efer, EferFlags, LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

use crate::gdt;

const MAX_CPUS: usize = crate::arch::cpu::MAX_CPUS;
/// Per-CPU user-rsp scratch for the entry stubs (index = CPU index; valid only
/// between a stub's first instructions).
static mut CPU_USER_RSP: [u64; MAX_CPUS] = [0; MAX_CPUS];
/// Per-CPU SYSCALL kernel stack: the running task's syscall stack top.
static mut CPU_KSTACK: [u64; MAX_CPUS] = [0; MAX_CPUS];

/// Point CPU `cpu`'s SYSCALL entry at kernel stack `top` (the task it is about
/// to run). Called on that CPU with interrupts disabled.
pub fn set_syscall_kstack(cpu: usize, top: u64) {
    unsafe { (core::ptr::addr_of_mut!(CPU_KSTACK) as *mut u64).add(cpu).write_volatile(top) };
}

/// Program this CPU's SYSCALL MSRs (they are per CPU): STAR (segment layout,
/// identical GDTs everywhere), SFMASK, and LSTAR = this CPU's own entry stub.
fn program_msrs(cpu: usize) {
    let sel = gdt::selectors();
    unsafe {
        Efer::update(|f| f.insert(EferFlags::SYSTEM_CALL_EXTENSIONS));
    }
    Star::write(sel.user_code, sel.user_data, sel.kernel_code, sel.kernel_data)
        .expect("STAR segment layout invalid");
    let table = core::ptr::addr_of!(syscall_table) as *const i32;
    let entry = (table as i64 + unsafe { table.add(cpu).read() } as i64) as u64;
    LStar::write(VirtAddr::new(entry));
    // Clear IF (no nested interrupts), DF and TF on entry.
    SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::DIRECTION_FLAG | RFlags::TRAP_FLAG);
}

/// Program the BSP's SYSCALL MSRs. Call once, after [`gdt::init`].
pub fn init() {
    program_msrs(0);
    set_syscall_kstack(0, gdt::privilege_stack_top().as_u64());
}

/// Program an application processor's SYSCALL MSRs.
pub fn init_on_ap(cpu: usize) {
    program_msrs(cpu);
}

pub fn user_active() -> bool {
    crate::task::user_active()
}

/// Enter ring 3 at `entry` with the given user stack pointer, and return the
/// exit code the program passes to the EXIT syscall. Implemented as a
/// setjmp/longjmp pair with [`return_to_kernel`]. Used by the early boot path.
pub fn enter_user(entry: u64, user_stack_top: u64) -> u64 {
    crate::task::set_user_active(true);
    let kctx = crate::task::current_kctx_ptr();
    crate::task::kernel_exit(); // ring 3 runs without the Big Kernel Lock
    let code = unsafe { enter_user_asm(entry, user_stack_top, kctx) };
    // Back via EXIT (which took the BKL again on its way in).
    crate::task::set_user_active(false);
    // The EXIT syscall's entry cleared IF (SFMASK) and its return path
    // (return_to_kernel) longjmps back here without restoring it. Re-enable
    // interrupts so the kernel keeps receiving the timer/IRQs.
    crate::arch::cpu::enable_interrupts();
    code
}

/// Enter ring 3 at a thread entry with `arg` in rdi (System V first argument),
/// on the given user stack. Returns the code passed to THREAD_EXIT/EXIT.
pub fn enter_user_thread(entry: u64, user_stack_top: u64, arg: u64) -> u64 {
    crate::task::set_user_active(true);
    let kctx = crate::task::current_kctx_ptr();
    crate::task::kernel_exit(); // ring 3 runs without the Big Kernel Lock
    // The thread runs inside the scheduler, which manages IF.
    unsafe { enter_user_thread_asm(entry, user_stack_top, arg, kctx) }
}

/// Called by the EXIT syscall handler (or a fatal ring-3 fault) when a ring-3
/// program exits. Never returns; unwinds back into [`enter_user`] /
/// [`enter_user_thread`] instead, still holding the Big Kernel Lock.
pub fn exit_user(code: u64) -> ! {
    let kctx = unsafe { crate::task::current_kctx_ptr().read() };
    unsafe { return_to_kernel(code, kctx) }
}

/// C entry called from every CPU's SYSCALL stub.
#[no_mangle]
extern "C" fn syscall_dispatch_c(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    use bz_abi::sysno as n;
    // Clock reads touch no kernel state: no Big Kernel Lock needed.
    match nr {
        n::TICKS => return crate::interrupts::ticks(),
        n::CLOCK_MONO => return crate::arch::cpu::timestamp(),
        _ => {}
    }
    let fine = crate::syscall::bkl_free(nr);
    if !fine {
        crate::task::kernel_enter();
    }
    // Serve the syscall with interrupts ON. SFMASK cleared IF on entry, but a
    // long syscall (a compose, a disk read, a spin on a subsystem lock) must
    // not keep this CPU deaf: device IRQs and the PIT go to particular CPUs
    // (I/O APIC), and with CPU 0 stuck IF-off the tick count froze and every
    // tick-timed loop in the system spun on. Kernel code is never preempted
    // (a timer in ring 0 does not switch tasks), and every lock an interrupt
    // handler also takes is taken with interrupts off, so this is safe —
    // kernel tasks have always run like this.
    crate::arch::cpu::enable_interrupts();
    crate::task::note_syscall(fine);
    // Concurrency statistic, for the BKL-free syscalls that never block.
    let track = fine
        && matches!(
            nr,
            n::MMAP | n::MPROTECT | n::MUNMAP | n::THREAD_SELF | n::DEBUG_WRITE | n::WIN_CMD | n::WIN_PRESENT | n::SYS_STAT | n::PROC_LIST
                | n::FS_READ | n::FS_WRITE | n::FS_LIST
                | n::PROC_SPAWN | n::NET_SEND | n::NET_RECV | n::PKG_LIST | n::AUDIO_STAT
        );
    if track {
        crate::task::fine_enter();
    }
    // Everything arriving here came from ring 3, so its pointer arguments are
    // untrusted and must be validated (v1.0 hardening). Kernel-internal callers
    // of `dispatch` pass their own addresses and are trusted.
    let r = crate::syscall::dispatch_from_user(nr, a0, a1, a2);
    if track {
        crate::task::fine_leave();
    }
    // Back to IF off before the return path: after `pop rsp` the stack is the
    // user's, and an interrupt taken in ring 0 there would push onto it.
    crate::arch::cpu::disable_interrupts();
    // Drops the BKL if this syscall took it; a killed task ends here.
    crate::task::kernel_exit();
    r
}

// Common body of the entry stubs: the stub already switched to the task's
// kernel stack and pushed the user rsp.
core::arch::global_asm!(
    ".global syscall_body",
    "syscall_body:",
    "push rcx",                 // user rip
    "push r11",                 // user rflags
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
    // Shared, CPU-independent return path.
    ".global syscall_exit_common",
    "syscall_exit_common:",
    "pop r11",                  // user rflags
    "pop rcx",                  // user rip
    "pop rsp",                  // user rsp
    "sysretq",
);

/// One SYSCALL entry stub per CPU: per-CPU slot `n` of the scratch/stack arrays.
macro_rules! syscall_stubs {
    ($($n:literal),*) => {
        $(
            core::arch::global_asm!(
                concat!(".global syscall_entry_", stringify!($n)),
                concat!("syscall_entry_", stringify!($n), ":"),
                // On entry: rcx = user rip, r11 = user rflags, rsp = user rsp.
                "mov [rip + {ursp} + {off}], rsp",
                "mov rsp, [rip + {kstack} + {off}]",
                "push qword ptr [rip + {ursp} + {off}]", // user rsp, kept with the task
                "jmp syscall_body",
                ursp = sym CPU_USER_RSP,
                kstack = sym CPU_KSTACK,
                off = const $n * 8,
            );
        )*
        core::arch::global_asm!(
            ".pushsection .rodata",
            ".p2align 3",
            ".global syscall_table",
            "syscall_table:",
            // Offsets relative to the table (the kernel is position-independent,
            // so it cannot hold absolute addresses in data).
            $( concat!(".long syscall_entry_", stringify!($n), " - syscall_table"), )*
            ".popsection",
        );
    };
}
syscall_stubs!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);

extern "C" {
    /// Entry stub offset (from the table) per CPU index.
    static syscall_table: [i32; MAX_CPUS];
}

core::arch::global_asm!(
    ".global enter_user_asm",
    "enter_user_asm:",
    // rdi = entry, rsi = user stack top, rdx = where to save the kernel rsp.
    "push rbx",
    "push rbp",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "mov [rdx], rsp",            // save kernel rsp for the longjmp
    "mov rcx, rdi",              // SYSRET loads rip from rcx
    "mov r11, 0x202",            // user rflags: IF set
    "mov rsp, rsi",              // user stack
    "sysretq",                   // -> ring 3 at entry
);

extern "C" {
    fn enter_user_asm(entry: u64, user_stack_top: u64, kctx: *mut u64) -> u64;
}

core::arch::global_asm!(
    ".global enter_user_thread_asm",
    "enter_user_thread_asm:",
    // rdi = entry, rsi = user stack top, rdx = arg, rcx = where to save rsp.
    "push rbx",
    "push rbp",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "mov [rcx], rsp",            // save this thread's kernel rsp for the longjmp
    "mov rcx, rdi",              // SYSRET loads rip from rcx
    "mov r11, 0x202",            // user rflags: IF set
    "mov rsp, rsi",              // user stack
    "mov rdi, rdx",              // arg -> first System V argument
    "sysretq",                   // -> ring 3 at entry(arg)
);

extern "C" {
    fn enter_user_thread_asm(entry: u64, user_stack_top: u64, arg: u64, kctx: *mut u64) -> u64;
}

core::arch::global_asm!(
    ".global return_to_kernel",
    "return_to_kernel:",
    // rdi = exit code, rsi = kernel rsp saved by enter_user(_thread)_asm.
    "mov rsp, rsi",
    "mov rax, rdi",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbp",
    "pop rbx",
    "ret",
);

extern "C" {
    fn return_to_kernel(code: u64, kctx: u64) -> !;
}

