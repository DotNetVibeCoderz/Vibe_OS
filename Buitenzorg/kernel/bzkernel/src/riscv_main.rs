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
// preemptive, with ring-3 threads and futexes. The rest of the shared kernel joins
// as subsystems gain an `arch/riscv64` backend -- the path the ARM64 port
// (M3) took.

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
mod process;
mod task;

pub(crate) use arch::riscv64::{aspace, gdt, interrupts, smp, usermode};
pub(crate) use platform::virt::uart16550 as serial;

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

/// The virtio drivers join in a later increment (virtio-mmio is shared with
/// ARM64); until then frames go to the loader's framebuffer only.
mod virtio {
    pub mod gpu {
        pub fn present(_back: &[u32]) {}
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

    // The periodic tick: the SBI timer at 100 Hz.
    let has_time = sbi::probe(sbi::EXT_TIME);
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

    umode_self_test();
    // The shared scheduler (task.rs): the boot flow becomes task 0, an idle
    // task joins, and programs run as preemptively scheduled processes.
    task::init();
    run_boot_programs(info);

    if timer_ok && uart.is_some() && info.loader == "limine" {
        println!(
            "[kernel] MILESTONE: RISCV BOOT OK (Limine -> S-mode kernel; live DTB -> {} console + SBI timer; {}; framebuffer {})",
            uart.as_ref().map_or("", |u| u.0.as_str()),
            paging::mode_name(),
            if info.framebuffer.is_some() { "on" } else { "absent" }
        );
    }
    println!("[kernel] BUITENZORG RISCV READY -- increments 1-3 (boot, traps, memory, address spaces, timer, context switch, U-mode + ECALL, ELF programs, scheduler + processes + threads); the rest of the kernel follows");
    loop {
        cpu::enable_and_halt();
    }
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
/// What user programs wrote (DEBUG_WRITE), for the self-tests.
static USER_OUTPUT: spin::Mutex<alloc::string::String> = spin::Mutex::new(alloc::string::String::new());

/// `ECALL` from U-mode (a7 = number, a0..a2 = arguments; the result goes to
/// a0; bz-abi numbers, the same table as on x86). Like x86's
/// `syscall_dispatch_c`: clock reads take no lock; everything else runs under
/// the Big Kernel Lock with interrupts enabled (the trap frame holds
/// `sepc`/`sstatus`, so a nested interrupt or a task switch inside the
/// syscall is safe). The shared `syscall.rs` replaces this table when the
/// rest of the kernel joins.
pub(crate) fn user_syscall(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    use bz_abi::sysno;
    match nr {
        sysno::TICKS => return interrupts::ticks(),
        sysno::CLOCK_MONO => return arch::cpu::timestamp(),
        _ => {}
    }
    task::kernel_enter();
    arch::cpu::enable_interrupts();
    let r = user_dispatch(nr, a0, a1, a2);
    arch::cpu::disable_interrupts();
    task::kernel_exit();
    r
}

fn user_dispatch(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    use bz_abi::{syserr, sysno};
    match nr {
        sysno::ABI_VERSION => bz_abi::ABI_VERSION,
        sysno::DEBUG_WRITE => {
            if a1 > 4096 || !memory::validate_user_range(a0, a1, false) {
                return syserr::INVAL;
            }
            let bytes: alloc::vec::Vec<u8> = (0..a1).map(|i| unsafe { ((a0 + i) as *const u8).read_volatile() }).collect();
            let text = core::str::from_utf8(&bytes).unwrap_or("<not UTF-8>");
            print!("{}", text);
            let mut out = USER_OUTPUT.lock();
            if out.len() < 4096 {
                out.push_str(text);
            }
            a1
        }
        sysno::EXIT | sysno::THREAD_EXIT => usermode::exit_user(a0),
        sysno::YIELD => {
            task::yield_now();
            0
        }
        // The generic user-memory PAL: same code as x86 and ARM64.
        sysno::MMAP => memory::user_mmap(a0, a1),
        sysno::MPROTECT => memory::user_mprotect(a0, a1, a2),
        sysno::MUNMAP => memory::user_munmap(a0, a1),
        // Threads + futexes: the shared scheduler.
        sysno::THREAD_CREATE => {
            if a0 == 0 || a2 == 0 {
                return syserr::INVAL;
            }
            task::ensure_main_user_thread();
            task::spawn_user_thread(a0, a1, a2)
        }
        sysno::THREAD_JOIN => {
            while !task::is_finished(a0) {
                task::yield_now();
            }
            0
        }
        sysno::THREAD_SELF => task::current_id(),
        sysno::FUTEX_WAIT => {
            if a0 % 4 != 0 || !memory::validate_user_range(a0, 4, false) {
                return syserr::INVAL;
            }
            while task::futex_wait_if(a0, a1 as u32) {}
            0
        }
        sysno::FUTEX_WAKE => task::futex_wake(a0, a1),
        _ => syserr::NOSYS,
    }
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
    let code = usermode::enter_user(ENTRY, STACK_TOP);
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

/// The PC kernel's `app.rs` launches programs from /disk with the window
/// system around them; until those reach RISC-V this is its core (as ARM64's
/// was at the same stage): start an ELF image as its own preemptively
/// scheduled process, wait for it, clean up.
mod app {
    use crate::{aspace, elf, memory, process, task};

    const USER_STACK_BASE: u64 = 0x7000_0000 - USER_STACK_PAGES * 4096;
    const USER_STACK_PAGES: u64 = 32; // 128 KiB
    const KILLED_EXIT_CODE: u64 = 0xDEAD;

    /// Start `image` as a process named `name`; returns its pid (= its space id).
    pub fn spawn_image(name: &str, image: &[u8]) -> Result<u64, &'static str> {
        let pid = process::app_start(name);
        let space = match aspace::create(pid) {
            Ok(s) => s,
            Err(e) => {
                process::app_exit(pid, u64::MAX);
                return Err(e);
            }
        };
        let prev = task::current_space();
        aspace::install(space);
        let loaded = memory::with_ctx(|ctx| {
            let program = elf::load(image, ctx)?;
            let (stack_top, _) = memory::map_user_region(USER_STACK_BASE, USER_STACK_PAGES, ctx)?;
            Ok::<_, &'static str>((program.entry, stack_top))
        });
        aspace::install(prev);
        crate::usermode::sync_icache();
        match loaded {
            Ok((entry, stack_top)) => {
                task::spawn_process(entry, stack_top, space);
                Ok(pid)
            }
            Err(e) => {
                aspace::destroy(space);
                process::app_exit(pid, u64::MAX);
                Err(e)
            }
        }
    }

    /// Block until process `pid` exits; its exit code.
    pub fn wait(pid: u64) -> u64 {
        while task::block_while(process::wait_key(pid), || process::is_alive(pid)) {}
        process::exit_code(pid).unwrap_or(u64::MAX)
    }

    fn destroy_when_home(space: aspace::Space) {
        if task::wait_space_stopped(space.id) {
            aspace::destroy(space);
        }
    }

    /// On the process's own task once its program exited (scheduler hook).
    pub fn process_exited(pid: u64, code: u64) {
        let space = task::current_space();
        task::detach_current_from_space(pid);
        destroy_when_home(space);
        process::app_exit(pid, code);
    }

    /// PROC_KILL.
    pub fn kill(pid: u64) -> bool {
        let Some(space) = task::kill_space(pid) else { return false };
        destroy_when_home(space);
        process::app_exit(pid, KILLED_EXIT_CODE);
        true
    }
}

/// Run one program as a process and wait for it: (exit code, its output).
fn run_program(name: &str, image: &[u8]) -> Option<(u64, alloc::string::String)> {
    USER_OUTPUT.lock().clear();
    println!("[riscv] --- program '{}' ({} bytes, ELF from a Limine module) ---", name, image.len());
    match app::spawn_image(name, image) {
        Ok(pid) => {
            let code = app::wait(pid);
            println!("[riscv] --- '{}' (pid {}) exited with code {:#x} ---", name, pid, code);
            Some((code, USER_OUTPUT.lock().clone()))
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
    let find = |name: &str| info.modules().iter().find(|m| m.name() == name).map(|m| m.bytes());
    if find("hello").is_none() {
        println!("[riscv] no RISC-V user programs among the boot modules");
        return;
    }
    let frames0 = memory::frames_in_use();
    let mut all_ok = true;

    if let Some(image) = find("hello") {
        let r = run_program("hello", image);
        let ok = matches!(&r, Some((0, out)) if out.contains("Hello from Rust on RISC-V"));
        all_ok &= ok;
        if ok {
            println!("[kernel] MILESTONE: RISCV ELF OK (a RISC-V ELF loaded by the generic elf.rs, run in U-mode as a process of the shared scheduler)");
        }
    }
    if let Some(image) = find("mem") {
        let ok = matches!(run_program("mem", image), Some((0, _)));
        all_ok &= ok;
        if ok {
            println!("[kernel] MILESTONE: RISCV USERMEM OK (MMAP/MPROTECT/MUNMAP from U-mode: rw region, read-only, unmap, 64 MiB reserve + commit on demand)");
        }
    }
    if let Some(image) = find("mproc") {
        let p0 = task::user_preemptions();
        let t0 = arch::cpu::timestamp();
        let pids: alloc::vec::Vec<u64> =
            ["mproc-a", "mproc-b"].iter().filter_map(|n| app::spawn_image(n, image).ok()).collect();
        let codes: alloc::vec::Vec<u64> = pids.iter().map(|&p| app::wait(p)).collect();
        let preempts = task::user_preemptions() - p0;
        let ms = (arch::cpu::timestamp() - t0) * 1000 / arch::riscv64::cpu::counter_hz().max(1);
        println!("[riscv] mproc x2: exit codes {:?}, {} timer preemptions of U-mode code, {} ms", codes, preempts, ms);
        let ok = pids.len() == 2 && codes.iter().all(|&c| c == 0) && preempts >= 10;
        all_ok &= ok;
        if ok {
            println!("[kernel] MILESTONE: RISCV MULTIPROC OK (shared scheduler on RISC-V: two processes preempted by the timer in U-mode, each in its own satp space with its own FP state)");
        }
    }
    for (name, milestone) in [
        ("thread", "RISCV THREAD OK (two ring-3 threads of one process share its address space; THREAD_CREATE/JOIN/EXIT)"),
        ("sync", "RISCV SYNC OK (futex-backed mutex under contention: FUTEX_WAIT blocks, FUTEX_WAKE resumes, mutual exclusion held)"),
    ] {
        let Some(image) = find(name) else { continue };
        let ok = matches!(run_program(name, image), Some((0, _)));
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

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("\n[kernel] PANIC: {}", info);
    loop {
        arch::cpu::halt();
    }
}
