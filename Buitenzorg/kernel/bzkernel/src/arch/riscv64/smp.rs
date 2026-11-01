//! Multi-core on RISC-V: secondary harts are started with the **SBI HSM**
//! extension (`hart_start(hartid, physical start address, opaque)`). A hart
//! starts in S-mode with paging off (`satp` = 0), a0 = its hart id and
//! a1 = the opaque value -- here the physical address of a per-hart boot
//! block. It enters [`bz_ap_trampoline`] (its own 4 KiB page in `.text`, at
//! `virt - kernel_virt_base + kernel_phys_base`), which loads every value
//! from the block first (it is unmapped once paging is on), installs a
//! temporary root -- the kernel half plus an identity mapping of the
//! trampoline page (`paging::identity_root`) -- and jumps to [`bz_ap_entry`]
//! in the higher half on its own kernel stack. There the hart switches to the
//! kernel root, records its CPU index in `tp`, installs the trap vector, its
//! PLIC context and its own SBI timer, and joins the shared scheduler
//! (`task::ap_start`).
//!
//! TLB maintenance across harts goes through the SBI as well: RISC-V has no
//! broadcast `sfence.vma`, so `flush_tlb_everywhere` asks the firmware for a
//! remote fence on every hart (RFENCE extension); idle harts are woken with an
//! SBI IPI (a supervisor software interrupt).

use core::arch::global_asm;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

global_asm!(
    r#"
.section .text.bz_ap_trampoline, "ax"
.option push
.option norelax
.balign 4096
// a0 = hart id, a1 = physical address of the boot block. Paging is off.
.global bz_ap_trampoline
bz_ap_trampoline:
    ld t0, 0(a1)            // temporary satp (identity map of this page)
    ld t1, 8(a1)            // the kernel root's satp
    ld t2, 16(a1)           // stack top (virtual)
    ld t3, 24(a1)           // entry (virtual)
    ld t4, 32(a1)           // CPU index
    csrw satp, t0
    sfence.vma
    mv sp, t2
    mv a0, t4
    mv a1, t1
    jr t3
.balign 4096
.global bz_ap_trampoline_end
bz_ap_trampoline_end:
.option pop
"#
);

extern "C" {
    static bz_ap_trampoline: u8;
}

const HSM_HART_START: u64 = 0;
const IPI_SEND: u64 = 0;
const RFENCE_SFENCE_VMA: u64 = 1;

/// CPUs started (the boot hart counts).
static ONLINE: AtomicUsize = AtomicUsize::new(1);
static AP_UP: [AtomicBool; crate::arch::cpu::MAX_CPUS] = [const { AtomicBool::new(false) }; crate::arch::cpu::MAX_CPUS];
/// Hart id of each CPU index (for IPIs).
static HART_OF: [AtomicU64; crate::arch::cpu::MAX_CPUS] = [const { AtomicU64::new(u64::MAX) }; crate::arch::cpu::MAX_CPUS];
/// Wake-up IPIs sent / supervisor software interrupts taken.
static IPIS_SENT: AtomicU64 = AtomicU64::new(0);

/// Start every hart in `harts` (hart ids from the DTB) except the boot hart.
/// `kernel_base` = (physical, virtual) base of the kernel image;
/// `plic_context(hart)` gives each hart's S-mode PLIC context. Returns how
/// many CPUs are online afterwards.
pub fn start_harts(harts: &[u64], boot_hart: u64, kernel_base: (u64, u64), plic_context: &dyn Fn(u64) -> Option<usize>) -> usize {
    HART_OF[0].store(boot_hart, Ordering::SeqCst);
    if !super::sbi::probe(super::sbi::EXT_HSM) {
        crate::println!("[smp] the SBI has no HSM extension");
        return 1;
    }
    let tramp_va = unsafe { &bz_ap_trampoline as *const u8 as u64 };
    let tramp_pa = tramp_va - kernel_base.1 + kernel_base.0;
    let kernel_root = crate::aspace::kernel().root;
    let Some(identity) = crate::memory::with_ctx(|ctx| super::paging::identity_root(&mut ctx.frames, kernel_root, tramp_pa)) else {
        crate::println!("[smp] no frame for the trampoline identity map");
        return 1;
    };
    let mut index = 1;
    for &hart in harts {
        if hart == boot_hart || index >= crate::arch::cpu::MAX_CPUS {
            continue;
        }
        let stack = alloc::vec![0u8; 64 * 1024].leak();
        let stack_top = (stack.as_ptr() as u64 + stack.len() as u64) & !0xF;
        let Some((boot_pa, boot_va)) = crate::memory::alloc_dma_frame() else { break };
        let block = boot_va as *mut u64;
        let values = [
            super::paging::satp_for(identity),
            super::paging::satp_for(kernel_root),
            stack_top,
            bz_ap_entry as *const () as u64,
            index as u64,
        ];
        for (i, v) in values.iter().enumerate() {
            unsafe { block.add(i).write_volatile(*v) };
        }
        if let Some(ctx) = plic_context(hart) {
            if super::plic::active() {
                super::plic::set_context(index, ctx);
            }
        }
        HART_OF[index].store(hart, Ordering::SeqCst);
        core::sync::atomic::fence(Ordering::SeqCst);
        let (err, _) = super::sbi::call(super::sbi::EXT_HSM, HSM_HART_START, hart, tramp_pa, boot_pa);
        if err != 0 {
            crate::println!("[smp] SBI hart_start({}) failed: {}", hart, err);
            HART_OF[index].store(u64::MAX, Ordering::SeqCst);
            continue;
        }
        // Wait (up to ~1 s of timebase) for it to report in.
        let t0 = super::cpu::timestamp();
        while !AP_UP[index].load(Ordering::Acquire) && super::cpu::timestamp() - t0 < super::cpu::counter_hz() {
            core::hint::spin_loop();
        }
        if AP_UP[index].load(Ordering::Acquire) {
            ONLINE.fetch_add(1, Ordering::SeqCst);
            index += 1;
        } else {
            crate::println!("[smp] hart {} did not come up", hart);
            HART_OF[index].store(u64::MAX, Ordering::SeqCst);
        }
    }
    ONLINE.load(Ordering::SeqCst)
}

/// A secondary hart, now in the higher half on its own stack under the
/// temporary root (a0 = CPU index, a1 = the kernel root's satp).
#[no_mangle]
extern "C" fn bz_ap_entry(cpu: u64, kernel_satp: u64) -> ! {
    unsafe { core::arch::asm!("csrw satp, {}", "sfence.vma", in(reg) kernel_satp, options(nostack)) };
    let cpu = cpu as usize;
    super::cpu::set_id(cpu);
    super::trap::init();
    super::context::enable_fp();
    crate::aspace::install(crate::aspace::kernel());
    super::cpu::sie_set(super::cpu::SIE_SSIE | super::cpu::SIE_SEIE);
    super::timer::start(100);
    AP_UP[cpu].store(true, Ordering::Release);
    crate::task::ap_start(cpu)
}

/// Wake idle CPUs because a task became runnable: an SBI IPI (supervisor
/// software interrupt) to every other online hart.
pub fn wake_others() {
    let n = cpus_online();
    if n < 2 {
        return;
    }
    let me = super::cpu::id();
    let mut mask = 0u64;
    for c in 0..n {
        let h = HART_OF[c].load(Ordering::Relaxed);
        if c != me && h < 64 {
            mask |= 1 << h;
        }
    }
    if mask != 0 {
        IPIS_SENT.fetch_add(1, Ordering::Relaxed);
        super::sbi::call(super::sbi::EXT_IPI, IPI_SEND, mask, 0, 0);
    }
}

/// Flush stale translations on every hart (the firmware's remote
/// `sfence.vma`; hart_mask_base = -1 selects all harts, size -1 = all).
pub fn remote_sfence_all() {
    if cpus_online() > 1 {
        super::sbi::call4(super::sbi::EXT_RFENCE, RFENCE_SFENCE_VMA, 0, u64::MAX, 0, u64::MAX);
    }
}

/// Harts the kernel runs on.
pub fn cpus_online() -> usize {
    ONLINE.load(Ordering::SeqCst)
}

/// Wake-up IPIs sent.
pub fn ipis_sent() -> u64 {
    IPIS_SENT.load(Ordering::Relaxed)
}

/// The `cpu` shell command's report.
pub fn report() -> alloc::vec::Vec<alloc::string::String> {
    let n = cpus_online();
    let mut out = alloc::vec![alloc::format!(
        "{} hart (RV64, S-mode, dinyalakan lewat SBI HSM), scheduler simetris + Big Kernel Lock; timebase {} MHz; {} IPI bangun",
        n,
        super::cpu::counter_hz() / 1_000_000,
        ipis_sent()
    )];
    for c in 0..n {
        out.push(alloc::format!(
            "  CPU {} (hart {}): {} interrupt, {} syscall, {} task-switch kernel",
            c,
            HART_OF[c].load(Ordering::Relaxed),
            super::trap::irqs_on(c),
            crate::task::syscalls_on(c),
            crate::task::kernel_runs_on(c)
        ));
    }
    out
}
