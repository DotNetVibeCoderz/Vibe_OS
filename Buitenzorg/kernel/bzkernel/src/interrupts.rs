//! IDT, exception handlers, and legacy PIC-based timer + PS/2 keyboard IRQs
//! (v0.2 "Akar": interrupt + timer; APIC migration is tracked in §17).

use core::sync::atomic::{AtomicU64, Ordering};
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use pic8259::ChainedPics;
use spin::{Mutex, Once};
use x86_64::instructions::port::Port;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::{gdt, print, println};

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

pub fn init() {
    let idt = IDT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
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
                    unsafe { PICS.lock().notify_end_of_interrupt($vector) };
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
        idt
    });
    idt.load();

    unsafe { PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();
}

extern "x86-interrupt" fn breakpoint_handler(frame: InterruptStackFrame) {
    println!("[kernel] EXCEPTION: breakpoint\n{:#?}", frame);
}

extern "x86-interrupt" fn double_fault_handler(frame: InterruptStackFrame, _code: u64) -> ! {
    panic!("EXCEPTION: double fault\n{:#?}", frame);
}

extern "x86-interrupt" fn general_protection_handler(frame: InterruptStackFrame, code: u64) {
    panic!(
        "EXCEPTION: general protection fault (code {:#x})\n{:#?}",
        code, frame
    );
}

extern "x86-interrupt" fn page_fault_handler(frame: InterruptStackFrame, code: PageFaultErrorCode) {
    use x86_64::registers::control::Cr2;
    panic!(
        "EXCEPTION: page fault\naccessed address: {:?}\nerror code: {:?}\n{:#?}",
        Cr2::read().ok(),
        code,
        frame
    );
}

extern "x86-interrupt" fn timer_handler(_frame: InterruptStackFrame) {
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::task::account_tick();
    crate::process::account_tick();
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
    // EOI is done, interrupts are still disabled: safe point to preempt.
    crate::task::preempt();
}

static KEYBOARD: Once<Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>>> = Once::new();

extern "x86-interrupt" fn keyboard_handler(_frame: InterruptStackFrame) {
    let keyboard = KEYBOARD.call_once(|| {
        Mutex::new(Keyboard::new(
            ScancodeSet1::new(),
            layouts::Us104Key,
            HandleControl::Ignore,
        ))
    });

    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    let mut kb = keyboard.lock();
    if let Ok(Some(event)) = kb.add_byte(scancode) {
        if let Some(key) = kb.process_keyevent(event) {
            match key {
                // Queue for the terminal once the desktop is up; otherwise
                // echo to the console (early boot).
                DecodedKey::Unicode(c) => {
                    if crate::terminal::is_active() {
                        crate::keyboard::push(c);
                    } else {
                        print!("{}", c);
                    }
                }
                DecodedKey::RawKey(_) => {}
            }
        }
    }

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}

extern "x86-interrupt" fn mouse_handler(_frame: InterruptStackFrame) {
    let mut port = Port::new(0x60);
    let byte: u8 = unsafe { port.read() };
    crate::mouse::handle_byte(byte);
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Mouse.as_u8());
    }
}
