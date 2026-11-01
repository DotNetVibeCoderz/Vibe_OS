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

mod allocator;
mod arch;
mod boot;
mod dtb;
mod elf;
mod framebuffer;
mod limine;
mod logo;
mod memory;
mod platform;

pub(crate) use platform::virt::pl011 as serial;

/// Mirrors the PC kernel's flag; the ARM64 desktop is not up yet.
mod interactive {
    pub fn is_active() -> bool {
        false
    }
}

pub(crate) use arch::aarch64::aspace;

use core::panic::PanicInfo;

pub(crate) fn kernel_main(info: &'static boot::BootInfo) -> ! {
    use arch::aarch64::{cpu, exceptions, gic, timer};

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

    // Interrupts: GICv3 + the virtual timer at 100 Hz.
    let gic_regs = fdt.as_ref().and_then(|f| {
        let i = f.compatible("arm,gic-v3").next()?;
        let r = f.reg(i);
        Some((r.first()?.0, r.get(1)?.0))
    });
    let mut timer_ok = false;
    if let Some((gicd_pa, gicr_pa)) = gic_regs {
        let mapped = memory::map_mmio(gicd_pa, 0x1_0000).zip(memory::map_mmio(gicr_pa, 0x2_0000)).ok_or("cannot map the GIC");
        match mapped.and_then(|(d, r)| gic::init(d, r)) {
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
                    "[arm64] GICv3 at {:#x}/{:#x}: {} timer interrupts in {} ms ({} IRQs taken)",
                    gicd_pa, gicr_pa, n, ms, exceptions::irq_count()
                );
                // 50 ticks at 100 Hz = 500 ms of counter time; never faster,
                // but under TCG on a busy host interrupts can arrive late.
                timer_ok = n >= 50 && (450..=2000).contains(&ms);
                if timer_ok {
                    println!("[kernel] MILESTONE: ARM64 TIMER OK (GICv3 distributor + redistributor + ICC sysregs; EL1 virtual timer PPI 27 at 100 Hz)");
                }
            }
            Err(e) => println!("[arm64] GIC: {}", e),
        }
    } else {
        println!("[arm64] no arm,gic-v3 in the Device Tree");
    }

    el0_self_test();
    run_boot_programs(info);

    let uart = platform.as_ref().and_then(|p| p.uart.clone()).map(|u| u.0).unwrap_or_default();
    if timer_ok && !uart.is_empty() && info.loader == "limine" {
        println!(
            "[kernel] MILESTONE: ARM64 BOOT OK (Limine -> EL{} kernel; live DTB -> {} console + GICv3 + timer; framebuffer {})",
            cpu::current_el(),
            uart,
            if info.framebuffer.is_some() { "on" } else { "absent" }
        );
    }
    println!("[kernel] BUITENZORG ARM64 READY -- increments 1-4 (boot, interrupts, memory, threads + EL0, ELF + C# programs); the rest of the kernel follows");
    loop {
        cpu::enable_and_halt();
    }
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

static USER_CTX: core::sync::atomic::AtomicPtr<arch::aarch64::user::KernelCtx> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());
static USER_WRITES: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static USER_FAULT_FAR: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// What user programs wrote (DEBUG_WRITE), for the self-tests.
static USER_OUTPUT: spin::Mutex<alloc::string::String> = spin::Mutex::new(alloc::string::String::new());

/// Exit code of a user program ended by a fault (as on x86).
const FAULT_EXIT_CODE: u64 = 0xDEAD;

/// `SVC #0` from EL0: x8 = number, x0..x2 = arguments (the bz-abi numbers,
/// the same table as on x86). Called from the exception handler.
pub(crate) fn el0_syscall(nr: u64, a0: u64, a1: u64, _a2: u64) -> u64 {
    use bz_abi::{syserr, sysno};
    match nr {
        sysno::ABI_VERSION => bz_abi::ABI_VERSION,
        sysno::DEBUG_WRITE => {
            if a1 > 4096 || !memory::validate_user_range(a0, a1, false) {
                return syserr::INVAL;
            }
            let bytes: alloc::vec::Vec<u8> =
                (0..a1).map(|i| unsafe { ((a0 + i) as *const u8).read_volatile() }).collect();
            let text = core::str::from_utf8(&bytes).unwrap_or("<not UTF-8>");
            print!("{}", text);
            let mut out = USER_OUTPUT.lock();
            if out.len() < 4096 {
                out.push_str(text);
            }
            USER_WRITES.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            a1
        }
        sysno::EXIT => {
            let ctx = USER_CTX.load(core::sync::atomic::Ordering::SeqCst);
            if ctx.is_null() {
                return syserr::INVAL;
            }
            arch::aarch64::user::return_to_kernel(unsafe { &*ctx }, a0)
        }
        sysno::TICKS => arch::aarch64::timer::ticks(),
        // The generic user-memory PAL (the C# runtime's heap): same code as x86.
        sysno::MMAP => memory::user_mmap(a0, a1),
        sysno::MPROTECT => memory::user_mprotect(a0, a1, _a2),
        sysno::MUNMAP => memory::user_munmap(a0, a1),
        sysno::CLOCK_MONO => arch::cpu::timestamp(),
        _ => syserr::NOSYS,
    }
}

/// A fatal exception in EL0 code: end the program (exit code 0xDEAD).
pub(crate) fn el0_fault(esr: u64, far: u64, elr: u64) -> ! {
    let ctx = USER_CTX.load(core::sync::atomic::Ordering::SeqCst);
    if ctx.is_null() {
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
    arch::aarch64::user::return_to_kernel(unsafe { &*ctx }, FAULT_EXIT_CODE)
}

/// Run `code` as an EL0 program in a fresh address space: code page at
/// 0x40_0000 (user, read-only, executable), one stack page below 0x7000_0000.
/// Returns (exit code, frames leaked by the space).
fn run_el0(id: u64, code: &[u8]) -> Option<(u64, i64)> {
    use arch::aarch64::user;
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
    user::sync_icache();
    let mut kctx = user::KernelCtx::new();
    USER_CTX.store(&mut kctx, core::sync::atomic::Ordering::SeqCst);
    let code = user::enter_user(ENTRY, STACK_TOP, &mut kctx);
    USER_CTX.store(core::ptr::null_mut(), core::sync::atomic::Ordering::SeqCst);
    aspace::install(aspace::kernel());
    aspace::destroy(space);
    Some((code, memory::frames_in_use() as i64 - before))
}

/// Two EL0 programs: one prints through DEBUG_WRITE, asks ABI_VERSION, spins
/// long enough to be interrupted by the timer *while in EL0*, computes
/// 1+..+100 and exits with (abi << 16) + 5050; the other reads a kernel-half
/// address and must be terminated by the fault, not crash the kernel.
fn el0_self_test() {
    use arch::aarch64::{exceptions, user};
    let (demo, fault) = user::demo_programs();
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
        && USER_WRITES.load(core::sync::atomic::Ordering::Relaxed) >= 1
        && irq1 - irq0 >= 3
        && r2 == Some((FAULT_EXIT_CODE, 0))
        && far == 0xFFFF_D000_0000_0000;
    if ok {
        println!("[kernel] MILESTONE: ARM64 EL0 OK (user mode in its own TTBR0 space: SVC syscalls, timer IRQs from EL0, EXIT back to the kernel, a kernel-address read terminates only the program)");
    }
}

// ---- ELF programs from boot modules ---------------------------------------

/// Load an ELF program into a fresh address space with the generic loader
/// (`elf.rs`, the same as on x86 -- it checks `e_machine` = EM_AARCH64 here),
/// give it a 128 KiB stack below 0x7000_0000, run it in EL0, tear it down.
/// Returns (exit code, frames leaked).
fn run_elf(id: u64, image: &[u8]) -> Result<(u64, i64), &'static str> {
    use arch::aarch64::user;
    const STACK_BASE: u64 = 0x7000_0000 - 32 * 4096;
    let before = memory::frames_in_use() as i64;
    let space = aspace::create(id)?;
    aspace::install(space);
    let loaded = memory::with_ctx(|ctx| {
        let program = elf::load(image, ctx)?;
        let (stack_top, _) = memory::map_user_region(STACK_BASE, 32, ctx)?;
        Ok::<_, &'static str>((program.entry, stack_top))
    });
    let (entry, stack_top) = match loaded {
        Ok(v) => v,
        Err(e) => {
            aspace::install(aspace::kernel());
            aspace::destroy(space);
            return Err(e);
        }
    };
    user::sync_icache();
    let mut kctx = user::KernelCtx::new();
    USER_CTX.store(&mut kctx, core::sync::atomic::Ordering::SeqCst);
    let code = user::enter_user(entry, stack_top, &mut kctx);
    USER_CTX.store(core::ptr::null_mut(), core::sync::atomic::Ordering::SeqCst);
    aspace::install(aspace::kernel());
    aspace::destroy(space);
    Ok((code, memory::frames_in_use() as i64 - before))
}

/// Run every ELF the loader handed over as a module (`module_path` in
/// limine.conf). `hello` is the v0.4 milestone program: C# compiled by bflat
/// for linux-arm64 with zerolib, linked with the aarch64 shim.
fn run_boot_programs(info: &boot::BootInfo) {
    let mut id = 4000;
    for m in info.modules() {
        let image = m.bytes();
        if image.len() < 4 || &image[..4] != b"\x7fELF" {
            continue;
        }
        println!("[arm64] --- program '{}' ({} bytes, from a Limine module) ---", m.name(), image.len());
        USER_OUTPUT.lock().clear();
        let r = run_elf(id, image);
        id += 1;
        match r {
            Ok((code, leaked)) => {
                println!("[arm64] --- '{}' exited with code {:#x}, {} frames leaked ---", m.name(), code, leaked);
                if m.name() == "hello" && code == 0 && leaked == 0 && USER_OUTPUT.lock().contains("Hello from C#!") {
                    println!("[kernel] MILESTONE: ARM64 CSHARP OK (C# compiled for linux-arm64 by bflat, loaded by the generic ELF loader, ran in EL0: Hello from C#!)");
                }
            }
            Err(e) => println!("[arm64] '{}': {}", m.name(), e),
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("\n[kernel] PANIC: {}", info);
    loop {
        arch::cpu::halt();
    }
}
