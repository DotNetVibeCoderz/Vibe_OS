//! I/O APIC: device interrupts routed through the APIC instead of the legacy
//! 8259 PIC, and spread over the CPUs (SMP).
//!
//! The ACPI MADT lists the I/O APIC(s) (MMIO base + first global system
//! interrupt, GSI) and "interrupt source overrides" mapping ISA IRQs to GSIs
//! with their polarity/trigger (on PC firmware the PIT's IRQ 0 is usually GSI
//! 2). `init` programs one redirection entry per ISA line the kernel uses —
//! PIT (IRQ 0), PS/2 keyboard (IRQ 1), PS/2 mouse (IRQ 12) — with the *same*
//! vectors the PIC used (32 + irq), so the IDT handlers stay as they are; it
//! then masks the 8259 completely and switches the handlers' end-of-interrupt
//! to the Local APIC (`interrupts::end_of_interrupt`).
//!
//! Routing: the PIT stays on CPU 0 (the scheduler charges PIT ticks to CPU 0's
//! task and the boot flow's timing relies on it); keyboard and mouse go to the
//! last CPU when there is more than one — their handlers only touch the
//! keyboard queue (spin lock) and the mouse state (atomics), which is safe on
//! any CPU. `self_test` proves the routes by making the 8042 controller raise
//! real IRQ 1 / IRQ 12 (commands 0xD2 / 0xD3 put a byte in its output buffer as
//! if a key or the mouse had sent it) and checking which CPU handled them.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use x86_64::instructions::port::Port;

use crate::memory;

/// Kernel virtual address of the I/O APIC's MMIO page (next to the LAPIC's).
const IOAPIC_VA: u64 = 0x4444_6000_1000;
const IOREGSEL: u64 = 0x00;
const IOWIN: u64 = 0x10;

static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Per-CPU count of keyboard / mouse IRQs handled (routing evidence).
static KBD_IRQS: [AtomicU64; crate::arch::cpu::MAX_CPUS] =
    [const { AtomicU64::new(0) }; crate::arch::cpu::MAX_CPUS];
static MOUSE_IRQS: [AtomicU64; crate::arch::cpu::MAX_CPUS] =
    [const { AtomicU64::new(0) }; crate::arch::cpu::MAX_CPUS];
/// Where each line was routed: (gsi, cpu) for PIT, keyboard, mouse.
static ROUTES: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

/// True once device IRQs arrive through the I/O APIC (the handlers then send
/// their EOI to the Local APIC instead of the 8259).
pub fn active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// Count an IRQ handled on this CPU (called by the keyboard / mouse handlers).
pub fn note_irq(irq: u8) {
    let c = crate::smp::cpu_index();
    match irq {
        1 => KBD_IRQS[c].fetch_add(1, Ordering::Relaxed),
        12 => MOUSE_IRQS[c].fetch_add(1, Ordering::Relaxed),
        _ => 0,
    };
}

/// (keyboard, mouse) IRQs handled by CPU `cpu`.
pub fn irq_counts(cpu: usize) -> (u64, u64) {
    (KBD_IRQS[cpu].load(Ordering::Relaxed), MOUSE_IRQS[cpu].load(Ordering::Relaxed))
}

fn read(reg: u32) -> u32 {
    unsafe {
        core::ptr::write_volatile((IOAPIC_VA + IOREGSEL) as *mut u32, reg);
        core::ptr::read_volatile((IOAPIC_VA + IOWIN) as *const u32)
    }
}

fn write(reg: u32, val: u32) {
    unsafe {
        core::ptr::write_volatile((IOAPIC_VA + IOREGSEL) as *mut u32, reg);
        core::ptr::write_volatile((IOAPIC_VA + IOWIN) as *mut u32, val);
    }
}

/// What the MADT says about the (first) I/O APIC and the ISA overrides.
struct Madt {
    ioapic_phys: u64,
    gsi_base: u32,
    /// ISA IRQ -> (gsi, flags) for IRQs 0..16.
    overrides: [(u32, u16); 16],
}

fn parse_madt() -> Option<Madt> {
    let madt = crate::power::find_table(b"APIC")?;
    let v = |off: u64| memory::phys_to_virt(madt + off) as *const u8;
    let len = unsafe { core::ptr::read_unaligned(v(4) as *const u32) } as u64;
    let mut out = Madt { ioapic_phys: 0, gsi_base: 0, overrides: [(0, 0); 16] };
    for irq in 0..16 {
        out.overrides[irq] = (irq as u32, 0); // identity, bus-default polarity/trigger
    }
    let mut off = 44;
    while off + 2 <= len {
        let (kind, elen) = unsafe { (*v(off), *v(off + 1) as u64) };
        if elen < 2 {
            break;
        }
        match kind {
            1 if out.ioapic_phys == 0 => unsafe {
                out.ioapic_phys = core::ptr::read_unaligned(v(off + 4) as *const u32) as u64;
                out.gsi_base = core::ptr::read_unaligned(v(off + 8) as *const u32);
            },
            2 => unsafe {
                let src = *v(off + 3) as usize;
                let gsi = core::ptr::read_unaligned(v(off + 4) as *const u32);
                let flags = core::ptr::read_unaligned(v(off + 8) as *const u16);
                if src < 16 {
                    out.overrides[src] = (gsi, flags);
                }
            },
            _ => {}
        }
        off += elen;
    }
    (out.ioapic_phys != 0).then_some(out)
}

/// Program the redirection entry for `gsi`: `vector`, fixed delivery, physical
/// destination `lapic_id`, polarity/trigger from the MADT override flags (ISA
/// defaults: active high, edge).
fn route(m: &Madt, gsi: u32, flags: u16, vector: u8, lapic_id: u32) {
    let pin = gsi - m.gsi_base;
    let active_low = flags & 0b11 == 0b11;
    let level = (flags >> 2) & 0b11 == 0b11;
    let mut lo = vector as u32;
    if active_low {
        lo |= 1 << 13;
    }
    if level {
        lo |= 1 << 15;
    }
    write(0x10 + 2 * pin + 1, lapic_id << 24);
    write(0x10 + 2 * pin, lo); // unmasked
}

/// Switch device interrupts to the I/O APIC (after `smp::init`). Returns a
/// description of the routes, or None if there is no I/O APIC (the 8259 stays).
pub fn init() -> Option<alloc::string::String> {
    let m = parse_madt()?;
    if !crate::smp::map_mmio(IOAPIC_VA, m.ioapic_phys) {
        return None;
    }
    let max_entry = (read(0x01) >> 16) & 0xFF;
    let cpus = crate::smp::cpus_online();
    let input_cpu = if cpus > 1 { cpus - 1 } else { 0 };
    let id0 = crate::smp::lapic_id_of(0)?;
    let id_in = crate::smp::lapic_id_of(input_cpu)?;
    let lines = [(0u8, 0usize, id0), (1, input_cpu, id_in), (12, input_cpu, id_in)];
    x86_64::instructions::interrupts::without_interrupts(|| {
        // Mask every entry first, then route the three lines.
        for pin in 0..=max_entry {
            write(0x10 + 2 * pin, 1 << 16);
        }
        for (i, &(irq, cpu, lapic)) in lines.iter().enumerate() {
            let (gsi, flags) = m.overrides[irq as usize];
            route(&m, gsi, flags, crate::interrupts::PIC_1_OFFSET + irq, lapic);
            ROUTES[i].store((gsi as u64) << 32 | cpu as u64, Ordering::Relaxed);
        }
        // The 8259 goes quiet; handlers now acknowledge the Local APIC.
        unsafe {
            Port::<u8>::new(0x21).write(0xFF);
            Port::<u8>::new(0xA1).write(0xFF);
        }
        ACTIVE.store(true, Ordering::Release);
    });
    let r = |i: usize| {
        let v = ROUTES[i].load(Ordering::Relaxed);
        ((v >> 32) as u32, (v & 0xFFFF_FFFF) as usize)
    };
    let ((pg, pc), (kg, kc), (mg, mc)) = (r(0), r(1), r(2));
    Some(alloc::format!(
        "I/O APIC at {:#x}, {} pins; PIT IRQ0->GSI{}->CPU#{}, keyboard IRQ1->GSI{}->CPU#{}, mouse IRQ12->GSI{}->CPU#{}; 8259 masked",
        m.ioapic_phys, max_entry + 1, pg, pc, kg, kc, mg, mc
    ))
}

/// Wait (bounded) for the 8042's input buffer to be empty.
fn kbc_wait_write() -> bool {
    let mut status = Port::<u8>::new(0x64);
    for _ in 0..100_000 {
        if unsafe { status.read() } & 0x02 == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

/// Wait (up to ~18 PIT ticks, ~1 s) until the 8042's output buffer is empty,
/// i.e. the interrupt handler consumed the last injected byte. Injecting the
/// next one while it is still full can merge two bytes into one IRQ.
fn kbc_wait_output_empty() -> bool {
    let mut status = Port::<u8>::new(0x64);
    let t0 = crate::interrupts::ticks();
    while crate::interrupts::ticks() < t0 + 18 {
        if unsafe { status.read() } & 0x01 == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

/// Prove the routes: the PIT still ticks, and IRQ 1 / IRQ 12 raised by the
/// 8042 itself are handled on the CPU they were routed to.
pub fn self_test() -> Result<alloc::string::String, alloc::string::String> {
    if !active() {
        return Err("I/O APIC not active".into());
    }
    let t0 = crate::interrupts::ticks();
    let start = crate::arch::cpu::timestamp();
    while crate::interrupts::ticks() < t0 + 3 {
        if crate::arch::cpu::timestamp() - start > 20_000_000_000 {
            return Err("the PIT stopped ticking after the switch".into());
        }
        core::hint::spin_loop();
    }
    let kbd_cpu = (ROUTES[1].load(Ordering::Relaxed) & 0xFFFF_FFFF) as usize;
    let mouse_cpu = (ROUTES[2].load(Ordering::Relaxed) & 0xFFFF_FFFF) as usize;
    let (k0, _) = irq_counts(kbd_cpu);
    let (_, m0) = irq_counts(mouse_cpu);
    let mut cmd = Port::<u8>::new(0x64);
    let mut data = Port::<u8>::new(0x60);
    // A key *release* code (A up): decoded, but produces no character.
    if kbc_wait_write() {
        unsafe { cmd.write(0xD2) };
        if kbc_wait_write() {
            unsafe { data.write(0x9E) };
        }
    }
    // One empty mouse packet (no movement, no buttons), byte by byte -- each
    // only once the previous byte was taken out of the output buffer.
    for b in [0x08u8, 0x00, 0x00] {
        kbc_wait_output_empty();
        if kbc_wait_write() {
            unsafe { cmd.write(0xD3) };
            if kbc_wait_write() {
                unsafe { data.write(b) };
            }
        }
        let t = crate::interrupts::ticks();
        while crate::interrupts::ticks() < t + 1 {
            core::hint::spin_loop();
        }
    }
    // The IRQs are handled on another CPU: give them up to ~1 s (a busy CI
    // host can deschedule that vCPU for a while), but stop as soon as all
    // four have arrived.
    let t = crate::interrupts::ticks();
    while crate::interrupts::ticks() < t + 18 {
        if irq_counts(kbd_cpu).0 > k0 && irq_counts(mouse_cpu).1 >= m0 + 3 {
            break;
        }
        core::hint::spin_loop();
    }
    let (k1, _) = irq_counts(kbd_cpu);
    let (_, m1) = irq_counts(mouse_cpu);
    if k1 <= k0 {
        return Err(alloc::format!("no keyboard IRQ arrived on CPU #{}", kbd_cpu));
    }
    if m1 < m0 + 3 {
        return Err(alloc::format!("mouse IRQs on CPU #{}: {} of 3", mouse_cpu, m1 - m0));
    }
    Ok(alloc::format!(
        "PIT ticking via the I/O APIC; injected keyboard IRQ handled on CPU #{}, 3 mouse IRQs on CPU #{}",
        kbd_cpu, mouse_cpu
    ))
}
