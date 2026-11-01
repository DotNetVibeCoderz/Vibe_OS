//! virtio-PCI transport, modern interface (virtio 1.x §4.1): the device's
//! registers sit in memory BARs, located through vendor-specific PCI
//! capabilities — common configuration, notification area, ISR and the
//! device-specific configuration.

use alloc::{boxed::Box, format, string::String, vec::Vec};

use super::Transport;
use crate::pci::PciDevice;

const VENDOR_VIRTIO: u16 = 0x1AF4;
const CAP_VENDOR: u8 = 0x09;
const CFG_COMMON: u8 = 1;
const CFG_NOTIFY: u8 = 2;
const CFG_ISR: u8 = 3;
const CFG_DEVICE: u8 = 4;

// Common configuration layout (§4.1.4.3).
const DEVICE_FEATURE_SELECT: usize = 0;
const DEVICE_FEATURE: usize = 4;
const DRIVER_FEATURE_SELECT: usize = 8;
const DRIVER_FEATURE: usize = 12;
const DEVICE_STATUS: usize = 20;
const QUEUE_SELECT: usize = 22;
const QUEUE_SIZE: usize = 24;
const QUEUE_MSIX_VECTOR: usize = 26;
const QUEUE_ENABLE: usize = 28;
const QUEUE_NOTIFY_OFF: usize = 30;
const QUEUE_DESC: usize = 32;
const QUEUE_DRIVER: usize = 40;
const QUEUE_DEVICE: usize = 48;

pub struct PciTransport {
    dev: PciDevice,
    common: u64,
    notify: u64,
    notify_mul: u32,
    #[allow(dead_code)]
    isr: u64,
    device_cfg: u64,
}

/// virtio device type of a PCI id: modern ids are 0x1040 + type; the
/// transitional ids 0x1000.. map to their legacy types.
fn type_of(device_id: u16) -> Option<u16> {
    match device_id {
        0x1040..=0x107F => Some(device_id - 0x1040),
        0x1000 => Some(super::device::NET),
        0x1001 => Some(super::device::BLOCK),
        _ => None,
    }
}

/// All virtio-PCI devices of `kind`, with their register windows mapped.
pub fn find(kind: u16) -> Vec<Box<dyn Transport>> {
    let mut out: Vec<Box<dyn Transport>> = Vec::new();
    for dev in crate::pci::scan() {
        if dev.vendor_id != VENDOR_VIRTIO || type_of(dev.device_id) != Some(kind) {
            continue;
        }
        match PciTransport::new(dev) {
            Ok(t) => out.push(Box::new(t)),
            Err(e) => crate::println!(
                "[virtio] pci {:02x}:{:02x}.{}: {}",
                dev.bus, dev.slot, dev.function, e
            ),
        }
    }
    out
}

impl PciTransport {
    fn new(dev: PciDevice) -> Result<PciTransport, &'static str> {
        dev.enable_mem_and_bus_master();
        let (mut common, mut notify, mut isr, mut device_cfg, mut notify_mul) = (0, 0, 0, 0, 0);
        for (id, off) in dev.capabilities() {
            if id != CAP_VENDOR {
                continue;
            }
            let cfg_type = dev.read8(off + 3);
            let bar = dev.read8(off + 4);
            let offset = dev.read32(off + 8) as u64;
            let length = dev.read32(off + 12) as u64;
            if bar > 5 || !matches!(cfg_type, CFG_COMMON | CFG_NOTIFY | CFG_ISR | CFG_DEVICE) {
                continue;
            }
            let base = dev.mem_bar(bar).ok_or("virtio capability points at a non-memory BAR")?;
            let va = crate::memory::map_mmio(base + offset, length).ok_or("cannot map virtio registers")?;
            match cfg_type {
                CFG_COMMON if common == 0 => common = va,
                CFG_NOTIFY if notify == 0 => {
                    notify = va;
                    notify_mul = dev.read32(off + 16);
                }
                CFG_ISR if isr == 0 => isr = va,
                CFG_DEVICE if device_cfg == 0 => device_cfg = va,
                _ => {}
            }
        }
        if common == 0 || notify == 0 {
            return Err("no modern virtio capabilities (legacy-only device)");
        }
        Ok(PciTransport { dev, common, notify, notify_mul, isr, device_cfg })
    }

    fn r8(&self, off: usize) -> u8 {
        unsafe { ((self.common + off as u64) as *const u8).read_volatile() }
    }
    fn w8(&self, off: usize, v: u8) {
        unsafe { ((self.common + off as u64) as *mut u8).write_volatile(v) }
    }
    fn r16(&self, off: usize) -> u16 {
        unsafe { ((self.common + off as u64) as *const u16).read_volatile() }
    }
    fn w16(&self, off: usize, v: u16) {
        unsafe { ((self.common + off as u64) as *mut u16).write_volatile(v) }
    }
    fn r32(&self, off: usize) -> u32 {
        unsafe { ((self.common + off as u64) as *const u32).read_volatile() }
    }
    fn w32(&self, off: usize, v: u32) {
        unsafe { ((self.common + off as u64) as *mut u32).write_volatile(v) }
    }
    fn w64(&self, off: usize, v: u64) {
        self.w32(off, v as u32);
        self.w32(off + 4, (v >> 32) as u32);
    }
}

impl Transport for PciTransport {
    fn device_features(&mut self) -> u64 {
        self.w32(DEVICE_FEATURE_SELECT, 0);
        let lo = self.r32(DEVICE_FEATURE) as u64;
        self.w32(DEVICE_FEATURE_SELECT, 1);
        let hi = self.r32(DEVICE_FEATURE) as u64;
        lo | hi << 32
    }
    fn set_driver_features(&mut self, features: u64) {
        self.w32(DRIVER_FEATURE_SELECT, 0);
        self.w32(DRIVER_FEATURE, features as u32);
        self.w32(DRIVER_FEATURE_SELECT, 1);
        self.w32(DRIVER_FEATURE, (features >> 32) as u32);
    }
    fn status(&self) -> u8 {
        self.r8(DEVICE_STATUS)
    }
    fn set_status(&mut self, status: u8) {
        self.w8(DEVICE_STATUS, status)
    }
    fn queue_max(&mut self, q: u16) -> u16 {
        self.w16(QUEUE_SELECT, q);
        self.r16(QUEUE_SIZE)
    }
    fn setup_queue(&mut self, q: u16, size: u16, desc: u64, avail: u64, used: u64) {
        self.w16(QUEUE_SELECT, q);
        self.w16(QUEUE_SIZE, size);
        self.w16(QUEUE_MSIX_VECTOR, 0xFFFF); // no MSI-X: we poll
        self.w64(QUEUE_DESC, desc);
        self.w64(QUEUE_DRIVER, avail);
        self.w64(QUEUE_DEVICE, used);
        self.w16(QUEUE_ENABLE, 1);
    }
    fn notify(&self, q: u16) {
        self.w16(QUEUE_SELECT, q);
        let off = self.r16(QUEUE_NOTIFY_OFF) as u64 * self.notify_mul as u64;
        unsafe { ((self.notify + off) as *mut u16).write_volatile(q) }
    }
    fn config_read8(&self, off: usize) -> u8 {
        unsafe { ((self.device_cfg + off as u64) as *const u8).read_volatile() }
    }
    fn config_read32(&self, off: usize) -> u32 {
        unsafe { ((self.device_cfg + off as u64) as *const u32).read_volatile() }
    }
    fn config_write8(&self, off: usize, v: u8) {
        unsafe { ((self.device_cfg + off as u64) as *mut u8).write_volatile(v) }
    }
    fn location(&self) -> String {
        format!("pci {:02x}:{:02x}.{}", self.dev.bus, self.dev.slot, self.dev.function)
    }
}
