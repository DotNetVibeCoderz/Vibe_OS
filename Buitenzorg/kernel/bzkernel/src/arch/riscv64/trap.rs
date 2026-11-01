//! The S-mode trap vector (`stvec`, direct mode): every exception and
//! interrupt enters [`bz_trap_entry`], which saves all 31 GPRs plus `sepc`,
//! `sstatus`, `stval` and `scause` in a 288-byte frame and calls [`bz_trap`].
//!
//! Stack choice uses `sscratch`, the classic RISC-V scheme: it is **0 while
//! the hart runs the kernel**, and holds the current task's kernel-stack top
//! while it runs U-mode. On entry `csrrw sp, sscratch, sp` swaps them; a zero
//! result means the trap came from the kernel (swap back, stay on this
//! stack), anything else is the kernel stack of a trap from user mode. The 16
//! bytes at that stack top hold the CPU index, reloaded into `tp` (user code
//! owns `tp`). Returning to U-mode re-arms `sscratch` and that slot.
//!
//! `sepc`/`sstatus` live in the frame because handlers may enable interrupts
//! or switch tasks (syscalls run with interrupts on; a timer tick preempts
//! U-mode code); a nested trap overwrites the CSRs.

use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicU64, Ordering};

/// Size of the saved frame (16-byte multiple).
pub const FRAME_SIZE: usize = 288;

global_asm!(
    r#"
.section .text.bz_trap, "ax"
.balign 4
.global bz_trap_entry
bz_trap_entry:
    csrrw sp, sscratch, sp
    beqz sp, 1f
    // From U-mode: sp = kernel stack top, sscratch = user sp.
    addi sp, sp, -288
    .irp n, 1,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
    sd x\n, (\n*8)(sp)
    .endr
    csrr t0, sscratch
    sd t0, 16(sp)
    csrw sscratch, zero
    ld tp, 288(sp)               // the CPU index stored at the stack top
    j 2f
1:  // From S-mode: restore sp (sscratch is 0 again).
    csrrw sp, sscratch, sp
    addi sp, sp, -288
    .irp n, 1,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
    sd x\n, (\n*8)(sp)
    .endr
    addi t0, sp, 288
    sd t0, 16(sp)
2:
    csrr t0, sepc
    csrr t1, sstatus
    csrr t2, stval
    csrr t3, scause
    sd t0, 256(sp)
    sd t1, 264(sp)
    sd t2, 272(sp)
    sd t3, 280(sp)
    mv a0, sp
    call bz_trap
.global bz_trap_return
bz_trap_return:
    // a frame at sp; interrupts off from here to sret.
    csrci sstatus, 2
    ld t0, 256(sp)
    ld t1, 264(sp)
    csrw sepc, t0
    csrw sstatus, t1
    andi t2, t1, 0x100           // SPP: 0 = returning to U-mode
    bnez t2, 3f
    addi t0, sp, 288             // the kernel stack top for the next trap
    sd tp, 0(t0)
    csrw sscratch, t0
    j 4f
3:  // Back to kernel code: keep this hart's tp (the CPU index), not the one
    // saved when the trap was taken -- the task may have moved to another hart.
    sd tp, 32(sp)
4:
    .irp n, 1,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
    ld x\n, (\n*8)(sp)
    .endr
    ld sp, 16(sp)
    sret
"#
);

extern "C" {
    fn bz_trap_entry();
}

/// The registers of an interrupted context.
#[repr(C)]
pub struct TrapFrame {
    /// x0..x31 (x0 unused; x2 = the interrupted sp).
    pub x: [u64; 32],
    pub sepc: u64,
    pub sstatus: u64,
    pub stval: u64,
    pub scause: u64,
}

const _: () = assert!(core::mem::size_of::<TrapFrame>() == FRAME_SIZE);

const INTERRUPT: u64 = 1 << 63;
const IRQ_S_SOFT: u64 = 1;
const IRQ_S_TIMER: u64 = 5;
const IRQ_S_EXT: u64 = 9;
const ECALL_FROM_U: u64 = 8;

static IRQS: AtomicU64 = AtomicU64::new(0);
static IRQS_FROM_USER: AtomicU64 = AtomicU64::new(0);
static ECALLS: AtomicU64 = AtomicU64::new(0);
/// Most harts seen running user code at once (sampled in interrupts from U-mode).
static MAX_PARALLEL_USER: AtomicU64 = AtomicU64::new(0);

/// Reset / read the U-mode parallelism peak.
pub fn reset_parallel_peak() {
    MAX_PARALLEL_USER.store(0, Ordering::SeqCst);
}
pub fn parallel_peak() -> u64 {
    MAX_PARALLEL_USER.load(Ordering::SeqCst)
}
static IRQS_ON: [AtomicU64; crate::arch::cpu::MAX_CPUS] = [const { AtomicU64::new(0) }; crate::arch::cpu::MAX_CPUS];

/// Install the trap vector on this hart (`sscratch` = 0: running the kernel).
pub fn init() {
    unsafe {
        asm!(
            "csrw sscratch, zero",
            "csrw stvec, {}",
            // scounteren.TM: U-mode may read the `time` counter (rdtime).
            "csrsi scounteren, 2",
            in(reg) bz_trap_entry as *const () as u64,
            options(nostack)
        );
    }
    super::cpu::allow_user_access();
}

/// Interrupts taken (all CPUs).
pub fn irq_count() -> u64 {
    IRQS.load(Ordering::Relaxed)
}

/// Interrupts CPU `cpu` has taken.
pub fn irqs_on(cpu: usize) -> u64 {
    IRQS_ON.get(cpu).map_or(0, |c| c.load(Ordering::Relaxed))
}

/// (interrupts taken while in U-mode, U-mode `ecall`s served).
pub fn user_stats() -> (u64, u64) {
    (IRQS_FROM_USER.load(Ordering::Relaxed), ECALLS.load(Ordering::Relaxed))
}

pub fn exception_name(code: u64) -> &'static str {
    match code {
        0 => "instruction address misaligned",
        1 => "instruction access fault",
        2 => "illegal instruction",
        3 => "breakpoint",
        4 => "load address misaligned",
        5 => "load access fault",
        6 => "store address misaligned",
        7 => "store access fault",
        8 => "ecall from U-mode",
        9 => "ecall from S-mode",
        12 => "instruction page fault",
        13 => "load page fault",
        15 => "store page fault",
        _ => "?",
    }
}

#[no_mangle]
extern "C" fn bz_trap(f: &mut TrapFrame) {
    let cause = f.scause;
    let from_user = f.sstatus & 0x100 == 0;
    if cause & INTERRUPT != 0 {
        IRQS.fetch_add(1, Ordering::Relaxed);
        if from_user {
            IRQS_FROM_USER.fetch_add(1, Ordering::Relaxed);
            let n = (0..super::smp::cpus_online()).filter(|&c| crate::task::cpu_runs_user(c)).count() as u64;
            MAX_PARALLEL_USER.fetch_max(n, Ordering::Relaxed);
        }
        if let Some(c) = IRQS_ON.get(super::cpu::id()) {
            c.fetch_add(1, Ordering::Relaxed);
        }
        match cause & !INTERRUPT {
            IRQ_S_TIMER => {
                super::timer::on_tick();
                // A tick may switch tasks (ring-3 code is always preemptible,
                // kernel code only in the scheduler demo) -- as on x86/ARM64.
                if super::cpu::id() == 0 {
                    crate::task::preempt(from_user);
                } else {
                    crate::task::ap_timer(from_user);
                }
            }
            IRQ_S_SOFT => unsafe { asm!("csrci sip, 2", options(nomem, nostack)) },
            IRQ_S_EXT => super::plic::handle_external(),
            other => crate::println!("[riscv] unexpected interrupt {}", other),
        }
        return;
    }
    if from_user {
        if cause == ECALL_FROM_U {
            ECALLS.fetch_add(1, Ordering::Relaxed);
            f.sepc += 4;
            f.x[10] = crate::user_syscall(f.x[17], f.x[10], f.x[11], f.x[12]);
            return;
        }
        crate::user_fault(cause, f.stval, f.sepc);
    }
    panic!(
        "{} trap: scause={} ({}) sepc={:#x} stval={:#x} sp={:#x} ra={:#x}",
        if from_user { "U-mode" } else { "kernel" },
        cause,
        exception_name(cause),
        f.sepc,
        f.stval,
        f.x[2],
        f.x[1]
    );
}
