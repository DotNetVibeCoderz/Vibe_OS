//! AHCI (SATA) disks as [`BlockDevice`]s: polled, one command slot, DMA into
//! a driver-owned bounce buffer of 16 pages (64 KiB per command).
//!
//! Every wait is bounded: a port that stops answering must end in an error,
//! not a hang (see the note on device registers in CLAUDE.md).

use alloc::{boxed::Box, format, string::String, vec::Vec};
use core::ptr::{read_volatile, write_volatile};

use crate::driver::{self, BlockDevice, SECTOR_SIZE};
use crate::pci::PciDevice;

const CAP: usize = 0x00;
const GHC: usize = 0x04;
const PI: usize = 0x0C;
const CAP2: usize = 0x24;
const BOHC: usize = 0x28;
const GHC_AE: u32 = 1 << 31;
const CAP_S64A: u32 = 1 << 31;

const PX_CLB: usize = 0x00;
const PX_CLBU: usize = 0x04;
const PX_FB: usize = 0x08;
const PX_FBU: usize = 0x0C;
const PX_IS: usize = 0x10;
const PX_IE: usize = 0x14;
const PX_CMD: usize = 0x18;
const PX_TFD: usize = 0x20;
const PX_SIG: usize = 0x24;
const PX_SSTS: usize = 0x28;
const PX_SERR: usize = 0x30;
const PX_CI: usize = 0x38;
const CMD_ST: u32 = 1 << 0;
const CMD_FRE: u32 = 1 << 4;
const CMD_FR: u32 = 1 << 14;
const CMD_CR: u32 = 1 << 15;
const SIG_ATA: u32 = 0x0000_0101;
const IS_TFES: u32 = 1 << 30;
const TFD_ERR: u32 = 1 << 0;
const TFD_BUSY: u32 = (1 << 7) | (1 << 3);

const ATA_IDENTIFY: u8 = 0xEC;
const ATA_READ_DMA_EXT: u8 = 0x25;
const ATA_WRITE_DMA_EXT: u8 = 0x35;
const ATA_FLUSH_EXT: u8 = 0xEA;

/// Offsets in the port's control page.
const FIS_OFF: usize = 1024;
const TABLE_OFF: usize = 2048;
const PRDT_OFF: usize = TABLE_OFF + 0x80;

const BOUNCE_PAGES: usize = 16;
const MAX_SECTORS: usize = BOUNCE_PAGES * 4096 / SECTOR_SIZE;
/// Polls of a register before giving up: seconds on real hardware and in QEMU
/// (an uncached read takes around a microsecond).
const WAIT_LONG: u32 = 20_000_000;
const WAIT_SHORT: u32 = 2_000_000;

struct Port {
    regs: *mut u8,
    ctl_phys: u64,
    ctl: *mut u8,
    pages: Vec<(u64, *mut u8)>,
}

impl Port {
    fn rd(&self, off: usize) -> u32 {
        unsafe { read_volatile(self.regs.add(off) as *const u32) }
    }
    fn wr(&self, off: usize, v: u32) {
        unsafe { write_volatile(self.regs.add(off) as *mut u32, v) }
    }
    fn wait(&self, off: usize, mask: u32, value: u32, tries: u32) -> bool {
        for _ in 0..tries {
            if self.rd(off) & mask == value {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }

    /// Stop the command + FIS engines, point them at our page, start them.
    fn start(&mut self) -> Result<(), &'static str> {
        self.wr(PX_CMD, self.rd(PX_CMD) & !CMD_ST);
        if !self.wait(PX_CMD, CMD_CR, 0, WAIT_SHORT) {
            return Err("command engine did not stop");
        }
        self.wr(PX_CMD, self.rd(PX_CMD) & !CMD_FRE);
        if !self.wait(PX_CMD, CMD_FR, 0, WAIT_SHORT) {
            return Err("FIS engine did not stop");
        }
        let clb = self.ctl_phys;
        let fb = self.ctl_phys + FIS_OFF as u64;
        self.wr(PX_CLB, clb as u32);
        self.wr(PX_CLBU, (clb >> 32) as u32);
        self.wr(PX_FB, fb as u32);
        self.wr(PX_FBU, (fb >> 32) as u32);
        self.wr(PX_IE, 0);
        self.wr(PX_IS, u32::MAX);
        self.wr(PX_SERR, u32::MAX);
        self.wr(PX_CMD, self.rd(PX_CMD) | CMD_FRE);
        if !self.wait(PX_TFD, TFD_BUSY, 0, WAIT_LONG) {
            return Err("drive stays busy");
        }
        self.wr(PX_CMD, self.rd(PX_CMD) | CMD_ST);
        Ok(())
    }

    /// Slot 0: `cmd` on `count` sectors at `lba`; `bytes` of data in the bounce
    /// pages (0 = no data).
    fn command(&mut self, cmd: u8, lba: u64, count: u16, bytes: usize, write: bool) -> Result<(), &'static str> {
        let table_phys = self.ctl_phys + TABLE_OFF as u64;
        let prdts = bytes.div_ceil(4096);
        unsafe {
            // command header 0: FIS length in dwords, direction, PRDT entries
            let hdr = self.ctl as *mut u32;
            write_volatile(hdr, 5 | (u32::from(write) << 6) | ((prdts as u32) << 16));
            write_volatile(hdr.add(1), 0);
            write_volatile(hdr.add(2), table_phys as u32);
            write_volatile(hdr.add(3), (table_phys >> 32) as u32);
            // register FIS, host to device
            let f = self.ctl.add(TABLE_OFF);
            let fis: [u8; 16] = [
                0x27,
                0x80,
                cmd,
                0,
                lba as u8,
                (lba >> 8) as u8,
                (lba >> 16) as u8,
                0x40,
                (lba >> 24) as u8,
                (lba >> 32) as u8,
                (lba >> 40) as u8,
                0,
                count as u8,
                (count >> 8) as u8,
                0,
                0,
            ];
            for (i, b) in fis.iter().enumerate() {
                write_volatile(f.add(i), *b);
            }
            let prdt = self.ctl.add(PRDT_OFF) as *mut u32;
            let mut left = bytes;
            for (i, (phys, _)) in self.pages.iter().take(prdts).enumerate() {
                let n = left.min(4096);
                write_volatile(prdt.add(i * 4), *phys as u32);
                write_volatile(prdt.add(i * 4 + 1), (*phys >> 32) as u32);
                write_volatile(prdt.add(i * 4 + 2), 0);
                write_volatile(prdt.add(i * 4 + 3), (n - 1) as u32);
                left -= n;
            }
        }
        self.wr(PX_IS, u32::MAX);
        self.wr(PX_CI, 1);
        for _ in 0..WAIT_LONG {
            if self.rd(PX_IS) & IS_TFES != 0 {
                return Err("AHCI: task file error");
            }
            if self.rd(PX_CI) & 1 == 0 {
                return if self.rd(PX_TFD) & TFD_ERR != 0 { Err("AHCI: drive reported an error") } else { Ok(()) };
            }
            core::hint::spin_loop();
        }
        Err("AHCI: command timed out")
    }

    fn page_bytes(&self, page: usize) -> *mut u8 {
        self.pages[page].1
    }
}

struct AhciDisk {
    port: Port,
    index: usize,
    model: String,
    sectors: u64,
}

// The port registers and DMA pages belong to this driver instance.
unsafe impl Send for AhciDisk {}

impl BlockDevice for AhciDisk {
    fn name(&self) -> String {
        format!("ahci{} ({})", self.index, self.model)
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
            return Err("AHCI: read past the end of the disk");
        }
        let mut done = 0;
        while done < total {
            let n = (total - done).min(MAX_SECTORS);
            self.port.command(ATA_READ_DMA_EXT, lba + done as u64, n as u16, n * SECTOR_SIZE, false)?;
            let dst = &mut buf[done * SECTOR_SIZE..(done + n) * SECTOR_SIZE];
            for (i, chunk) in dst.chunks_mut(4096).enumerate() {
                unsafe { core::ptr::copy_nonoverlapping(self.port.page_bytes(i), chunk.as_mut_ptr(), chunk.len()) };
            }
            done += n;
        }
        Ok(())
    }

    fn write_sector(&mut self, lba: u64, buf: &[u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        if lba >= self.sectors {
            return Err("AHCI: write past the end of the disk");
        }
        unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), self.port.page_bytes(0), SECTOR_SIZE) };
        self.port.command(ATA_WRITE_DMA_EXT, lba, 1, SECTOR_SIZE, true)?;
        self.port.command(ATA_FLUSH_EXT, 0, 0, 0, false)
    }
}

/// Take the controller from the firmware, if it says it owns it.
fn bios_handoff(abar: *mut u8) {
    let rd = |o: usize| unsafe { read_volatile(abar.add(o) as *const u32) };
    if rd(CAP2) & 1 == 0 {
        return;
    }
    unsafe { write_volatile(abar.add(BOHC) as *mut u32, rd(BOHC) | 2) };
    for _ in 0..WAIT_SHORT {
        if rd(BOHC) & 0b1_0001 == 0 {
            return; // BIOS owned + BIOS busy both clear
        }
        core::hint::spin_loop();
    }
}

fn identify(port: &mut Port) -> Result<(String, u64), &'static str> {
    port.command(ATA_IDENTIFY, 0, 0, SECTOR_SIZE, false)?;
    let p = port.page_bytes(0) as *const u16;
    let word = |i: usize| unsafe { read_volatile(p.add(i)) };
    let lba48 = word(83) & (1 << 10) != 0;
    let sectors = if lba48 {
        (0..4).fold(0u64, |acc, i| acc | (word(100 + i) as u64) << (16 * i))
    } else {
        word(60) as u64 | (word(61) as u64) << 16
    };
    let mut model = String::new();
    for i in 27..47 {
        model.push((word(i) >> 8) as u8 as char);
        model.push((word(i) & 0xFF) as u8 as char);
    }
    Ok((String::from(model.trim()), sectors))
}

/// Register every SATA disk behind every AHCI controller. Returns how many.
pub fn init(devices: &[PciDevice]) -> usize {
    let mut found = 0;
    for dev in devices.iter().filter(|d| (d.class, d.subclass, d.prog_if) == (0x01, 0x06, 0x01)) {
        let Some(bar) = dev.mem_bar(5) else { continue };
        dev.enable_mem_and_bus_master();
        let Some(va) = crate::memory::map_mmio(bar, 0x2000) else { continue };
        let abar = va as *mut u8;
        bios_handoff(abar);
        let (cap, ports) = unsafe {
            write_volatile(abar.add(GHC) as *mut u32, read_volatile(abar.add(GHC) as *const u32) | GHC_AE);
            write_volatile(abar.add(GHC) as *mut u32, read_volatile(abar.add(GHC) as *const u32) & !2); // no interrupts
            (read_volatile(abar.add(CAP) as *const u32), read_volatile(abar.add(PI) as *const u32))
        };
        for p in 0..32 {
            if ports & (1 << p) == 0 {
                continue;
            }
            let regs = unsafe { abar.add(0x100 + p * 0x80) };
            let rd = |o: usize| unsafe { read_volatile(regs.add(o) as *const u32) };
            if rd(PX_SSTS) & 0xF != 3 || rd(PX_SIG) != SIG_ATA {
                continue; // nothing attached, or a CD/port multiplier
            }
            match bring_up(regs, cap, found) {
                Ok(disk) => {
                    found += 1;
                    driver::register_block_device(Box::new(disk));
                }
                Err(e) => crate::println!("[ahci] port {}: {}", p, e),
            }
        }
    }
    found
}

fn bring_up(regs: *mut u8, cap: u32, index: usize) -> Result<AhciDisk, &'static str> {
    // without 64-bit addressing every structure must sit below 4 GiB
    let fits = |phys: u64| cap & CAP_S64A != 0 || phys + 4096 <= 1 << 32;
    let (ctl_phys, ctl) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
    if !fits(ctl_phys) {
        return Err("controller without 64-bit DMA and no memory below 4 GiB");
    }
    let mut pages = Vec::with_capacity(BOUNCE_PAGES);
    for _ in 0..BOUNCE_PAGES {
        let page = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
        if !fits(page.0) {
            return Err("controller without 64-bit DMA and no memory below 4 GiB");
        }
        pages.push(page);
    }
    let mut port = Port { regs, ctl_phys, ctl, pages };
    port.start()?;
    let (model, sectors) = identify(&mut port)?;
    if sectors == 0 {
        return Err("drive reports no capacity");
    }
    Ok(AhciDisk { port, index, model, sectors })
}
