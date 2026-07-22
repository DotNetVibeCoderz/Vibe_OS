//! IDE/PATA driver, PIO mode with LBA28 (v0.3 "Batang" storage baseline).
//!
//! Polling only: device interrupts are suppressed via nIEN, so no IRQ14/15
//! handling is needed. DMA and LBA48 are follow-up work (§8.3 targets DMA).

use alloc::{boxed::Box, format, string::String};
use x86_64::instructions::port::Port;

use crate::driver::{self, BlockDevice, SECTOR_SIZE};

const STATUS_BSY: u8 = 0x80;
const STATUS_DRQ: u8 = 0x08;
const STATUS_ERR: u8 = 0x01;

const CMD_READ_SECTORS: u8 = 0x20;
const CMD_IDENTIFY: u8 = 0xEC;

struct Channel {
    io_base: u16,
    ctrl_base: u16,
}

impl Channel {
    const fn new(io_base: u16, ctrl_base: u16) -> Self {
        Self { io_base, ctrl_base }
    }

    fn data(&self) -> Port<u16> {
        Port::new(self.io_base)
    }
    fn reg(&self, offset: u16) -> Port<u8> {
        Port::new(self.io_base + offset)
    }
    fn alt_status(&self) -> Port<u8> {
        Port::new(self.ctrl_base)
    }

    /// ~400 ns settle delay: four reads of the alternate status register.
    fn io_delay(&self) {
        for _ in 0..4 {
            unsafe {
                self.alt_status().read();
            }
        }
    }

    fn wait_not_busy(&self) -> Result<u8, &'static str> {
        for _ in 0..1_000_000 {
            let status = unsafe { self.reg(7).read() };
            if status & STATUS_BSY == 0 {
                return Ok(status);
            }
            core::hint::spin_loop();
        }
        Err("ATA timeout waiting for BSY clear")
    }

    fn wait_data_request(&self) -> Result<(), &'static str> {
        for _ in 0..1_000_000 {
            let status = unsafe { self.reg(7).read() };
            if status & STATUS_ERR != 0 {
                return Err("ATA device reported error");
            }
            if status & STATUS_BSY == 0 && status & STATUS_DRQ != 0 {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err("ATA timeout waiting for DRQ")
    }

    fn select(&self, slave: bool, lba_bits: u8) {
        unsafe {
            self.reg(6).write(0xE0 | ((slave as u8) << 4) | (lba_bits & 0x0F));
        }
        self.io_delay();
    }

    /// IDENTIFY DEVICE; returns (model, sector count) for an ATA disk.
    fn identify(&self, slave: bool) -> Option<(String, u64)> {
        unsafe {
            // Suppress device interrupts (nIEN): we poll.
            Port::<u8>::new(self.ctrl_base).write(0x02);
            self.reg(6).write(0xA0 | ((slave as u8) << 4));
            self.io_delay();
            self.reg(2).write(0);
            self.reg(3).write(0);
            self.reg(4).write(0);
            self.reg(5).write(0);
            self.reg(7).write(CMD_IDENTIFY);
        }
        let status = unsafe { self.reg(7).read() };
        if status == 0 || status == 0xFF {
            return None; // no device on this position
        }
        self.wait_not_busy().ok()?;
        // ATAPI/SATA devices set the signature registers; plain IDENTIFY fails.
        let (mid, hi) = unsafe { (self.reg(4).read(), self.reg(5).read()) };
        if mid != 0 || hi != 0 {
            return None; // not a plain ATA disk (likely ATAPI)
        }
        self.wait_data_request().ok()?;

        let mut identify = [0u16; 256];
        for word in identify.iter_mut() {
            *word = unsafe { self.data().read() };
        }

        let sectors = (identify[60] as u64) | ((identify[61] as u64) << 16);
        if sectors == 0 {
            return None;
        }
        let mut model = String::new();
        for word in &identify[27..47] {
            // Model string is byte-swapped per word.
            model.push((word >> 8) as u8 as char);
            model.push((word & 0xFF) as u8 as char);
        }
        Some((String::from(model.trim()), sectors))
    }

    fn read_sector(&self, slave: bool, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        if lba >= 1 << 28 {
            return Err("LBA28 limit exceeded");
        }
        self.wait_not_busy()?;
        self.select(slave, (lba >> 24) as u8);
        unsafe {
            self.reg(2).write(1); // sector count
            self.reg(3).write(lba as u8);
            self.reg(4).write((lba >> 8) as u8);
            self.reg(5).write((lba >> 16) as u8);
            self.reg(7).write(CMD_READ_SECTORS);
        }
        self.wait_data_request()?;
        for chunk in buf.chunks_exact_mut(2) {
            let word = unsafe { self.data().read() };
            chunk[0] = word as u8;
            chunk[1] = (word >> 8) as u8;
        }
        Ok(())
    }
}

struct AtaDisk {
    channel: Channel,
    slave: bool,
    model: String,
    sectors: u64,
}

impl BlockDevice for AtaDisk {
    fn name(&self) -> String {
        format!(
            "ata{}{} ({})",
            if self.channel.io_base == 0x1F0 { 0 } else { 1 },
            if self.slave { ".1" } else { ".0" },
            self.model
        )
    }

    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read_sector(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        self.channel.read_sector(self.slave, lba, buf)
    }
}

/// Probe both legacy IDE channels and register every ATA disk found.
pub fn init() -> usize {
    let mut found = 0;
    for (io, ctrl) in [(0x1F0u16, 0x3F6u16), (0x170, 0x376)] {
        for slave in [false, true] {
            let channel = Channel::new(io, ctrl);
            if let Some((model, sectors)) = channel.identify(slave) {
                driver::register_block_device(Box::new(AtaDisk {
                    channel: Channel::new(io, ctrl),
                    slave,
                    model,
                    sectors,
                }));
                found += 1;
            }
        }
    }
    found
}
