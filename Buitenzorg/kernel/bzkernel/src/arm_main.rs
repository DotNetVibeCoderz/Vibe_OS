// The ARM64 kernel (multi-arch M3). Included by main.rs on aarch64. Loaded by
// Limine (UEFI) on QEMU `virt`.
//
// Increment 1: boot hand-off, framebuffer console, exception vectors, live
// Device Tree, PL011, GICv3 + timer. Increment 2: the generic memory
// subsystem (`memory.rs` frame allocator + paging context, `allocator.rs`
// kernel heap) over the AArch64 paging HAL — TTBR1 for the kernel half,
// TTBR0 for user space. The x86 kernel's other subsystems join one by one as
// they get an `arch/aarch64` backend (virtio-mmio, the scheduler, the
// desktop). Increment 3: kernel-thread context switch (with per-task FP/SIMD
// state) and user mode -- EL0 programs in their own address space calling the
// kernel with `SVC #0`, faults contained. Increment 4: real ELF programs
// (`elf.rs`), handed over by Limine as boot modules -- the first C# app built
// for ARM64 (bflat linux-arm64 + the aarch64 `bzstart.rs` shim).
// Increment 5: the shared scheduler (`task.rs`) and process registry
// (`process.rs`) run unchanged on ARM64 -- preemptive processes, ring-3
// threads and futexes. Increment 6: virtio over virtio-mmio (slots from the
// DTB) -- `/disk` on virtio-blk through the shared FAT + VFS code, programs
// loaded from it -- the PL031 clock, and virtio-gpu. Increment 7: the rest of
// the PC kernel's generic code builds for ARM64 too -- the real `syscall.rs`
// (same dispatcher, pointer validation, BKL-free paths) and `app.rs`, the
// window system, shell, network, packages, VMM, scripting -- with PSCI power,
// virtio input / net, and C# GUI apps composited into a desktop frame.

mod ai;
mod aio;
mod allocator;
mod app;
mod clr;
mod arch;
mod boot;
mod compute;
mod desktop;
mod driver;
mod dtb;
mod elf;
mod fat;
mod framebuffer;
mod gfx;
mod icons;
mod ipc;
mod keyboard;
mod limine;
mod logo;
mod memory;
mod model;
mod net;
mod pkg;
mod platform;
mod process;
mod profile;
mod ramdisk;
mod screensaver;
mod script;
mod service;
mod shell;
mod syscall;
mod task;
mod terminal;
mod theme;
mod vfs;
mod virtio;
mod vmm;
mod wallpaper;
mod wm;

pub(crate) use arch::aarch64::{aspace, gdt, interrupts, smp, usermode, vmx};
pub(crate) use platform::virt::pl011 as serial;
pub(crate) use platform::virt::pl031 as rtc;
pub(crate) use platform::virt::{audio, mouse, power};

/// Set once the interactive desktop owns the screen (as on the PC).
mod interactive {
    use core::sync::atomic::{AtomicBool, Ordering};
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    #[allow(dead_code)]
    pub fn set_active() {
        ACTIVE.store(true, Ordering::SeqCst);
    }
    pub fn is_active() -> bool {
        ACTIVE.load(Ordering::SeqCst)
    }
}

use core::panic::PanicInfo;

pub(crate) fn kernel_main(info: &'static boot::BootInfo) -> ! {
    use arch::aarch64::{cpu, exceptions, gic, timer};

    // The boot CPU is CPU 0 (TPIDR_EL1 may hold anything the loader left).
    cpu::set_id(0);
    if let Some(fb) = info.framebuffer {
        framebuffer::init(fb);
    }
    exceptions::init();

    // Memory: the generic frame allocator + paging context, then the heap —
    // the same code as on the PC, over the AArch64 paging HAL.
    let mapper = unsafe { memory::init(info.phys_offset) };
    let mut frames = unsafe { memory::BootInfoFrameAllocator::init(info.regions()) };
    let user_root = arch::paging::init_user_half(&mut frames);
    let usable_mib = frames.usable_frame_count() * 4096 / (1024 * 1024);
    memory::install_ctx(mapper, frames);
    memory::set_total_usable_mib(usable_mib as u64);
    allocator::init().expect("heap init failed");
    aspace::init(user_root.expect("no frame for the user-half root"));

    // The console UART, from the live Device Tree.
    let fdt = info.firmware.dtb.and_then(|p| unsafe { dtb::Fdt::from_phys(p) }.ok());
    let platform = fdt.as_ref().map(dtb::Platform::discover);
    if let Some((_, base)) = platform.as_ref().and_then(|p| p.uart.clone()) {
        if let Some(va) = memory::map_mmio(base, 0x1000) {
            serial::set_base(va);
        }
    }

    print!("{}", logo::BOOT_LOGO);
    println!();
    println!("[kernel] Hello Kernel -- Buitenzorg OS (ARM64 / AArch64)");
    println!("[kernel] MILESTONE: HELLO KERNEL OK");
    let midr = cpu::midr();
    println!(
        "[arm64] running at EL{} on implementer {:#x} part {:#x}; counter {} MHz; exception vectors installed",
        cpu::current_el(),
        midr >> 24,
        (midr >> 4) & 0xFFF,
        cpu::counter_hz() / 1_000_000
    );
    println!("[boot] {}", boot::describe(info));
    println!(
        "[kernel] physical memory: {} MiB usable; {} MiB kernel heap at {:#x} (TTBR1); user half root (TTBR0) at {:#x}",
        usable_mib,
        allocator::HEAP_SIZE / (1024 * 1024),
        allocator::HEAP_START,
        user_root.unwrap_or(0)
    );
    if memory_self_test() {
        println!("[kernel] MILESTONE: MEMORY OK");
    }
    aspace_self_test();
    context_self_test();
    match platform.as_ref() {
        Some(p) => println!("[arm64] platform (live DTB): {}", p.summary()),
        None => println!("[arm64] no Device Tree from the loader"),
    }
    dtb::self_test();

    // Platform devices from the DTB: the PL031 clock and the virtio-mmio slots.
    if let Some(f) = fdt.as_ref() {
        if let Some(i) = f.compatible("arm,pl031").next() {
            if let Some(va) = f.reg(i).first().and_then(|r| memory::map_mmio(r.0, 0x1000)) {
                rtc::init(va);
            }
        }
    }
    if let Some(p) = platform.as_ref() {
        virtio::mmio::set_slots(&p.virtio_mmio);
    }
    rtc_self_test();
    storage_self_test();

    // Interrupts: the GIC (v3 on `virt`, v2 on a Raspberry Pi) + the virtual
    // timer at 100 Hz.
    let gic_regs = fdt.as_ref().and_then(|f| {
        if let Some(i) = f.compatible("arm,gic-v3").next() {
            let r = f.reg(i);
            return Some((3, r.first()?.0, r.get(1)?.0, r.get(1)?.1));
        }
        let i = f.compatible("arm,gic-400").next().or_else(|| f.compatible("arm,cortex-a15-gic").next())?;
        let r = f.reg(i);
        Some((2, r.first()?.0, r.get(1)?.0, 0x2000))
    });
    let mut timer_ok = false;
    let mut gic_name = "no GIC";
    if let Some((version, gicd_pa, gicr_pa, gicr_len)) = gic_regs {
        gic_name = if version == 3 { "GICv3" } else { "GICv2" };
        let up = if version == 3 {
            // Every CPU's redistributor (0x20000 each), not just the boot CPU's.
            let gicr_len = gicr_len.min(0x2_0000 * arch::cpu::MAX_CPUS as u64);
            memory::map_mmio(gicd_pa, 0x1_0000).zip(memory::map_mmio(gicr_pa, gicr_len)).ok_or("cannot map the GIC").and_then(|(d, r)| gic::init(d, r))
        } else {
            memory::map_mmio(gicd_pa, 0x1000).zip(memory::map_mmio(gicr_pa, gicr_len)).ok_or("cannot map the GIC").and_then(|(d, c)| gic::init_v2(d, c))
        };
        match up {
            Ok(()) => {
                gic::enable_ppi(timer::INTID);
                timer::start(100);
                cpu::enable_interrupts();
                let t0 = cpu::timestamp();
                let hz = cpu::counter_hz();
                while timer::ticks() < 50 && cpu::timestamp() - t0 < hz * 3 {
                    cpu::halt();
                }
                let ms = (cpu::timestamp() - t0) * 1000 / hz.max(1);
                let n = timer::ticks();
                println!(
                    "[arm64] {} at {:#x}/{:#x}: {} timer interrupts in {} ms ({} IRQs taken)",
                    gic_name, gicd_pa, gicr_pa, n, ms, exceptions::irq_count()
                );
                // 50 ticks at 100 Hz = 500 ms of counter time; never faster,
                // but under TCG on a busy host interrupts can arrive late.
                timer_ok = n >= 50 && (450..=2000).contains(&ms);
                if timer_ok {
                    println!("[kernel] MILESTONE: ARM64 TIMER OK ({}; EL1 virtual timer PPI 27 at 100 Hz)", gic_name);
                }
            }
            Err(e) => println!("[arm64] GIC: {}", e),
        }
    } else {
        println!("[arm64] no GIC in the Device Tree");
    }

    el0_self_test();
    gpu_self_test(info);
    // The shared scheduler (task.rs): the boot flow becomes task 0, an idle
    // task joins, and programs run as preemptively scheduled processes.
    task::init();
    start_secondary_cpus(info, fdt.as_ref());
    shared_kernel_setup(info, fdt.as_ref());
    run_boot_programs(info);
    // The managed C# path (RustCLR) next to the native one (bflat).
    clr::boot_demo("ARM64");
    show_desktop();

    let uart = platform.as_ref().and_then(|p| p.uart.clone()).map(|u| u.0).unwrap_or_default();
    if timer_ok && !uart.is_empty() && info.loader == "limine" {
        println!(
            "[kernel] MILESTONE: ARM64 BOOT OK (Limine -> EL{} kernel; live DTB -> {} console + {} + timer; framebuffer {})",
            cpu::current_el(),
            uart,
            gic_name,
            if info.framebuffer.is_some() { "on" } else { "absent" }
        );
    }
    let raspberry_pi = fdt.as_ref().is_some_and(|f| f.root().is_compatible("brcm,bcm2711"));
    if timer_ok && !uart.is_empty() && raspberry_pi {
        println!(
            "[kernel] MILESTONE: RPI4 BOOT OK ({} via {} -> EL{} kernel; {} console, {} + generic timer from the DTB)",
            platform.as_ref().map(|p| p.model.as_str()).unwrap_or("?"),
            info.loader,
            cpu::current_el(),
            uart,
            gic_name
        );
    }
    println!("[kernel] BUITENZORG ARM64 READY -- increments 1-8 (boot, interrupts, memory, EL0, scheduler, virtio-mmio, RTC, PSCI, shared syscalls + window system, C# GUI apps, interactive desktop); the rest of the kernel follows");
    // The interactive desktop session, shared with the PC kernel: virtio
    // keyboard / tablet -> terminal + window manager, frames through the
    // framebuffer and the virtio GPU.
    desktop::run()
}

/// The generic memory subsystem on AArch64 page tables:
/// - the heap (mapped page by page through `arch::paging`) holds a 1 MiB
///   vector intact;
/// - a kernel page is mapped, written, made read-only, and unmapped, and its
///   frame goes back to the allocator;
/// - a **user** page (TTBR0, lower half) reports EL0 read/write through
///   `user_access` (what syscall pointer validation uses), read-only after
///   `protect`, nothing after unmap — and `validate_user_range` agrees;
/// - a device page maps uncached in the MMIO window.
fn memory_self_test() -> bool {
    use arch::paging::{self, Flags};
    let mut ok = true;
    let mut check = |what: &str, cond: bool| {
        if !cond {
            println!("[arm64] memory check failed: {}", what);
            ok = false;
        }
    };

    let v: alloc::vec::Vec<u64> = (0..131_072u64).map(|i| i * 3).collect();
    check("heap 1 MiB vector", v.iter().enumerate().all(|(i, &x)| x == i as u64 * 3));
    drop(v);

    const KVA: u64 = 0xFFFF_E000_0000_0000;
    const UVA: u64 = 0x40_0000;
    let (used0, _) = memory::frame_stats();
    let kernel = memory::with_ctx(|ctx| {
        let pa = ctx.frames.alloc()?;
        paging::map(&mut ctx.mapper, &mut ctx.frames, KVA, pa, Flags { write: true, user: false, exec: false }).ok()?;
        unsafe { (KVA as *mut u64).write_volatile(0xB0_17E2_20B6) };
        let back = unsafe { (KVA as *const u64).read_volatile() };
        let ro = paging::protect(&mut ctx.mapper, KVA, Flags { write: false, user: false, exec: false });
        let mapped = paging::is_mapped(&mut ctx.mapper, KVA);
        let freed = paging::unmap(&mut ctx.mapper, KVA)?;
        ctx.frames.free(freed);
        Some(back == 0xB0_17E2_20B6 && ro && mapped && freed == pa && !paging::is_mapped(&mut ctx.mapper, KVA))
    });
    check("kernel page map/write/protect/unmap", kernel == Some(true));

    let user = memory::with_ctx(|ctx| {
        let pa = ctx.frames.alloc()?;
        paging::map(&mut ctx.mapper, &mut ctx.frames, UVA, pa, Flags { write: true, user: true, exec: false }).ok()?;
        let rw = paging::user_access(UVA);
        paging::protect(&mut ctx.mapper, UVA, Flags { write: false, user: true, exec: false });
        let ro = paging::user_access(UVA);
        Some((pa, rw, ro))
    });
    let (rw, ro) = user.map(|u| (u.1, u.2)).unwrap_or((None, None));
    check("user page EL0 read/write", rw == Some((true, true)));
    check("user page read-only after protect", ro == Some((true, false)));
    check("validate_user_range: readable", memory::validate_user_range(UVA, 64, false));
    check("validate_user_range: not writable", !memory::validate_user_range(UVA, 64, true));
    check("validate_user_range: kernel address refused", !memory::validate_user_range(allocator::HEAP_START, 64, false));
    if let Some((pa, _, _)) = user {
        memory::with_ctx(|ctx| {
            if let Some(f) = paging::unmap(&mut ctx.mapper, UVA) {
                ctx.frames.free(f);
            }
            let _ = pa;
        });
    }
    check("user page gone after unmap", paging::user_access(UVA).is_none());
    let (used1, _) = memory::frame_stats();
    let leaked = memory::frames_in_use();
    check("device window", memory::map_mmio(0x0900_0000, 0x1000).is_some());

    if ok {
        println!(
            "[kernel] MILESTONE: ARM64 MEMORY OK (frame allocator + paging HAL on AArch64: heap via TTBR1, kernel page map/protect/unmap, user page in TTBR0 with EL0 rights checked, frames returned; {} frames handed out, {} in use)",
            used1 - used0,
            leaked
        );
    }
    ok
}

/// Two processes' address spaces map the same user address to different
/// frames: installing one (TTBR0) shows its own data, the other its own, the
/// kernel space neither; destroying both returns every frame.
fn aspace_self_test() {
    use arch::paging::{self, Flags};
    const UVA: u64 = 0x40_0000;
    let before = memory::frames_in_use();
    let user = Flags { write: true, user: true, exec: false };
    let mut seen = [0u64; 2];
    let mut spaces = alloc::vec::Vec::new();
    for (k, value) in [0xA11CE_u64, 0xB0B_u64].into_iter().enumerate() {
        let Ok(s) = aspace::create(1000 + k as u64) else { break };
        aspace::install(s);
        let ok = memory::with_ctx(|ctx| {
            let pa = ctx.frames.alloc()?;
            paging::map(&mut ctx.mapper, &mut ctx.frames, UVA, pa, user).ok()
        });
        if ok.is_some() {
            unsafe { (UVA as *mut u64).write_volatile(value) };
        }
        spaces.push(s);
    }
    for (k, s) in spaces.iter().enumerate() {
        aspace::install(*s);
        seen[k] = unsafe { (UVA as *const u64).read_volatile() };
    }
    aspace::install(aspace::kernel());
    let hidden = paging::user_access(UVA).is_none();
    let freed: u64 = spaces.into_iter().map(aspace::destroy).sum();
    let leaked = memory::frames_in_use() as i64 - before as i64;
    let ok = seen == [0xA11CE, 0xB0B] && hidden && leaked == 0;
    println!(
        "[arm64] address spaces: space 1000 reads {:#x}, space 1001 reads {:#x} at the same address; kernel space sees nothing: {}; {} frames freed, {} leaked",
        seen[0], seen[1], hidden, freed, leaked
    );
    if ok {
        println!("[kernel] MILESTONE: ARM64 ASPACE OK (a TTBR0 root per process, shared TTBR1 kernel half; switch = TTBR0 + TLB invalidate; teardown frees every frame)");
    }
}

// ---- Kernel threads: context switch + FP state ---------------------------

const SWITCH_ROUNDS: u64 = 1000;
static mut CTX_SP: [u64; 3] = [0; 3]; // 0 = boot flow, 1 = thread A, 2 = thread B
static mut CTX_FPU: Option<[alloc::boxed::Box<arch::context::FpuState>; 3]> = None;
static CTX_ERRORS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static CTX_SWITCHES: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

fn ctx_switch(from: usize, to: usize) {
    use core::ptr::addr_of_mut;
    CTX_SWITCHES.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    unsafe {
        let sp = &mut *addr_of_mut!(CTX_SP);
        let fpu = (*addr_of_mut!(CTX_FPU)).as_mut().unwrap();
        let prev: *mut arch::context::FpuState = &mut *fpu[from];
        let next: *const arch::context::FpuState = &*fpu[to];
        let next_sp = sp[to];
        arch::context::switch(&mut sp[from], next_sp, prev, next);
    }
}

fn ctx_worker(me: usize, other: usize, tag: u64) {
    use arch::aarch64::context::{fp_get_d0, fp_set_d0};
    for i in 0..SWITCH_ROUNDS {
        fp_set_d0(tag + i);
        ctx_switch(me, other);
        if fp_get_d0() != tag + i {
            CTX_ERRORS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }
    }
}

extern "C" fn ctx_thread_a() -> ! {
    ctx_worker(1, 2, 0xA000_0000);
    ctx_switch(1, 0); // back to the boot flow for good
    loop {
        arch::cpu::halt();
    }
}

extern "C" fn ctx_thread_b() -> ! {
    ctx_worker(2, 1, 0xB000_0000);
    loop {
        ctx_switch(2, 1);
    }
}

/// Two kernel threads on their own stacks ping-pong `SWITCH_ROUNDS` times
/// through `arch::context::switch`; each keeps a different value in the FP
/// register d0 across every switch (the FP state follows the task), and the
/// boot flow's own d0 survives too.
fn context_self_test() {
    use core::ptr::addr_of_mut;
    let stacks: [alloc::vec::Vec<u8>; 2] = [alloc::vec![0u8; 16 * 1024], alloc::vec![0u8; 16 * 1024]];
    unsafe {
        *addr_of_mut!(CTX_FPU) = Some([
            arch::context::FpuState::clean(),
            arch::context::FpuState::clean(),
            arch::context::FpuState::clean(),
        ]);
        let sp = &mut *addr_of_mut!(CTX_SP);
        sp[1] = arch::context::prepare_stack(&stacks[0], ctx_thread_a);
        sp[2] = arch::context::prepare_stack(&stacks[1], ctx_thread_b);
    }
    arch::aarch64::context::fp_set_d0(0x0B007);
    ctx_switch(0, 1);
    let boot_fp_kept = arch::aarch64::context::fp_get_d0() == 0x0B007;
    let switches = CTX_SWITCHES.load(core::sync::atomic::Ordering::Relaxed);
    let errors = CTX_ERRORS.load(core::sync::atomic::Ordering::Relaxed);
    core::mem::forget(stacks); // thread B stays parked on its stack
    println!(
        "[arm64] context switch: {} switches between two kernel threads, {} FP mismatches, boot flow's d0 kept: {}",
        switches, errors, boot_fp_kept
    );
    if errors == 0 && boot_fp_kept && switches >= 2 * SWITCH_ROUNDS {
        println!("[kernel] MILESTONE: ARM64 CONTEXT OK (arch::context on AArch64: x19-x30 + SP + DAIF, q0-q31/FPCR/FPSR per task)");
    }
}

// ---- User mode (EL0) ---------------------------------------------------------

static USER_FAULT_FAR: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Exit code of a user program ended by a fault (as on x86).
const FAULT_EXIT_CODE: u64 = 0xDEAD;

/// `SVC #0` from EL0 (x8 = number, x0..x2 = arguments): the counterpart of
/// x86 `usermode::syscall_dispatch_c` -- clock reads lock-free; BKL-free
/// syscalls (`syscall::bkl_free`) skip the Big Kernel Lock; served with
/// interrupts enabled (the exception frame holds ELR/SPSR/SP_EL0); then the
/// shared `syscall.rs` dispatcher, pointer validation included.
pub(crate) fn el0_syscall(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    use bz_abi::sysno as n;
    match nr {
        n::TICKS => return interrupts::ticks(),
        n::CLOCK_MONO => return arch::cpu::timestamp(),
        _ => {}
    }
    let fine = syscall::bkl_free(nr);
    if !fine {
        task::kernel_enter();
    }
    arch::cpu::enable_interrupts();
    task::note_syscall(fine);
    let r = syscall::dispatch_from_user(nr, a0, a1, a2);
    arch::cpu::disable_interrupts();
    task::kernel_exit();
    r
}

/// A fatal exception in EL0 code: end that program (exit code 0xDEAD), like
/// the x86 ring-3 #PF/#GP containment.
pub(crate) fn el0_fault(esr: u64, far: u64, elr: u64) -> ! {
    if !usermode::user_active() {
        panic!("EL0 fault outside a user program: ESR={:#x} FAR={:#x} ELR={:#x}", esr, far, elr);
    }
    println!(
        "[arm64] user fault: ESR_EL1={:#x} (EC={:#x}) at {:#x}, address {:#x} -- program terminated",
        esr,
        esr >> 26,
        elr,
        far
    );
    USER_FAULT_FAR.store(far, core::sync::atomic::Ordering::SeqCst);
    task::kernel_enter();
    usermode::exit_user(FAULT_EXIT_CODE)
}

/// Run `code` as an EL0 program on the boot flow itself (before the
/// scheduler starts), in a fresh address space: code page at 0x40_0000 (user,
/// read-only, executable), one stack page below 0x7000_0000. Returns (exit
/// code, frames leaked by the space).
fn run_el0(id: u64, code: &[u8]) -> Option<(u64, i64)> {
    use arch::paging::{self, Flags};
    const ENTRY: u64 = 0x40_0000;
    const STACK_TOP: u64 = 0x7000_0000;
    let before = memory::frames_in_use() as i64;
    let space = aspace::create(id).ok()?;
    aspace::install(space);
    memory::with_ctx(|ctx| {
        let pa = ctx.frames.alloc()?;
        let dst = memory::phys_to_virt(pa) as *mut u8;
        unsafe {
            core::ptr::write_bytes(dst, 0, 4096);
            core::ptr::copy_nonoverlapping(code.as_ptr(), dst, code.len().min(4096));
        }
        paging::map(&mut ctx.mapper, &mut ctx.frames, ENTRY, pa, Flags { write: false, user: true, exec: true }).ok()?;
        let st = ctx.frames.alloc()?;
        unsafe { core::ptr::write_bytes(memory::phys_to_virt(st) as *mut u8, 0, 4096) };
        paging::map(&mut ctx.mapper, &mut ctx.frames, STACK_TOP - 4096, st, Flags { write: true, user: true, exec: false }).ok()
    })?;
    usermode::sync_icache();
    let code = usermode::enter_user(ENTRY, STACK_TOP);
    aspace::install(aspace::kernel());
    aspace::destroy(space);
    Some((code, memory::frames_in_use() as i64 - before))
}

/// Two EL0 programs: one prints through DEBUG_WRITE, asks ABI_VERSION, spins
/// long enough to be interrupted by the timer *while in EL0*, computes
/// 1+..+100 and exits with (abi << 16) + 5050; the other reads a kernel-half
/// address and must be terminated by the fault, not crash the kernel.
fn el0_self_test() {
    use arch::aarch64::exceptions;
    let (demo, fault) = usermode::demo_programs();
    let (irq0, _) = exceptions::el0_stats();
    let r1 = run_el0(3000, demo);
    let (irq1, svcs) = exceptions::el0_stats();
    let r2 = run_el0(3001, fault);
    let want = (bz_abi::ABI_VERSION << 16) + 5050;
    let far = USER_FAULT_FAR.load(core::sync::atomic::Ordering::SeqCst);
    println!(
        "[arm64] EL0 program 1: exit {:#x} (want {:#x}), {} SVC calls, {} timer IRQs taken in EL0, leaked {:?} frames; program 2: exit {:#x}, fault address {:#x}, leaked {:?}",
        r1.map(|r| r.0).unwrap_or(0),
        want,
        svcs,
        irq1 - irq0,
        r1.map(|r| r.1),
        r2.map(|r| r.0).unwrap_or(0),
        far,
        r2.map(|r| r.1)
    );
    let ok = r1 == Some((want, 0))
        && svcs >= 3
        && irq1 - irq0 >= 3
        && r2 == Some((FAULT_EXIT_CODE, 0))
        && far == 0xFFFF_D000_0000_0000;
    if ok {
        println!("[kernel] MILESTONE: ARM64 EL0 OK (user mode in its own TTBR0 space: SVC syscalls, timer IRQs from EL0, EXIT back to the kernel, a kernel-address read terminates only the program)");
    }
}

// ---- Processes (the shared scheduler) -----------------------------------------

/// Run the ELF programs Limine handed over as modules, as processes of the
/// shared scheduler (`task.rs`, the same code as on x86):
/// - one at a time: hello (the v0.4 milestone), matang, heap, gcmem;
/// - **two copies of `mproc` at once**: each spins ~2 s without yielding,
///   keeps a running sum in an FP register and stamps its own mmap page at the
///   same virtual address -- they can only both finish (exact sums, own
///   pages) if the timer preempts EL0 and every switch saves FP state and
///   swaps TTBR0;
/// - `thread` (ring-3 threads + join) and `sync` (futex-backed mutex).
fn run_boot_programs(info: &boot::BootInfo) {
    // From /disk (virtio-blk) when it is mounted, else from the Limine modules.
    let from_disk = core::cell::Cell::new(false);
    let find = |name: &str| -> Option<alloc::vec::Vec<u8>> {
        let path = alloc::format!("/disk/{}.ELF", name.to_uppercase());
        match vfs::read(&path) {
            Ok(bytes) => {
                from_disk.set(true);
                Some(bytes)
            }
            Err(_) => {
                from_disk.set(false);
                info.modules().iter().find(|m| m.name() == name).map(|m| m.bytes().to_vec())
            }
        }
    };
    let frames0 = memory::frames_in_use();
    for name in ["hello", "matang", "heap", "gcmem", "bcl", "proctest"] {
        let Some(image) = find(name) else { continue };
        println!("[arm64] --- program '{}' ({} bytes, from {}) ---", name, image.len(), if from_disk.get() { "/disk" } else { "a Limine module" });
        match app::spawn_image(name, &image) {
            Ok(pid) => {
                let code = app::wait(pid);
                println!("[arm64] --- '{}' (pid {}) exited with code {:#x} ---", name, pid, code);
                if name == "hello" && code == 0 {
                    println!("[kernel] MILESTONE: ARM64 CSHARP OK (C# compiled for linux-arm64 by bflat, loaded by the generic ELF loader, ran in EL0 as a process: Hello from C#!)");
                }
            }
            Err(e) => println!("[arm64] '{}': {}", name, e),
        }
    }

    if let Some(image) = find("mproc") {
        // Timer preemption is the point here: keep both copies on CPU 0 (the
        // x86 MULTIPROC test does the same); the SMP test below spreads them.
        let aps = task::set_ap_scheduling(false);
        let p0 = task::user_preemptions();
        let t0 = interrupts::ticks();
        let pids: alloc::vec::Vec<u64> = (0..2).filter_map(|i| app::spawn_image(if i == 0 { "mproc-a" } else { "mproc-b" }, &image).ok()).collect();
        let codes: alloc::vec::Vec<u64> = pids.iter().map(|&p| app::wait(p)).collect();
        let preempts = task::user_preemptions() - p0;
        let secs10 = (interrupts::ticks() - t0) * 10 / 18;
        println!(
            "[arm64] mproc x2: exit codes {:?}, {} timer preemptions of EL0 code, {}.{} s",
            codes,
            preempts,
            secs10 / 10,
            secs10 % 10
        );
        if pids.len() == 2 && codes.iter().all(|&c| c == 0) && preempts >= 10 {
            println!("[kernel] MILESTONE: ARM64 MULTIPROC OK (shared scheduler on AArch64: two processes preempted by the timer, each in its own TTBR0 space with its own FP state)");
        }
        task::set_ap_scheduling(aps);
        smp_processes_test(&image);
    }

    // GUI apps (window syscalls into the shared window manager) and the rest.
    for name in ["thread", "sync", "calc", "g2048", "ui", "draw", "xox"] {
        let Some(image) = find(name) else { continue };
        println!("[arm64] --- program '{}' ---", name);
        match app::spawn_image(name, &image) {
            Ok(pid) => println!("[arm64] --- '{}' exited with code {:#x} ---", name, app::wait(pid)),
            Err(e) => println!("[arm64] '{}': {}", name, e),
        }
    }
    let leaked = memory::frames_in_use() as i64 - frames0 as i64;
    println!("[arm64] programs done; frames leaked: {}", leaked);
}

// ---- Platform devices: clock, storage, display ----------------------------

/// The PL031 clock answers through the same `rtc::read` the FAT code stamps
/// files with.
fn rtc_self_test() {
    let (y, mo, d, h, mi, s) = rtc::read();
    println!("[arm64] PL031 clock: {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC (unix {})", y, mo, d, h, mi, s, rtc::unix_time());
    if (2024..2200).contains(&y) && (1..=12).contains(&mo) && (1..=31).contains(&d) {
        println!("[kernel] MILESTONE: ARM64 RTC OK (PL031 from the DTB, epoch -> calendar)");
    }
}

/// `/disk` on ARM64: the virtio-blk driver (shared with x86) over the
/// virtio-mmio transport, mounted by the shared FAT + VFS code. Lists the
/// root, reads BATANG.TXT, writes a file and reads it back.
fn storage_self_test() {
    let n = virtio::init_block();
    if n == 0 {
        println!("[arm64] storage: no virtio-blk device (QEMU: -device virtio-blk-device + -global virtio-mmio.force-legacy=false)");
        return;
    }
    let Some(dev) = driver::take_boot_block_device() else { return };
    if let Err(e) = vfs::mount("disk", dev, false) {
        println!("[arm64] storage: mount /disk failed: {}", e);
        return;
    }
    let names = vfs::list("/disk").unwrap_or_default();
    let batang = vfs::read("/disk/BATANG.TXT").ok().and_then(|b| alloc::string::String::from_utf8(b).ok());
    let (y, mo, d, h, mi, s) = rtc::read();
    let stamp = alloc::format!("BZARM64 written {:04}-{:02}-{:02} {:02}:{:02}:{:02}\n", y, mo, d, h, mi, s);
    let wrote = vfs::write("/disk/ARM64.TXT", stamp.as_bytes()).is_ok();
    let back = vfs::read("/disk/ARM64.TXT").ok();
    println!(
        "[arm64] /disk: {} entries, BATANG.TXT {}, ARM64.TXT = {}",
        names.len(),
        if batang.is_some() { "read" } else { "missing" },
        stamp.trim_end()
    );
    if names.len() > 5 && batang.as_deref().map_or(false, |t| t.contains("batang")) && wrote && back.as_deref() == Some(stamp.as_bytes()) {
        println!("[kernel] MILESTONE: ARM64 DISK OK (virtio-blk over virtio-mmio, shared FAT + VFS: /disk listed, read, written + read back)");
    }
}

/// virtio-gpu over virtio-mmio: copy the framebuffer console's pixels into a
/// back buffer and present it -- the driver prints `VIRTIO GPU OK` on its
/// first frame (the ARM64 desktop will present through it).
fn gpu_self_test(info: &boot::BootInfo) {
    let Some(fb) = info.framebuffer else { return };
    if fb.bytes_per_pixel != 4 {
        return;
    }
    virtio::gpu::init(fb.width, fb.height);
    let mut back = alloc::vec![0u32; fb.width * fb.height];
    for y in 0..fb.height {
        for x in 0..fb.width {
            let px = unsafe { ((fb.addr as *const u32).add(y * fb.stride + x)).read_volatile() };
            back[y * fb.width + x] = match fb.format {
                boot::PixelFormat::Rgb => ((px & 0xFF) << 16) | (px & 0xFF00) | ((px >> 16) & 0xFF),
                _ => px & 0x00FF_FFFF,
            };
        }
    }
    virtio::gpu::present(&back);
}

// ---- SMP --------------------------------------------------------------------

/// Start the other CPUs of the DTB's `/cpus` with PSCI CPU_ON; each joins
/// the shared scheduler. Verified by every CPU taking its own timer
/// interrupts.
fn start_secondary_cpus(info: &boot::BootInfo, fdt: Option<&dtb::Fdt>) {
    let (Some(f), Some(base)) = (fdt, info.kernel_base) else { return };
    let Some(cpus) = f.find_path("/cpus") else { return };
    let mpidrs: alloc::vec::Vec<u64> = (0..f.nodes.len())
        .filter(|&i| f.nodes[i].parent == Some(cpus) && f.nodes[i].strings("device_type").any(|s| s == "cpu"))
        .filter_map(|i| f.reg(i).first().map(|r| r.0))
        .collect();
    let method = f.find_path("/psci").and_then(|i| f.nodes[i].strings("method").next()).unwrap_or("");
    if mpidrs.len() < 2 || !power::init(method) {
        println!("[smp] single CPU ({} in the DTB)", mpidrs.len());
        return;
    }
    let online = arch::aarch64::smp::start_aps(&mpidrs, base);
    let irq0: alloc::vec::Vec<u64> = (0..online).map(arch::aarch64::exceptions::irqs_on).collect();
    let t0 = arch::cpu::timestamp();
    while arch::cpu::timestamp() - t0 < arch::aarch64::cpu::counter_hz() / 5 {
        arch::cpu::halt();
    }
    let ticked = (0..online).filter(|&c| arch::aarch64::exceptions::irqs_on(c) > irq0[c] + 5).count();
    println!(
        "[smp] {} of {} CPUs online via PSCI CPU_ON (MPIDRs {:x?}); {} took their own timer interrupts in 200 ms",
        online,
        mpidrs.len(),
        mpidrs,
        ticked
    );
    if online == mpidrs.len() && ticked == online {
        println!("[kernel] MILESTONE: ARM64 SMP OK ({} CPUs: PSCI CPU_ON + MMU-off trampoline, per-CPU GIC redistributor + timer, shared scheduler)", online);
    }
}

/// One `mproc` per CPU at once, the scheduler free to spread them: they must
/// all finish correctly, and at some timer interrupt several CPUs must have
/// been running user code simultaneously.
fn smp_processes_test(image: &[u8]) {
    let n = arch::aarch64::smp::cpus_online();
    if n < 2 {
        return;
    }
    arch::aarch64::exceptions::reset_parallel_peak();
    let sys0: u64 = (1..n).map(task::syscalls_on).sum();
    let pids: alloc::vec::Vec<u64> =
        (0..n).filter_map(|i| app::spawn_image(&alloc::format!("mproc-smp{}", i), image).ok()).collect();
    let codes: alloc::vec::Vec<u64> = pids.iter().map(|&p| app::wait(p)).collect();
    let peak = arch::aarch64::exceptions::parallel_peak();
    let ap_sys: u64 = (1..n).map(task::syscalls_on).sum::<u64>() - sys0;
    println!(
        "[smp] {} mproc copies: exit codes {:?}; at most {} CPUs ran user code at once; {} syscalls served on secondary CPUs; {} migrations",
        pids.len(),
        codes,
        peak,
        ap_sys,
        task::migrations()
    );
    if pids.len() == n && codes.iter().all(|&c| c == 0) && peak >= 2 && ap_sys > 0 {
        println!("[kernel] MILESTONE: ARM64 SMP SCHED OK (processes ran in parallel on {} CPUs, syscalls served on the secondary CPUs, exact FP results)", peak);
    }
}

// ---- The shared kernel on ARM64 ---------------------------------------------

/// Bring up the generic subsystems the PC kernel uses, on ARM64: the syscall
/// hardening test, PSCI power, the window manager (theme, icons, official
/// wallpaper), virtio input, and the network (loopback, then a virtio NIC).
fn shared_kernel_setup(info: &boot::BootInfo, fdt: Option<&dtb::Fdt>) {
    if syscall::security_self_test() {
        println!("[kernel] MILESTONE: SECURITY OK (syscall user-pointer validation)");
    } else {
        println!("[kernel] security self-test FAILED");
    }

    let method = fdt
        .and_then(|f| f.find_path("/psci").map(|i| f.nodes[i].strings("method").next().unwrap_or("").into()))
        .unwrap_or(alloc::string::String::new());
    if power::init(&method) {
        let v = arch::aarch64::psci::call(arch::aarch64::psci::PSCI_VERSION, 0, 0, 0);
        println!(
            "[kernel] MILESTONE: ARM64 PSCI OK (conduit {}, PSCI {}.{}: SYSTEM_OFF / SYSTEM_RESET for shutdown / restart)",
            method,
            v >> 16,
            v & 0xFFFF
        );
    }

    if let Some(fb) = info.framebuffer {
        wm::init(fb.width, fb.height);
        mouse::set_bounds(fb.width, fb.height);
        theme::set_default();
        let n = icons::load();
        let wp = wallpaper::set_official("nature").is_ok();
        println!("[arm64] window system: {}x{}, {} app icons, wallpaper 'nature' loaded: {}", fb.width, fb.height, n, wp);
    }
    virtio::input::init();

    net::init([10, 0, 0, 1]);
    net::send_ping([10, 0, 0, 1], 0xA64, 1);
    for _ in 0..4 {
        if net::poll() == 0 {
            break;
        }
    }
    if net::counters().0 > 0 {
        println!("[kernel] MILESTONE: NETWORK OK (Ethernet/ARP/IPv4/ICMP stack, loopback on ARM64)");
    }
    if let Some(nic) = virtio::net::probe() {
        const ME: [u8; 4] = [10, 0, 2, 15];
        const GW: [u8; 4] = [10, 0, 2, 2];
        net::attach(alloc::boxed::Box::new(nic), ME);
        let icmp0 = net::counters().0;
        let wait = |done: &dyn Fn() -> bool| {
            let t0 = interrupts::ticks();
            while !done() && interrupts::ticks() - t0 < 36 {
                net::poll();
                task::yield_now();
            }
            done()
        };
        net::arp_request(GW);
        if wait(&|| net::resolve(GW).is_some()) {
            net::send_ping(GW, 0xA64, 2);
            if wait(&|| net::counters().0 > icmp0) {
                println!("[kernel] MILESTONE: VIRTIO NET OK (virtio-net over virtio-mmio: ARP + ICMP echo with the gateway 10.0.2.2)");
            }
        }
    }
}

/// Compose the desktop -- wallpaper, taskbar, and the windows the GUI apps
/// left -- with the shared window manager, and present it (framebuffer +
/// virtio-gpu). From here the console stops drawing over the screen.
fn show_desktop() {
    let Some((w, h)) = framebuffer::dimensions() else { return };
    let windows = wm::window_count();
    wm::tile();
    let mut back = alloc::vec::Vec::new();
    wm::render_frame(&mut back, w, h);
    let lit = back.iter().filter(|&&p| p != 0).count();
    interactive::set_active();
    framebuffer::present(&back);
    println!("[arm64] desktop frame: {}x{}, {} windows, {} lit pixels", w, h, windows, lit);
    if windows >= 3 && lit > w * h / 2 {
        println!("[kernel] MILESTONE: ARM64 DESKTOP FRAME OK (shared window manager on ARM64: C# GUI app windows composited over the wallpaper and presented)");
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("\n[kernel] PANIC: {}", info);
    loop {
        arch::cpu::halt();
    }
}
