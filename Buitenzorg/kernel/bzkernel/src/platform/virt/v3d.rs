//! VideoCore VI V3D (Raspberry Pi 4) bring-up for G3, after Linux's `v3d`
//! driver (`v3d_drv.c`, `v3d_regs.h`) and `raspberrypi-power`.
//!
//! What this does: find the `brcm,2711-v3d` node in the Device Tree (hub +
//! core0 register blocks, through the `/v3dbus` ranges), power the V3D
//! domain on through the firmware (mailbox SET_DOMAIN_STATE, domain 10), set
//! the V3D clock (id 5) to its maximum, map the registers and read the
//! identification registers: the hub's IDENT1 gives the technology version,
//! revision, core count and the TFU / L3 cache / MMU options, core 0's IDENT0
//! carries the "V3D" magic and IDENT1 the slice and QPU counts.
//!
//! Step 2a (shell `gpu v3d`, untested on a board): the first jobs. The GPU's
//! own MMU is set up like `v3d_mmu.c` (a 4 MiB page table, one 32-bit entry
//! per 4 KiB page: physical page number | WRITEABLE | VALID; a scratch page
//! for illegal accesses), buffers are mapped into GPU addresses, and a
//! **render-only job** runs on the control list executor's render thread
//! (CT1): a render control list in the layout of Mesa's v3dv
//! `emit_copy_buffer_rcl` (TILE_RENDERING_MODE_CFG common / clear colour /
//! colour / ZS, initial block size, tile list base, supertile config, the two
//! GFXH-1742 dummy tiles with CLEAR_TILE_BUFFERS, a generic tile list =
//! implicit coordinates + END_OF_LOADS + STORE_TILE_BUFFER_GENERAL (raster
//! RGBA8) + END_OF_TILE, then SUPERTILE_COORDINATES per supertile and
//! END_OF_RENDERING) -- no binning job, no shader: every tile is the clear
//! colour, stored to memory. The CPU reads the image back, checks every pixel
//! and copies it to the top right of the screen for 4 s. Packet layouts are
//! Mesa's `v3d_packet_v33.xml` (min_ver 41 variants), registers Linux's
//! `v3d_regs.h`. Drawing triangles needs a binning job and QPU shaders (next
//! steps); the 3D API keeps using the software rasterizer on the Pi.
//!
//! QEMU `raspi4b` does not model V3D, so every register access at init goes
//! through `probe_read32` and an absent block is reported, not faulted on.

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::arch::aarch64::exceptions::probe_read32;
use crate::arch::aarch64::rpi::firmware_property;

const SET_DOMAIN_STATE: u32 = 0x0003_8030;
const GET_MAX_CLOCK_RATE: u32 = 0x0003_0004;
const SET_CLOCK_RATE: u32 = 0x0003_8002;
const SET_CLOCK_STATE: u32 = 0x0003_8001;
// the SoC power manager (brcm,bcm2711-pm: reg "pm", "asb", "rpivid_asb"),
// after Linux drivers/pmdomain/bcm/bcm2835-power.c
const PM_GRAFX: u64 = 0x10c;
const PM_V3DRSTN: u32 = 1 << 6;
const PM_PASSWORD: u32 = 0x5a00_0000;
const ASB_V3D_S_CTRL: u64 = 0x08;
const ASB_V3D_M_CTRL: u64 = 0x0c;
const ASB_AXI_BRDG_ID: u64 = 0x20;
const ASB_REQ_STOP: u32 = 1 << 0;
const ASB_ACK: u32 = 1 << 1;
const BRDG_ID: u32 = 0x6272_6467; // "brdg"

/// What init found or where it stopped (shown by `gpu v3d` when there is no V3D).
static STATUS: Mutex<String> = Mutex::new(String::new());

fn status(s: String) {
    crate::println!("[v3d] {}", s);
    *STATUS.lock() = s;
}

/// Power the V3D domain the way Linux does on a BCM2711: the GRAFX domain is
/// the firmware's; for GRAFX_V3D the V3D clock must run, the V3D reset is
/// released in PM_GRAFX, then the V3D's asynchronous AXI bridges (master,
/// then slave) are enabled in the `rpivid_asb` block. Returns a summary.
fn power_on(f: &crate::dtb::Fdt) -> String {
    let Some(i) = f.compatible("brcm,bcm2711-pm").next() else { return String::from("no brcm,bcm2711-pm node") };
    let regs = f.reg(i);
    let names: Vec<&str> = f.nodes[i].strings("reg-names").collect();
    let by = |n: &str| names.iter().position(|s| *s == n).and_then(|k| regs.get(k).copied());
    let (Some(pm), Some(asb)) = (by("pm"), by("rpivid_asb").or(by("asb"))) else { return String::from("pm node without pm / asb registers") };
    let (Some(pm_va), Some(asb_va)) = (crate::memory::map_mmio(pm.0, 0x1000), crate::memory::map_mmio(asb.0, 0x1000)) else {
        return String::from("mapping pm / asb failed");
    };
    let Some(grafx) = probe_read32(pm_va + PM_GRAFX) else { return format!("PM at {:#x} does not answer", pm.0) };
    PM_VA.store(pm_va, Ordering::Relaxed);
    ASB_VA.store(asb_va, Ordering::Relaxed);
    if grafx & PM_V3DRSTN == 0 {
        w(pm_va, PM_GRAFX, PM_PASSWORD | (grafx & 0x00FF_FFFF) | PM_V3DRSTN);
    }
    let grafx2 = probe_read32(pm_va + PM_GRAFX).unwrap_or(0);
    let Some(id) = probe_read32(asb_va + ASB_AXI_BRDG_ID) else { return format!("PM_GRAFX {:#x} -> {:#x}; ASB at {:#x} does not answer", grafx, grafx2, asb.0) };
    let mut acks = [false; 2];
    for (k, reg) in [ASB_V3D_M_CTRL, ASB_V3D_S_CTRL].into_iter().enumerate() {
        let v = probe_read32(asb_va + reg).unwrap_or(0);
        w(asb_va, reg, PM_PASSWORD | (v & !ASB_REQ_STOP & 0x00FF_FFFF));
        acks[k] = wait_us(1000, || probe_read32(asb_va + reg).is_some_and(|v| v & ASB_ACK == 0));
    }
    format!(
        "power: PM_GRAFX {:#x} -> {:#x} (V3D reset released), ASB id {:#x}{} at {:#x}, bridges master {} slave {}",
        grafx,
        grafx2,
        id,
        if id == BRDG_ID { " (brdg)" } else { " (not brdg!)" },
        asb.0,
        if acks[0] { "on" } else { "NO ACK" },
        if acks[1] { "on" } else { "NO ACK" }
    )
}
/// The SoC power manager and the V3D AXI bridges (for `reset`).
static PM_VA: AtomicU64 = AtomicU64::new(0);
static ASB_VA: AtomicU64 = AtomicU64::new(0);
/// GPU resets after a hung or faulted job.
pub(super) static RESETS: AtomicU32 = AtomicU32::new(0);

/// Reset a hung V3D as Linux bcm2835-power does for GRAFX_V3D: stop the AXI
/// bridges (slave, then master), pulse the V3D reset in PM_GRAFX, restart
/// the bridges. Every V3D register is at its reset value afterwards.
fn hw_reset() -> bool {
    let (pm, asb) = (PM_VA.load(Ordering::Relaxed), ASB_VA.load(Ordering::Relaxed));
    if pm == 0 || asb == 0 {
        return false;
    }
    let mut ok = true;
    for reg in [ASB_V3D_S_CTRL, ASB_V3D_M_CTRL] {
        let v = r(asb, reg);
        w(asb, reg, PM_PASSWORD | (v & 0x00FF_FFFF) | ASB_REQ_STOP);
        ok &= wait_us(1000, || r(asb, reg) & ASB_ACK != 0);
    }
    let g = r(pm, PM_GRAFX) & 0x00FF_FFFF;
    w(pm, PM_GRAFX, PM_PASSWORD | (g & !PM_V3DRSTN));
    wait_us(10, || false);
    w(pm, PM_GRAFX, PM_PASSWORD | g | PM_V3DRSTN);
    wait_us(10, || false);
    for reg in [ASB_V3D_M_CTRL, ASB_V3D_S_CTRL] {
        let v = r(asb, reg);
        w(asb, reg, PM_PASSWORD | (v & !ASB_REQ_STOP & 0x00FF_FFFF));
        ok &= wait_us(1000, || r(asb, reg) & ASB_ACK == 0);
    }
    ok
}

const DOMAIN_V3D: u32 = 10;
const CLOCK_V3D: u32 = 5;

// hub registers
pub(super) const HUB_IDENT0: u64 = 0x08;
pub(super) const HUB_IDENT1: u64 = 0x0c;
pub(super) const HUB_IDENT2: u64 = 0x10;
pub(super) const HUB_IDENT3: u64 = 0x14;
// core registers
pub(super) const CTL_IDENT0: u64 = 0x00;
pub(super) const CTL_IDENT1: u64 = 0x04;
pub(super) const CTL_IDENT2: u64 = 0x08;

/// What was found: version * 10 + revision (42 = V3D 4.2), 0 = none.
static VERSION: AtomicU32 = AtomicU32::new(0);

/// Identification of the V3D on this board.
pub struct Info {
    pub version: u32,
    pub cores: u32,
    pub slices: u32,
    pub qpus_per_slice: u32,
    pub tfu: bool,
    pub l3c: bool,
    pub mmu: bool,
    pub clock_hz: u32,
    pub ident: [u32; 7],
}

/// V3D version * 10 + revision when one was identified (shell `gfx3d`).
pub fn version() -> u32 {
    VERSION.load(Ordering::Relaxed)
}

/// Bring V3D up as far as identification. `None` when the board has none or
/// it does not answer (QEMU).
pub fn init(f: &crate::dtb::Fdt) -> Option<Info> {
    let Some(i) = f.compatible("brcm,2711-v3d").next() else {
        status(String::from("no brcm,2711-v3d node in the Device Tree"));
        return None;
    };
    let regs = f.reg(i);
    // reg-names says which block is which ("hub", "core0"); else hub is the lower address
    let names: alloc::vec::Vec<&str> = f.nodes[i].strings("reg-names").collect();
    let by = |n: &str| names.iter().position(|s| *s == n).and_then(|k| regs.get(k).copied());
    let (hub, core) = match (by("hub"), by("core0")) {
        (Some(h), Some(c)) => (h, c),
        _ if regs.len() >= 2 => {
            let (a, b) = (regs[0], regs[1]);
            if a.0 < b.0 { (a, b) } else { (b, a) }
        }
        _ => return None,
    };
    // power and clock (the firmware answers on a board; QEMU leaves them alone)
    let mut dom = [DOMAIN_V3D, 1];
    let powered = firmware_property(SET_DOMAIN_STATE, &mut dom);
    let mut on = [CLOCK_V3D, 1];
    let clock_on = firmware_property(SET_CLOCK_STATE, &mut on);
    let mut max = [CLOCK_V3D, 0];
    let mut clock_hz = 0;
    if firmware_property(GET_MAX_CLOCK_RATE, &mut max) && max[1] != 0 {
        let mut set = [CLOCK_V3D, max[1], 0];
        if firmware_property(SET_CLOCK_RATE, &mut set) {
            clock_hz = set[1];
        }
    }
    let pw = power_on(f);
    crate::println!("[v3d] clock {} ({} MHz), domain {}, {}", if clock_on { "on" } else { "?" }, clock_hz / 1_000_000, if powered { "on" } else { "?" }, pw);
    let hub_va = crate::memory::map_mmio(hub.0, hub.1.max(0x1000))?;
    let core_va = crate::memory::map_mmio(core.0, core.1.max(0x1000))?;
    let Some(c0) = probe_read32(core_va + CTL_IDENT0) else {
        status(format!(
            "V3D registers at {:#x} / {:#x} do not answer (QEMU raspi4b has no V3D) -- 3D stays in software; {}",
            hub.0, core.0, pw
        ));
        return None;
    };
    let rd = |va: u64, off: u64| probe_read32(va + off).unwrap_or(0);
    let ident = [
        rd(hub_va, HUB_IDENT0),
        rd(hub_va, HUB_IDENT1),
        rd(hub_va, HUB_IDENT2),
        rd(hub_va, HUB_IDENT3),
        c0,
        rd(core_va, CTL_IDENT1),
        rd(core_va, CTL_IDENT2),
    ];
    let h1 = ident[1];
    let tver = h1 & 0xF;
    let rev = (h1 >> 4) & 0xF;
    let info = Info {
        version: tver * 10 + rev,
        cores: (h1 >> 8) & 0xF,
        slices: (ident[5] >> 4) & 0xF,
        qpus_per_slice: (ident[5] >> 8) & 0xF,
        tfu: h1 & (1 << 17) != 0,
        l3c: h1 & (1 << 16) != 0,
        mmu: ident[2] & (1 << 4) != 0,
        clock_hz,
        ident,
    };
    let magic = c0 & 0x00FF_FFFF == 0x0044_3356; // "V3D"
    crate::println!(
        "[v3d] hub {:#x} core0 {:#x}: power {} clock {} MHz, ident {:08x?}, magic {}",
        hub.0,
        core.0,
        if powered { "on" } else { "?" },
        clock_hz / 1_000_000,
        ident,
        if magic { "V3D" } else { "missing" }
    );
    if !magic || info.version == 0 {
        status(format!("IDENT read {:08x?}: no \"V3D\" magic (powered off?); {}", ident, pw));
        return None;
    }
    status(format!("V3D {}.{} up; {}", info.version / 10, info.version % 10, pw));
    VERSION.store(info.version, Ordering::Relaxed);
    *GPU.lock() = Some(Gpu { hub: hub_va, core: core_va, mmu: None, next_page: FIRST_PAGE });
    Some(info)
}

// ---- step 2a: MMU + render-only jobs ---------------------------------------

// hub registers
pub(super) const MMUC_CONTROL: u64 = 0x1000;
pub(super) const MMUC_ENABLE: u32 = 1 << 0;
pub(super) const MMUC_FLUSH: u32 = 1 << 1;
pub(super) const MMUC_FLUSHING: u32 = 1 << 2;
pub(super) const MMU_CTL: u64 = 0x1200;
pub(super) const MMU_PT_PA_BASE: u64 = 0x1204;
pub(super) const MMU_VIO_ID: u64 = 0x122c;
pub(super) const MMU_ILLEGAL_ADDR: u64 = 0x1230;
pub(super) const MMU_VIO_ADDR: u64 = 0x1234;
pub(super) const MMU_DEBUG_INFO: u64 = 0x1238;
pub(super) const MMU_TLB_CLEAR: u32 = 1 << 2;
pub(super) const MMU_TLB_CLEARING: u32 = 1 << 7;
pub(super) const HUB_INT_STS: u64 = 0x50;
// core registers
pub(super) const CTL_SLCACTL: u64 = 0x24;
pub(super) const CTL_L2TCACTL: u64 = 0x30;
pub(super) const CTL_L2TFLSTA: u64 = 0x34;
pub(super) const CTL_L2TFLEND: u64 = 0x38;
pub(super) const CTL_INT_STS: u64 = 0x50;
pub(super) const CTL_INT_CLR: u64 = 0x58;
pub(super) const CTL_INT_MSK_CLR: u64 = 0x64;
pub(super) const INT_FRDONE: u32 = 1 << 0;
pub(super) const INT_FLDONE: u32 = 1 << 1;
pub(super) const INT_OUTOMEM: u32 = 1 << 2;
pub(super) const INT_GMPV: u32 = 1 << 5;
pub(super) const INT_CSDDONE: u32 = 1 << 7;
pub(super) const CLE_CT1CS: u64 = 0x104;
pub(super) const CLE_CT1EA: u64 = 0x10c;
pub(super) const CLE_CT1CA: u64 = 0x114;
pub(super) const CLE_PCS: u64 = 0x130;
pub(super) const CLE_RFC: u64 = 0x138;
pub(super) const CLE_CT1QBA: u64 = 0x164;
pub(super) const CLE_CT1QEA: u64 = 0x16c;

pub(super) const PTE_WRITEABLE: u32 = 1 << 29;
pub(super) const PTE_VALID: u32 = 1 << 28;
/// GPU pages below this stay unmapped (the hardware treats address 0 as "none").
pub(super) const FIRST_PAGE: u32 = 0x100;
/// 4 MiB page table: one 32-bit entry per 4 KiB page of the 4 GiB GPU space.
pub(super) const PT_PAGES: usize = 1024;

pub(super) struct Mmu {
    pt: *mut u32,
    pt_phys: u64,
    scratch: u64,
    pa_bits: u32,
}

pub(super) struct Gpu {
    pub(super) hub: u64,
    pub(super) core: u64,
    pub(super) mmu: Option<Mmu>,
    pub(super) next_page: u32,
}

unsafe impl Send for Gpu {}

pub(super) static GPU: Mutex<Option<Gpu>> = Mutex::new(None);

pub(super) fn r(base: u64, off: u64) -> u32 {
    unsafe { ((base + off) as *const u32).read_volatile() }
}
pub(super) fn w(base: u64, off: u64, v: u32) {
    unsafe { ((base + off) as *mut u32).write_volatile(v) }
}
pub(super) fn elapsed_us(t0: u64) -> u64 {
    (crate::arch::cpu::timestamp() - t0) * 1_000_000 / crate::arch::cpu::counter_hz().max(1)
}
pub(super) fn wait_us(us: u64, mut done: impl FnMut() -> bool) -> bool {
    let t0 = crate::arch::cpu::timestamp();
    while !done() {
        if elapsed_us(t0) > us {
            return false;
        }
        core::hint::spin_loop();
    }
    true
}

/// Memory the CPU and the GPU share: frames of our own, consecutive in GPU
/// address space (`va`), not necessarily in physical memory.
pub(super) struct Buffer {
    pub(super) va: u32,
    pub(super) frames: Vec<(u64, *mut u8)>,
}

impl Buffer {
    pub(super) fn write(&self, off: usize, bytes: &[u8]) {
        for (k, b) in bytes.iter().enumerate() {
            let o = off + k;
            unsafe { self.frames[o / 4096].1.add(o % 4096).write_volatile(*b) };
        }
    }
    /// Copy bytes in, a page at a time.
    pub(super) fn write_bytes(&self, off: usize, data: &[u8]) {
        let mut done = 0;
        while done < data.len() {
            let o = off + done;
            let n = (4096 - o % 4096).min(data.len() - done);
            unsafe { core::ptr::copy_nonoverlapping(data.as_ptr().add(done), self.frames[o / 4096].1.add(o % 4096), n) };
            done += n;
        }
    }
    /// Copy bytes out, a page at a time.
    pub(super) fn read_bytes(&self, off: usize, out: &mut [u8]) {
        let mut done = 0;
        while done < out.len() {
            let o = off + done;
            let n = (4096 - o % 4096).min(out.len() - done);
            unsafe { core::ptr::copy_nonoverlapping(self.frames[o / 4096].1.add(o % 4096) as *const u8, out.as_mut_ptr().add(done), n) };
            done += n;
        }
    }
    /// Copy 32-bit words in, a page at a time (`off` 4-byte aligned).
    pub(super) fn write_words(&self, off: usize, words: &[u32]) {
        let mut done = 0;
        while done < words.len() {
            let o = off + done * 4;
            let n = ((4096 - o % 4096) / 4).min(words.len() - done);
            unsafe { core::ptr::copy_nonoverlapping(words.as_ptr().add(done), self.frames[o / 4096].1.add(o % 4096) as *mut u32, n) };
            done += n;
        }
    }
    /// Copy 32-bit words out, a page at a time.
    pub(super) fn read_words(&self, off: usize, out: &mut [u32]) {
        let mut done = 0;
        while done < out.len() {
            let o = off + done * 4;
            let n = ((4096 - o % 4096) / 4).min(out.len() - done);
            unsafe { core::ptr::copy_nonoverlapping(self.frames[o / 4096].1.add(o % 4096) as *const u32, out.as_mut_ptr().add(done), n) };
            done += n;
        }
    }
    /// Clean + invalidate only the pages covering `[off, off + len)`.
    pub(super) fn sync_range(&self, off: usize, len: usize) {
        if len == 0 {
            return;
        }
        for p in off / 4096..=(off + len - 1) / 4096 {
            if let Some(&(_, va)) = self.frames.get(p) {
                crate::arch::cpu::dma_sync(va as u64, 4096);
            }
        }
    }
    pub(super) fn word(&self, off: usize) -> u32 {
        unsafe { (self.frames[off / 4096].1.add(off % 4096) as *const u32).read_volatile() }
    }
    pub(super) fn fill(&self, v: u32) {
        for &(_, va) in &self.frames {
            for k in 0..1024 {
                unsafe { (va as *mut u32).add(k).write_volatile(v) };
            }
        }
    }
    /// Clean + invalidate the CPU caches (before the GPU reads, after it wrote).
    pub(super) fn sync(&self) {
        for &(_, va) in &self.frames {
            crate::arch::cpu::dma_sync(va as u64, 4096);
        }
    }
}

impl Gpu {
    pub(super) fn mmu_flush(&self) -> bool {
        w(self.hub, MMUC_CONTROL, MMUC_FLUSH | MMUC_ENABLE);
        let a = wait_us(100_000, || r(self.hub, MMUC_CONTROL) & MMUC_FLUSHING == 0);
        w(self.hub, MMU_CTL, r(self.hub, MMU_CTL) | MMU_TLB_CLEAR);
        let b = wait_us(100_000, || r(self.hub, MMU_CTL) & MMU_TLB_CLEARING == 0);
        a && b
    }

    /// Page table + scratch page, MMU on (`v3d_mmu_set_page_table`).
    pub(super) fn bring_up(&mut self, out: &mut Vec<String>) -> bool {
        if self.mmu.is_some() {
            return true;
        }
        let dbg = r(self.hub, MMU_DEBUG_INFO);
        let pa_bits = 30 + ((dbg >> 8) & 0xF);
        out.push(format!("  MMU: version {}, VA {} bits, PA {} bits", dbg & 0xF, 30 + ((dbg >> 4) & 0xF), pa_bits));
        let Some((pt_phys, pt_va)) = crate::memory::alloc_dma_contiguous(PT_PAGES) else {
            out.push(String::from("  no 4 MiB of contiguous memory for the page table"));
            return false;
        };
        if pt_phys + (PT_PAGES as u64) * 4096 > 1u64 << pa_bits.min(40) {
            out.push(format!("  page table at {:#x} is beyond the GPU's {} address bits", pt_phys, pa_bits));
            return false;
        }
        unsafe { core::ptr::write_bytes(pt_va, 0, PT_PAGES * 4096) };
        crate::arch::cpu::dma_sync(pt_va as u64, PT_PAGES * 4096);
        let Some((scratch, _)) = crate::memory::alloc_dma_frame() else { return false };
        let flushed = self.program_mmu(pt_phys, scratch);
        out.push(format!(
            "  page table at {:#x} (4 MiB), MMU_CTL {:#010x}, MMU flush {}",
            pt_phys,
            r(self.hub, MMU_CTL),
            if flushed { "ok" } else { "TIMED OUT" }
        ));
        self.mmu = Some(Mmu { pt: pt_va as *mut u32, pt_phys, scratch, pa_bits });
        true
    }

    /// Point the MMU at the page table, set the core's cache flush range and
    /// interrupt mask (after bring-up and after every reset).
    fn program_mmu(&self, pt_phys: u64, scratch: u64) -> bool {
        w(self.hub, MMU_PT_PA_BASE, (pt_phys >> 12) as u32);
        // enable; abort + interrupt on invalid PTEs, write violations, cap exceeded
        w(self.hub, MMU_CTL, 1 | (1 << 16) | (1 << 18) | (1 << 19) | (1 << 10) | (1 << 11) | (1 << 25) | (1 << 26));
        w(self.hub, MMU_ILLEGAL_ADDR, (scratch >> 12) as u32 | (1 << 31));
        w(self.hub, MMUC_CONTROL, MMUC_ENABLE);
        let flushed = self.mmu_flush();
        // whole-cache L2T flushes; core interrupts unmasked (the GIC does not
        // route the V3D line -- completion is polled in INT_STS)
        w(self.core, CTL_L2TFLSTA, 0);
        w(self.core, CTL_L2TFLEND, !0);
        w(self.core, CTL_INT_CLR, 0xFF);
        w(self.core, CTL_INT_MSK_CLR, INT_FRDONE | INT_FLDONE | INT_OUTOMEM | INT_GMPV | INT_CSDDONE);
        flushed
    }

    /// Recover from a hung or faulted job: hardware reset, then the MMU again
    /// (the page table in memory is unchanged, so every buffer stays mapped).
    pub(super) fn reset(&mut self) -> bool {
        RESETS.fetch_add(1, Ordering::Relaxed);
        let bridges = hw_reset();
        let mmu = match self.mmu.as_ref() {
            Some(m) => self.program_mmu(m.pt_phys, m.scratch),
            None => true,
        };
        bridges && mmu
    }

    /// Error / debug registers for a dump (ERRSTAT, FDBGO/B/R/S).
    pub(super) fn debug_regs(&self) -> String {
        format!(
            "ERRSTAT {:#x} FDBG O {:#x} B {:#x} R {:#x} S {:#x}",
            r(self.core, 0xf20),
            r(self.core, 0xf04),
            r(self.core, 0xf08),
            r(self.core, 0xf0c),
            r(self.core, 0xf10)
        )
    }

    /// `pages` fresh zeroed frames mapped at consecutive GPU addresses.
    pub(super) fn alloc(&mut self, pages: usize) -> Option<Buffer> {
        let mmu = self.mmu.as_ref()?;
        let limit = 1u64 << mmu.pa_bits.min(40);
        let first = self.next_page;
        if first as usize + pages >= PT_PAGES * 1024 {
            return None;
        }
        let mut frames: Vec<(u64, *mut u8)> = Vec::with_capacity(pages);
        for _ in 0..pages {
            match crate::memory::alloc_dma_frame() {
                Some(f) if f.0 + 4096 <= limit => frames.push(f),
                other => {
                    if let Some(f) = other {
                        crate::memory::free_frame(f.0);
                    }
                    for &(p, _) in &frames {
                        crate::memory::free_frame(p);
                    }
                    return None;
                }
            }
        }
        self.next_page += pages as u32;
        for (k, &(phys, _)) in frames.iter().enumerate() {
            unsafe { mmu.pt.add(first as usize + k).write_volatile((phys >> 12) as u32 | PTE_WRITEABLE | PTE_VALID) };
        }
        crate::arch::cpu::dma_sync(unsafe { mmu.pt.add(first as usize) } as u64, pages * 4);
        self.mmu_flush();
        let b = Buffer { va: first << 12, frames };
        b.sync();
        Some(b)
    }

    /// Map `pages` of existing, physically contiguous memory (not owned:
    /// a staging surface or the framebuffer) at consecutive GPU addresses.
    pub(super) fn map_contig(&mut self, phys: u64, pages: usize) -> Option<u32> {
        let mmu = self.mmu.as_ref()?;
        if phys + (pages as u64) * 4096 > 1u64 << mmu.pa_bits.min(40) {
            return None;
        }
        let first = self.next_page;
        if first as usize + pages >= PT_PAGES * 1024 {
            return None;
        }
        self.next_page += pages as u32;
        for k in 0..pages {
            unsafe { mmu.pt.add(first as usize + k).write_volatile(((phys >> 12) + k as u64) as u32 | PTE_WRITEABLE | PTE_VALID) };
        }
        crate::arch::cpu::dma_sync(unsafe { mmu.pt.add(first as usize) } as u64, pages * 4);
        self.mmu_flush();
        Some(first << 12)
    }

    /// Unmap and free a buffer (the test's buffers are freed in reverse order,
    /// so the GPU address space is reused).
    pub(super) fn free(&mut self, b: Buffer) {
        let Some(mmu) = self.mmu.as_ref() else { return };
        let first = (b.va >> 12) as usize;
        for k in 0..b.frames.len() {
            unsafe { mmu.pt.add(first + k).write_volatile(0) };
        }
        crate::arch::cpu::dma_sync(unsafe { mmu.pt.add(first) } as u64, b.frames.len() * 4);
        self.mmu_flush();
        if self.next_page == (b.va >> 12) + b.frames.len() as u32 {
            self.next_page = b.va >> 12;
        }
        for &(phys, _) in &b.frames {
            crate::memory::free_frame(phys);
        }
    }

    /// Run a render control list `[start, end)` on CT1 and wait for the frame.
    /// Returns the GPU time in microseconds.
    pub(super) fn run_render(&mut self, start: u32, end: u32, out: &mut Vec<String>) -> Option<u64> {
        // flush L2T and invalidate the slice caches (`v3d_invalidate_caches`)
        w(self.core, CTL_L2TCACTL, 1);
        w(self.core, CTL_SLCACTL, 0x0F0F_0F0F);
        w(self.core, CTL_INT_CLR, 0xFF);
        let rfc0 = r(self.core, CLE_RFC);
        let t0 = crate::arch::cpu::timestamp();
        w(self.core, CLE_CT1QBA, start);
        w(self.core, CLE_CT1QEA, end); // writing the end address starts the job
        let ok = wait_us(500_000, || r(self.core, CTL_INT_STS) & (INT_FRDONE | INT_GMPV | INT_OUTOMEM) != 0 || r(self.core, CLE_RFC) != rfc0);
        let us = elapsed_us(t0);
        let sts = r(self.core, CTL_INT_STS);
        if !ok || sts & (INT_GMPV | INT_OUTOMEM) != 0 {
            out.push(format!(
                "  job {}: INT_STS {:#x} HUB_INT {:#x} CT1CS {:#x} CT1CA {:#x} CT1EA {:#x} PCS {:#x} RFC {} -> {} MMU_CTL {:#x} VIO addr {:#x} id {:#x}",
                if ok { "FAULTED" } else { "TIMED OUT (500 ms)" },
                sts,
                r(self.hub, HUB_INT_STS),
                r(self.core, CLE_CT1CS),
                r(self.core, CLE_CT1CA),
                r(self.core, CLE_CT1EA),
                r(self.core, CLE_PCS),
                rfc0,
                r(self.core, CLE_RFC),
                r(self.hub, MMU_CTL),
                r(self.hub, MMU_VIO_ADDR),
                r(self.hub, MMU_VIO_ID)
            ));
            out.push(format!("  {}", self.debug_regs()));
            let ok = self.reset();
            out.push(format!("  V3D reset {}", if ok { "done" } else { "INCOMPLETE (bridges did not answer)" }));
            return None;
        }
        w(self.core, CTL_INT_CLR, 0xFF);
        Some(us)
    }
}

/// Control-list bytes: packets are an opcode byte + little-endian bit fields.
pub(super) struct Cl {
    pub(super) bytes: Vec<u8>,
}

impl Cl {
    pub(super) fn op(&mut self, code: u8, body: &[u8]) {
        self.bytes.push(code);
        self.bytes.extend_from_slice(body);
    }
    pub(super) fn op64(&mut self, code: u8, v: u64) {
        self.op(code, &v.to_le_bytes());
    }
    /// STORE_TILE_BUFFER_GENERAL (V3D 4.1+): low 64 bits of fields + address.
    pub(super) fn store(&mut self, fields: u64, address: u32) {
        let mut b = [0u8; 12];
        b[..8].copy_from_slice(&fields.to_le_bytes());
        b[8..].copy_from_slice(&address.to_le_bytes());
        self.op(29, &b);
    }
}

/// Render control list (+ its generic tile list, placed at GPU address
/// `generic`) that clears a `w` x `h` RGBA8 raster image at GPU address
/// `image` (pitch `stride` bytes) to `rgba` (red in the low byte).
/// `tile_list` = a zeroed 64-byte-aligned block for the tile list base.
fn clear_rcl(w: u32, h: u32, image: u32, stride: u32, rgba: u32, tile_list: u32, generic: u32) -> (Vec<u8>, Vec<u8>) {
    // 32 bpp, one render target, no MSAA: 64x64 tiles
    let tiles_x = w.div_ceil(64) as u64;
    let tiles_y = h.div_ceil(64) as u64;
    let mut g = Cl { bytes: Vec::new() };
    g.op(125, &[]); // TILE_COORDINATES_IMPLICIT
    g.op(26, &[]); // END_OF_LOADS
    // RT0, memory format raster (0), output rgba8 (27), clear the buffer
    // being stored, stride in bytes at bit 28
    g.store((27 << 12) | (1 << 18) | ((stride as u64) << 28), image);
    g.op(27, &[]); // END_OF_TILE_MARKER
    g.op(18, &[]); // RETURN_FROM_SUB_LIST
    let gen_end = generic + g.bytes.len() as u32;

    let mut c = Cl { bytes: Vec::new() };
    // TILE_RENDERING_MODE_CFG (common, sub-id 0): 1 RT (minus one = 0),
    // width, height, max bpp 32 (0), early Z disabled
    c.op64(121, ((w as u64) << 8) | ((h as u64) << 24) | (1 << 46));
    // clear colours part 1 (sub-id 3), RT0: the low 32 bits = RGBA8
    c.op64(121, 3 | ((rgba as u64) << 8));
    // colour config (sub-id 1): RT0 32 bpp (0), internal type "8" (2), no clamp
    c.op64(121, 1 | (2 << 6));
    // ZS clear values (sub-id 2): Z 1.0, stencil 0 -- the last config packet
    c.op64(121, 2 | (0x3F80_0000u64 << 16));
    c.op(126, &[1 << 2]); // TILE_LIST_INITIAL_BLOCK_SIZE: auto-chained, 64 B blocks
    c.op(123, &tile_list.to_le_bytes()); // MULTICORE_RENDERING_TILE_LIST_SET_BASE, set 0
    // MULTICORE_RENDERING_SUPERTILE_CFG: 1x1-tile supertiles (minus one = 0),
    // frame size in supertiles and in tiles, 1 bin tile list (minus one = 0)
    c.op64(122, (tiles_x << 16) | (tiles_y << 24) | (tiles_x << 32) | (tiles_y << 44));
    // GFXH-1742: two dummy tiles; the first clears the tile buffers
    for i in 0..2 {
        c.op(124, &[0, 0, 0]); // TILE_COORDINATES 0, 0
        c.op(26, &[]); // END_OF_LOADS
        c.store(8, 0); // store buffer "none"
        if i == 0 {
            c.op(25, &[0b11]); // CLEAR_TILE_BUFFERS: Z/S + all render targets
        }
        c.op(27, &[]); // END_OF_TILE_MARKER
    }
    c.op(19, &[]); // FLUSH_VCD_CACHE
    let mut gl = [0u8; 8];
    gl[..4].copy_from_slice(&generic.to_le_bytes());
    gl[4..].copy_from_slice(&gen_end.to_le_bytes());
    c.op(20, &gl); // START_ADDRESS_OF_GENERIC_TILE_LIST
    for y in 0..tiles_y {
        for x in 0..tiles_x {
            c.op(23, &[x as u8, y as u8]); // SUPERTILE_COORDINATES column, row
        }
    }
    c.op(13, &[]); // END_OF_RENDERING
    (c.bytes, g.bytes)
}

/// Shell `gpu v3d`: MMU bring-up and two render-only clear jobs, checked
/// pixel by pixel and shown at the top right of the screen.
pub fn self_test() -> Vec<String> {
    let mut out = Vec::new();
    let mut guard = GPU.lock();
    let Some(gpu) = guard.as_mut() else {
        out.push(String::from("gpu v3d: no V3D found (a Raspberry Pi 4 has one; QEMU raspi4b does not)"));
        let st = STATUS.lock().clone();
        if !st.is_empty() {
            out.push(format!("  boot: {}", st));
        }
        return out;
    };
    out.push(format!("V3D {}.{} self-test (step 2a: MMU + render-only clear jobs; 2b/2c: 3D)", version() / 10, version() % 10));
    if !gpu.bring_up(&mut out) {
        out.push(String::from("  the MMU did not come up; nothing ran"));
        return out;
    }
    const W: u32 = 256;
    const H: u32 = 128;
    let stride = W * 4;
    let cl = gpu.alloc(1);
    let scratch = gpu.alloc(1);
    let image = gpu.alloc((stride * H) as usize / 4096);
    let (Some(cl), Some(scratch), Some(image)) = (cl, scratch, image) else {
        out.push(String::from("  out of GPU-reachable memory"));
        return out;
    };
    out.push(format!("  GPU addresses: control list {:#x}, tile list {:#x}, image {:#x}", cl.va, scratch.va, image.va));
    image.fill(0xDEAD_BEEF);
    image.sync();
    let mut passed = 0;
    // job 1: the whole image leaf green; job 2: the top-left quarter amber,
    // a smaller frame with the same pitch (the rest must stay green)
    let jobs = [(W, H, 0xFF7C_B04Fu32), (W / 2, H / 2, 0xFF1E_A0F0u32)];
    for (n, &(w, h, rgba)) in jobs.iter().enumerate() {
        let generic = cl.va + 0x800;
        let (rcl, gl) = clear_rcl(w, h, image.va, stride, rgba, scratch.va, generic);
        cl.write(0, &rcl);
        cl.write(0x800, &gl);
        cl.sync();
        let Some(us) = gpu.run_render(cl.va, cl.va + rcl.len() as u32, &mut out) else { break };
        image.sync();
        let mut bad = 0u32;
        let mut first_bad = None;
        for y in 0..H {
            for x in 0..W {
                let want = if n >= 1 && x < W / 2 && y < H / 2 { jobs[1].2 } else { jobs[0].2 };
                let got = image.word((y * stride + x * 4) as usize);
                if got != want {
                    bad += 1;
                    first_bad.get_or_insert((x, y, got, want));
                }
            }
        }
        match first_bad {
            None => {
                passed += 1;
                out.push(format!("  job {}: {}x{} cleared to {:#010x} in {} us ({} RCL bytes): every pixel ok", n + 1, w, h, rgba, us, rcl.len()));
            }
            Some((x, y, got, want)) => {
                out.push(format!("  job {}: done in {} us, {} pixel(s) wrong, first at {},{}: {:#010x} (want {:#010x})", n + 1, us, bad, x, y, got, want))
            }
        }
    }
    let shown = show(&image, W, H, stride);
    out.push(format!("  {}/2 jobs correct{}", passed, if shown { "; the image was shown at the top right for 4 s" } else { "" }));
    if passed == 2 {
        crate::println!("[kernel] MILESTONE: V3D RENDER OK (GPU MMU + render control list on the Pi's V3D)");
        out.push(String::from("  MILESTONE: V3D RENDER OK"));
    }
    gpu.free(image);
    gpu.free(scratch);
    gpu.free(cl);
    drop(guard);
    // steps 2b / 2c: binning + shaders, colour, depth, texture
    out.push(String::from("V3D 3D (steps 2b/2c: binning job, QPU shaders, varyings, depth, texture)"));
    let (pass, total) = super::v3d_draw::self_test(&mut out);
    out.push(format!("  {}/{} 3D checks correct", pass, total));
    if pass == total {
        crate::println!("[kernel] MILESTONE: V3D 3D OK (binning + QPU shaders + depth + texture on the Pi's V3D)");
        out.push(String::from("  MILESTONE: V3D 3D OK"));
    }
    out.extend(super::v3d_draw::report());
    out
}

/// Copy the RGBA8 image to the top right of the screen, keep it 4 s, then
/// let the compositor redraw the desktop.
fn show(image: &Buffer, w: u32, h: u32, stride: u32) -> bool {
    let Some(fb) = crate::boot::info().framebuffer else { return false };
    if fb.bytes_per_pixel != 4 || fb.width < w as usize + 16 || fb.height < h as usize + 16 {
        return false;
    }
    let x0 = fb.width - w as usize - 16;
    for y in 0..h as usize {
        for x in 0..w as usize {
            let p = image.word(y * stride as usize + x * 4);
            let (rr, gg, bb) = (p & 0xFF, (p >> 8) & 0xFF, (p >> 16) & 0xFF);
            let v = match fb.format {
                crate::boot::PixelFormat::Rgb => rr | gg << 8 | bb << 16,
                _ => bb | gg << 8 | rr << 16,
            };
            unsafe { ((fb.addr + (((16 + y) * fb.stride + x0 + x) * 4) as u64) as *mut u32).write_volatile(v) };
        }
    }
    wait_us(4_000_000, || false);
    crate::framebuffer::forget_screen();
    crate::wm::screen_overwritten();
    true
}
