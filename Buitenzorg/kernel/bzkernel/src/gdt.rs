//! GDT + TSS. Provides kernel and user (ring 3) segments in the order the
//! SYSCALL/SYSRET STAR MSR requires, a double-fault IST stack, and a ring-0
//! stack (`privilege_stack_table[0]`) for interrupts taken while in ring 3.

use spin::Once;
use x86_64::instructions::tables::load_tss;
use x86_64::registers::segmentation::{Segment, CS, DS, ES, SS};
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
const STACK_SIZE: usize = 4096 * 5;
// The privilege stack doubles as the SYSCALL kernel stack for a non-threaded
// app's main thread (usermode.rs sets SYSCALL_KSTACK_TOP to its top). The
// deepest thing that runs on it is a WIN_PRESENT syscall -> wm::present_now ->
// compose_into, which composites every open window (title bars, Noto text,
// canvas blits) — so it needs real headroom, not the 20 KiB the fault stack
// gets. Undersizing it overflowed into adjacent statics intermittently
// (layout-dependent), smashing return addresses (breakpoint/#PF/#DF cascade).
const PRIV_STACK_SIZE: usize = 64 * 1024;

/// A statically allocated kernel stack, 16-byte aligned. A bare `[u8; N]` has
/// alignment 1, so the linker may place it at any byte: the privilege stack
/// once landed at an odd address (…4b1), making SYSCALL_KSTACK_TOP / TSS rsp0
/// misaligned, so every syscall from ring 3 ran on a stack violating the
/// x86-64 16-byte ABI alignment. Whether it was aligned depended on the binary
/// layout — the boot crashed (rodata-dump cascade at the first ring-3 syscall)
/// or not depending on unrelated code changes.
#[repr(C, align(16))]
struct Stack<const N: usize>([u8; N]);

static mut DOUBLE_FAULT_STACK: Stack<STACK_SIZE> = Stack([0; STACK_SIZE]);
static mut PRIVILEGE_STACK: Stack<PRIV_STACK_SIZE> = Stack([0; PRIV_STACK_SIZE]);
/// The TSS lives in a `static mut` (not a `Once`) because its ring-0 stack
/// pointer changes on every context switch between user processes: each
/// process/thread takes ring-3 interrupts on its own kernel stack, so a timer
/// that preempts one process never lands on another's suspended frame.
static mut TSS: TaskStateSegment = TaskStateSegment::new();
static GDT: Once<(GlobalDescriptorTable, Selectors)> = Once::new();

#[derive(Clone, Copy)]
pub struct Selectors {
    pub kernel_code: SegmentSelector,
    pub kernel_data: SegmentSelector,
    pub user_code: SegmentSelector,
    pub user_data: SegmentSelector,
    tss: SegmentSelector,
}

static SELECTORS: Once<Selectors> = Once::new();

/// Top of the ring-0 stack the CPU switches to when an interrupt is taken
/// from ring 3 (also reused as the SYSCALL kernel stack; see usermode.rs).
pub fn privilege_stack_top() -> VirtAddr {
    VirtAddr::from_ptr(core::ptr::addr_of!(PRIVILEGE_STACK)) + PRIV_STACK_SIZE as u64
}

pub fn selectors() -> Selectors {
    *SELECTORS.get().expect("gdt not initialized")
}

/// Enable SSE/SSE2 (clear CR0.EM, set CR0.MP, set CR4.OSFXSR + OSXMMEXCPT).
/// NativeAOT-compiled user code uses xmm registers, so this must run before
/// any managed code executes (and it is harmless for the kernel too).
pub fn enable_sse() {
    use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
    unsafe {
        Cr0::update(|f| {
            f.remove(Cr0Flags::EMULATE_COPROCESSOR); // EM = 0
            f.insert(Cr0Flags::MONITOR_COPROCESSOR); // MP = 1
        });
        Cr4::update(|f| {
            f.insert(Cr4Flags::OSFXSR);
            f.insert(Cr4Flags::OSXMMEXCPT_ENABLE);
        });
    }
}

/// Point the TSS ring-0 stack (rsp0) at `top`: the stack the CPU switches to
/// when an interrupt or exception is taken from ring 3. Called by the scheduler
/// with interrupts disabled.
///
/// `TaskStateSegment` is `packed(4)`, so rsp0 (a u64) sits at offset 4 and is
/// NOT 8-byte aligned, so it must be written with `write_unaligned` — a
/// `write_volatile` through a misaligned `*mut VirtAddr` is undefined behavior.
pub fn set_rsp0(top: u64) {
    unsafe {
        core::ptr::write_unaligned(
            core::ptr::addr_of_mut!(TSS.privilege_stack_table[0]),
            VirtAddr::new(top),
        );
    }
}

/// Load the kernel GDT on an application processor and reload its segment
/// registers. The TSS is not loaded: it is marked busy by the BSP, and a parked
/// AP (interrupts off) never needs a privilege-level stack switch.
pub fn load_on_ap() {
    let (gdt, selectors) = GDT.get().expect("gdt not initialized");
    gdt.load();
    unsafe {
        CS::set_reg(selectors.kernel_code);
        SS::set_reg(selectors.kernel_data);
        DS::set_reg(selectors.kernel_data);
        ES::set_reg(selectors.kernel_data);
    }
}

pub fn init() {
    let tss: &'static TaskStateSegment = unsafe {
        let tss = &mut *core::ptr::addr_of_mut!(TSS);
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            let start = VirtAddr::from_ptr(core::ptr::addr_of!(DOUBLE_FAULT_STACK));
            start + STACK_SIZE as u64
        };
        tss.privilege_stack_table[0] = privilege_stack_top();
        &*core::ptr::addr_of!(TSS)
    };

    let (gdt, selectors) = GDT.call_once(|| {
        let mut gdt = GlobalDescriptorTable::new();
        // Order is fixed by the STAR MSR layout: kernel code, kernel data,
        // then user data immediately before user code (SYSRET requirement).
        let kernel_code = gdt.append(Descriptor::kernel_code_segment());
        let kernel_data = gdt.append(Descriptor::kernel_data_segment());
        let user_data = gdt.append(Descriptor::user_data_segment());
        let user_code = gdt.append(Descriptor::user_code_segment());
        let tss_sel = gdt.append(Descriptor::tss_segment(tss));
        (
            gdt,
            Selectors {
                kernel_code,
                kernel_data,
                user_code,
                user_data,
                tss: tss_sel,
            },
        )
    });

    gdt.load();
    unsafe {
        CS::set_reg(selectors.kernel_code);
        // Stale bootloader selectors in SS/DS/ES would #GP on the first
        // interrupt; reload them from the new GDT.
        SS::set_reg(selectors.kernel_data);
        DS::set_reg(selectors.kernel_data);
        ES::set_reg(selectors.kernel_data);
        load_tss(selectors.tss);
    }
    SELECTORS.call_once(|| *selectors);
}
