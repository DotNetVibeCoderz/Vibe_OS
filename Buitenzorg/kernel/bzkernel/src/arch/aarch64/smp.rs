//! Multi-core on ARM64: secondary CPUs are started with **PSCI CPU_ON** (QEMU
//! `virt`, UEFI boards) or, on a Raspberry Pi booted directly, through their
//! **spin tables**: each parked core polls its `cpu-release-addr` (from the
//! DTB) and jumps to the address written there, at EL2 with the MMU off; a
//! stub first drops it to EL1 and fetches its boot block.
//! (target MPIDR, physical entry point, context id). A CPU starts at EL1 with
//! its MMU off, so it enters a small trampoline at the *physical* address of
//! the kernel image, which loads the boot CPU's MAIR / TCR / TTBR1 / SCTLR plus
//! a temporary TTBR0 that identity-maps the trampoline page, turns the MMU on,
//! and jumps to [`bz_ap_main`] in the higher half on its own kernel stack.
//! There each CPU records its index in TPIDR_EL1, installs the exception
//! vectors, wakes its own GIC redistributor, starts its own timer, and joins
//! the shared scheduler (`task::ap_start`). TLB maintenance is broadcast by
//! hardware (`tlbi ...is`), so no shootdown IPIs are needed.

use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const PSCI_CPU_ON: u32 = 0xC400_0003;

global_asm!(
    r#"
.section .text.bz_ap_trampoline, "ax"
.balign 4096
// x0 = physical address of the ApBoot block (PSCI context id). MMU off.
.global bz_ap_trampoline
bz_ap_trampoline:
    ldr x1, [x0, #0]       // MAIR
    ldr x2, [x0, #8]       // TCR
    ldr x3, [x0, #16]      // TTBR0 (identity map of this page)
    ldr x4, [x0, #24]      // TTBR1 (kernel half)
    ldr x5, [x0, #32]      // SCTLR (MMU + caches on)
    ldr x6, [x0, #40]      // stack top (virtual)
    ldr x7, [x0, #48]      // entry (virtual)
    ldr x8, [x0, #56]      // CPU index
    msr mair_el1, x1
    msr tcr_el1, x2
    msr ttbr0_el1, x3
    msr ttbr1_el1, x4
    isb
    tlbi vmalle1
    dsb nsh
    isb
    msr sctlr_el1, x5
    isb
    msr spsel, #1
    mov sp, x6
    mov x0, x8
    br x7

// Spin-table entry (Raspberry Pi firmware): EL2 or EL1, MMU off, no argument.
// The boot block address waits in bz_ap_spin_block (one core starts at a time).
.global bz_ap_spin_entry
bz_ap_spin_entry:
    mrs x9, currentel
    lsr x9, x9, #2
    cmp x9, #2
    b.ne 1f
    mov x9, #(1 << 31)          // HCR_EL2.RW
    msr hcr_el2, x9
    mov x9, #3                  // EL1 reaches the counter + timer
    msr cnthctl_el2, x9
    msr cntvoff_el2, xzr
    mov x9, #0x33ff
    msr cptr_el2, x9
    msr hstr_el2, xzr
    ldr x9, =0x30d00800
    msr sctlr_el1, x9
    mov x9, #0x3c5
    msr spsr_el2, x9
    adr x9, 1f
    msr elr_el2, x9
    eret
1:  adrp x0, bz_ap_spin_block
    ldr x0, [x0, :lo12:bz_ap_spin_block]
    b bz_ap_trampoline
    .ltorg
.balign 4096
.global bz_ap_trampoline_end
bz_ap_trampoline_end:
"#
);

extern "C" {
    static bz_ap_trampoline: u8;
    static bz_ap_spin_entry: u8;
}

/// Physical address of the boot block of the core being released from its spin table.
#[no_mangle]
static mut bz_ap_spin_block: u64 = 0;

/// CPUs started (the boot CPU counts).
static ONLINE: AtomicUsize = AtomicUsize::new(1);
static AP_UP: [AtomicBool; crate::arch::cpu::MAX_CPUS] = [const { AtomicBool::new(false) }; crate::arch::cpu::MAX_CPUS];

fn sysreg_mair() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, mair_el1", out(reg) v, options(nomem, nostack)) };
    v
}
fn sysreg_tcr() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, tcr_el1", out(reg) v, options(nomem, nostack)) };
    v
}
fn sysreg_ttbr1() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, ttbr1_el1", out(reg) v, options(nomem, nostack)) };
    v
}
fn sysreg_sctlr() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, sctlr_el1", out(reg) v, options(nomem, nostack)) };
    v
}

/// Clean the cache lines of `[va, va+len)` to the point of coherency, so a
/// CPU reading with its MMU (and caches) off sees the data.
fn clean_to_poc(va: u64, len: u64) {
    let mut a = va & !63;
    while a < va + len {
        unsafe { asm!("dc cvac, {}", in(reg) a, options(nostack)) };
        a += 64;
    }
    unsafe { asm!("dsb sy", options(nostack)) };
}

/// Start every CPU in `mpidrs` (MPIDR affinities from the DTB) except the
/// boot CPU, with PSCI CPU_ON. `kernel_base` = (physical, virtual) base of the
/// kernel image. Returns how many CPUs are online afterwards.
pub fn start_aps(mpidrs: &[u64], kernel_base: (u64, u64)) -> usize {
    start_with(mpidrs, kernel_base, &|mpidr, tramp_pa, boot_pa| {
        let r = super::psci::call(PSCI_CPU_ON, mpidr, tramp_pa, boot_pa);
        if r != 0 {
            crate::println!("[smp] PSCI CPU_ON {:#x} failed: {:#x}", mpidr, r as i64);
        }
        r == 0
    })
}

/// Start CPUs parked in spin tables: `cpus` = (MPIDR, physical cpu-release-addr).
pub fn start_aps_spin(cpus: &[(u64, u64)], kernel_base: (u64, u64)) -> usize {
    let mpidrs: alloc::vec::Vec<u64> = cpus.iter().map(|c| c.0).collect();
    let entry_va = unsafe { &bz_ap_spin_entry as *const u8 as u64 };
    let entry_pa = entry_va - kernel_base.1 + kernel_base.0;
    start_with(&mpidrs, kernel_base, &|mpidr, _tramp_pa, boot_pa| {
        let Some(&(_, release)) = cpus.iter().find(|c| c.0 == mpidr) else { return false };
        unsafe {
            let slot = core::ptr::addr_of_mut!(bz_ap_spin_block);
            slot.write_volatile(boot_pa);
            clean_to_poc(slot as u64, 8);
            let rel = crate::memory::phys_to_virt(release) as *mut u64;
            rel.write_volatile(entry_pa);
            clean_to_poc(rel as u64, 8);
            asm!("sev", options(nostack));
        }
        true
    })
}

fn start_with(mpidrs: &[u64], kernel_base: (u64, u64), kick: &dyn Fn(u64, u64, u64) -> bool) -> usize {
    let me = super::cpu::mpidr_affinity();
    let tramp_va = unsafe { &bz_ap_trampoline as *const u8 as u64 };
    let tramp_pa = tramp_va - kernel_base.1 + kernel_base.0;
    clean_to_poc(tramp_va, 4096);
    let Some(identity) = crate::memory::with_ctx(|ctx| super::paging::identity_root(&mut ctx.frames, tramp_pa)) else {
        crate::println!("[smp] no frame for the trampoline identity map");
        return 1;
    };
    let mut index = 1;
    for &mpidr in mpidrs {
        if mpidr == me || index >= crate::arch::cpu::MAX_CPUS {
            continue;
        }
        let stack = alloc::vec![0u8; 64 * 1024].leak();
        let stack_top = (stack.as_ptr() as u64 + stack.len() as u64) & !0xF;
        let Some((boot_pa, boot_va)) = crate::memory::alloc_dma_frame() else { break };
        let block = boot_va as *mut u64;
        let values = [
            sysreg_mair(),
            sysreg_tcr(),
            identity,
            sysreg_ttbr1(),
            sysreg_sctlr(),
            stack_top,
            bz_ap_main as *const () as u64,
            index as u64,
        ];
        for (i, v) in values.iter().enumerate() {
            unsafe { block.add(i).write_volatile(*v) };
        }
        clean_to_poc(boot_va as u64, 64);
        if !kick(mpidr, tramp_pa, boot_pa) {
            continue;
        }
        // Wait (up to ~1 s of counter time) for it to report in.
        let t0 = super::cpu::timestamp();
        while !AP_UP[index].load(Ordering::Acquire) && super::cpu::timestamp() - t0 < super::cpu::counter_hz() {
            core::hint::spin_loop();
        }
        if AP_UP[index].load(Ordering::Acquire) {
            ONLINE.fetch_add(1, Ordering::SeqCst);
            index += 1;
        } else {
            crate::println!("[smp] CPU {:#x} did not come up", mpidr);
        }
    }
    ONLINE.load(Ordering::SeqCst)
}

/// A secondary CPU, now in the higher half on its own stack (x0 = index).
#[no_mangle]
extern "C" fn bz_ap_main(cpu: u64) -> ! {
    let cpu = cpu as usize;
    super::cpu::set_id(cpu);
    super::exceptions::init();
    crate::aspace::install(crate::aspace::kernel());
    let gic_ok = super::gic::init_cpu().is_ok();
    if gic_ok {
        super::gic::enable_ppi(super::timer::INTID);
        super::timer::start(100);
    }
    AP_UP[cpu].store(true, Ordering::Release);
    crate::task::ap_start(cpu)
}

/// Wake idle CPUs because a task became runnable: they wake on their own
/// timer tick (10 ms) -- no wake-up IPI yet.
pub fn wake_others() {}

/// CPUs running the kernel.
pub fn cpus_online() -> usize {
    ONLINE.load(Ordering::SeqCst)
}

/// The `cpu` shell command's report.
pub fn report() -> alloc::vec::Vec<alloc::string::String> {
    let n = cpus_online();
    let mut out = alloc::vec![alloc::format!(
        "{} CPU (AArch64, dinyalakan lewat PSCI CPU_ON atau spin table), scheduler simetris + Big Kernel Lock; timer generik {} MHz",
        n,
        super::cpu::counter_hz() / 1_000_000
    )];
    for c in 0..n {
        out.push(alloc::format!(
            "  CPU {}: {} interrupt, {} syscall, {} task-switch kernel",
            c,
            super::exceptions::irqs_on(c),
            crate::task::syscalls_on(c),
            crate::task::kernel_runs_on(c)
        ));
    }
    out
}
