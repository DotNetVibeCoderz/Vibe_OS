//! virtio (multi-arch M2): paravirtual devices for every architecture.
//!
//! One driver per device class (block, input, network, GPU, sound) runs over a
//! [`Transport`] — virtio-PCI on PCs (`pci.rs`), virtio-mmio on the ARM64 /
//! RISC-V `virt` machines (found through the Device Tree, M3/M4). Devices are
//! driven through split virtqueues ([`Virtqueue`]): a descriptor table, an
//! "available" ring the driver fills and a "used" ring the device fills, all
//! in DMA memory (virtio 1.x spec, §2.7).
//!
//! The drivers poll for completions (no interrupts yet): a request is posted,
//! the device is notified, and the driver waits for the used ring to advance
//! — synchronous like the IDE PIO path it replaces, and enough for boot media.

pub mod blk;
pub mod gpu;
pub mod input;
pub mod mmio;
pub mod net;
#[cfg(target_arch = "x86_64")]
pub mod pci;
pub mod snd;

/// Every virtio device of type `kind` the platform exposes: virtio-PCI on
/// PCs, virtio-mmio slots from the Device Tree on ARM64.
pub fn find(kind: u16) -> alloc::vec::Vec<alloc::boxed::Box<dyn Transport>> {
    #[cfg(target_arch = "x86_64")]
    return pci::find(kind);
    #[cfg(not(target_arch = "x86_64"))]
    return mmio::find(kind);
}

use alloc::vec::Vec;
use core::sync::atomic::{fence, Ordering};

/// Device status bits (§2.1).
pub const STATUS_ACKNOWLEDGE: u8 = 1;
pub const STATUS_DRIVER: u8 = 2;
pub const STATUS_DRIVER_OK: u8 = 4;
pub const STATUS_FEATURES_OK: u8 = 8;
pub const STATUS_FAILED: u8 = 128;

/// "This is a virtio 1.x (non-legacy) device" — mandatory for modern drivers.
pub const F_VERSION_1: u64 = 1 << 32;

/// Device types (§5).
pub mod device {
    pub const NET: u16 = 1;
    pub const BLOCK: u16 = 2;
    pub const GPU: u16 = 16;
    pub const INPUT: u16 = 18;
    pub const SOUND: u16 = 25;
}

/// How a driver talks to its device's registers, independent of the bus.
pub trait Transport: Send {
    fn device_features(&mut self) -> u64;
    fn set_driver_features(&mut self, features: u64);
    fn status(&self) -> u8;
    fn set_status(&mut self, status: u8);
    /// Largest size of queue `q` (0: the queue does not exist).
    fn queue_max(&mut self, q: u16) -> u16;
    /// Give queue `q` its rings and enable it.
    fn setup_queue(&mut self, q: u16, size: u16, desc: u64, avail: u64, used: u64);
    /// Tell the device queue `q` has new buffers.
    fn notify(&self, q: u16);
    fn config_read8(&self, off: usize) -> u8;
    fn config_read32(&self, off: usize) -> u32;
    fn config_write8(&self, off: usize, v: u8);
    fn config_read64(&self, off: usize) -> u64 {
        self.config_read32(off) as u64 | (self.config_read32(off + 4) as u64) << 32
    }
    /// Human-readable location (e.g. `pci 00:04.0`).
    fn location(&self) -> alloc::string::String;
}

/// Device initialization (§3.1.1): reset, ACKNOWLEDGE, DRIVER, negotiate
/// features (`wanted` ∩ offered, VERSION_1 required), FEATURES_OK. Queues are
/// set up afterwards, then [`finish_init`]. Returns the negotiated features.
pub fn begin_init(t: &mut dyn Transport, wanted: u64) -> Result<u64, &'static str> {
    t.set_status(0);
    for _ in 0..1_000_000 {
        if t.status() == 0 {
            break;
        }
        core::hint::spin_loop();
    }
    t.set_status(STATUS_ACKNOWLEDGE);
    t.set_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER);
    let offered = t.device_features();
    if offered & F_VERSION_1 == 0 {
        t.set_status(STATUS_FAILED);
        return Err("virtio: device is legacy-only (no VERSION_1)");
    }
    let features = offered & (wanted | F_VERSION_1);
    t.set_driver_features(features);
    t.set_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK);
    if t.status() & STATUS_FEATURES_OK == 0 {
        t.set_status(STATUS_FAILED);
        return Err("virtio: device refused the feature set");
    }
    Ok(features)
}

/// Last step of initialization: the driver is ready.
pub fn finish_init(t: &mut dyn Transport) {
    let s = t.status();
    t.set_status(s | STATUS_DRIVER_OK);
}

const DESC_F_NEXT: u16 = 1;
const DESC_F_WRITE: u16 = 2;
/// avail.flags: do not interrupt us (we poll).
const AVAIL_F_NO_INTERRUPT: u16 = 1;

/// One buffer of a request: physical address, length, and whether the
/// *device* writes it (an output of the request).
#[derive(Clone, Copy)]
pub struct Buf {
    pub phys: u64,
    pub len: u32,
    pub device_writes: bool,
}

/// A split virtqueue in DMA memory, one page each for the descriptor table
/// (16 bytes per entry), the available ring and the used ring.
pub struct Virtqueue {
    pub index: u16,
    size: u16,
    desc: *mut u8,
    avail: *mut u8,
    used: *mut u8,
    free: Vec<u16>,
    avail_idx: u16,
    last_used: u16,
}

// The rings are plain DMA memory owned by this queue.
unsafe impl Send for Virtqueue {}

impl Virtqueue {
    /// Create queue `index` (size capped to 256) and register it.
    pub fn new(t: &mut dyn Transport, index: u16) -> Result<Virtqueue, &'static str> {
        let max = t.queue_max(index);
        if max == 0 {
            return Err("virtio: queue not available");
        }
        let size = max.min(256);
        // One page each: descriptors (16 x 256 = 4096 bytes), the available
        // ring (6 + 2 x 256) and the used ring (6 + 8 x 256 = 2054 bytes). The
        // used ring used to share the available ring's page at offset 2048 --
        // 6 bytes too few for 256 entries, so once the queue wrapped the
        // device wrote element 255's `len` (e.g. 513 for a sector read) into
        // the first bytes of the *next* physical frame. Found on RISC-V,
        // where that frame was on the allocator's free list.
        let (desc_phys, desc) = crate::memory::alloc_dma_frame().ok_or("virtio: out of memory")?;
        let (ring_phys, ring) = crate::memory::alloc_dma_frame().ok_or("virtio: out of memory")?;
        let (used_phys, used) = crate::memory::alloc_dma_frame().ok_or("virtio: out of memory")?;
        t.setup_queue(index, size, desc_phys, ring_phys, used_phys);
        let q = Virtqueue {
            index,
            size,
            desc,
            avail: ring,
            used,
            free: (0..size).rev().collect(),
            avail_idx: 0,
            last_used: 0,
        };
        unsafe { (q.avail as *mut u16).write_volatile(AVAIL_F_NO_INTERRUPT) };
        Ok(q)
    }

    pub fn size(&self) -> u16 {
        self.size
    }

    fn write_desc(&mut self, i: u16, b: Buf, next: Option<u16>) {
        let d = unsafe { self.desc.add(i as usize * 16) };
        let mut flags = if b.device_writes { DESC_F_WRITE } else { 0 };
        if next.is_some() {
            flags |= DESC_F_NEXT;
        }
        unsafe {
            (d as *mut u64).write_volatile(b.phys);
            (d.add(8) as *mut u32).write_volatile(b.len);
            (d.add(12) as *mut u16).write_volatile(flags);
            (d.add(14) as *mut u16).write_volatile(next.unwrap_or(0));
        }
    }

    /// Post a chain of buffers (device-readable ones first, by convention).
    /// Returns its head descriptor id; the device is *not* notified yet.
    pub fn push(&mut self, bufs: &[Buf]) -> Result<u16, &'static str> {
        if bufs.is_empty() || bufs.len() > self.free.len() {
            return Err("virtio: queue full");
        }
        let ids: Vec<u16> = (0..bufs.len()).map(|_| self.free.pop().unwrap()).collect();
        for (k, b) in bufs.iter().enumerate() {
            self.write_desc(ids[k], *b, ids.get(k + 1).copied());
        }
        let slot = self.avail_idx % self.size;
        unsafe { (self.avail.add(4 + slot as usize * 2) as *mut u16).write_volatile(ids[0]) };
        fence(Ordering::SeqCst);
        self.avail_idx = self.avail_idx.wrapping_add(1);
        unsafe { (self.avail.add(2) as *mut u16).write_volatile(self.avail_idx) };
        fence(Ordering::SeqCst);
        Ok(ids[0])
    }

    /// Take one completion from the used ring: (head id, bytes written by the
    /// device). Frees the chain's descriptors.
    pub fn pop_used(&mut self) -> Option<(u16, u32)> {
        fence(Ordering::SeqCst);
        let idx = unsafe { (self.used.add(2) as *const u16).read_volatile() };
        if idx == self.last_used {
            return None;
        }
        let slot = self.last_used % self.size;
        let e = unsafe { self.used.add(4 + slot as usize * 8) };
        let id = unsafe { (e as *const u32).read_volatile() } as u16;
        let len = unsafe { (e.add(4) as *const u32).read_volatile() };
        self.last_used = self.last_used.wrapping_add(1);
        // Return the chain's descriptors to the free list.
        let mut i = id;
        loop {
            self.free.push(i);
            let d = unsafe { self.desc.add(i as usize * 16) };
            let flags = unsafe { (d.add(12) as *const u16).read_volatile() };
            if flags & DESC_F_NEXT == 0 {
                break;
            }
            i = unsafe { (d.add(14) as *const u16).read_volatile() };
        }
        Some((id, len))
    }

    /// Post `bufs`, notify, and wait (polling) for that request to complete.
    /// Returns the length the device wrote.
    pub fn submit_wait(&mut self, t: &dyn Transport, bufs: &[Buf]) -> Result<u32, &'static str> {
        let head = self.push(bufs)?;
        t.notify(self.index);
        for _ in 0..200_000_000u64 {
            if let Some((id, len)) = self.pop_used() {
                if id == head {
                    return Ok(len);
                }
                continue; // a stale completion of an older request
            }
            core::hint::spin_loop();
        }
        Err("virtio: request timed out")
    }
}

/// Probe every supported virtio device; called once after PCI is up.
/// Returns how many block devices were registered.
pub fn init_block() -> usize {
    let mut n = 0;
    for t in find(device::BLOCK) {
        let loc = t.location();
        match blk::VirtioBlk::new(t) {
            Ok(mut dev) => {
                use crate::driver::BlockDevice;
                let mut s0 = [0u8; 512];
                let mbr = dev.read_sector(0, &mut s0).is_ok() && s0[510] == 0x55 && s0[511] == 0xAA;
                if mbr {
                    crate::println!(
                        "[kernel] MILESTONE: VIRTIO BLK OK ({}, split virtqueue, sector 0 read: MBR signature present)",
                        if loc.starts_with("pci") { "virtio-PCI modern transport" } else { "virtio-mmio v2 transport" }
                    );
                }
                crate::driver::register_block_device(alloc::boxed::Box::new(dev));
                n += 1;
            }
            Err(e) => crate::println!("[virtio] blk at {}: {}", loc, e),
        }
    }
    n
}
