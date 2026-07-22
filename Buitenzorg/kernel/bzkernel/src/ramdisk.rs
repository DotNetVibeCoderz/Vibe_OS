//! Heap-backed RAM block device (v0.5 "Dahan"). Gives the VFS and the FAT
//! writer a read/write volume that is deterministic across boots, without
//! mutating the boot image.

use alloc::{boxed::Box, format, string::String, vec, vec::Vec};

use crate::driver::{BlockDevice, SECTOR_SIZE};

pub struct RamDisk {
    name: String,
    data: Vec<u8>,
}

impl RamDisk {
    pub fn new(name: &str, sectors: usize) -> Self {
        Self {
            name: String::from(name),
            data: vec![0u8; sectors * SECTOR_SIZE],
        }
    }
}

impl BlockDevice for RamDisk {
    fn name(&self) -> String {
        format!("ramdisk:{}", self.name)
    }

    fn sector_count(&self) -> u64 {
        (self.data.len() / SECTOR_SIZE) as u64
    }

    fn read_sector(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        let start = lba as usize * SECTOR_SIZE;
        let end = start + SECTOR_SIZE;
        if end > self.data.len() {
            return Err("ramdisk read out of range");
        }
        buf.copy_from_slice(&self.data[start..end]);
        Ok(())
    }

    fn write_sector(&mut self, lba: u64, buf: &[u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        let start = lba as usize * SECTOR_SIZE;
        let end = start + SECTOR_SIZE;
        if end > self.data.len() {
            return Err("ramdisk write out of range");
        }
        self.data[start..end].copy_from_slice(buf);
        Ok(())
    }
}

/// Allocate a boxed ramdisk with `sectors` 512-byte sectors.
pub fn new(name: &str, sectors: usize) -> Box<dyn BlockDevice> {
    Box::new(RamDisk::new(name, sectors))
}
