//! IDT, exception handlers, and legacy PIC-based timer + PS/2 keyboard IRQs
//! (v0.2 "Akar": interrupt + timer; APIC migration is tracked in §17).

use core::sync::atomic::{AtomicU64, Ordering};
use pic8259::ChainedPics;
use spin::{Mutex, Once};
use x86_64::instructions::port::Port;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::gdt;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
    Mouse = PIC_1_OFFSET + 12,
}

impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }
}

static TICKS: AtomicU64 = AtomicU64::new(0);
static SPURIOUS: AtomicU64 = AtomicU64::new(0);

/// Monotonic timer ticks since boot (PIT default rate, ~18.2 Hz).
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

static IDT: Once<InterruptDescriptorTable> = Once::new();

/// One system tick (18.2 Hz units) driven by CPU 0's LAPIC timer, on PCs whose
/// firmware gates the 8254's clock: everything `timer_handler` does after EOI.
pub fn lapic_system_tick(frame: &InterruptStackFrame) {
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::task::account_tick();
    crate::process::account_tick();
    crate::task::watchdog(frame.instruction_pointer.as_u64(), frame.stack_pointer.as_u64(), from_user(frame));
    crate::task::preempt(from_user(frame));
}

/// Program PIT channel 0 for the 18.2 Hz system tick. A BIOS leaves it like
/// that; UEFI firmware need not (it times with the HPET or the LAPIC).
fn program_pit() {
    unsafe {
        Port::<u8>::new(0x43).write(0b0011_0100); // channel 0, lo/hi, mode 2 (rate generator)
        Port::<u8>::new(0x40).write(0); // divisor 65536
        Port::<u8>::new(0x40).write(0);
    }
}

pub fn init() {
    program_pit();
    let idt = IDT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        // #BP (int3, callable from ring 3) and #DB (single step) go through
        // stubs that save every GPR: the debugger reads and changes them
        unsafe {
            idt.breakpoint
                .set_handler_addr(x86_64::VirtAddr::new(bz_bp_entry as *const () as u64))
                .set_privilege_level(x86_64::PrivilegeLevel::Ring3);
            idt.debug.set_handler_addr(x86_64::VirtAddr::new(bz_db_entry as *const () as u64));
        }
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault
            .set_handler_fn(general_protection_handler);
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt[InterruptIndex::Timer.as_u8()].set_handler_fn(timer_handler);
        idt[InterruptIndex::Keyboard.as_u8()].set_handler_fn(keyboard_handler);
        idt[InterruptIndex::Mouse.as_u8()].set_handler_fn(mouse_handler);
        // Every remaining PIC vector gets an EOI-only stub so a stray IRQ
        // (e.g. from a probed device) can never triple-fault the kernel.
        macro_rules! stub {
            ($vector:expr) => {{
                extern "x86-interrupt" fn handler(_frame: InterruptStackFrame) {
                    SPURIOUS.fetch_add(1, Ordering::Relaxed);
                    // With the I/O APIC active the 8259 is masked: only its
                    // spurious IRQ 7/15 can still show up, which needs no EOI.
                    if !crate::ioapic::active() {
                        unsafe { PICS.lock().notify_end_of_interrupt($vector) };
                    }
                }
                idt[$vector].set_handler_fn(handler);
            }};
        }
        stub!(PIC_1_OFFSET + 2);
        stub!(PIC_1_OFFSET + 3);
        stub!(PIC_1_OFFSET + 4);
        stub!(PIC_1_OFFSET + 5);
        stub!(PIC_1_OFFSET + 6);
        stub!(PIC_1_OFFSET + 7);
        stub!(PIC_2_OFFSET);
        stub!(PIC_2_OFFSET + 1);
        stub!(PIC_2_OFFSET + 2);
        stub!(PIC_2_OFFSET + 3);
        stub!(PIC_2_OFFSET + 5);
        stub!(PIC_2_OFFSET + 6);
        stub!(PIC_2_OFFSET + 7);
        // Local APIC vectors: AP timer, IPIs, spurious (smp.rs).
        crate::smp::install_vectors(&mut idt);
        idt
    });
    idt.load();

    // `initialize` restores the masks the firmware left behind. Legacy BIOS
    // leaves the PIC lines open; UEFI firmware (OVMF) drives its own LAPIC
    // timer and masks the whole 8259, so on a UEFI boot no timer IRQ ever
    // arrived and the first timer wait hung forever. Unmask the lines the
    // kernel uses: timer (0), keyboard (1), cascade (2) and PS/2 mouse (12).
    let (fw_master, fw_slave) = unsafe {
        let mut pics = PICS.lock();
        let [m, s] = pics.read_masks();
        pics.initialize();
        let [m2, s2] = pics.read_masks();
        pics.write_masks(m2 & !0b0000_0111, s2 & !0b0001_0000);
        (m, s)
    };
    x86_64::instructions::interrupts::enable();
    crate::serial::_print(format_args!(
        "[kernel] PIC masks left by firmware: master {:#04x} slave {:#04x} (timer/kbd/cascade/mouse unmasked)\n",
        fw_master, fw_slave
    ));
}

/// Acknowledge device IRQ `vector`: at the Local APIC when the I/O APIC
/// delivers device interrupts, else at the 8259.
fn end_of_interrupt(vector: u8) {
    if crate::ioapic::active() {
        crate::smp::lapic_eoi();
    } else {
        unsafe { PICS.lock().notify_end_of_interrupt(vector) };
    }
}

/// Load the (shared) IDT on an application processor.
pub fn load_on_ap() {
    IDT.get().expect("idt not initialized").load();
}

extern "C" {
    fn bz_bp_entry();
    fn bz_db_entry();
}

// Frame built by the stubs (u64 slots, lowest address first): r15 r14 r13
// r12 r11 r10 r9 r8 rbp rdi rsi rdx rcx rbx rax vector | rip cs rflags rsp ss
core::arch::global_asm!(
    ".global bz_bp_entry",
    "bz_bp_entry:",
    "    push 3",
    "    jmp 2f",
    ".global bz_db_entry",
    "bz_db_entry:",
    "    push 1",
    "2:",
    "    push rax", "push rbx", "push rcx", "push rdx", "push rsi", "push rdi", "push rbp",
    "    push r8", "push r9", "push r10", "push r11", "push r12", "push r13", "push r14", "push r15",
    "    mov rdi, rsp",
    "    mov rbp, rsp",
    "    and rsp, -16",
    "    call bz_debug_trap",
    "    mov rsp, rbp",
    "    pop r15", "pop r14", "pop r13", "pop r12", "pop r11", "pop r10", "pop r9", "pop r8",
    "    pop rbp", "pop rdi", "pop rsi", "pop rdx", "pop rcx", "pop rbx", "pop rax",
    "    add rsp, 8",
    "    iretq",
);

const DBG_NAMES: [&str; 21] = [
    "r15", "r14", "r13", "r12", "r11", "r10", "r9", "r8", "rbp", "rdi", "rsi", "rdx", "rcx", "rbx", "rax", "", "rip", "",
    "rflags", "rsp", "",
];

/// #BP / #DB. From ring 3: the debugger (or, for a program nobody debugs,
/// a fault that ends it); from the kernel: a log line.
#[no_mangle]
extern "C" fn bz_debug_trap(f: *mut u64) {
    let frame = unsafe { core::slice::from_raw_parts_mut(f, 21) };
    let vector = frame[15];
    if frame[17] & 3 != 3 {
        if vector == 3 {
            println!("[kernel] EXCEPTION: breakpoint at {:#x}", frame[16]);
        }
        return;
    }
    crate::task::kernel_enter();
    x86_64::instructions::interrupts::enable();
    let kind = if vector == 3 {
        frame[16] -= 1; // int3 is a trap: rip is past it
        crate::debug::Trap::Break
    } else {
        crate::debug::Trap::Step
    };
    let mut regs = crate::debug::Regs { r: frame, names: &DBG_NAMES, pc: 16, sp: 19, link: None, flags: Some(18) };
    if !crate::debug::on_trap(kind, &mut regs) {
        let (rip, rsp) = (regs.r[16], regs.r[19]);
        let what = if vector == 3 { "breakpoint (int3) without a debugger" } else { "debug trap" };
        println!("[kernel] ring-3 {} at rip={:#x} -- program terminated", what, rip);
        if let Some(pid) = Some(crate::task::current_space().id).filter(|&id| id != 0) {
            crate::crash::record(pid, what, rip, rsp, None, None);
        }
        crate::usermode::exit_user(FAULT_EXIT_CODE)
    }
    x86_64::instructions::interrupts::disable();
    crate::task::kernel_exit();
}

extern "x86-interrupt" fn double_fault_handler(frame: InterruptStackFrame, _code: u64) -> ! {
    panic!("EXCEPTION: double fault\n{:#?}", frame);
}

/// True if the interrupted code was running in ring 3.
fn from_user(frame: &InterruptStackFrame) -> bool {
    frame.code_segment.rpl() == x86_64::PrivilegeLevel::Ring3
}

/// Exit code reported for a program the kernel terminated because it faulted.
pub const FAULT_EXIT_CODE: u64 = 0xDEAD;

/// A fault in ring-3 code is the program's bug, not the kernel's: report it
/// and end just that program (unwinding back into its launcher, exactly like
/// an EXIT syscall) instead of panicking the whole OS.
fn kill_faulting_program(what: &str, frame: &InterruptStackFrame, addr: Option<u64>) -> ! {
    let (rip, rsp) = (frame.instruction_pointer.as_u64(), frame.stack_pointer.as_u64());
    println!("[kernel] ring-3 {} at rip={:#x} rsp={:#x} -- program terminated", what, rip, rsp);
    // the report reads the user stack: the faulting space is still installed
    if let Some(pid) = Some(crate::task::current_space().id).filter(|&id| id != 0) {
        crate::crash::record(pid, what, rip, rsp, addr, None);
    }
    x86_64::instructions::interrupts::enable(); // the debugger may park this task
    crate::debug::on_fault(what, rip, rsp, None);
    crate::usermode::exit_user(FAULT_EXIT_CODE)
}

extern "x86-interrupt" fn general_protection_handler(frame: InterruptStackFrame, code: u64) {
    if from_user(&frame) {
        crate::task::kernel_enter(); // ending a program is kernel work
        kill_faulting_program("general protection fault", &frame, None);
    }
    panic!(
        "EXCEPTION: general protection fault (code {:#x})\n{:#?}",
        code, frame
    );
}

extern "x86-interrupt" fn page_fault_handler(frame: InterruptStackFrame, code: PageFaultErrorCode) {
    use x86_64::registers::control::Cr2;
    if from_user(&frame) {
        let addr = Cr2::read().ok(); // read on the faulting CPU, before moving
        crate::task::kernel_enter(); // ending a program is kernel work
        println!("[kernel] page fault address {:?} ({:?})", addr, code);
        let what = if code.contains(PageFaultErrorCode::INSTRUCTION_FETCH) {
            "page fault (instruction fetch)"
        } else if code.contains(PageFaultErrorCode::CAUSED_BY_WRITE) {
            "page fault (write)"
        } else {
            "page fault (read)"
        };
        kill_faulting_program(what, &frame, addr.map(|a| a.as_u64()));
    }
    panic!(
        "EXCEPTION: page fault\naccessed address: {:?}\nerror code: {:?}\n{:#?}",
        Cr2::read().ok(),
        code,
        frame
    );
}

extern "x86-interrupt" fn timer_handler(frame: InterruptStackFrame) {
    if crate::smp::ticks_from_lapic() {
        end_of_interrupt(InterruptIndex::Timer.as_u8());
        return; // the LAPIC drives the system tick (see smp::pit_alive)
    }
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::task::account_tick();
    crate::process::account_tick();
    end_of_interrupt(InterruptIndex::Timer.as_u8());
    crate::task::watchdog(frame.instruction_pointer.as_u64(), frame.stack_pointer.as_u64(), from_user(&frame));
    // EOI is done, interrupts are still disabled: safe point to preempt. Ring-3
    // code is always preemptible (it holds no kernel lock); kernel code is not.
    crate::task::preempt(from_user(&frame));
}

extern "x86-interrupt" fn keyboard_handler(_frame: InterruptStackFrame) {
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    crate::keyboard::feed_scancode(scancode);
    crate::ioapic::note_irq(1);
    end_of_interrupt(InterruptIndex::Keyboard.as_u8());
}

extern "x86-interrupt" fn mouse_handler(_frame: InterruptStackFrame) {
    let mut port = Port::new(0x60);
    let byte: u8 = unsafe { port.read() };
    crate::mouse::handle_byte(byte);
    crate::ioapic::note_irq(12);
    end_of_interrupt(InterruptIndex::Mouse.as_u8());
}
