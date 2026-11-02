// The RISC-V kernel (multi-arch M4). Included by main.rs on riscv64. Loaded
// by Limine (UEFI: OpenSBI -> edk2 -> Limine) on QEMU `virt`, entered in
// S-mode with Sv48 paging and the kernel in the higher half.
//
// Increment 1: boot hand-off, SBI console then the NS16550 UART from the live
// Device Tree, the trap vector, the generic memory subsystem (`memory.rs` +
// `allocator.rs`) over the RISC-V paging HAL, per-process address spaces,
// and the SBI timer as the periodic tick. Increment 2: kernel-thread context
// switch (with per-task F/D state) and user mode -- U-mode programs in their
// own address space calling the kernel with `ECALL`, faults contained.
// Increment 3: ELF programs (Rust, `userland/riscv`; Limine boot modules) run
// as processes of the shared scheduler (`task.rs`, `process.rs`) --
// preemptive, with ring-3 threads and futexes. Increment 4: the PLIC
// (device interrupts), the Goldfish RTC, and virtio-mmio -- /disk on
// virtio-blk through the shared FAT + VFS code (programs now load from it)
// and virtio-gpu. Increment 5: the rest of the PC kernel's generic code
// builds for RISC-V too -- the real `syscall.rs` (same dispatcher, pointer
// validation, BKL-free paths) and `app.rs`, the window system, shell,
// network, packages, VMM, the JS/TS/Python runtime -- with SBI SRST power,
// virtio input / net, Rust GUI apps, and the interactive desktop.
// Increment 6: SMP -- secondary harts started through SBI HSM join the
// shared scheduler. The rest of the shared kernel joins
// as subsystems gain an `arch/riscv64` backend -- the path the ARM64 port
// (M3) took.

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
mod usb;
mod virtio;
mod vmm;
mod wallpaper;
mod wm;

pub(crate) use arch::riscv64::{aspace, gdt, interrupts, smp, usermode, vmx};
pub(crate) use platform::virt::goldfish_rtc as rtc;
pub(crate) use platform::virt::uart16550 as serial;
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
    use arch::riscv64::{cpu, paging, sbi, timer, trap};

    // The boot hart is CPU 0 (`tp` may hold anything the loader left).
    cpu::set_id(0);
    trap::init();
    arch::riscv64::context::enable_fp();
    if let Some(fb) = info.framebuffer {
        framebuffer::init(fb);
    }

    // Memory: the generic frame allocator + paging context, then the heap --
    // the same code as on the PC and ARM64, over the RISC-V paging HAL.
    let mapper = unsafe { memory::init(info.phys_offset) };
    let mut frames = unsafe { memory::BootInfoFrameAllocator::init(info.regions()) };
    let kernel_root = paging::init_user_half(&mut frames);
    let usable_mib = frames.usable_frame_count() * 4096 / (1024 * 1024);
    memory::install_ctx(mapper, frames);
    memory::set_total_usable_mib(usable_mib as u64);
    allocator::init().expect("heap init failed");
    aspace::init(kernel_root.expect("no frame for the kernel root"));

    // The Device Tree: console UART, timebase.
    let fdt = info.firmware.dtb.and_then(|p| unsafe { dtb::Fdt::from_phys(p) }.ok());
    let platform = fdt.as_ref().map(dtb::Platform::discover);
    if let Some(f) = fdt.as_ref() {
        if let Some(hz) = f.find_path("/cpus").and_then(|i| f.nodes[i].u32_prop("timebase-frequency")) {
            cpu::set_counter_hz(hz as u64);
        }
    }
    let uart = platform.as_ref().and_then(|p| p.uart.clone());
    if let Some((_, base)) = uart.as_ref() {
        if let Some(va) = memory::map_mmio(*base, 0x1000) {
            serial::set_base(va);
        }
    }

    print!("{}", logo::BOOT_LOGO);
    println!();
    println!("[kernel] Hello Kernel -- Buitenzorg OS (RISC-V / RV64)");
    println!("[kernel] MILESTONE: HELLO KERNEL OK");
    let (smaj, smin, imp, iver) = sbi::version();
    println!(
        "[riscv] S-mode on hart {} (DTB boot hart {}); SBI {}.{} ({}, version {:#x}); {} paging; timebase {} MHz; trap vector installed",
        info.bsp_hartid.map_or(alloc::string::String::from("?"), |h| alloc::format!("{}", h)),
        fdt.as_ref().map_or(0, |f| f.boot_cpu),
        smaj,
        smin,
        match imp {
            0 => "BBL",
            1 => "OpenSBI",
            2 => "Xvisor",
            3 => "KVM",
            _ => "other",
        },
        iver,
        paging::mode_name(),
        cpu::counter_hz() / 1_000_000
    );
    println!("[boot] {}", boot::describe(info));
    println!(
        "[kernel] physical memory: {} MiB usable; {} MiB kernel heap at {:#x}; kernel root at {:#x}",
        usable_mib,
        allocator::HEAP_SIZE / (1024 * 1024),
        allocator::HEAP_START,
        kernel_root.unwrap_or(0)
    );
    if memory_self_test() {
        println!("[kernel] MILESTONE: MEMORY OK");
    }
    aspace_self_test();
    context_self_test();
    match platform.as_ref() {
        Some(p) => println!("[riscv] platform (live DTB): {}", p.summary()),
        None => println!("[riscv] no Device Tree from the loader"),
    }
    dtb::self_test();

    // Platform devices from the DTB: the Goldfish clock, the virtio-mmio
    // slots, the PLIC.
    if let Some(f) = fdt.as_ref() {
        if let Some(i) = f.compatible("google,goldfish-rtc").next() {
            if let Some(va) = f.reg(i).first().and_then(|r| memory::map_mmio(r.0, 0x1000)) {
                rtc::init(va);
            }
        }
    }
    if let Some(p) = platform.as_ref() {
        virtio::mmio::set_slots(&p.virtio_mmio);
    }
    let hart = info.bsp_hartid.unwrap_or(fdt.as_ref().map_or(0, |f| f.boot_cpu as u64));
    let plic_up = fdt.as_ref().map_or(false, |f| plic_init(f, hart));
    rtc_self_test();
    storage_self_test();

    // The periodic tick: the SBI timer at 100 Hz.
    let has_time = sbi::probe(sbi::EXT_TIME);
    timer::start(100);
    cpu::sie_set(cpu::SIE_SSIE); // wake-up IPIs from other harts
    cpu::enable_interrupts();
    let t0 = cpu::timestamp();
    let hz = cpu::counter_hz();
    while timer::ticks() < 50 && cpu::timestamp() - t0 < hz * 3 {
        cpu::halt();
    }
    let ms = (cpu::timestamp() - t0) * 1000 / hz.max(1);
    let n = timer::ticks();
    println!(
        "[riscv] SBI timer (TIME extension: {}): {} timer interrupts in {} ms ({} interrupts taken)",
        has_time,
        n,
        ms,
        trap::irq_count()
    );
    // 50 ticks at 100 Hz = 500 ms; under TCG on a busy host they can be late.
    let timer_ok = n >= 50 && (450..=2000).contains(&ms);
    if timer_ok {
        println!("[kernel] MILESTONE: RISCV TIMER OK (stvec trap vector; SBI set_timer at 100 Hz on the timebase from the DTB)");
    }

    if plic_up {
        plic_self_test(fdt.as_ref());
    }
    umode_self_test();
    // The shared scheduler (task.rs): the boot flow becomes task 0, an idle
    // task joins, and programs run as preemptively scheduled processes.
    task::init();
    start_secondary_harts(info, fdt.as_ref());
    shared_kernel_setup(info);
    polyglot_test();
    run_boot_programs(info);
    // The managed C# path (RustCLR): C# on RISC-V, where bflat has no target.
    clr::boot_demo("RISC-V");
    gpu_self_test(info);
    show_desktop();
    if timer_ok && uart.is_some() && info.loader == "limine" {
        println!(
            "[kernel] MILESTONE: RISCV BOOT OK (Limine -> S-mode kernel; live DTB -> {} console + SBI timer; {}; framebuffer {})",
            uart.as_ref().map_or("", |u| u.0.as_str()),
            paging::mode_name(),
            if info.framebuffer.is_some() { "on" } else { "absent" }
        );
    }
    println!("[kernel] BUITENZORG RISCV READY -- increments 1-6 (boot, traps, memory, address spaces, timer, U-mode, ELF programs, scheduler + processes + threads, PLIC, virtio-mmio /disk + GPU + input + net, RTC, SBI power, shared syscalls + window system, JS/TS/Python, interactive desktop, SMP via SBI HSM) -- M4 complete");
    // The interactive desktop session, shared with the PC and ARM64 kernels:
    // virtio keyboard / tablet -> terminal + window manager, frames through
    // the framebuffer and the virtio GPU.
    desktop::run()
}

/// The generic memory subsystem on RISC-V page tables:
/// - the heap (mapped page by page through `arch::paging`) holds a 1 MiB
///   vector intact;
/// - a kernel page is mapped, written, made read-only, and unmapped, and its
///   frame goes back to the allocator;
/// - a **user** page (lower half) reports U-mode read/write through
///   `user_access` (what syscall pointer validation uses), read-only after
///   `protect`, nothing after unmap -- and `validate_user_range` agrees;
/// - a device page maps in the MMIO window.
fn memory_self_test() -> bool {
    use arch::paging::{self, Flags};
    let mut ok = true;
    let mut check = |what: &str, cond: bool| {
        if !cond {
            println!("[riscv] memory check failed: {}", what);
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
        // The kernel reaches user pages through sstatus.SUM.
        unsafe { (UVA as *mut u64).write_volatile(0x5EED) };
        let seen = unsafe { (UVA as *const u64).read_volatile() };
        let rw = paging::user_access(UVA);
        paging::protect(&mut ctx.mapper, UVA, Flags { write: false, user: true, exec: false });
        let ro = paging::user_access(UVA);
        Some((pa, rw, ro, seen))
    });
    let (rw, ro, seen) = user.map(|u| (u.1, u.2, u.3)).unwrap_or((None, None, 0));
    check("kernel access to a user page (SUM)", seen == 0x5EED);
    check("user page U-mode read/write", rw == Some((true, true)));
    check("user page read-only after protect", ro == Some((true, false)));
    check("validate_user_range: readable", memory::validate_user_range(UVA, 64, false));
    check("validate_user_range: not writable", !memory::validate_user_range(UVA, 64, true));
    check("validate_user_range: kernel address refused", !memory::validate_user_range(allocator::HEAP_START, 64, false));
    if user.is_some() {
        memory::with_ctx(|ctx| {
            if let Some(f) = paging::unmap(&mut ctx.mapper, UVA) {
                ctx.frames.free(f);
            }
        });
    }
    check("user page gone after unmap", paging::user_access(UVA).is_none());
    let (used1, _) = memory::frame_stats();
    let in_use = memory::frames_in_use();
    check("device window", memory::map_mmio(0x1000_0000, 0x1000).is_some());

    if ok {
        println!(
            "[kernel] MILESTONE: RISCV MEMORY OK (frame allocator + paging HAL on {}: kernel heap, kernel page map/protect/unmap, user page with U-mode rights checked, frames returned; {} frames handed out, {} in use)",
            arch::riscv64::paging::mode_name(),
            used1 - used0,
            in_use
        );
    }
    ok
}

/// Two processes' address spaces map the same user address to different
/// frames: installing one shows its own data, the other its own, the kernel
/// space neither; destroying both returns every frame.
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
    // The kernel half is the same in every space: the heap stays readable.
    let heap_probe = alloc::boxed::Box::new(0x4EA9_u64);
    let freed: u64 = spaces.into_iter().map(aspace::destroy).sum();
    let leaked = memory::frames_in_use() as i64 - before as i64;
    let ok = seen == [0xA11CE, 0xB0B] && hidden && leaked == 0 && *heap_probe == 0x4EA9;
    println!(
        "[riscv] address spaces: space 1000 reads {:#x}, space 1001 reads {:#x} at the same address; kernel space sees nothing: {}; {} frames freed, {} leaked",
        seen[0], seen[1], hidden, freed, leaked
    );
    if ok {
        println!("[kernel] MILESTONE: RISCV ASPACE OK (a root per process with the kernel half shared; switch = satp + sfence.vma; teardown frees every frame)");
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
    use arch::riscv64::context::{fp_get_f0, fp_set_f0};
    for i in 0..SWITCH_ROUNDS {
        fp_set_f0(tag + i);
        ctx_switch(me, other);
        if fp_get_f0() != tag + i {
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
/// register f0 across every switch (the FP state follows the task), and the
/// boot flow's own f0 survives too.
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
    arch::riscv64::context::fp_set_f0(0x0B007);
    ctx_switch(0, 1);
    let boot_fp_kept = arch::riscv64::context::fp_get_f0() == 0x0B007;
    let switches = CTX_SWITCHES.load(core::sync::atomic::Ordering::Relaxed);
    let errors = CTX_ERRORS.load(core::sync::atomic::Ordering::Relaxed);
    core::mem::forget(stacks); // thread B stays parked on its stack
    println!(
        "[riscv] context switch: {} switches between two kernel threads, {} FP mismatches, boot flow's f0 kept: {}",
        switches, errors, boot_fp_kept
    );
    if errors == 0 && boot_fp_kept && switches >= 2 * SWITCH_ROUNDS {
        println!("[kernel] MILESTONE: RISCV CONTEXT OK (arch::context on RISC-V: ra + s0-s11 + sp + SIE, f0-f31/fcsr per task)");
    }
}

// ---- User mode (U-mode) ----------------------------------------------------

/// Exit code of a user program ended by a fault (as on x86 / ARM64).
const FAULT_EXIT_CODE: u64 = 0xDEAD;

static USER_FAULT_ADDR: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// `ECALL` from U-mode (a7 = number, a0..a2 = arguments; the result goes to
/// a0; bz-abi numbers, the same table as on x86): the counterpart of x86
/// `usermode::syscall_dispatch_c` -- clock reads lock-free; BKL-free syscalls
/// (`syscall::bkl_free`) skip the Big Kernel Lock; served with interrupts
/// enabled (the trap frame holds `sepc`/`sstatus`); then the shared
/// `syscall.rs` dispatcher, pointer validation included.
pub(crate) fn user_syscall(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
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

/// A fatal exception in U-mode code: end that program (exit code 0xDEAD),
/// like the x86 ring-3 #PF/#GP containment.
pub(crate) fn user_fault(cause: u64, addr: u64, pc: u64) -> ! {
    if !usermode::user_active() {
        panic!("U-mode fault outside a user program: scause={} stval={:#x} sepc={:#x}", cause, addr, pc);
    }
    println!(
        "[riscv] user fault: scause={} ({}) at {:#x}, address {:#x} -- program terminated",
        cause,
        arch::riscv64::trap::exception_name(cause),
        pc,
        addr
    );
    USER_FAULT_ADDR.store(addr, core::sync::atomic::Ordering::SeqCst);
    task::kernel_enter();
    usermode::exit_user(FAULT_EXIT_CODE)
}

/// Run `code` as a U-mode program on the boot flow itself (before the
/// scheduler starts), in a fresh address space: code page at 0x40_0000
/// (user, read-only, executable), one stack page below 0x7000_0000. Returns
/// (exit code, frames leaked by the space).
fn run_umode(id: u64, code: &[u8]) -> Option<(u64, i64)> {
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
    // The demo program spins this long (300 ms of `time`) in U-mode.
    let spin = arch::riscv64::cpu::counter_hz() * 3 / 10;
    let code = usermode::enter_user_thread(ENTRY, STACK_TOP, spin);
    arch::cpu::enable_interrupts();
    aspace::install(aspace::kernel());
    aspace::destroy(space);
    Some((code, memory::frames_in_use() as i64 - before))
}

/// Two U-mode programs: one prints through DEBUG_WRITE, asks ABI_VERSION,
/// spins long enough to be interrupted by the timer *while in U-mode*,
/// computes 1+..+100 through the FP unit and exits with (abi << 16) + 5050;
/// the other reads a kernel-half address and must be terminated by the
/// fault, not crash the kernel.
fn umode_self_test() {
    use arch::riscv64::trap;
    let (demo, fault) = usermode::demo_programs();
    let (irq0, ecalls0) = trap::user_stats();
    let r1 = run_umode(3000, demo);
    let (irq1, ecalls1) = trap::user_stats();
    let r2 = run_umode(3001, fault);
    let want = (bz_abi::ABI_VERSION << 16) + 5050;
    let addr = USER_FAULT_ADDR.load(core::sync::atomic::Ordering::SeqCst);
    println!(
        "[riscv] U-mode program 1: exit {:#x} (want {:#x}), {} ECALLs, {} timer interrupts taken in U-mode, leaked {:?} frames; program 2: exit {:#x}, fault address {:#x}, leaked {:?}",
        r1.map(|r| r.0).unwrap_or(0),
        want,
        ecalls1 - ecalls0,
        irq1 - irq0,
        r1.map(|r| r.1),
        r2.map(|r| r.0).unwrap_or(0),
        addr,
        r2.map(|r| r.1)
    );
    let ok = r1 == Some((want, 0))
        && ecalls1 - ecalls0 >= 3
        && irq1 - irq0 >= 3
        && r2 == Some((FAULT_EXIT_CODE, 0))
        && addr == 0xFFFF_D000_0000_0000;
    if ok {
        println!("[kernel] MILESTONE: RISCV UMODE OK (user mode in its own address space: ECALL syscalls, timer interrupts from U-mode, FP in U-mode, EXIT back to the kernel, a kernel-address read terminates only the program)");
    }
}

// ---- Processes (the shared scheduler) -----------------------------------------

/// Run one program as a process (the shared `app.rs`) and wait for it; its
/// exit code.
fn run_program(name: &str, image: &[u8]) -> Option<u64> {
    println!("[riscv] --- program '{}' ({} bytes) ---", name, image.len());
    match app::spawn_image(name, image) {
        Ok(pid) => {
            let code = app::wait(pid);
            println!("[riscv] --- '{}' (pid {}) exited with code {:#x} ---", name, pid, code);
            Some(code)
        }
        Err(e) => {
            println!("[riscv] '{}': {}", name, e);
            None
        }
    }
}

/// Run the RISC-V ELF programs (`userland/riscv`, Rust) Limine handed over as
/// modules, as processes of the shared scheduler (`task.rs` + `process.rs`,
/// the same code as on x86 and ARM64):
/// - `hello` (the generic ELF loader + a process of its own), `mem` (the
///   mmap / mprotect / munmap PAL incl. reserve + commit on demand);
/// - **two copies of `mproc` at once**: each spins ~1.5 s without yielding,
///   keeps a running sum in an FP register and stamps its own mmap page at
///   the same virtual address -- they can only both finish with exact
///   results if the timer preempts U-mode and every switch saves FP state
///   and swaps `satp`;
/// - `thread` (ring-3 threads + join) and `sync` (futex-backed mutex);
/// - every frame the processes used comes back.
fn run_boot_programs(info: &boot::BootInfo) {
    // From /disk (virtio-blk) when it is mounted, else from the Limine modules.
    let find = |name: &str| -> Option<alloc::vec::Vec<u8>> {
        let path = alloc::format!("/disk/{}.ELF", name.to_uppercase());
        vfs::read(&path).ok().or_else(|| info.modules().iter().find(|m| m.name() == name).map(|m| m.bytes().to_vec()))
    };
    let from_disk = vfs::read("/disk/HELLO.ELF").is_ok();
    if find("hello").is_none() {
        println!("[riscv] no RISC-V user programs on /disk or among the boot modules");
        return;
    }
    println!("[riscv] user programs from {}", if from_disk { "/disk (virtio-blk)" } else { "Limine boot modules" });
    let frames0 = memory::frames_in_use();
    let mut all_ok = true;

    if let Some(image) = find("hello") {
        let ok = run_program("hello", &image) == Some(0);
        all_ok &= ok;
        if ok {
            println!(
                "[kernel] MILESTONE: RISCV ELF OK (a RISC-V ELF {}, loaded by the generic elf.rs, run in U-mode as a process of the shared scheduler)",
                if from_disk { "read from /disk" } else { "from a Limine module" }
            );
        }
    }
    if let Some(image) = find("mem") {
        let ok = run_program("mem", &image) == Some(0);
        all_ok &= ok;
        if ok {
            println!("[kernel] MILESTONE: RISCV USERMEM OK (MMAP/MPROTECT/MUNMAP from U-mode: rw region, read-only, unmap, 64 MiB reserve + commit on demand)");
        }
    }
    if let Some(image) = find("mproc") {
        // Timer preemption is the point here: keep both copies on CPU 0 (the
        // x86 / ARM64 MULTIPROC tests do the same); the SMP test spreads them.
        let aps = task::set_ap_scheduling(false);
        let p0 = task::user_preemptions();
        let t0 = arch::cpu::timestamp();
        let pids: alloc::vec::Vec<u64> =
            ["mproc-a", "mproc-b"].iter().filter_map(|n| app::spawn_image(n, &image).ok()).collect();
        let codes: alloc::vec::Vec<u64> = pids.iter().map(|&p| app::wait(p)).collect();
        let preempts = task::user_preemptions() - p0;
        let ms = (arch::cpu::timestamp() - t0) * 1000 / arch::riscv64::cpu::counter_hz().max(1);
        println!("[riscv] mproc x2: exit codes {:?}, {} timer preemptions of U-mode code, {} ms", codes, preempts, ms);
        let ok = pids.len() == 2 && codes.iter().all(|&c| c == 0) && preempts >= 10;
        all_ok &= ok;
        if ok {
            println!("[kernel] MILESTONE: RISCV MULTIPROC OK (shared scheduler on RISC-V: two processes preempted by the timer in U-mode, each in its own satp space with its own FP state)");
        }
        task::set_ap_scheduling(aps);
        all_ok &= smp_processes_test(&image);
    }
    for (name, milestone) in [
        ("thread", "RISCV THREAD OK (two ring-3 threads of one process share its address space; THREAD_CREATE/JOIN/EXIT)"),
        ("sync", "RISCV SYNC OK (futex-backed mutex under contention: FUTEX_WAIT blocks, FUTEX_WAKE resumes, mutual exclusion held)"),
        ("proc", "RISCV PROCCTL OK (shared syscalls from U-mode: FS_WRITE + FS_READ on /disk, PROC_SPAWN of /disk/HELLO.ELF with arguments, PROC_WAIT)"),
        ("gui", "RISCV GUI OK (a RISC-V app drew a window through the shared window manager: WIN_CREATE / WIN_CMD / WIN_PRESENT, CLOCK_RTC, FB_INFO)"),
    ] {
        let Some(image) = find(name) else { continue };
        let ok = run_program(name, &image) == Some(0);
        all_ok &= ok;
        if ok {
            println!("[kernel] MILESTONE: {}", milestone);
        }
    }
    let leaked = memory::frames_in_use() as i64 - frames0 as i64;
    println!("[riscv] programs done; frames leaked: {}", leaked);
    if all_ok && leaked == 0 {
        println!("[kernel] MILESTONE: RISCV PROCESSES OK (every program exited 0, every frame returned)");
    }
}

// ---- Platform devices: interrupts, clock, storage, display -----------------

/// UART interrupt source on the PLIC (from the DTB), for the self-test.
static UART_IRQ: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// The PLIC context that delivers S-mode external interrupts to `hart`: the
/// PLIC's `interrupts-extended` lists (cpu-intc phandle, cause) pairs, one
/// per context, in context order; ours names the hart's interrupt
/// controller with cause 9 (supervisor external interrupt).
fn plic_context(f: &dtb::Fdt, plic: usize, hart: u64) -> Option<usize> {
    let intc_phandle = (0..f.nodes.len()).find_map(|i| {
        let n = &f.nodes[i];
        if !n.is_compatible("riscv,cpu-intc") {
            return None;
        }
        let cpu = n.parent?;
        (f.reg(cpu).first()?.0 == hart).then(|| n.u32_prop("phandle"))?
    })?;
    let ext = f.nodes[plic].prop("interrupts-extended")?;
    ext.chunks_exact(8).position(|pair| {
        let ph = u32::from_be_bytes([pair[0], pair[1], pair[2], pair[3]]);
        let cause = u32::from_be_bytes([pair[4], pair[5], pair[6], pair[7]]);
        ph == intc_phandle && cause == 9
    })
}

/// Map the PLIC from the DTB and bind the boot hart to its S-mode context.
fn plic_init(f: &dtb::Fdt, hart: u64) -> bool {
    use arch::riscv64::{cpu, plic};
    let Some(node) = f.compatible("sifive,plic-1.0.0").next().or_else(|| f.compatible("riscv,plic0").next()) else {
        println!("[riscv] no PLIC in the Device Tree");
        return false;
    };
    let Some(&(pa, _)) = f.reg(node).first() else { return false };
    let Some(ctx) = plic_context(f, node, hart) else {
        println!("[riscv] PLIC: no S-mode context for hart {}", hart);
        return false;
    };
    let Some(va) = memory::map_mmio(pa, plic::WINDOW) else { return false };
    plic::init(va, 0, ctx);
    cpu::sie_set(cpu::SIE_SEIE);
    if let Some(irq) = f.stdout().and_then(|i| f.nodes[i].u32_prop("interrupts")) {
        UART_IRQ.store(irq as usize, core::sync::atomic::Ordering::SeqCst);
    }
    println!("[riscv] PLIC at {:#x}: hart {} -> S-mode context {}; UART on source {}", pa, hart, ctx, UART_IRQ.load(core::sync::atomic::Ordering::SeqCst));
    true
}

/// A real device interrupt through the PLIC: the UART is asked for a
/// "transmit holding register empty" interrupt, which it raises at once; the
/// PLIC must deliver it as a supervisor external interrupt, the handler
/// claims + completes it.
fn plic_self_test(f: Option<&dtb::Fdt>) {
    use arch::riscv64::plic;
    let irq = UART_IRQ.load(core::sync::atomic::Ordering::SeqCst);
    if irq == 0 || f.is_none() {
        return;
    }
    let before = plic::count(irq);
    plic::enable(irq, 1, 0, serial::on_irq);
    serial::request_tx_irq();
    let t0 = arch::cpu::timestamp();
    while plic::count(irq) == before && arch::cpu::timestamp() - t0 < arch::riscv64::cpu::counter_hz() / 2 {
        arch::cpu::halt();
    }
    let got = plic::count(irq) - before;
    plic::disable(irq, 0);
    println!("[riscv] PLIC: UART (source {}) transmit interrupt delivered {} time(s)", irq, got);
    if got >= 1 {
        println!("[kernel] MILESTONE: RISCV PLIC OK (device interrupt -> PLIC -> S-mode external interrupt; claimed, handled, completed)");
    }
}

/// The Goldfish clock answers through the same `rtc::read` the FAT code
/// stamps files with.
fn rtc_self_test() {
    let (y, mo, d, h, mi, s) = rtc::read();
    println!("[riscv] Goldfish RTC: {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC (unix {})", y, mo, d, h, mi, s, rtc::unix_time());
    if (2024..2200).contains(&y) && (1..=12).contains(&mo) && (1..=31).contains(&d) {
        println!("[kernel] MILESTONE: RISCV RTC OK (google,goldfish-rtc from the DTB, nanoseconds since the epoch -> calendar)");
    }
}

/// `/disk` on RISC-V: the virtio-blk driver (shared with x86 and ARM64) over
/// the virtio-mmio transport, mounted by the shared FAT + VFS code. Lists
/// the root, reads BATANG.TXT, writes a file and reads it back.
fn storage_self_test() {
    let n = virtio::init_block();
    if n == 0 {
        println!("[riscv] storage: no virtio-blk device (QEMU: -device virtio-blk-device + -global virtio-mmio.force-legacy=false)");
        return;
    }
    let Some(dev) = driver::take_boot_block_device() else { return };
    if let Err(e) = vfs::mount("disk", dev, false) {
        println!("[riscv] storage: mount /disk failed: {}", e);
        return;
    }
    let names = vfs::list("/disk").unwrap_or_default();
    let batang = vfs::read("/disk/BATANG.TXT").ok().and_then(|b| alloc::string::String::from_utf8(b).ok());
    let (y, mo, d, h, mi, s) = rtc::read();
    let stamp = alloc::format!("BZRISCV written {:04}-{:02}-{:02} {:02}:{:02}:{:02}\n", y, mo, d, h, mi, s);
    let wrote = vfs::write("/disk/RISCV.TXT", stamp.as_bytes()).is_ok();
    let back = vfs::read("/disk/RISCV.TXT").ok();
    println!(
        "[riscv] /disk: {} entries, BATANG.TXT {}, RISCV.TXT = {}",
        names.len(),
        if batang.is_some() { "read" } else { "missing" },
        stamp.trim_end()
    );
    if names.len() > 5 && batang.as_deref().map_or(false, |t| t.contains("batang")) && wrote && back.as_deref() == Some(stamp.as_bytes()) {
        println!("[kernel] MILESTONE: RISCV DISK OK (virtio-blk over virtio-mmio, shared FAT + VFS: /disk listed, read, written + read back)");
    }
}

/// virtio-gpu over virtio-mmio: copy the framebuffer console's pixels into a
/// back buffer and present it -- the driver prints `VIRTIO GPU OK` on its
/// first frame (the RISC-V desktop will present through it).
fn gpu_self_test(info: &boot::BootInfo) {
    let Some(fb) = info.framebuffer else { return };
    if fb.bytes_per_pixel != 4 || virtio::find(virtio::device::GPU).is_empty() {
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

// ---- SMP ---------------------------------------------------------------------

/// Start every other hart of the DTB through SBI HSM, give each its PLIC
/// context and timer, and check they all take their own timer interrupts.
fn start_secondary_harts(info: &boot::BootInfo, fdt: Option<&dtb::Fdt>) {
    let (Some(f), Some(base)) = (fdt, info.kernel_base) else { return };
    let Some(cpus) = f.find_path("/cpus") else { return };
    let harts: alloc::vec::Vec<u64> = (0..f.nodes.len())
        .filter(|&i| f.nodes[i].parent == Some(cpus) && f.nodes[i].strings("device_type").any(|s| s == "cpu"))
        .filter_map(|i| f.reg(i).first().map(|r| r.0))
        .collect();
    let boot_hart = info.bsp_hartid.unwrap_or(f.boot_cpu as u64);
    if harts.len() < 2 {
        println!("[smp] single hart ({} in the DTB)", harts.len());
        return;
    }
    let plic = f.compatible("sifive,plic-1.0.0").next().or_else(|| f.compatible("riscv,plic0").next());
    let ctx_of = |hart: u64| plic.and_then(|p| plic_context(f, p, hart));
    let online = arch::riscv64::smp::start_harts(&harts, boot_hart, base, &ctx_of);
    let irq0: alloc::vec::Vec<u64> = (0..online).map(arch::riscv64::trap::irqs_on).collect();
    let t0 = arch::cpu::timestamp();
    while arch::cpu::timestamp() - t0 < arch::riscv64::cpu::counter_hz() / 5 {
        arch::cpu::halt();
    }
    let ticked = (0..online).filter(|&c| arch::riscv64::trap::irqs_on(c) > irq0[c] + 5).count();
    println!(
        "[smp] {} of {} harts online via SBI HSM hart_start (hart ids {:?}, boot hart {}); {} took their own timer interrupts in 200 ms",
        online,
        harts.len(),
        harts,
        boot_hart,
        ticked
    );
    if online == harts.len() && ticked == online {
        println!("[kernel] MILESTONE: RISCV SMP OK ({} harts: SBI HSM + paging-off trampoline, per-hart SBI timer + PLIC context, shared scheduler)", online);
    }
}

/// One `mproc` per hart at once, the scheduler free to spread them: they must
/// all finish correctly, and at some interrupt several harts must have been
/// running user code simultaneously.
fn smp_processes_test(image: &[u8]) -> bool {
    let n = arch::riscv64::smp::cpus_online();
    if n < 2 {
        return true;
    }
    arch::riscv64::trap::reset_parallel_peak();
    let sys0: u64 = (1..n).map(task::syscalls_on).sum();
    let pids: alloc::vec::Vec<u64> =
        (0..n).filter_map(|i| app::spawn_image(&alloc::format!("mproc-smp{}", i), image).ok()).collect();
    let codes: alloc::vec::Vec<u64> = pids.iter().map(|&p| app::wait(p)).collect();
    let peak = arch::riscv64::trap::parallel_peak();
    let ap_sys: u64 = (1..n).map(task::syscalls_on).sum::<u64>() - sys0;
    println!(
        "[smp] {} mproc copies: exit codes {:?}; at most {} harts ran user code at once; {} syscalls served on secondary harts; {} migrations; {} wake-up IPIs",
        pids.len(),
        codes,
        peak,
        ap_sys,
        task::migrations(),
        arch::riscv64::smp::ipis_sent()
    );
    let ok = pids.len() == n && codes.iter().all(|&c| c == 0) && peak >= 2 && ap_sys > 0;
    if ok {
        println!("[kernel] MILESTONE: RISCV SMP SCHED OK (processes ran in parallel on {} harts, syscalls served on the secondary harts, exact FP results)", peak);
    }
    ok
}

// ---- The shared kernel --------------------------------------------------------

/// Bring up the generic subsystems the PC kernel uses, on RISC-V: the
/// syscall hardening test, SBI power, the window manager (theme, icons,
/// official wallpaper), virtio input, and the network (loopback, then a
/// virtio NIC).
fn shared_kernel_setup(info: &boot::BootInfo) {
    if syscall::security_self_test() {
        println!("[kernel] MILESTONE: SECURITY OK (syscall user-pointer validation)");
    } else {
        println!("[kernel] security self-test FAILED");
    }

    if power::init("") {
        println!(
            "[kernel] MILESTONE: RISCV POWER OK ({} extension present: shutdown / restart through the SBI firmware)",
            power::firmware()
        );
    }
    for line in vmx::summary() {
        println!("[riscv/vmx] {}", line);
    }

    if let Some(fb) = info.framebuffer {
        wm::init(fb.width, fb.height);
        mouse::set_bounds(fb.width, fb.height);
        theme::set_default();
        let n = icons::load();
        let wp = wallpaper::set_official("nature").is_ok();
        println!("[riscv] window system: {}x{}, {} app icons, wallpaper 'nature' loaded: {}", fb.width, fb.height, n, wp);
    }
    virtio::input::init();

    net::init([10, 0, 0, 1]);
    net::send_ping([10, 0, 0, 1], 0x5256, 1);
    for _ in 0..4 {
        if net::poll() == 0 {
            break;
        }
    }
    if net::counters().0 > 0 {
        println!("[kernel] MILESTONE: NETWORK OK (Ethernet/ARP/IPv4/ICMP stack, loopback on RISC-V)");
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
            net::send_ping(GW, 0x5256, 2);
            if wait(&|| net::counters().0 > icmp0) {
                println!("[kernel] MILESTONE: VIRTIO NET OK (virtio-net over virtio-mmio: ARP + ICMP echo with the gateway 10.0.2.2)");
            }
        }
    }
}

/// The polyglot runtime (`script.rs`, v0.14 "Babel") on RISC-V: the same fib
/// program in JavaScript, TypeScript and Python through the shared in-kernel
/// interpreter -- PLAN.md's "first apps through JS/TS/Python" while .NET has
/// no RISC-V NativeAOT.
fn polyglot_test() {
    let mut ok = 0;
    for lang in [script::Lang::Js, script::Lang::Ts, script::Lang::Python] {
        let out = script::run(lang, script::demo_source(lang));
        for l in &out.lines {
            println!("[riscv/{}] {}", lang.name(), l);
        }
        if let Some(e) = &out.error {
            println!("[riscv/{}] ERROR: {}", lang.name(), e);
        }
        let joined = out.lines.join("\n");
        if out.error.is_none() && joined.contains("55") && joined.contains("88") {
            ok += 1;
        }
    }
    if ok == 3 {
        println!("[kernel] MILESTONE: RISCV POLYGLOT OK (JavaScript, TypeScript and Python run on RISC-V through the shared interpreter: fib(10) = 55, sum = 88)");
    }
}

/// Compose the desktop -- wallpaper, taskbar, and the windows the apps left
/// -- with the shared window manager, and present it (framebuffer +
/// virtio-gpu). From here the console stops drawing over the screen.
fn show_desktop() {
    let Some((w, h)) = framebuffer::dimensions() else { return };
    desktop::ensure_session_windows();
    let windows = wm::window_count();
    wm::tile();
    let mut back = alloc::vec::Vec::new();
    wm::render_frame(&mut back, w, h);
    let lit = back.iter().filter(|&&p| p != 0).count();
    interactive::set_active();
    framebuffer::present(&back);
    println!("[riscv] desktop frame: {}x{}, {} windows, {} lit pixels", w, h, windows, lit);
    if windows >= 3 && lit > w * h / 2 {
        println!("[kernel] MILESTONE: RISCV DESKTOP FRAME OK (shared window manager on RISC-V: the session windows and the RISC-V GUI app composited over the wallpaper and presented)");
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("\n[kernel] PANIC: {}", info);
    loop {
        arch::cpu::halt();
    }
}
