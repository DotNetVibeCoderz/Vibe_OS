//! SMP bring-up, increment 1: discover the CPUs (ACPI MADT), enable the
//! bootstrap CPU's Local APIC, and start every application processor (AP)
//! with INIT-SIPI-SIPI through a real-mode trampoline. The APs reach 64-bit
//! Rust code on their own stacks, report in, and then **park** (interrupts off,
//! `hlt`).
//!
//! Why they park: the rest of the kernel is still single-core by design — the
//! SYSCALL path keeps per-CPU state in plain globals (no GS/swapgs yet), user
//! address spaces are switched by rewriting PDPT entries shared by everyone
//! (each core needs its own root), and kernel code relies on "single core +
//! interrupts off" for mutual exclusion in places. Scheduling tasks on the APs
//! comes after those are made per-CPU; this increment proves the hardware side.
//!
//! Trampoline: 16-bit code copied to physical 0x8000 (the SIPI vector 0x08).
//! It loads a tiny GDT, enters protected mode, enables PAE + long mode + NX,
//! loads a *temporary* CR3 and jumps into 64-bit code, which calls [`ap_main`].
//! The temporary page tables share every kernel mapping with the real ones but
//! add an identity mapping of the trampoline page (needed at the instant
//! paging switches on); the real, shared tables are never modified. The frame
//! allocator never hands out memory below 1 MiB, so 0x8000 is free.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

use crate::memory;

const TRAMPOLINE: u64 = 0x8000;
const LAPIC_VA: u64 = 0x4444_6000_0000; // above the kernel heap
const AP_STACK: usize = 16 * 1024;

const LAPIC_ID: u64 = 0x20;
const LAPIC_SVR: u64 = 0xF0;
const LAPIC_ICR_LO: u64 = 0x300;
const LAPIC_ICR_HI: u64 = 0x310;

static ONLINE: AtomicU64 = AtomicU64::new(0);

core::arch::global_asm!(
    ".global ap_tramp_start",
    ".global ap_tramp_end",
    ".global ap_cr3",
    ".global ap_stack",
    ".global ap_entry",
    ".global ap_arg",
    ".code16",
    "ap_tramp_start:",
    "    cli",
    "    cld",
    "    xorw %ax, %ax",
    "    movw %ax, %ds",
    "    lgdtl (ap_gdtr - ap_tramp_start + 0x8000)",
    "    movl %cr0, %eax",
    "    orl $1, %eax",
    "    movl %eax, %cr0",
    "    ljmpl $0x08, $(ap_pm32 - ap_tramp_start + 0x8000)",
    ".code32",
    "ap_pm32:",
    "    movw $0x10, %ax",
    "    movw %ax, %ds",
    "    movw %ax, %es",
    "    movw %ax, %ss",
    "    movl %cr4, %eax",
    "    orl $0x20, %eax", // PAE
    "    movl %eax, %cr4",
    "    movl (ap_cr3 - ap_tramp_start + 0x8000), %eax",
    "    movl %eax, %cr3",
    "    movl $0xC0000080, %ecx", // EFER
    "    rdmsr",
    "    orl $0x900, %eax", // LME | NXE (the kernel's tables use NX)
    "    wrmsr",
    "    movl %cr0, %eax",
    "    orl $0x80000001, %eax", // PG | PE
    "    movl %eax, %cr0",
    "    ljmpl $0x18, $(ap_lm64 - ap_tramp_start + 0x8000)",
    ".code64",
    "ap_lm64:",
    "    movw $0x10, %ax",
    "    movw %ax, %ds",
    "    movw %ax, %es",
    "    movw %ax, %ss",
    "    movq (ap_stack - ap_tramp_start + 0x8000), %rsp",
    "    movq (ap_arg - ap_tramp_start + 0x8000), %rdi",
    "    movq (ap_entry - ap_tramp_start + 0x8000), %rax",
    "    callq *%rax",
    "2:  cli",
    "    hlt",
    "    jmp 2b",
    ".p2align 3",
    "ap_gdt:",
    "    .quad 0",
    "    .quad 0x00CF9A000000FFFF", // 0x08: 32-bit code
    "    .quad 0x00CF92000000FFFF", // 0x10: data
    "    .quad 0x00AF9A000000FFFF", // 0x18: 64-bit code
    "ap_gdt_end:",
    "ap_gdtr:",
    "    .word ap_gdt_end - ap_gdt - 1",
    "    .long ap_gdt - ap_tramp_start + 0x8000",
    ".p2align 3",
    "ap_cr3: .quad 0",
    "ap_stack: .quad 0",
    "ap_entry: .quad 0",
    "ap_arg: .quad 0",
    "ap_tramp_end:",
    options(att_syntax)
);

extern "C" {
    static ap_tramp_start: u8;
    static ap_tramp_end: u8;
    static ap_cr3: u8;
    static ap_stack: u8;
    static ap_entry: u8;
    static ap_arg: u8;
}

fn sym_offset(sym: *const u8) -> u64 {
    sym as u64 - core::ptr::addr_of!(ap_tramp_start) as u64
}

fn lapic_read(reg: u64) -> u32 {
    unsafe { core::ptr::read_volatile((LAPIC_VA + reg) as *const u32) }
}

fn lapic_write(reg: u64, v: u32) {
    unsafe { core::ptr::write_volatile((LAPIC_VA + reg) as *mut u32, v) }
}

/// Parse the MADT: (Local APIC physical address, enabled LAPIC ids).
fn parse_madt() -> Option<(u64, Vec<u8>)> {
    let madt = crate::power::find_table(b"APIC")?;
    let v = |off: u64| memory::phys_to_virt(madt + off) as *const u8;
    let len = unsafe { core::ptr::read_unaligned(v(4) as *const u32) } as u64;
    let lapic = unsafe { core::ptr::read_unaligned(v(36) as *const u32) } as u64;
    let mut ids = Vec::new();
    let mut off = 44;
    while off + 2 <= len {
        let (kind, elen) = unsafe { (*v(off), *v(off + 1) as u64) };
        if elen < 2 {
            break;
        }
        // Type 0: Processor Local APIC (acpi id, apic id, flags: bit0 enabled,
        // bit1 online-capable).
        if kind == 0 && elen >= 8 {
            let apic_id = unsafe { *v(off + 3) };
            let flags = unsafe { core::ptr::read_unaligned(v(off + 4) as *const u32) };
            if flags & 0b11 != 0 {
                ids.push(apic_id);
            }
        }
        off += elen;
    }
    Some((lapic, ids))
}

/// Build temporary page tables for the APs: a copy of the live PML4 whose
/// path to the trampoline page is cloned (PDPT, PD, PT) with an identity
/// mapping of that page added. Everything else is shared by pointer, so the
/// kernel's own mappings are identical. Returns the new PML4's physical
/// address (below 4 GiB, as the 32-bit `mov cr3` requires).
fn temp_tables() -> Option<u64> {
    use x86_64::registers::control::Cr3;
    const ADDR: u64 = 0x000F_FFFF_FFFF_F000;
    /// A new frame holding a copy of `src` (or zeroed when `src` is None).
    fn copy_frame(frames: &mut memory::BootInfoFrameAllocator, src: Option<u64>) -> Option<u64> {
        let f = frames.allocate_frame()?.start_address().as_u64();
        let dst = memory::phys_to_virt(f) as *mut u8;
        match src {
            Some(s) => unsafe {
                core::ptr::copy_nonoverlapping(memory::phys_to_virt(s) as *const u8, dst, 4096)
            },
            None => unsafe { core::ptr::write_bytes(dst, 0, 4096) },
        }
        Some(f)
    }
    memory::with_ctx(|ctx| {
        let entry = |table: u64, i: u64| memory::phys_to_virt(table + i * 8) as *mut u64;
        let (l4, _) = Cr3::read();
        let pml4 = copy_frame(&mut ctx.frames, Some(l4.start_address().as_u64()))?;
        // Walk PML4 -> PDPT -> PD -> PT for the trampoline, cloning each level.
        let mut table = pml4;
        for shift in [39u64, 30, 21] {
            let idx = (TRAMPOLINE >> shift) & 0x1FF;
            let e = unsafe { *entry(table, idx) };
            if e & 1 != 0 && e & (1 << 7) != 0 {
                return None; // a huge page covers it; not handled (not the case on QEMU)
            }
            let src = (e & 1 != 0).then_some(e & ADDR);
            let next = copy_frame(&mut ctx.frames, src)?;
            unsafe { *entry(table, idx) = next | 0x3 | (e & 0x4) }; // present + writable
            table = next;
        }
        // Identity-map the trampoline page, executable.
        let idx = (TRAMPOLINE >> 12) & 0x1FF;
        unsafe { *entry(table, idx) = TRAMPOLINE | 0x3 };
        (pml4 < 0x1_0000_0000).then_some(pml4)
    })
}

/// Map the Local APIC's MMIO page (uncached) at `LAPIC_VA`.
fn map_lapic(phys: u64) -> bool {
    memory::with_ctx(|ctx| {
        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(LAPIC_VA));
        if ctx.mapper.translate_page(page).is_ok() {
            return true;
        }
        let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(phys));
        let flags = PageTableFlags::PRESENT
            | PageTableFlags::WRITABLE
            | PageTableFlags::NO_CACHE
            | PageTableFlags::WRITE_THROUGH
            | PageTableFlags::NO_EXECUTE;
        match unsafe { ctx.mapper.map_to(page, frame, flags, &mut ctx.frames) } {
            Ok(flush) => {
                flush.flush();
                true
            }
            Err(_) => false,
        }
    })
}

/// Put the bootstrap CPU's Local APIC into "virtual wire" mode: software-
/// enabled, LINT0 = ExtINT (the legacy 8259 PIC's interrupt line), LINT1 = NMI,
/// task priority 0. The kernel still takes its timer/keyboard/mouse IRQs from
/// the PIC, and those reach the CPU through LINT0 only in this mode. Legacy BIOS
/// leaves the LAPIC configured this way; UEFI firmware (OVMF) does not — which
/// is why UEFI boots used to hang at the first timer wait (ticks stuck at 0).
/// Call early, before anything waits on the timer. Uses the IA32_APIC_BASE MSR,
/// so it needs neither ACPI nor the heap.
pub fn enable_bsp_lapic() -> bool {
    use x86_64::registers::model_specific::Msr;
    let mut apic_base = Msr::new(0x1B);
    let base = unsafe { apic_base.read() };
    if base & (1 << 11) == 0 {
        unsafe { apic_base.write(base | (1 << 11)) }; // global enable
    }
    if !map_lapic(base & 0x000F_FFFF_FFFF_F000) {
        return false;
    }
    lapic_write(0x80, 0); // TPR: accept every priority
    lapic_write(LAPIC_SVR, (lapic_read(LAPIC_SVR) & !0xFF) | 0x100 | 0xFF);
    lapic_write(0x350, 0x700); // LVT LINT0: ExtINT, unmasked
    lapic_write(0x360, 0x400); // LVT LINT1: NMI
    true
}

fn wait_ticks(n: u64) {
    let start = crate::interrupts::ticks();
    while crate::interrupts::ticks() - start < n {
        core::hint::spin_loop();
    }
}

fn wait_icr_idle() {
    for _ in 0..1_000_000 {
        if lapic_read(LAPIC_ICR_LO) & (1 << 12) == 0 {
            return;
        }
        core::hint::spin_loop();
    }
}

/// Discover the CPUs and start every AP. Returns (cpus, aps online).
pub fn init() -> (u64, u64) {
    let Some((lapic_phys, ids)) = parse_madt() else {
        crate::println!("[smp] no MADT: single CPU");
        return (1, 0);
    };
    if ids.len() <= 1 {
        crate::println!("[smp] 1 CPU in the MADT -- nothing to start");
        return (1, 0);
    }
    // The trampoline page must be ordinary RAM (on UEFI, low memory can belong
    // to the firmware); the frame allocator keeps it free (never below 1 MiB).
    if !memory::is_usable_ram(TRAMPOLINE) {
        crate::println!("[smp] {:#x} is not usable RAM here -- APs not started", TRAMPOLINE);
        return (ids.len() as u64, 0);
    }
    if !map_lapic(lapic_phys) {
        crate::println!("[smp] could not map the Local APIC");
        return (ids.len() as u64, 0);
    }
    // Software-enable the BSP's LAPIC (it must be on to send IPIs).
    let svr = lapic_read(LAPIC_SVR);
    if svr & 0x100 == 0 {
        lapic_write(LAPIC_SVR, svr | 0x100 | 0xFF);
    }
    let bsp = (lapic_read(LAPIC_ID) >> 24) as u8;
    let Some(cr3) = temp_tables() else {
        crate::println!("[smp] could not build AP page tables");
        return (ids.len() as u64, 0);
    };

    // Copy the trampoline to 0x8000 and fill in the shared slots.
    let start = core::ptr::addr_of!(ap_tramp_start);
    let len = core::ptr::addr_of!(ap_tramp_end) as u64 - start as u64;
    let dst = memory::phys_to_virt(TRAMPOLINE) as *mut u8;
    unsafe { core::ptr::copy_nonoverlapping(start, dst, len as usize) };
    let slot = |sym: *const u8| unsafe { dst.add(sym_offset(sym) as usize) as *mut u64 };
    unsafe {
        slot(core::ptr::addr_of!(ap_cr3)).write_volatile(cr3);
        slot(core::ptr::addr_of!(ap_entry)).write_volatile(ap_main as *const () as u64);
    }

    let mut stacks: Vec<Vec<u8>> = Vec::new();
    for (n, &id) in ids.iter().enumerate() {
        if id == bsp {
            continue;
        }
        let stack = alloc::vec![0u8; AP_STACK];
        let top = (stack.as_ptr() as u64 + AP_STACK as u64) & !0xF;
        stacks.push(stack);
        unsafe {
            slot(core::ptr::addr_of!(ap_stack)).write_volatile(top);
            slot(core::ptr::addr_of!(ap_arg)).write_volatile(id as u64);
        }
        let before = ONLINE.load(Ordering::SeqCst);
        // INIT, then two STARTUP IPIs pointing at page 0x08 (= 0x8000).
        lapic_write(LAPIC_ICR_HI, (id as u32) << 24);
        lapic_write(LAPIC_ICR_LO, 0x0000_4500);
        wait_icr_idle();
        wait_ticks(1); // >= 10 ms
        for _ in 0..2 {
            lapic_write(LAPIC_ICR_HI, (id as u32) << 24);
            lapic_write(LAPIC_ICR_LO, 0x0000_4600 | (TRAMPOLINE >> 12) as u32);
            wait_icr_idle();
            for _ in 0..200_000 {
                core::hint::spin_loop(); // >= 200 us
            }
            if ONLINE.load(Ordering::SeqCst) > before {
                break;
            }
        }
        // Wait (up to ~1 s) for it to report in before reusing the slots.
        let t0 = crate::interrupts::ticks();
        while ONLINE.load(Ordering::SeqCst) == before && crate::interrupts::ticks() - t0 < 18 {
            core::hint::spin_loop();
        }
        let _ = n;
    }
    // The stacks belong to parked CPUs forever.
    core::mem::forget(stacks);
    let online = ONLINE.load(Ordering::SeqCst);
    crate::println!(
        "[smp] {} CPUs in the MADT (BSP lapic {}); {} application processor(s) online, parked",
        ids.len(),
        bsp,
        online
    );
    (ids.len() as u64, online)
}

/// First Rust code an AP runs (still on the temporary page tables, on its own
/// stack). Switch to the kernel's GDT and page tables, report in, and park.
extern "C" fn ap_main(_lapic_id: u64) -> ! {
    crate::gdt::load_on_ap();
    unsafe {
        x86_64::registers::control::Cr3::write(
            PhysFrame::containing_address(PhysAddr::new(KERNEL_CR3.load(Ordering::SeqCst))),
            x86_64::registers::control::Cr3Flags::empty(),
        )
    };
    ONLINE.fetch_add(1, Ordering::SeqCst);
    loop {
        x86_64::instructions::interrupts::disable();
        x86_64::instructions::hlt();
    }
}

/// The kernel's PML4 (what every CPU runs on once started).
static KERNEL_CR3: AtomicU64 = AtomicU64::new(0);

/// Record the kernel's CR3 for the APs (call on the BSP before `init`).
pub fn record_kernel_cr3() {
    let (l4, _) = x86_64::registers::control::Cr3::read();
    KERNEL_CR3.store(l4.start_address().as_u64(), Ordering::SeqCst);
}
