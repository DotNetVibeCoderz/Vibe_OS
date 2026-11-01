//! virtio-mmio transport, version 2 ("modern", virtio 1.x §4.2.2) — how
//! Device-Tree boards (QEMU `virt` on ARM64 and RISC-V) attach virtio devices:
//! a 512-byte register window per slot, listed in the DTB as
//! `compatible = "virtio,mmio"`. The platform code hands the slots over with
//! [`set_slots`]; [`find`] maps them and returns the occupied ones of a kind.
//!
//! QEMU creates version-1 (legacy) windows unless started with
//! `-global virtio-mmio.force-legacy=false`; legacy ones are skipped (the
//! drivers here speak only VERSION_1 virtio).

// Used by the Device-Tree boards (ARM64); x86 reaches virtio over PCI.
#![cfg_attr(target_arch = "x86_64", allow(dead_code))]

use alloc::{boxed::Box, format, string::String, vec::Vec};
use spin::Mutex;

use super::Transport;

const MAGIC: usize = 0x000;
const VERSION: usize = 0x004;
const DEVICE_ID: usize = 0x008;
const DEVICE_FEATURES: usize = 0x010;
const DEVICE_FEATURES_SEL: usize = 0x014;
const DRIVER_FEATURES: usize = 0x020;
const DRIVER_FEATURES_SEL: usize = 0x024;
const QUEUE_SEL: usize = 0x030;
const QUEUE_NUM_MAX: usize = 0x034;
const QUEUE_NUM: usize = 0x038;
const QUEUE_READY: usize = 0x044;
const QUEUE_NOTIFY: usize = 0x050;
const STATUS: usize = 0x070;
const QUEUE_DESC_LOW: usize = 0x080;
const QUEUE_DESC_HIGH: usize = 0x084;
const QUEUE_DRIVER_LOW: usize = 0x090;
const QUEUE_DRIVER_HIGH: usize = 0x094;
const QUEUE_DEVICE_LOW: usize = 0x0a0;
const QUEUE_DEVICE_HIGH: usize = 0x0a4;
const CONFIG: usize = 0x100;

const MAGIC_VALUE: u32 = 0x7472_6976; // "virt"

/// The slots from the DTB: (physical address, mapped virtual address or 0).
static SLOTS: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());

/// Register the virtio-mmio slots the platform found.
pub fn set_slots(slots: &[u64]) {
    *SLOTS.lock() = slots.iter().map(|&p| (p, 0)).collect();
}

pub struct MmioTransport {
    phys: u64,
    base: u64,
}

impl MmioTransport {
    fn r32(&self, off: usize) -> u32 {
        unsafe { ((self.base + off as u64) as *const u32).read_volatile() }
    }
    fn w32(&self, off: usize, v: u32) {
        unsafe { ((self.base + off as u64) as *mut u32).write_volatile(v) }
    }
}

/// Occupied version-2 slots holding a device of type `kind`.
pub fn find(kind: u16) -> Vec<Box<dyn Transport>> {
    let mut out: Vec<Box<dyn Transport>> = Vec::new();
    let mut slots = SLOTS.lock();
    for slot in slots.iter_mut() {
        if slot.1 == 0 {
            match crate::memory::map_mmio(slot.0, 0x200) {
                Some(va) => slot.1 = va,
                None => continue,
            }
        }
        let t = MmioTransport { phys: slot.0, base: slot.1 };
        if t.r32(MAGIC) != MAGIC_VALUE || t.r32(DEVICE_ID) != kind as u32 {
            continue;
        }
        if t.r32(VERSION) != 2 {
            crate::println!(
                "[virtio] mmio {:#x}: legacy (version 1) device skipped -- start QEMU with -global virtio-mmio.force-legacy=false",
                slot.0
            );
            continue;
        }
        out.push(Box::new(t));
    }
    out
}

impl Transport for MmioTransport {
    fn device_features(&mut self) -> u64 {
        self.w32(DEVICE_FEATURES_SEL, 0);
        let lo = self.r32(DEVICE_FEATURES) as u64;
        self.w32(DEVICE_FEATURES_SEL, 1);
        let hi = self.r32(DEVICE_FEATURES) as u64;
        lo | hi << 32
    }
    fn set_driver_features(&mut self, features: u64) {
        self.w32(DRIVER_FEATURES_SEL, 0);
        self.w32(DRIVER_FEATURES, features as u32);
        self.w32(DRIVER_FEATURES_SEL, 1);
        self.w32(DRIVER_FEATURES, (features >> 32) as u32);
    }
    fn status(&self) -> u8 {
        self.r32(STATUS) as u8
    }
    fn set_status(&mut self, status: u8) {
        self.w32(STATUS, status as u32)
    }
    fn queue_max(&mut self, q: u16) -> u16 {
        self.w32(QUEUE_SEL, q as u32);
        self.r32(QUEUE_NUM_MAX) as u16
    }
    fn setup_queue(&mut self, q: u16, size: u16, desc: u64, avail: u64, used: u64) {
        self.w32(QUEUE_SEL, q as u32);
        self.w32(QUEUE_NUM, size as u32);
        self.w32(QUEUE_DESC_LOW, desc as u32);
        self.w32(QUEUE_DESC_HIGH, (desc >> 32) as u32);
        self.w32(QUEUE_DRIVER_LOW, avail as u32);
        self.w32(QUEUE_DRIVER_HIGH, (avail >> 32) as u32);
        self.w32(QUEUE_DEVICE_LOW, used as u32);
        self.w32(QUEUE_DEVICE_HIGH, (used >> 32) as u32);
        self.w32(QUEUE_READY, 1);
    }
    fn notify(&self, q: u16) {
        self.w32(QUEUE_NOTIFY, q as u32)
    }
    fn config_read8(&self, off: usize) -> u8 {
        unsafe { ((self.base + (CONFIG + off) as u64) as *const u8).read_volatile() }
    }
    fn config_read32(&self, off: usize) -> u32 {
        unsafe { ((self.base + (CONFIG + off) as u64) as *const u32).read_volatile() }
    }
    fn config_write8(&self, off: usize, v: u8) {
        unsafe { ((self.base + (CONFIG + off) as u64) as *mut u8).write_volatile(v) }
    }
    fn location(&self) -> String {
        format!("mmio {:#x}", self.phys)
    }
}
