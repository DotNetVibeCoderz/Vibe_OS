//! Metadata journal for FAT volumes (ordered mode, like ext3's default).
//!
//! A volume that carries the file `JOURNAL.SYS` in its root (every Buitenzorg
//! image does; bzimage makes it 256 KiB) gets crash-consistent metadata:
//! every mutating FAT operation (write, append, delete, mkdir, rename) runs
//! inside a [`Tx`], which hands reads through to the device but *keeps* every
//! metadata sector it writes (FAT copies, directory entries, FSInfo). Bulk
//! file data goes straight to freshly allocated clusters (`write_data`) -- it
//! is not reachable until the metadata that points at it commits, so a crash
//! can never expose half-written data under a valid entry.
//!
//! Commit: (1) the sector list + new sector images into the journal file,
//! (2) a header sector marked *committed* with a checksum over (1), (3) the
//! images to their home locations, (4) the header back to *empty*. A crash
//! before (2) leaves the old state (the journal is ignored); after (2) the
//! next mount replays (3). Sector writes of every driver complete
//! synchronously; ATA additionally flushes its cache per sector.
//!
//! Journal file layout (sector index within the file's chain):
//! 0 = header (`BZJRNL01`, seq u64, count u32, state u32 [1 committed],
//! checksum u64 [FNV-1a over the list bytes then the images]), 1..=L = the
//! home LBAs (u64 each, 64 per sector), then `count` images.

use alloc::{boxed::Box, collections::BTreeMap, string::String, vec::Vec};
use core::sync::atomic::{AtomicU8, Ordering};

use crate::driver::{BlockDevice, SECTOR_SIZE};

pub const FILE: &str = "JOURNAL.SYS";
const MAGIC: &[u8; 8] = b"BZJRNL01";

/// Self-test hook: 1 = stop after writing the committed header (a crash
/// before the home writes), 2 = stop before it (a crash mid-journal).
static CRASH: AtomicU8 = AtomicU8::new(0);

pub fn simulate_crash(point: u8) {
    CRASH.store(point, Ordering::SeqCst);
}

/// A transaction over a device: reads see its own writes.
pub struct Tx<'a> {
    dev: &'a mut dyn BlockDevice,
    pending: BTreeMap<u64, Box<[u8; SECTOR_SIZE]>>,
    order: Vec<u64>,
}

impl<'a> Tx<'a> {
    pub fn new(dev: &'a mut dyn BlockDevice) -> Self {
        Self { dev, pending: BTreeMap::new(), order: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Write everything out through the journal in `lbas` (the journal file's
    /// sectors), or directly in write order without one.
    pub fn commit(self, journal: Option<&Journal>) -> Result<usize, &'static str> {
        let Tx { dev, mut pending, order } = self;
        let n = pending.len();
        if n == 0 {
            return Ok(0);
        }
        let j = match journal {
            Some(j) if j.fits(n) => j,
            _ => {
                if journal.is_some() {
                    crate::println!("[journal] {} sectors do not fit; written unjournaled", n);
                }
                for lba in order {
                    if let Some(s) = pending.remove(&lba) {
                        dev.write_sector(lba, &s)?;
                    }
                }
                return Ok(n);
            }
        };
        let homes: Vec<u64> = pending.keys().copied().collect();
        let list_secs = n.div_ceil(64);
        let mut hash = Fnv::new();
        // 1. the home list
        for ls in 0..list_secs {
            let mut s = [0u8; SECTOR_SIZE];
            for (i, lba) in homes.iter().skip(ls * 64).take(64).enumerate() {
                s[i * 8..i * 8 + 8].copy_from_slice(&lba.to_le_bytes());
            }
            hash.feed(&s);
            dev.write_sector(j.lbas[1 + ls], &s)?;
        }
        // ... and the images
        for (i, lba) in homes.iter().enumerate() {
            let img = &pending[lba];
            hash.feed(&img[..]);
            dev.write_sector(j.lbas[1 + list_secs + i], img)?;
        }
        if CRASH.load(Ordering::SeqCst) == 2 {
            CRASH.store(0, Ordering::SeqCst);
            return Err("simulated crash before commit");
        }
        // 2. the commit record
        let seq = j.next_seq();
        dev.write_sector(j.lbas[0], &header(seq, n as u32, 1, hash.0))?;
        if CRASH.load(Ordering::SeqCst) == 1 {
            CRASH.store(0, Ordering::SeqCst);
            return Err("simulated crash after commit");
        }
        // 3. home locations
        for lba in &homes {
            dev.write_sector(*lba, &pending[lba])?;
        }
        // 4. empty again
        dev.write_sector(j.lbas[0], &header(seq, 0, 0, 0))?;
        Ok(n)
    }
}

fn header(seq: u64, count: u32, state: u32, sum: u64) -> [u8; SECTOR_SIZE] {
    let mut h = [0u8; SECTOR_SIZE];
    h[..8].copy_from_slice(MAGIC);
    h[8..16].copy_from_slice(&seq.to_le_bytes());
    h[16..20].copy_from_slice(&count.to_le_bytes());
    h[20..24].copy_from_slice(&state.to_le_bytes());
    h[24..32].copy_from_slice(&sum.to_le_bytes());
    h
}

struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    fn feed(&mut self, b: &[u8]) {
        for &x in b {
            self.0 = (self.0 ^ x as u64).wrapping_mul(0x100_0000_01b3);
        }
    }
}

impl BlockDevice for Tx<'_> {
    fn name(&self) -> String {
        self.dev.name()
    }
    fn sector_count(&self) -> u64 {
        self.dev.sector_count()
    }
    fn read_sector(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        match self.pending.get(&lba) {
            Some(s) => {
                buf.copy_from_slice(&s[..]);
                Ok(())
            }
            None => self.dev.read_sector(lba, buf),
        }
    }
    fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        let n = (buf.len() / SECTOR_SIZE) as u64;
        if self.pending.range(lba..lba + n).next().is_none() {
            return self.dev.read_sectors(lba, buf);
        }
        let mut s = [0u8; SECTOR_SIZE];
        for (i, chunk) in buf.chunks_exact_mut(SECTOR_SIZE).enumerate() {
            self.read_sector(lba + i as u64, &mut s)?;
            chunk.copy_from_slice(&s);
        }
        Ok(())
    }
    fn write_sector(&mut self, lba: u64, buf: &[u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        if !self.pending.contains_key(&lba) {
            self.order.push(lba);
        }
        self.pending.insert(lba, Box::new(*buf));
        Ok(())
    }
    fn write_data(&mut self, lba: u64, buf: &[u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        if let Some(s) = self.pending.get_mut(&lba) {
            s.copy_from_slice(buf); // a sector already journaled stays journaled
            return Ok(());
        }
        self.dev.write_sector(lba, buf)
    }
}

/// The journal file's sectors.
pub struct Journal {
    pub lbas: Vec<u64>,
    seq: spin::Mutex<u64>,
}

impl Journal {
    pub fn new(lbas: Vec<u64>) -> Self {
        Self { lbas, seq: spin::Mutex::new(0) }
    }
    fn fits(&self, n: usize) -> bool {
        1 + n.div_ceil(64) + n <= self.lbas.len()
    }
    /// Sectors one transaction can hold.
    pub fn capacity(&self) -> usize {
        let mut n = self.lbas.len().saturating_sub(1);
        while n > 0 && !self.fits(n) {
            n -= 1;
        }
        n
    }
    fn next_seq(&self) -> u64 {
        let mut s = self.seq.lock();
        *s += 1;
        *s
    }

    /// Apply a committed transaction left by a crash. Returns the number of
    /// sectors replayed (0 = the journal was empty or not committed).
    pub fn replay(&self, dev: &mut dyn BlockDevice) -> Result<usize, &'static str> {
        let mut h = [0u8; SECTOR_SIZE];
        dev.read_sector(self.lbas[0], &mut h)?;
        if &h[..8] != MAGIC {
            return Ok(0); // a fresh (zeroed) journal
        }
        let seq = u64::from_le_bytes(h[8..16].try_into().unwrap());
        *self.seq.lock() = seq;
        let count = u32::from_le_bytes(h[16..20].try_into().unwrap()) as usize;
        let state = u32::from_le_bytes(h[20..24].try_into().unwrap());
        let sum = u64::from_le_bytes(h[24..32].try_into().unwrap());
        if state != 1 || count == 0 {
            return Ok(0);
        }
        if !self.fits(count) {
            return Err("journal header is corrupt");
        }
        let list_secs = count.div_ceil(64);
        let mut hash = Fnv::new();
        let mut homes = Vec::with_capacity(count);
        let mut s = [0u8; SECTOR_SIZE];
        for ls in 0..list_secs {
            dev.read_sector(self.lbas[1 + ls], &mut s)?;
            hash.feed(&s);
            for i in 0..64 {
                if homes.len() < count {
                    homes.push(u64::from_le_bytes(s[i * 8..i * 8 + 8].try_into().unwrap()));
                }
            }
        }
        let mut imgs = Vec::with_capacity(count);
        for i in 0..count {
            dev.read_sector(self.lbas[1 + list_secs + i], &mut s)?;
            hash.feed(&s);
            imgs.push(s);
        }
        if hash.0 != sum {
            // the commit record names data that is not there: never apply it
            dev.write_sector(self.lbas[0], &header(seq, 0, 0, 0))?;
            return Err("journal checksum mismatch (ignored)");
        }
        for (lba, img) in homes.iter().zip(imgs.iter()) {
            dev.write_sector(*lba, img)?;
        }
        dev.write_sector(self.lbas[0], &header(seq, 0, 0, 0))?;
        Ok(count)
    }
}
