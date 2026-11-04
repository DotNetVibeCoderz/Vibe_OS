//! NVMe namespaces as [`BlockDevice`]s: polled, one admin + one I/O queue pair,
//! DMA into a driver-owned bounce buffer of 16 pages (64 KiB per command).
//! Only 512-byte logical blocks are supported: FAT and the rest of the storage
//! stack work in those.

use alloc::{boxed::Box, format, string::String, vec::Vec};
use core::ptr::{read_volatile, write_volatile};

use crate::driver::{self, BlockDevice, SECTOR_SIZE};
use crate::pci::PciDevice;

const REG_CAP: usize = 0x00;
const REG_CC: usize = 0x14;
const REG_CSTS: usize = 0x1C;
const REG_AQA: usize = 0x24;
const REG_ASQ: usize = 0x28;
const REG_ACQ: usize = 0x30;
const DOORBELLS: usize = 0x1000;
const CSTS_RDY: u32 = 1;
const CSTS_CFS: u32 = 2;

const ADMIN_CREATE_SQ: u8 = 0x01;
const ADMIN_CREATE_CQ: u8 = 0x05;
const ADMIN_IDENTIFY: u8 = 0x06;
const ADMIN_SET_FEATURES: u8 = 0x09;
const IO_FLUSH: u8 = 0x00;
const IO_WRITE: u8 = 0x01;
const IO_READ: u8 = 0x02;

/// Entries per queue: one page of 64-byte commands, and that many 16-byte
/// completions fit in one page as well.
const QUEUE: u16 = 64;
const BOUNCE_PAGES: usize = 16;
const MAX_SECTORS: usize = BOUNCE_PAGES * 4096 / SECTOR_SIZE;
const WAIT: u32 = 20_000_000;

struct Queue {
    id: u16,
    sq: *mut u8,
    cq: *mut u8,
    sq_phys: u64,
    cq_phys: u64,
    tail: u16,
    head: u16,
    phase: bool,
}

impl Queue {
    fn new(id: u16) -> Result<Queue, &'static str> {
        let (sq_phys, sq) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
        let (cq_phys, cq) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
        Ok(Queue { id, sq, cq, sq_phys, cq_phys, tail: 0, head: 0, phase: true })
    }
}

struct Controller {
    regs: *mut u8,
    stride: usize,
}

impl Controller {
    fn rd32(&self, off: usize) -> u32 {
        unsafe { read_volatile(self.regs.add(off) as *const u32) }
    }
    fn wr32(&self, off: usize, v: u32) {
        unsafe { write_volatile(self.regs.add(off) as *mut u32, v) }
    }
    fn wr64(&self, off: usize, v: u64) {
        self.wr32(off, v as u32);
        self.wr32(off + 4, (v >> 32) as u32);
    }
    fn wait_ready(&self, ready: bool) -> Result<(), &'static str> {
        for _ in 0..WAIT {
            let s = self.rd32(REG_CSTS);
            if ready && s & CSTS_CFS != 0 {
                return Err("controller fatal status");
            }
            if (s & CSTS_RDY != 0) == ready {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err("controller did not change state")
    }

    /// Submit one command on `q` and poll its completion. `dw` = dwords 10..15.
    fn submit(&self, q: &mut Queue, opcode: u8, nsid: u32, prp1: u64, prp2: u64, dw: [u32; 6]) -> Result<(), &'static str> {
        let cid = q.tail;
        unsafe {
            let e = q.sq.add(q.tail as usize * 64) as *mut u32;
            write_volatile(e, opcode as u32 | (cid as u32) << 16);
            write_volatile(e.add(1), nsid);
            for i in 2..6 {
                write_volatile(e.add(i), 0);
            }
            write_volatile(e.add(6), prp1 as u32);
            write_volatile(e.add(7), (prp1 >> 32) as u32);
            write_volatile(e.add(8), prp2 as u32);
            write_volatile(e.add(9), (prp2 >> 32) as u32);
            for (i, v) in dw.iter().enumerate() {
                write_volatile(e.add(10 + i), *v);
            }
        }
        q.tail = (q.tail + 1) % QUEUE;
        self.wr32(DOORBELLS + 2 * q.id as usize * self.stride, q.tail as u32);
        for _ in 0..WAIT {
            let status = unsafe { read_volatile((q.cq.add(q.head as usize * 16 + 12)) as *const u32) };
            if (status >> 16) & 1 == u32::from(q.phase) {
                q.head = (q.head + 1) % QUEUE;
                if q.head == 0 {
                    q.phase = !q.phase;
                }
                self.wr32(DOORBELLS + (2 * q.id as usize + 1) * self.stride, q.head as u32);
                return if (status >> 17) & 0x7FFF == 0 { Ok(()) } else { Err("NVMe: command failed") };
            }
            core::hint::spin_loop();
        }
        Err("NVMe: command timed out")
    }
}

struct NvmeDisk {
    ctl: Controller,
    io: Queue,
    index: usize,
    nsid: u32,
    model: String,
    sectors: u64,
    pages: Vec<(u64, *mut u8)>,
    prp_phys: u64,
    prp: *mut u64,
}

// The registers, queues and DMA pages belong to this driver instance.
unsafe impl Send for NvmeDisk {}

impl NvmeDisk {
    fn io(&mut self, opcode: u8, lba: u64, count: usize) -> Result<(), &'static str> {
        let bytes = count * SECTOR_SIZE;
        let pages = bytes.div_ceil(4096);
        let prp1 = self.pages[0].0;
        let prp2 = match pages {
            0 | 1 => 0,
            2 => self.pages[1].0,
            _ => {
                for i in 1..pages {
                    unsafe { write_volatile(self.prp.add(i - 1), self.pages[i].0) };
                }
                self.prp_phys
            }
        };
        let dw = [lba as u32, (lba >> 32) as u32, count as u32 - 1, 0, 0, 0];
        self.ctl.submit(&mut self.io, opcode, self.nsid, prp1, prp2, dw)
    }
}

impl BlockDevice for NvmeDisk {
    fn name(&self) -> String {
        format!("nvme{} ({})", self.index, self.model)
    }

    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read_sector(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        self.read_sectors(lba, buf)
    }

    fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        let total = buf.len() / SECTOR_SIZE;
        if lba + total as u64 > self.sectors {
            return Err("NVMe: read past the end of the namespace");
        }
        let mut done = 0;
        while done < total {
            let n = (total - done).min(MAX_SECTORS);
            self.io(IO_READ, lba + done as u64, n)?;
            let dst = &mut buf[done * SECTOR_SIZE..(done + n) * SECTOR_SIZE];
            for (i, chunk) in dst.chunks_mut(4096).enumerate() {
                unsafe { core::ptr::copy_nonoverlapping(self.pages[i].1, chunk.as_mut_ptr(), chunk.len()) };
            }
            done += n;
        }
        Ok(())
    }

    fn write_sector(&mut self, lba: u64, buf: &[u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        if lba >= self.sectors {
            return Err("NVMe: write past the end of the namespace");
        }
        unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), self.pages[0].1, SECTOR_SIZE) };
        self.io(IO_WRITE, lba, 1)?;
        let nsid = self.nsid;
        self.ctl.submit(&mut self.io, IO_FLUSH, nsid, 0, 0, [0; 6])
    }
}

fn bring_up(dev: &PciDevice, index: usize) -> Result<NvmeDisk, &'static str> {
    let bar = dev.mem_bar(0).ok_or("no memory BAR")?;
    dev.enable_mem_and_bus_master();
    // registers + the first doorbells of any stride we accept
    let va = crate::memory::map_mmio(bar, 0x4000).ok_or("cannot map the registers")?;
    let regs = va as *mut u8;
    let cap = unsafe { read_volatile(regs.add(REG_CAP) as *const u32) as u64 | (read_volatile(regs.add(REG_CAP + 4) as *const u32) as u64) << 32 };
    let stride_shift = (cap >> 32) & 0xF;
    if stride_shift > 8 {
        return Err("doorbell stride too large");
    }
    if (cap >> 48) & 0xF != 0 {
        return Err("controller does not take 4 KiB pages");
    }
    let ctl = Controller { regs, stride: 4 << stride_shift };

    ctl.wr32(REG_CC, ctl.rd32(REG_CC) & !1);
    ctl.wait_ready(false)?;
    let mut admin = Queue::new(0)?;
    let size = (QUEUE as u32 - 1) | (QUEUE as u32 - 1) << 16;
    ctl.wr32(REG_AQA, size);
    ctl.wr64(REG_ASQ, admin.sq_phys);
    ctl.wr64(REG_ACQ, admin.cq_phys);
    // enabled, 4 KiB pages, 64-byte commands, 16-byte completions
    ctl.wr32(REG_CC, 1 | 6 << 16 | 4 << 20);
    ctl.wait_ready(true)?;

    let (id_phys, id) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
    // controller identity (model number at byte 24, 40 bytes)
    ctl.submit(&mut admin, ADMIN_IDENTIFY, 0, id_phys, 0, [1, 0, 0, 0, 0, 0])?;
    let mut model = String::new();
    for i in 24..64 {
        model.push(unsafe { read_volatile(id.add(i)) } as char);
    }
    let model = String::from(model.trim());
    // the first active namespace
    ctl.submit(&mut admin, ADMIN_IDENTIFY, 0, id_phys, 0, [2, 0, 0, 0, 0, 0])?;
    let nsid = unsafe { read_volatile(id as *const u32) };
    if nsid == 0 {
        return Err("no active namespace");
    }
    ctl.submit(&mut admin, ADMIN_IDENTIFY, nsid, id_phys, 0, [0, 0, 0, 0, 0, 0])?;
    let sectors = unsafe { read_volatile(id as *const u32) as u64 | (read_volatile(id.add(4) as *const u32) as u64) << 32 };
    let format = unsafe { read_volatile(id.add(26)) } as usize & 0xF;
    let block_shift = unsafe { read_volatile(id.add(128 + 4 * format + 2)) };
    if block_shift != 9 {
        return Err("logical block size is not 512 bytes");
    }

    // one I/O queue pair (the controller must be told how many first)
    ctl.submit(&mut admin, ADMIN_SET_FEATURES, 0, 0, 0, [7, 0, 0, 0, 0, 0])?;
    let io = Queue::new(1)?;
    let qsize = (QUEUE as u32 - 1) << 16 | 1;
    ctl.submit(&mut admin, ADMIN_CREATE_CQ, 0, io.cq_phys, 0, [qsize, 1, 0, 0, 0, 0])?;
    ctl.submit(&mut admin, ADMIN_CREATE_SQ, 0, io.sq_phys, 0, [qsize, 1 << 16 | 1, 0, 0, 0, 0])?;

    let mut pages = Vec::with_capacity(BOUNCE_PAGES);
    for _ in 0..BOUNCE_PAGES {
        pages.push(crate::memory::alloc_dma_frame().ok_or("out of memory")?);
    }
    let (prp_phys, prp) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
    Ok(NvmeDisk { ctl, io, index, nsid, model, sectors, pages, prp_phys, prp: prp as *mut u64 })
}

/// Register the first namespace of every NVMe controller. Returns how many.
pub fn init(devices: &[PciDevice]) -> usize {
    let mut found = 0;
    for dev in devices.iter().filter(|d| (d.class, d.subclass, d.prog_if) == (0x01, 0x08, 0x02)) {
        match bring_up(dev, found) {
            Ok(disk) => {
                found += 1;
                driver::register_block_device(Box::new(disk));
            }
            Err(e) => crate::println!("[nvme] {:02x}:{:02x}.{}: {}", dev.bus, dev.slot, dev.function, e),
        }
    }
    found
}
