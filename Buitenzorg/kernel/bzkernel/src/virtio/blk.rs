//! virtio-blk (§5.2): a disk as a [`BlockDevice`]. Each request is a
//! three-buffer chain — a 16-byte header (type, sector), the data, and a
//! one-byte status the device writes — in one DMA page owned by the driver.

use alloc::{boxed::Box, format, string::String};

use super::{Buf, Transport, Virtqueue};
use crate::driver::{BlockDevice, SECTOR_SIZE};

const T_IN: u32 = 0;
const T_OUT: u32 = 1;
const T_FLUSH: u32 = 4;
const F_RO: u64 = 1 << 5;
const F_FLUSH: u64 = 1 << 9;

const HDR: usize = 0;
const DATA: usize = 512;
const STATUS: usize = 1024;

pub struct VirtioBlk {
    t: Box<dyn Transport>,
    q: Virtqueue,
    capacity: u64,
    read_only: bool,
    flush: bool,
    dma_phys: u64,
    dma: *mut u8,
}

// The DMA page and the queue are owned by this driver instance.
unsafe impl Send for VirtioBlk {}

impl VirtioBlk {
    pub fn new(mut t: Box<dyn Transport>) -> Result<VirtioBlk, &'static str> {
        let features = super::begin_init(t.as_mut(), F_RO | F_FLUSH)?;
        let q = Virtqueue::new(t.as_mut(), 0)?;
        super::finish_init(t.as_mut());
        let capacity = t.config_read64(0); // in 512-byte sectors
        let (dma_phys, dma) = crate::memory::alloc_dma_frame().ok_or("virtio-blk: out of memory")?;
        crate::println!(
            "[virtio] blk at {}: {} MiB, queue {} entries, features{}{}",
            t.location(),
            capacity / 2048,
            q.size(),
            if features & F_FLUSH != 0 { " flush" } else { "" },
            if features & F_RO != 0 { " read-only" } else { "" },
        );
        Ok(VirtioBlk { t, q, capacity, read_only: features & F_RO != 0, flush: features & F_FLUSH != 0, dma_phys, dma })
    }

    fn request(&mut self, kind: u32, lba: u64, with_data: bool) -> Result<(), &'static str> {
        unsafe {
            (self.dma.add(HDR) as *mut u32).write_volatile(kind);
            (self.dma.add(HDR + 4) as *mut u32).write_volatile(0);
            (self.dma.add(HDR + 8) as *mut u64).write_volatile(lba);
            self.dma.add(STATUS).write_volatile(0xFF);
        }
        let hdr = Buf { phys: self.dma_phys + HDR as u64, len: 16, device_writes: false };
        let data = Buf { phys: self.dma_phys + DATA as u64, len: SECTOR_SIZE as u32, device_writes: kind == T_IN };
        let status = Buf { phys: self.dma_phys + STATUS as u64, len: 1, device_writes: true };
        if with_data {
            self.q.submit_wait(self.t.as_ref(), &[hdr, data, status])?;
        } else {
            self.q.submit_wait(self.t.as_ref(), &[hdr, status])?;
        }
        match unsafe { self.dma.add(STATUS).read_volatile() } {
            0 => Ok(()),
            1 => Err("virtio-blk: I/O error"),
            2 => Err("virtio-blk: unsupported request"),
            _ => Err("virtio-blk: no status"),
        }
    }
}

impl BlockDevice for VirtioBlk {
    fn name(&self) -> String {
        format!("virtio-blk ({})", self.t.location())
    }

    fn sector_count(&self) -> u64 {
        self.capacity
    }

    fn read_sector(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        if lba >= self.capacity {
            return Err("virtio-blk: sector out of range");
        }
        self.request(T_IN, lba, true)?;
        for (i, b) in buf.iter_mut().enumerate() {
            *b = unsafe { self.dma.add(DATA + i).read_volatile() };
        }
        Ok(())
    }

    fn write_sector(&mut self, lba: u64, buf: &[u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        if self.read_only {
            return Err("virtio-blk: read-only device");
        }
        if lba >= self.capacity {
            return Err("virtio-blk: sector out of range");
        }
        for (i, b) in buf.iter().enumerate() {
            unsafe { self.dma.add(DATA + i).write_volatile(*b) };
        }
        self.request(T_OUT, lba, true)?;
        if self.flush {
            self.request(T_FLUSH, 0, false)?;
        }
        Ok(())
    }
}
