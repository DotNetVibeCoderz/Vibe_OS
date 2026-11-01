// The ARM64 kernel (multi-arch M3, increment 1). Included by main.rs on
// aarch64. Loaded by Limine (UEFI) on QEMU `virt`; this first increment brings
// up what everything else stands on: the boot hand-off, a heap, the
// framebuffer console, exception vectors, platform discovery from the live
// Device Tree, MMIO mappings, the PL011 console, the GICv3 and the timer.
// The x86 kernel's subsystems join one by one as they get an `arch/aarch64`
// backend (paging + address spaces, context switch + EL0/SVC, virtio-mmio,
// the desktop).

mod arch;
mod boot;
mod dtb;
mod framebuffer;
mod limine;
mod logo;
mod platform;

pub(crate) use platform::virt::pl011 as serial;

/// Mirrors the PC kernel's flag; the ARM64 desktop is not up yet.
mod interactive {
    pub fn is_active() -> bool {
        false
    }
}

use core::panic::PanicInfo;
use linked_list_allocator::LockedHeap;

/// Kernel heap: a static 16 MiB arena for now (the paging-backed heap of the
/// PC kernel arrives with the ARM64 paging increment).
const HEAP_SIZE: usize = 16 * 1024 * 1024;
#[repr(C, align(4096))]
struct Arena([u8; HEAP_SIZE]);
static mut ARENA: Arena = Arena([0; HEAP_SIZE]);

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub(crate) fn kernel_main(info: &'static boot::BootInfo) -> ! {
    use arch::aarch64::{cpu, exceptions, gic, mmu, timer};

    if let Some(fb) = info.framebuffer {
        framebuffer::init(fb);
    }
    unsafe {
        ALLOCATOR.lock().init(core::ptr::addr_of_mut!(ARENA) as *mut u8, HEAP_SIZE);
    }
    exceptions::init();
    mmu::init();

    // The console UART, from the live Device Tree.
    let fdt = info.firmware.dtb.and_then(|p| unsafe { dtb::Fdt::from_phys(p) }.ok());
    let platform = fdt.as_ref().map(dtb::Platform::discover);
    if let Some((_, base)) = platform.as_ref().and_then(|p| p.uart.clone()) {
        if let Ok(va) = mmu::map_device(base, 0x1000) {
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
        let mapped = mmu::map_device(gicd_pa, 0x1_0000).and_then(|d| Ok((d, mmu::map_device(gicr_pa, 0x2_0000)?)));
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
                    "[arm64] GICv3 at {:#x}/{:#x}: {} timer interrupts in {} ms ({} IRQs taken, {} page tables built for MMIO)",
                    gicd_pa, gicr_pa, n, ms, exceptions::irq_count(), mmu::tables_created()
                );
                timer_ok = n >= 50 && (400..=700).contains(&ms);
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
    println!("[kernel] BUITENZORG ARM64 READY -- increment 1 (boot + interrupts); the rest of the kernel follows");
    loop {
        cpu::enable_and_halt();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("\n[kernel] PANIC: {}", info);
    loop {
        arch::cpu::halt();
    }
}
