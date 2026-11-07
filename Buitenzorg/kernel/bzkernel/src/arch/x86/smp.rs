//! SMP bring-up. Increment 1: discover the CPUs (ACPI MADT), enable the
//! bootstrap CPU's Local APIC, and start every application processor (AP)
//! with INIT-SIPI-SIPI through a real-mode trampoline. Increment 2: each AP
//! gets its own GDT + TSS (`gdt::load_on_ap`), loads the shared IDT, runs a
//! periodic **Local APIC timer** calibrated against the PIT, and answers
//! **IPIs** (a ping and a TLB shootdown with acknowledgement); per-CPU state
//! lives in [`CPUS`], indexed by [`cpu_index`]. The APs then idle in
//! `sti; hlt`, woken by their timer and by IPIs — they still run no tasks.
//!
//! `cpu_index` identifies the CPU by its Local APIC id (an MMIO register that
//! ring-3 code cannot change). The usual fast path — a per-CPU pointer in the
//! GS base — needs `swapgs` on every kernel entry from ring 3, which comes with
//! the per-CPU SYSCALL entry in the next increment; until then user code could
//! reload GS and the kernel must not trust it.
//!
//! Why the APs run no tasks yet: the rest of the kernel is still single-core by design — the
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

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};
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

const LAPIC_EOI: u64 = 0xB0;
const LAPIC_TPR: u64 = 0x80;
const LAPIC_LVT_TIMER: u64 = 0x320;
const LAPIC_TIMER_INIT: u64 = 0x380;
const LAPIC_TIMER_CUR: u64 = 0x390;
const LAPIC_TIMER_DIV: u64 = 0x3E0;

/// Interrupt vectors served by the Local APIC.
pub const TIMER_VECTOR: u8 = 0xF0;
pub const IPI_PING_VECTOR: u8 = 0xF1;
pub const IPI_TLB_VECTOR: u8 = 0xF2;
pub const SPURIOUS_VECTOR: u8 = 0xFF;

/// Rate of each AP's Local APIC timer.
pub const AP_TIMER_HZ: u64 = 100;
/// The PIT's rate (1193182 / 65536 Hz), in milli-hertz.
const PIT_MILLIHZ: u64 = 18_207;

pub const MAX_CPUS: usize = crate::arch::cpu::MAX_CPUS;

static ONLINE: AtomicU64 = AtomicU64::new(0);

/// Per-CPU state. Index 0 is the bootstrap CPU; APs follow in MADT order.
pub struct PerCpu {
    lapic_id: AtomicU32,
    online: AtomicBool,
    /// The index the CPU computed for itself with `cpu_index` once running.
    seen_index: AtomicUsize,
    timer_ticks: AtomicU64,
    pings: AtomicU64,
    tlb_flushes: AtomicU64,
}

impl PerCpu {
    const fn new() -> Self {
        Self {
            lapic_id: AtomicU32::new(u32::MAX),
            online: AtomicBool::new(false),
            seen_index: AtomicUsize::new(usize::MAX),
            timer_ticks: AtomicU64::new(0),
            pings: AtomicU64::new(0),
            tlb_flushes: AtomicU64::new(0),
        }
    }
}

static CPUS: [PerCpu; MAX_CPUS] = [const { PerCpu::new() }; MAX_CPUS];
/// Entries of `CPUS` in use.
static CPU_COUNT: AtomicUsize = AtomicUsize::new(0);
/// The LAPIC page is mapped (so `cpu_index` may read the id register).
static LAPIC_READY: AtomicBool = AtomicBool::new(false);
/// Initial count of an AP timer period (divide-by-16), from calibration.
static TIMER_COUNT: AtomicU32 = AtomicU32::new(0);
/// Local APIC timer input clock in Hz (bus clock / 16), from calibration.
static TIMER_HZ: AtomicU64 = AtomicU64::new(0);
/// TSC rate in Hz, measured against the PIT with the LAPIC timer.
static TSC_HZ: AtomicU64 = AtomicU64::new(0);

/// The TSC rate (0 until `smp::init` calibrated it).
pub fn tsc_hz() -> u64 {
    TSC_HZ.load(Ordering::Relaxed)
}

/// Index of the CPU running this code (0 = bootstrap CPU). Reads the Local
/// APIC id — every CPU sees its own LAPIC at the same address.
pub fn cpu_index() -> usize {
    if !LAPIC_READY.load(Ordering::Acquire) {
        return 0;
    }
    let id = lapic_read(LAPIC_ID) >> 24;
    let n = CPU_COUNT.load(Ordering::Acquire);
    CPUS[..n].iter().position(|c| c.lapic_id.load(Ordering::Relaxed) == id).unwrap_or(0)
}

fn this_cpu() -> &'static PerCpu {
    &CPUS[cpu_index()]
}

fn eoi() {
    lapic_write(LAPIC_EOI, 0);
}

extern "x86-interrupt" fn lapic_timer_handler(frame: InterruptStackFrame) {
    this_cpu().timer_ticks.fetch_add(1, Ordering::Relaxed);
    eoi();
    if cpu_index() == 0 && LAPIC_TICKS.load(Ordering::Relaxed) {
        // 100 Hz -> the 18.2 Hz system tick generic code counts in
        let n = BSP_LAPIC_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if n * 182 / 1000 != (n - 1) * 182 / 1000 {
            crate::interrupts::lapic_system_tick(&frame);
        }
        return;
    }
    // After EOI, interrupts still off: the AP scheduler may switch tasks here.
    crate::appprof::sample(frame.instruction_pointer.as_u64(), frame.code_segment.rpl() == x86_64::PrivilegeLevel::Ring3);
    crate::task::ap_timer(frame.code_segment.rpl() == x86_64::PrivilegeLevel::Ring3);
}

extern "x86-interrupt" fn ping_handler(_frame: InterruptStackFrame) {
    this_cpu().pings.fetch_add(1, Ordering::Release);
    eoi();
}

extern "x86-interrupt" fn tlb_handler(_frame: InterruptStackFrame) {
    crate::arch::cpu::flush_tlb();
    this_cpu().tlb_flushes.fetch_add(1, Ordering::Release);
    eoi();
}

/// A spurious LAPIC interrupt must not be acknowledged.
extern "x86-interrupt" fn spurious_handler(_frame: InterruptStackFrame) {}

/// Register the Local APIC vectors in the (shared) IDT.
pub fn install_vectors(idt: &mut InterruptDescriptorTable) {
    idt[TIMER_VECTOR].set_handler_fn(lapic_timer_handler);
    idt[IPI_PING_VECTOR].set_handler_fn(ping_handler);
    idt[IPI_TLB_VECTOR].set_handler_fn(tlb_handler);
    idt[SPURIOUS_VECTOR].set_handler_fn(spurious_handler);
}

/// Send a fixed IPI with `vector` to the CPU with Local APIC id `lapic_id`.
fn send_ipi(lapic_id: u32, vector: u8) {
    crate::arch::cpu::without_interrupts(|| {
        wait_icr_idle();
        lapic_write(LAPIC_ICR_HI, lapic_id << 24);
        lapic_write(LAPIC_ICR_LO, 0x4000 | vector as u32); // fixed, assert
        wait_icr_idle();
    });
}

/// Send a fixed IPI with `vector` to every CPU except this one.
fn broadcast_ipi(vector: u8) {
    crate::arch::cpu::without_interrupts(|| {
        wait_icr_idle();
        lapic_write(LAPIC_ICR_HI, 0);
        lapic_write(LAPIC_ICR_LO, (0b11 << 18) | 0x4000 | vector as u32); // all excluding self
        wait_icr_idle();
    });
}

/// Spin until `done()` or roughly `ticks` PIT ticks pass; true if done.
fn wait_until(ticks: u64, done: impl Fn() -> bool) -> bool {
    let t0 = crate::interrupts::ticks();
    loop {
        if done() {
            return true;
        }
        if crate::interrupts::ticks() - t0 > ticks {
            return done();
        }
        core::hint::spin_loop();
    }
}

/// Online CPUs other than this one, as indices.
fn other_online() -> Vec<usize> {
    let me = cpu_index();
    (0..CPU_COUNT.load(Ordering::Acquire))
        .filter(|&i| i != me && CPUS[i].online.load(Ordering::Acquire))
        .collect()
}

/// Flush the TLB on every other online CPU and wait until each acknowledged
/// (the building block for unmapping memory that other cores may cache).
/// Returns false if a CPU did not answer within the spin budget.
pub fn tlb_shootdown() -> bool {
    let others = other_online();
    if others.is_empty() {
        return true;
    }
    let before: Vec<u64> = others.iter().map(|&i| CPUS[i].tlb_flushes.load(Ordering::Acquire)).collect();
    broadcast_ipi(IPI_TLB_VECTOR);
    // Wait with interrupts enabled: two CPUs may shoot down at the same time
    // (fine-grained syscalls run without the BKL), and each must still answer
    // the other's IPI. Spin budget, not PIT ticks: the tick count may not
    // advance while a syscall waits.
    let was_on = x86_64::instructions::interrupts::are_enabled();
    x86_64::instructions::interrupts::enable();
    let mut done = false;
    for _ in 0..50_000_000u64 {
        if others.iter().zip(&before).all(|(&i, &b)| CPUS[i].tlb_flushes.load(Ordering::Acquire) > b) {
            done = true;
            break;
        }
        core::hint::spin_loop();
    }
    if !was_on {
        x86_64::instructions::interrupts::disable();
    }
    done
}

/// Measure the Local APIC timer against the PIT (on the BSP, timer masked) and
/// derive the initial count of one AP timer period.
/// The 8254 counts: channel 2 must move between two latched reads. Laptops
/// with Intel's "8254 clock gating" (S0ix power saving) have a PIT whose
/// registers answer but never count -- every PIT wait then hangs or runs into
/// its iteration bound, and no timer IRQ 0 ever comes (on a Dell Latitude 7290
/// that was a black screen at boot).
fn pit_alive() -> bool {
    use x86_64::instructions::port::Port;
    unsafe {
        let mut p61 = Port::<u8>::new(0x61);
        let v = p61.read();
        p61.write((v & !0x02) | 0x01);
        Port::<u8>::new(0x43).write(0b1011_0000); // channel 2, lo/hi, mode 0
        Port::<u8>::new(0x42).write(0xFF);
        Port::<u8>::new(0x42).write(0xFF);
        let read = || {
            Port::<u8>::new(0x43).write(0b1000_0000); // latch channel 2
            let lo = Port::<u8>::new(0x42).read() as u16;
            let hi = Port::<u8>::new(0x42).read() as u16;
            lo | hi << 8
        };
        let a = read();
        let t0 = core::arch::x86_64::_rdtsc();
        // ~100 us at any plausible clock; the counter moves every 0.84 us
        while core::arch::x86_64::_rdtsc().wrapping_sub(t0) < 400_000 {
            core::hint::spin_loop();
        }
        read() != a
    }
}

/// The TSC rate from CPUID (leaf 0x15, else the base frequency in leaf 0x16).
fn cpuid_tsc_hz() -> Option<u64> {
    use core::arch::x86_64::__cpuid;
    let max = unsafe { __cpuid(0) }.eax;
    if max >= 0x15 {
        let l = unsafe { __cpuid(0x15) };
        if l.eax != 0 && l.ebx != 0 {
            let mut crystal = l.ecx as u64;
            if crystal == 0 {
                // Skylake/Kaby Lake/Coffee Lake client parts leave it out: 24 MHz
                let sig = unsafe { __cpuid(1) }.eax;
                let model = ((sig >> 4) & 0xF) | ((sig >> 12) & 0xF0);
                if (sig >> 8) & 0xF == 6 && matches!(model, 0x4E | 0x5E | 0x8E | 0x9E | 0xA5 | 0xA6) {
                    crystal = 24_000_000;
                }
            }
            if crystal != 0 {
                return Some(crystal * l.ebx as u64 / l.eax as u64);
            }
        }
    }
    if max >= 0x16 {
        let mhz = unsafe { __cpuid(0x16) }.eax as u64 & 0xFFFF;
        if mhz != 0 {
            return Some(mhz * 1_000_000);
        }
    }
    None
}

/// CPU 0's LAPIC timer drives the system tick (no working PIT).
static LAPIC_TICKS: AtomicBool = AtomicBool::new(false);
static PIT_OK: AtomicBool = AtomicBool::new(true);
static BSP_LAPIC_COUNT: AtomicU64 = AtomicU64::new(0);

pub fn ticks_from_lapic() -> bool {
    LAPIC_TICKS.load(Ordering::Relaxed)
}

/// Busy-wait on the TSC (once its rate is known).
fn tsc_wait_us(us: u64) {
    let hz = TSC_HZ.load(Ordering::Relaxed).max(1);
    let t0 = unsafe { core::arch::x86_64::_rdtsc() };
    while unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(t0) < us * hz / 1_000_000 {
        core::hint::spin_loop();
    }
}

/// Wait `us` microseconds: PIT channel 2 when it works, else the TSC.
fn wait_us(us: u32) {
    if PIT_OK.load(Ordering::Relaxed) {
        pit_wait_us(us);
    } else {
        tsc_wait_us(us as u64);
    }
}

/// The TSC rate measured against the ACPI PM timer over ~10 ms.
fn pm_timer_tsc_hz() -> Option<u64> {
    use x86_64::instructions::port::Port;
    let (port, wide) = crate::power::pm_timer()?;
    let mask: u32 = if wide { u32::MAX } else { 0xFF_FFFF };
    let read = || unsafe { Port::<u32>::new(port).read() } & mask;
    const PM_HZ: u64 = 3_579_545;
    let want = PM_HZ / 100; // 10 ms
    let start = read();
    // the timer must move at all (a floating port reads all ones)
    let t = unsafe { core::arch::x86_64::_rdtsc() };
    while read() == start {
        if unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(t) > 50_000_000 {
            return None;
        }
    }
    let p0 = read();
    let c0 = unsafe { core::arch::x86_64::_rdtsc() };
    loop {
        let d = (read().wrapping_sub(p0) & mask) as u64;
        if d >= want {
            let c = unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(c0);
            return Some(c * PM_HZ / d);
        }
    }
}

/// Without a PIT: the TSC rate from the ACPI PM timer (else CPUID), the LAPIC
/// timer measured against the TSC, and CPU 0's LAPIC timer as the system tick.
fn calibrate_without_pit() -> bool {
    let (tsc_hz, from) = match pm_timer_tsc_hz() {
        Some(hz) => (hz, "the ACPI PM timer"),
        None => match cpuid_tsc_hz() {
            Some(hz) => (hz, "CPUID"),
            None => {
                crate::println!("[smp] no working PIT, ACPI PM timer or CPUID TSC rate: timers stay uncalibrated");
                return false;
            }
        },
    };
    TSC_HZ.store(tsc_hz, Ordering::Relaxed);
    const US: u64 = 10_000;
    lapic_write(LAPIC_TIMER_INIT, u32::MAX);
    tsc_wait_us(US);
    let elapsed = (u32::MAX - lapic_read(LAPIC_TIMER_CUR)) as u64;
    lapic_write(LAPIC_TIMER_INIT, 0);
    let hz = elapsed * 1_000_000 / US;
    TIMER_HZ.store(hz, Ordering::Relaxed);
    TIMER_COUNT.store((hz / AP_TIMER_HZ).clamp(1, u32::MAX as u64) as u32, Ordering::Relaxed);
    LAPIC_TICKS.store(true, Ordering::SeqCst);
    start_local_timer(); // on CPU 0: the handler turns it into system ticks
    crate::println!(
        "[smp] the 8254 PIT does not count (clock gated by the firmware): TSC {} MHz from {}, system tick from the Local APIC timer",
        tsc_hz / 1_000_000,
        from
    );
    true
}

fn calibrate_timer() {
    // 3 PIT periods on the self-test boot (its timer checks expect that
    // precision), 1 on a normal boot: the wait is boot time
    let cal_ticks: u64 = if crate::bootmode::selftest() { 3 } else { 1 };
    lapic_write(LAPIC_TIMER_DIV, 0x3); // divide by 16
    lapic_write(LAPIC_LVT_TIMER, (1 << 16) | TIMER_VECTOR as u32); // masked, one-shot
    if !pit_alive() {
        PIT_OK.store(false, Ordering::SeqCst);
        if calibrate_without_pit() {
            return;
        }
    }
    if !crate::bootmode::selftest() {
        // normal boot: 10 ms on PIT channel 2 instead of up to two 55 ms ticks
        const US: u64 = 10_000;
        let tsc0 = unsafe { core::arch::x86_64::_rdtsc() };
        lapic_write(LAPIC_TIMER_INIT, u32::MAX);
        pit_wait_us(US as u32);
        let elapsed = (u32::MAX - lapic_read(LAPIC_TIMER_CUR)) as u64;
        let tsc = unsafe { core::arch::x86_64::_rdtsc() } - tsc0;
        lapic_write(LAPIC_TIMER_INIT, 0);
        TSC_HZ.store(tsc * 1_000_000 / US, Ordering::Relaxed);
        let hz = elapsed * 1_000_000 / US;
        TIMER_HZ.store(hz, Ordering::Relaxed);
        TIMER_COUNT.store((hz / AP_TIMER_HZ).clamp(1, u32::MAX as u64) as u32, Ordering::Relaxed);
        return;
    }
    wait_ticks(1); // start on a PIT tick edge
    let tsc0 = unsafe { core::arch::x86_64::_rdtsc() };
    lapic_write(LAPIC_TIMER_INIT, u32::MAX);
    wait_ticks(cal_ticks);
    let elapsed = (u32::MAX - lapic_read(LAPIC_TIMER_CUR)) as u64;
    let tsc = unsafe { core::arch::x86_64::_rdtsc() } - tsc0;
    TSC_HZ.store(tsc * PIT_MILLIHZ / (cal_ticks * 1000), Ordering::Relaxed);
    lapic_write(LAPIC_TIMER_INIT, 0); // stop
    let hz = elapsed * PIT_MILLIHZ / (cal_ticks * 1000);
    TIMER_HZ.store(hz, Ordering::Relaxed);
    TIMER_COUNT.store((hz / AP_TIMER_HZ).clamp(1, u32::MAX as u64) as u32, Ordering::Relaxed);
}

/// Program this CPU's LAPIC: accept all priorities, software-enable with the
/// spurious vector, and start the periodic timer.
fn start_local_timer() {
    lapic_write(LAPIC_TPR, 0);
    lapic_write(LAPIC_SVR, (lapic_read(LAPIC_SVR) & !0xFF) | 0x100 | SPURIOUS_VECTOR as u32);
    lapic_write(LAPIC_TIMER_DIV, 0x3);
    lapic_write(LAPIC_LVT_TIMER, (1 << 17) | TIMER_VECTOR as u32); // periodic
    lapic_write(LAPIC_TIMER_INIT, TIMER_COUNT.load(Ordering::Relaxed));
}

/// One line per CPU for the shell's `cpu` command.
pub fn report() -> Vec<String> {
    let n = CPU_COUNT.load(Ordering::Acquire);
    let mut out = Vec::new();
    out.push(alloc::format!(
        "{} CPU, scheduler simetris + Big Kernel Lock (tunggu {}x, migrasi {}); timer LAPIC {} MHz; CPU ini: #{}",
        n.max(1),
        crate::task::bkl_waits(),
        crate::task::migrations(),
        TIMER_HZ.load(Ordering::Relaxed) / 1_000_000,
        cpu_index()
    ));
    for (i, c) in CPUS[..n].iter().enumerate() {
        let state = if !c.online.load(Ordering::Relaxed) {
            "offline"
        } else if crate::task::cpu_runs_user(i) {
            "ring 3 "
        } else if crate::task::ap_running(i) {
            "kernel "
        } else {
            "idle   "
        };
        out.push(alloc::format!(
            "  #{} lapic {:>2} {} {}  syscalls {:>6}  kernel-tasks {:>5}  ring-3 {:>5}  ipi {:>4}  tlb {:>3}  kbd/mouse irq {}/{}",
            i,
            c.lapic_id.load(Ordering::Relaxed),
            if i == 0 { "BSP" } else { "AP " },
            state,
            crate::task::syscalls_on(i),
            crate::task::kernel_runs_on(i),
            crate::task::ap_user_ticks(i),
            c.pings.load(Ordering::Relaxed),
            c.tlb_flushes.load(Ordering::Relaxed),
            crate::ioapic::irq_counts(i).0,
            crate::ioapic::irq_counts(i).1
        ));
    }
    out
}

/// Increment-2 self-test (run on the BSP after `init`): every AP is on its own
/// GDT/TSS and knows its own index, its LAPIC timer ticks, it answers a
/// directed IPI, and a broadcast TLB shootdown is acknowledged by all.
/// Returns a summary on success, or what failed.
pub fn self_test() -> Result<String, String> {
    let aps: Vec<usize> = other_online();
    if aps.is_empty() {
        return Err(String::from("no application processor online"));
    }
    if cpu_index() != 0 {
        return Err(String::from("BSP does not see itself as CPU #0"));
    }
    for &i in &aps {
        let seen = CPUS[i].seen_index.load(Ordering::Acquire);
        if seen != i {
            return Err(alloc::format!("CPU #{} computed index {}", i, seen));
        }
    }
    // Timers: the wait is 2-3 PIT ticks (110-165 ms, depending on phase), so
    // 11-16 ticks at 100 Hz; require at least 5.
    let t0: Vec<u64> = aps.iter().map(|&i| CPUS[i].timer_ticks.load(Ordering::Relaxed)).collect();
    wait_ticks(3);
    let mut min_ticks = u64::MAX;
    for (k, &i) in aps.iter().enumerate() {
        min_ticks = min_ticks.min(CPUS[i].timer_ticks.load(Ordering::Relaxed) - t0[k]);
    }
    if min_ticks < 5 {
        return Err(alloc::format!("AP timer too slow ({} ticks in 110-165 ms)", min_ticks));
    }
    // Directed IPIs.
    for &i in &aps {
        let before = CPUS[i].pings.load(Ordering::Acquire);
        send_ipi(CPUS[i].lapic_id.load(Ordering::Relaxed), IPI_PING_VECTOR);
        if !wait_until(9, || CPUS[i].pings.load(Ordering::Acquire) > before) {
            return Err(alloc::format!("CPU #{} did not answer a ping IPI", i));
        }
    }
    // Broadcast TLB shootdown, twice.
    if !(tlb_shootdown() && tlb_shootdown()) {
        return Err(String::from("TLB shootdown not acknowledged"));
    }
    Ok(alloc::format!(
        "{} AP(s): own GDT/TSS, LAPIC timer {} Hz ({} ticks in 110-165 ms), ping IPI + TLB shootdown acked",
        aps.len(),
        AP_TIMER_HZ,
        min_ticks
    ))
}

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
    map_mmio(LAPIC_VA, phys)
}

/// Acknowledge the interrupt being handled on this CPU at its Local APIC.
pub fn lapic_eoi() {
    eoi();
}

/// CPUs registered and online (the BSP counts).
pub fn cpus_online() -> usize {
    let n = CPU_COUNT.load(Ordering::Acquire);
    (0..n).filter(|&i| CPUS[i].online.load(Ordering::Acquire)).count().max(1)
}

/// Local APIC id of CPU `cpu` (None before `init` registered it).
pub fn lapic_id_of(cpu: usize) -> Option<u32> {
    if cpu == 0 && CPU_COUNT.load(Ordering::Acquire) == 0 {
        // Single CPU (no MADT walk happened): ask the hardware.
        return LAPIC_READY.load(Ordering::Acquire).then(|| lapic_read(LAPIC_ID) >> 24);
    }
    let id = CPUS.get(cpu)?.lapic_id.load(Ordering::Relaxed);
    (id != u32::MAX).then_some(id)
}

/// Map one uncached MMIO page at kernel address `va` (LAPIC, I/O APIC).
pub fn map_mmio(va: u64, phys: u64) -> bool {
    memory::with_ctx(|ctx| {
        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(va));
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
    LAPIC_READY.store(true, Ordering::Release);
    true
}

/// Busy-wait `us` microseconds (<= 54 ms) on PIT channel 2 in one-shot mode,
/// polling its output on port 0x61 -- far finer than the 55 ms system tick.
fn pit_wait_us(us: u32) {
    use x86_64::instructions::port::Port;
    let count = ((us as u64 * 1_193_182 / 1_000_000).clamp(1, 0xFFFF)) as u16;
    unsafe {
        let mut p61 = Port::<u8>::new(0x61);
        let v = p61.read();
        p61.write((v & !0x02) | 0x01); // gate channel 2 on, speaker off
        Port::<u8>::new(0x43).write(0b1011_0000); // channel 2, lo/hi byte, mode 0, binary
        let mut ch2 = Port::<u8>::new(0x42);
        ch2.write(count as u8);
        ch2.write((count >> 8) as u8); // counting starts now
        for _ in 0..50_000_000u64 {
            if p61.read() & 0x20 != 0 {
                break; // OUT2 went high: the count ran out
            }
            core::hint::spin_loop();
        }
    }
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
    // Register the CPUs: the BSP is #0, the APs follow in MADT order.
    let mut order: Vec<u8> = alloc::vec![bsp];
    order.extend(ids.iter().copied().filter(|&id| id != bsp));
    order.truncate(MAX_CPUS);
    for (i, &id) in order.iter().enumerate() {
        CPUS[i].lapic_id.store(id as u32, Ordering::Relaxed);
    }
    CPUS[0].online.store(true, Ordering::Relaxed);
    CPUS[0].seen_index.store(0, Ordering::Relaxed);
    CPU_COUNT.store(order.len(), Ordering::Release);
    LAPIC_READY.store(true, Ordering::Release);
    calibrate_timer();
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

    memory::prepare_mmio_window();
    let mut stacks: Vec<Vec<u8>> = Vec::new();
    for (cpu, &id) in order.iter().enumerate().skip(1) {
        if crate::aspace::create_ap_root(cpu).is_none() {
            crate::println!("[smp] could not build a page-table root for CPU #{}", cpu);
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
        if crate::bootmode::selftest() {
            wait_ticks(1); // >= 10 ms
        } else {
            wait_us(10_000); // exactly the 10 ms INIT needs
        }
        for _ in 0..2 {
            lapic_write(LAPIC_ICR_HI, (id as u32) << 24);
            lapic_write(LAPIC_ICR_LO, 0x0000_4600 | (TRAMPOLINE >> 12) as u32);
            wait_icr_idle();
            wait_us(200); // the 200 us between STARTUPs (a pause loop is slow under TCG)
            if ONLINE.load(Ordering::SeqCst) > before {
                break;
            }
        }
        // Wait (up to ~1 s) for it to report in before reusing the slots.
        let (t0, c0) = (crate::interrupts::ticks(), unsafe { core::arch::x86_64::_rdtsc() });
        let limit = TSC_HZ.load(Ordering::Relaxed).max(1_000_000_000); // ~1 s, whatever ticks do
        while ONLINE.load(Ordering::SeqCst) == before
            && crate::interrupts::ticks() - t0 < 18
            && unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(c0) < limit
        {
            core::hint::spin_loop();
        }
    }
    // The stacks belong to parked CPUs forever.
    core::mem::forget(stacks);
    let online = ONLINE.load(Ordering::SeqCst);
    crate::println!(
        "[smp] {} CPUs in the MADT (BSP lapic {}); {} application processor(s) online, idle; LAPIC timer {} kHz",
        ids.len(),
        bsp,
        online,
        TIMER_HZ.load(Ordering::Relaxed) / 1000
    );
    (ids.len() as u64, online)
}

/// First Rust code an AP runs (still on the temporary page tables, on its own
/// stack). Switch to the kernel's page tables, take its own GDT + TSS, load
/// the IDT, start its LAPIC timer, report in, and idle with interrupts on.
extern "C" fn ap_main(_lapic_id: u64) -> ! {
    // The temporary tables share the kernel mappings (LAPIC included), so the
    // CPU can find its index before switching to its own root.
    let me = cpu_index();
    unsafe {
        x86_64::registers::control::Cr3::write(
            PhysFrame::containing_address(PhysAddr::new(crate::aspace::ap_root(me))),
            x86_64::registers::control::Cr3Flags::empty(),
        )
    };
    // The GDT/TSS are heap-allocated; the BSP is spinning in `init` (not
    // holding the heap lock) while it waits for this AP to report in.
    crate::gdt::load_on_ap(me);
    crate::interrupts::load_on_ap();
    crate::gdt::enable_sse();
    crate::usermode::init_on_ap(me);
    start_local_timer();
    CPUS[me].seen_index.store(me, Ordering::Release);
    CPUS[me].online.store(true, Ordering::Release);
    ONLINE.fetch_add(1, Ordering::SeqCst);
    // Join the (symmetric) scheduler: this context becomes the CPU's idle task.
    crate::task::ap_start(me)
}

/// Number of application processors online.
pub fn aps_online() -> usize {
    ONLINE.load(Ordering::Acquire) as usize
}

/// Wake every other CPU (a ping IPI to all but this one) so an idle one
/// re-runs its scheduler — e.g. after a task became runnable.
pub fn wake_others() {
    if aps_online() > 0 && LAPIC_READY.load(Ordering::Acquire) {
        broadcast_ipi(IPI_PING_VECTOR);
    }
}

/// The kernel's PML4 (what every CPU runs on once started).
static KERNEL_CR3: AtomicU64 = AtomicU64::new(0);

/// Record the kernel's CR3 for the APs (call on the BSP before `init`).
pub fn record_kernel_cr3() {
    let (l4, _) = x86_64::registers::control::Cr3::read();
    KERNEL_CR3.store(l4.start_address().as_u64(), Ordering::SeqCst);
}
