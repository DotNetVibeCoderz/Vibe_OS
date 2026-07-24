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

static mut DOUBLE_FAULT_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
static mut PRIVILEGE_STACK: [u8; PRIV_STACK_SIZE] = [0; PRIV_STACK_SIZE];
static TSS: Once<TaskStateSegment> = Once::new();
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

pub fn init() {
    let tss = TSS.call_once(|| {
        let mut tss = TaskStateSegment::new();
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            let start = VirtAddr::from_ptr(core::ptr::addr_of!(DOUBLE_FAULT_STACK));
            start + STACK_SIZE as u64
        };
        tss.privilege_stack_table[0] = privilege_stack_top();
        tss
    });

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
