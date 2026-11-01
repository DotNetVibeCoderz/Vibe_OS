// The ARM64 kernel (multi-arch M3). Included by main.rs on aarch64. Loaded by
// Limine (UEFI) on QEMU `virt`.
//
// Increment 1: boot hand-off, framebuffer console, exception vectors, live
// Device Tree, PL011, GICv3 + timer. Increment 2: the generic memory
// subsystem (`memory.rs` frame allocator + paging context, `allocator.rs`
// kernel heap) over the AArch64 paging HAL — TTBR1 for the kernel half,
// TTBR0 for user space. The x86 kernel's other subsystems join one by one as
// they get an `arch/aarch64` backend (context switch + EL0/SVC, virtio-mmio,
// the desktop).

mod allocator;
mod arch;
mod boot;
mod dtb;
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

    let uart = platform.as_ref().and_then(|p| p.uart.clone()).map(|u| u.0).unwrap_or_default();
    if timer_ok && !uart.is_empty() && info.loader == "limine" {
        println!(
            "[kernel] MILESTONE: ARM64 BOOT OK (Limine -> EL{} kernel; live DTB -> {} console + GICv3 + timer; framebuffer {})",
            cpu::current_el(),
            uart,
            if info.framebuffer.is_some() { "on" } else { "absent" }
        );
    }
    println!("[kernel] BUITENZORG ARM64 READY -- increments 1-2 (boot, interrupts, memory); the rest of the kernel follows");
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

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("\n[kernel] PANIC: {}", info);
    loop {
        arch::cpu::halt();
    }
}
