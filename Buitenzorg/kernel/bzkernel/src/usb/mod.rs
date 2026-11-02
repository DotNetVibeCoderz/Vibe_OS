//! USB (roadmap R4): xHCI host controllers, USB 2.0 hubs and HID boot-protocol
//! keyboards and mice, in generic code. A PC finds its controllers on PCI
//! (`init_pci`); a Raspberry Pi 4 brings up its PCIe root complex and the
//! VL805 behind it first (`platform::virt::brcm_pcie`), then calls
//! `add_controller`. The desktop loop calls `poll`.

pub mod hid;
pub mod xhci;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

static CONTROLLERS: Mutex<Vec<xhci::Xhci>> = Mutex::new(Vec::new());
static INPUT_REPORTED: AtomicBool = AtomicBool::new(false);
static LAST_REPORT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static REPORTS_LEFT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(12);

/// Start the xHCI controller mapped at `mmio`, enumerate its devices and keep
/// it for `poll`. `dma_limit` = the first bus address the controller cannot
/// reach. Returns (keyboards, mice).
pub fn add_controller(mmio: u64, dma_limit: u64, name: &'static str) -> Result<(usize, usize), &'static str> {
    let mut c = xhci::Xhci::new(mmio, dma_limit, name)?;
    crate::println!("[usb] {}: xHCI running, {} ports", name, c.ports());
    c.scan();
    let (k, m, h, n) = (c.keyboards, c.mice, c.hubs, c.device_count());
    if n > 0 {
        crate::println!(
            "[kernel] MILESTONE: USB OK ({}: xHCI, {} device(s): {} hub(s), {} keyboard(s), {} mouse/mice; HID boot protocol)",
            name,
            n,
            h,
            k,
            m
        );
    } else {
        crate::println!("[usb] {}: no devices connected", name);
    }
    CONTROLLERS.lock().push(c);
    Ok((k, m))
}

/// Every xHCI controller on PCI (class 0C/03, interface 30).
#[cfg(target_arch = "x86_64")]
pub fn init_pci() -> usize {
    let mut found = 0;
    for dev in crate::pci::scan() {
        if (dev.class, dev.subclass, dev.prog_if) != (0x0C, 0x03, 0x30) {
            continue;
        }
        let Some(bar) = dev.mem_bar(0) else { continue };
        dev.enable_mem_and_bus_master();
        let Some(va) = crate::memory::map_mmio(bar, 0x10000) else { continue };
        match add_controller(va, u64::MAX, "xhci-pci") {
            Ok(_) => found += 1,
            Err(e) => crate::println!("[usb] xHCI {:04x}:{:04x} at {:#x}: {}", dev.vendor_id, dev.device_id, bar, e),
        }
    }
    found
}

/// Process reports from keyboards and mice (the desktop loop calls this).
pub fn poll() {
    if let Some(mut list) = CONTROLLERS.try_lock() {
        for c in list.iter_mut() {
            c.poll();
        }
        // endpoint states every 5 s, a dozen times, for hardware bring-up
        let now = crate::arch::cpu::timestamp();
        let hz = crate::arch::cpu::counter_hz().max(1);
        if !list.is_empty() && REPORTS_LEFT.load(Ordering::Relaxed) > 0 && now - LAST_REPORT.load(Ordering::Relaxed) > 5 * hz {
            LAST_REPORT.store(now, Ordering::Relaxed);
            REPORTS_LEFT.fetch_sub(1, Ordering::Relaxed);
            for c in list.iter_mut() {
                c.report_endpoints();
            }
        }
    }
    let (k, p) = (hid::KEYS.load(Ordering::Relaxed), hid::POINTER.load(Ordering::Relaxed));
    if k > 0 && p > 0 && !INPUT_REPORTED.swap(true, Ordering::Relaxed) {
        crate::println!("[kernel] MILESTONE: USB INPUT OK (a key and a pointer movement arrived from USB HID devices)");
    }
}
