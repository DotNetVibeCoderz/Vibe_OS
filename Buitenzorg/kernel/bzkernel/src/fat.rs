//! Read-only FAT12/16/32 access over a [`BlockDevice`] (v0.3 "Batang"
//! milestone: "baca file dari disk"). The full VFS arrives in v0.5 "Dahan";
//! this is deliberately minimal: MBR partition 0, root directory, 8.3 names.

use alloc::{string::String, vec::Vec};

use crate::driver::{BlockDevice, SECTOR_SIZE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FatKind {
    Fat12,
    Fat16,
    Fat32,
}

pub struct FatVolume {
    partition_start: u64,
    bytes_per_sector: u64,
    sectors_per_cluster: u64,
    fat_start: u64,
    root_dir_start: u64,   // FAT12/16: sector of root dir; FAT32: unused
    root_dir_sectors: u64, // FAT12/16 only
    data_start: u64,
    root_cluster: u32, // FAT32 only
    kind: FatKind,
}

fn u16_at(buf: &[u8], off: usize) -> u64 {
    u16::from_le_bytes([buf[off], buf[off + 1]]) as u64
}

fn u32_at(buf: &[u8], off: usize) -> u64 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]) as u64
}

impl FatVolume {
    /// Mount `dev`: try every MBR partition, then LBA 0 as a superfloppy,
    /// and accept the first sector that validates as a FAT VBR.
    pub fn mount(dev: &mut dyn BlockDevice) -> Result<Self, &'static str> {
        let mut sector = [0u8; SECTOR_SIZE];
        dev.read_sector(0, &mut sector)?;
        if sector[510] != 0x55 || sector[511] != 0xAA {
            return Err("no boot signature in sector 0");
        }
        let mut candidates = [0u64; 5];
        let mut n = 0;
        for entry in 0..4 {
            let off = 0x1BE + entry * 16;
            if sector[off + 4] != 0 {
                candidates[n] = u32_at(&sector, off + 8);
                n += 1;
            }
        }
        candidates[n] = 0; // superfloppy fallback
        n += 1;

        for &start in &candidates[..n] {
            if let Ok(vol) = Self::parse_vbr(dev, start) {
                return Ok(vol);
            }
        }
        Err("no FAT volume found on device")
    }

    fn parse_vbr(dev: &mut dyn BlockDevice, partition_start: u64) -> Result<Self, &'static str> {
        let mut vbr = [0u8; SECTOR_SIZE];
        dev.read_sector(partition_start, &mut vbr)?;

        // A FAT VBR starts with a jump instruction; this rejects bootloader
        // stage sectors that happen to sit in partition slots.
        if vbr[0] != 0xEB && vbr[0] != 0xE9 {
            return Err("not a FAT VBR (no jump instruction)");
        }
        let bytes_per_sector = u16_at(&vbr, 11);
        if bytes_per_sector != SECTOR_SIZE as u64 {
            return Err("unsupported sector size");
        }
        let sectors_per_cluster = vbr[13] as u64;
        if !sectors_per_cluster.is_power_of_two() {
            return Err("invalid sectors per cluster");
        }
        let reserved = u16_at(&vbr, 14);
        let num_fats = vbr[16] as u64;
        let root_entries = u16_at(&vbr, 17);
        let fat_size = match u16_at(&vbr, 22) {
            0 => u32_at(&vbr, 36), // FAT32
            n => n,
        };
        let total_sectors = match u16_at(&vbr, 19) {
            0 => u32_at(&vbr, 32),
            n => n,
        };
        if sectors_per_cluster == 0 || fat_size == 0 || total_sectors == 0 {
            return Err("invalid FAT BPB");
        }

        let fat_start = partition_start + reserved;
        let root_dir_sectors = (root_entries * 32).div_ceil(SECTOR_SIZE as u64);
        let root_dir_start = fat_start + num_fats * fat_size;
        let data_start = root_dir_start + root_dir_sectors;
        let cluster_count = (total_sectors - reserved - num_fats * fat_size - root_dir_sectors)
            / sectors_per_cluster;
        let kind = if cluster_count < 4085 {
            FatKind::Fat12
        } else if cluster_count < 65525 {
            FatKind::Fat16
        } else {
            FatKind::Fat32
        };

        Ok(Self {
            partition_start,
            bytes_per_sector,
            sectors_per_cluster,
            fat_start,
            root_dir_start,
            root_dir_sectors,
            data_start,
            root_cluster: u32_at(&vbr, 44) as u32,
            kind,
        })
    }

    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            FatKind::Fat12 => "FAT12",
            FatKind::Fat16 => "FAT16",
            FatKind::Fat32 => "FAT32",
        }
    }

    fn cluster_to_sector(&self, cluster: u32) -> u64 {
        self.data_start + (cluster as u64 - 2) * self.sectors_per_cluster
    }

    fn next_cluster(&self, dev: &mut dyn BlockDevice, cluster: u32) -> Result<Option<u32>, &'static str> {
        let mut sector = [0u8; SECTOR_SIZE];
        let entry = match self.kind {
            FatKind::Fat12 => {
                let offset = cluster as u64 + cluster as u64 / 2;
                let sec = self.fat_start + offset / self.bytes_per_sector;
                let idx = (offset % self.bytes_per_sector) as usize;
                dev.read_sector(sec, &mut sector)?;
                let lo = sector[idx] as u32;
                // The 12-bit entry may straddle a sector boundary.
                let hi = if idx + 1 < SECTOR_SIZE {
                    sector[idx + 1] as u32
                } else {
                    dev.read_sector(sec + 1, &mut sector)?;
                    sector[0] as u32
                };
                let raw = lo | (hi << 8);
                let val = if cluster & 1 == 0 { raw & 0xFFF } else { raw >> 4 };
                if val >= 0xFF8 { None } else { Some(val) }
            }
            FatKind::Fat16 => {
                let offset = cluster as u64 * 2;
                let sec = self.fat_start + offset / self.bytes_per_sector;
                dev.read_sector(sec, &mut sector)?;
                let idx = (offset % self.bytes_per_sector) as usize;
                let val = u16_at(&sector, idx) as u32;
                if val >= 0xFFF8 { None } else { Some(val) }
            }
            FatKind::Fat32 => {
                let offset = cluster as u64 * 4;
                let sec = self.fat_start + offset / self.bytes_per_sector;
                dev.read_sector(sec, &mut sector)?;
                let idx = (offset % self.bytes_per_sector) as usize;
                let val = (u32_at(&sector, idx) & 0x0FFF_FFFF) as u32;
                if val >= 0x0FFF_FFF8 { None } else { Some(val) }
            }
        };
        Ok(entry.filter(|&c| c >= 2))
    }

    /// List root-directory 8.3 names (for diagnostics).
    pub fn list_root(&self, dev: &mut dyn BlockDevice) -> Result<Vec<String>, &'static str> {
        let mut names = Vec::new();
        self.walk_root(dev, |entry| {
            names.push(decode_83(&entry[..11]));
            false
        })?;
        Ok(names)
    }

    /// Read a file from the root directory by 8.3 name (e.g. "BATANG.TXT").
    pub fn read_file(&self, dev: &mut dyn BlockDevice, name: &str) -> Result<Vec<u8>, &'static str> {
        let wanted = encode_83(name)?;
        let mut found: Option<(u32, u64)> = None;
        self.walk_root(dev, |entry| {
            if entry[..11] == wanted {
                let hi = u16_at(entry, 20) as u32;
                let lo = u16_at(entry, 26) as u32;
                found = Some(((hi << 16) | lo, u32_at(entry, 28)));
                true
            } else {
                false
            }
        })?;
        let (mut cluster, size) = found.ok_or("file not found in root directory")?;

        let mut data = Vec::with_capacity(size as usize);
        let mut sector = [0u8; SECTOR_SIZE];
        while data.len() < size as usize {
            if cluster < 2 {
                return Err("corrupt cluster chain");
            }
            let first = self.cluster_to_sector(cluster);
            for s in 0..self.sectors_per_cluster {
                if data.len() >= size as usize {
                    break;
                }
                dev.read_sector(first + s, &mut sector)?;
                let remaining = size as usize - data.len();
                data.extend_from_slice(&sector[..remaining.min(SECTOR_SIZE)]);
            }
            match self.next_cluster(dev, cluster)? {
                Some(next) => cluster = next,
                None => break,
            }
        }
        if data.len() < size as usize {
            return Err("cluster chain ended early");
        }
        Ok(data)
    }

    /// Visit every used root-directory entry; stop early when `f` returns true.
    fn walk_root(
        &self,
        dev: &mut dyn BlockDevice,
        mut f: impl FnMut(&[u8]) -> bool,
    ) -> Result<(), &'static str> {
        let mut sector = [0u8; SECTOR_SIZE];
        let mut visit_sector = |dev: &mut dyn BlockDevice, lba: u64, sector: &mut [u8; SECTOR_SIZE]| -> Result<bool, &'static str> {
            dev.read_sector(lba, sector)?;
            for entry in sector.chunks_exact(32) {
                match entry[0] {
                    0x00 => return Ok(true), // end of directory
                    0xE5 => continue,        // deleted
                    _ => {}
                }
                if entry[11] & 0x0F == 0x0F {
                    continue; // long-file-name entry
                }
                if f(entry) {
                    return Ok(true);
                }
            }
            Ok(false)
        };

        match self.kind {
            FatKind::Fat12 | FatKind::Fat16 => {
                for s in 0..self.root_dir_sectors {
                    if visit_sector(dev, self.root_dir_start + s, &mut sector)? {
                        return Ok(());
                    }
                }
            }
            FatKind::Fat32 => {
                let mut cluster = self.root_cluster;
                loop {
                    let first = self.cluster_to_sector(cluster);
                    for s in 0..self.sectors_per_cluster {
                        if visit_sector(dev, first + s, &mut sector)? {
                            return Ok(());
                        }
                    }
                    match self.next_cluster(dev, cluster)? {
                        Some(next) => cluster = next,
                        None => break,
                    }
                }
            }
        }
        Ok(())
    }
}

/// "BATANG.TXT" → 11-byte space-padded 8.3 directory form.
fn encode_83(name: &str) -> Result<[u8; 11], &'static str> {
    let mut out = [b' '; 11];
    let mut parts = name.split('.');
    let base = parts.next().ok_or("empty name")?;
    let ext = parts.next().unwrap_or("");
    if base.is_empty() || base.len() > 8 || ext.len() > 3 || parts.next().is_some() {
        return Err("not a valid 8.3 name");
    }
    for (i, b) in base.bytes().enumerate() {
        out[i] = b.to_ascii_uppercase();
    }
    for (i, b) in ext.bytes().enumerate() {
        out[8 + i] = b.to_ascii_uppercase();
    }
    Ok(out)
}

fn decode_83(raw: &[u8]) -> String {
    let base: String = raw[..8].iter().map(|&b| b as char).collect();
    let ext: String = raw[8..11].iter().map(|&b| b as char).collect();
    let base = base.trim_end();
    let ext = ext.trim_end();
    if ext.is_empty() {
        String::from(base)
    } else {
        let mut s = String::from(base);
        s.push('.');
        s.push_str(ext);
        s
    }
}

impl FatVolume {
    pub fn partition_start(&self) -> u64 {
        self.partition_start
    }

    /// Read one FAT12 entry (only FAT12 write is implemented for the ramdisk).
    fn fat12_get(&self, dev: &mut dyn BlockDevice, cluster: u32) -> Result<u32, &'static str> {
        let offset = cluster as u64 + cluster as u64 / 2;
        let sec = self.fat_start + offset / self.bytes_per_sector;
        let idx = (offset % self.bytes_per_sector) as usize;
        let mut s0 = [0u8; SECTOR_SIZE];
        dev.read_sector(sec, &mut s0)?;
        let lo = s0[idx] as u32;
        let hi = if idx + 1 < SECTOR_SIZE {
            s0[idx + 1] as u32
        } else {
            let mut s1 = [0u8; SECTOR_SIZE];
            dev.read_sector(sec + 1, &mut s1)?;
            s1[0] as u32
        };
        let raw = lo | (hi << 8);
        Ok(if cluster & 1 == 0 { raw & 0xFFF } else { raw >> 4 })
    }

    /// Write one FAT12 entry into both FAT copies.
    fn fat12_set(&self, dev: &mut dyn BlockDevice, cluster: u32, value: u32) -> Result<(), &'static str> {
        let fat_sectors = (self.data_start - self.fat_start) / 2; // 2 FATs
        for copy in 0..2u64 {
            let base = self.fat_start + copy * fat_sectors;
            let offset = cluster as u64 + cluster as u64 / 2;
            let sec = base + offset / self.bytes_per_sector;
            let idx = (offset % self.bytes_per_sector) as usize;

            let mut s0 = [0u8; SECTOR_SIZE];
            dev.read_sector(sec, &mut s0)?;
            let mut s1 = [0u8; SECTOR_SIZE];
            let straddle = idx + 1 >= SECTOR_SIZE;
            if straddle {
                dev.read_sector(sec + 1, &mut s1)?;
            }
            let byte1 = |s0: &[u8; SECTOR_SIZE], s1: &[u8; SECTOR_SIZE]| {
                if straddle { s1[0] } else { s0[idx + 1] }
            };
            let (mut b0, mut b1) = (s0[idx], byte1(&s0, &s1));
            if cluster & 1 == 0 {
                b0 = (value & 0xFF) as u8;
                b1 = (b1 & 0xF0) | ((value >> 8) & 0x0F) as u8;
            } else {
                b0 = (b0 & 0x0F) | ((value << 4) & 0xF0) as u8;
                b1 = ((value >> 4) & 0xFF) as u8;
            }
            s0[idx] = b0;
            if straddle {
                s1[0] = b1;
                dev.write_sector(sec, &s0)?;
                dev.write_sector(sec + 1, &s1)?;
            } else {
                s0[idx + 1] = b1;
                dev.write_sector(sec, &s0)?;
            }
        }
        Ok(())
    }

    /// Create or overwrite a root-directory file with `data` (FAT12 volumes).
    pub fn write_file(
        &self,
        dev: &mut dyn BlockDevice,
        name: &str,
        data: &[u8],
    ) -> Result<(), &'static str> {
        if self.kind != FatKind::Fat12 {
            return Err("write only implemented for FAT12");
        }
        let cluster_bytes = self.sectors_per_cluster * self.bytes_per_sector;
        let needed = (data.len() as u64).div_ceil(cluster_bytes).max(1);

        // Find `needed` free clusters (value 0), scanning from cluster 2.
        let max_cluster = ((dev.sector_count() - self.data_start) / self.sectors_per_cluster) as u32 + 2;
        let mut chain: Vec<u32> = Vec::new();
        let mut c = 2u32;
        while (chain.len() as u64) < needed && c < max_cluster {
            if self.fat12_get(dev, c)? == 0 {
                chain.push(c);
            }
            c += 1;
        }
        if (chain.len() as u64) < needed {
            return Err("not enough free clusters");
        }

        // Write data into the cluster chain.
        for (i, &cluster) in chain.iter().enumerate() {
            let first = self.data_start + (cluster as u64 - 2) * self.sectors_per_cluster;
            for s in 0..self.sectors_per_cluster {
                let mut sector = [0u8; SECTOR_SIZE];
                let file_off = (i as u64 * cluster_bytes + s * self.bytes_per_sector) as usize;
                if file_off < data.len() {
                    let len = core::cmp::min(SECTOR_SIZE, data.len() - file_off);
                    sector[..len].copy_from_slice(&data[file_off..file_off + len]);
                }
                dev.write_sector(first + s, &sector)?;
            }
        }

        // Link the FAT chain (each entry -> next, last -> EOC).
        for i in 0..chain.len() {
            let value = if i + 1 < chain.len() { chain[i + 1] } else { 0xFFF };
            self.fat12_set(dev, chain[i], value)?;
        }

        // Write the directory entry (create or overwrite in the root dir).
        self.put_dir_entry(dev, name, chain[0], data.len() as u32)?;
        Ok(())
    }

    fn put_dir_entry(
        &self,
        dev: &mut dyn BlockDevice,
        name: &str,
        first_cluster: u32,
        size: u32,
    ) -> Result<(), &'static str> {
        let wanted = encode_83(name)?;
        for s in 0..self.root_dir_sectors {
            let lba = self.root_dir_start + s;
            let mut sector = [0u8; SECTOR_SIZE];
            dev.read_sector(lba, &mut sector)?;
            for e in 0..(SECTOR_SIZE / 32) {
                let off = e * 32;
                let first = sector[off];
                let matches = sector[off..off + 11] == wanted;
                if first == 0x00 || first == 0xE5 || matches {
                    let entry = &mut sector[off..off + 32];
                    entry.fill(0);
                    entry[..11].copy_from_slice(&wanted);
                    entry[11] = 0x20; // archive
                    entry[26] = (first_cluster & 0xFF) as u8;
                    entry[27] = ((first_cluster >> 8) & 0xFF) as u8;
                    entry[28..32].copy_from_slice(&size.to_le_bytes());
                    dev.write_sector(lba, &sector)?;
                    return Ok(());
                }
            }
        }
        Err("root directory is full")
    }
}

/// Format a device as an empty FAT12 volume (superfloppy, VBR at LBA 0).
pub fn format_fat12(dev: &mut dyn BlockDevice) -> Result<(), &'static str> {
    let total = dev.sector_count();
    if total < 64 || total > 0xFFFF {
        return Err("ramdisk size unsuitable for FAT12");
    }
    let reserved = 1u64;
    let num_fats = 2u64;
    let root_entries = 512u64;
    let root_sectors = root_entries * 32 / SECTOR_SIZE as u64;
    let fat_size = 12u64; // 12 sectors -> 4096 FAT12 entries, enough here

    let mut vbr = [0u8; SECTOR_SIZE];
    vbr[0] = 0xEB;
    vbr[1] = 0x3C;
    vbr[2] = 0x90;
    vbr[3..11].copy_from_slice(b"BZDAHAN ");
    vbr[11..13].copy_from_slice(&(SECTOR_SIZE as u16).to_le_bytes());
    vbr[13] = 1; // sectors per cluster
    vbr[14..16].copy_from_slice(&(reserved as u16).to_le_bytes());
    vbr[16] = num_fats as u8;
    vbr[17..19].copy_from_slice(&(root_entries as u16).to_le_bytes());
    vbr[19..21].copy_from_slice(&(total as u16).to_le_bytes());
    vbr[21] = 0xF8; // media
    vbr[22..24].copy_from_slice(&(fat_size as u16).to_le_bytes());
    vbr[24..26].copy_from_slice(&32u16.to_le_bytes()); // sectors per track
    vbr[26..28].copy_from_slice(&2u16.to_le_bytes()); // heads
    vbr[38] = 0x29; // extended boot signature
    vbr[43..54].copy_from_slice(b"BZ RAMDISK "); // volume label
    vbr[54..62].copy_from_slice(b"FAT12   ");
    vbr[510] = 0x55;
    vbr[511] = 0xAA;
    dev.write_sector(0, &vbr)?;

    // Zero the FATs and root directory.
    let zero = [0u8; SECTOR_SIZE];
    let data_start = reserved + num_fats * fat_size + root_sectors;
    for lba in 1..data_start {
        dev.write_sector(lba, &zero)?;
    }
    // FAT[0] = media | 0xF00, FAT[1] = EOC. In FAT12 packed bytes: F8 FF FF.
    for copy in 0..num_fats {
        let mut fat0 = [0u8; SECTOR_SIZE];
        fat0[0] = 0xF8;
        fat0[1] = 0xFF;
        fat0[2] = 0xFF;
        dev.write_sector(reserved + copy * fat_size, &fat0)?;
    }
    Ok(())
}
