//! Driver framework, first cut (v0.3 "Batang"): a registry of block devices
//! that storage drivers publish into and filesystems consume from.
//! User-space driver isolation is later roadmap work (§10.2).

use alloc::{boxed::Box, string::String, vec::Vec};
use spin::Mutex;

pub const SECTOR_SIZE: usize = 512;

pub trait BlockDevice: Send {
    fn name(&self) -> String;
    fn sector_count(&self) -> u64;
    fn read_sector(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str>;

    /// Read `buf.len() / SECTOR_SIZE` consecutive sectors from `lba` (`buf`
    /// is a whole number of sectors). Default: one `read_sector` each; a
    /// driver with a multi-sector command overrides it.
    fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        let mut sector = [0u8; SECTOR_SIZE];
        for (i, chunk) in buf.chunks_exact_mut(SECTOR_SIZE).enumerate() {
            self.read_sector(lba + i as u64, &mut sector)?;
            chunk.copy_from_slice(&sector);
        }
        Ok(())
    }

    /// Write one sector. Default: read-only device.
    fn write_sector(&mut self, _lba: u64, _buf: &[u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        Err("block device is read-only")
    }
}

static BLOCK_DEVICES: Mutex<Vec<Box<dyn BlockDevice>>> = Mutex::new(Vec::new());

pub fn register_block_device(dev: Box<dyn BlockDevice>) {
    crate::println!(
        "[driver] block device registered: {} ({} sectors)",
        dev.name(),
        dev.sector_count()
    );
    BLOCK_DEVICES.lock().push(dev);
}

pub fn block_device_count() -> usize {
    BLOCK_DEVICES.lock().len()
}

/// Run `f` with the first registered block device, if any.
pub fn with_boot_block_device<R>(
    f: impl FnOnce(&mut dyn BlockDevice) -> R,
) -> Option<R> {
    let mut devices = BLOCK_DEVICES.lock();
    let i = boot_index(&mut devices)?;
    Some(f(devices[i].as_mut()))
}

/// The device holding Buitenzorg's own volume, by its marker file. Never just
/// the first disk: on a real PC that is the owner's Windows or Linux disk, and
/// `/disk` is written to at every boot.
fn boot_index(devices: &mut [Box<dyn BlockDevice>]) -> Option<usize> {
    devices.iter_mut().position(|d| {
        let dev = d.as_mut();
        crate::fat::FatVolume::mount(dev).is_ok_and(|v| v.has_marker(dev))
    })
}

/// Remove and return the first registered block device (transfers ownership,
/// e.g. to the VFS). Returns `None` if the registry is empty.
pub fn take_boot_block_device() -> Option<Box<dyn BlockDevice>> {
    let mut devices = BLOCK_DEVICES.lock();
    let i = boot_index(&mut devices)?;
    Some(devices.remove(i))
}
