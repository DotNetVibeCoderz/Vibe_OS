//! Realtek network cards as `net::NetDevice`s, polled:
//!
//! - **RTL8139** (10ec:8139, what QEMU's `-device rtl8139` and many old PCs
//!   have): one 8 KiB receive ring the card writes packets into back to back,
//!   four transmit slots.
//! - **RTL8169 / 8168 / 8111 / 8101** (10ec:8161/8168/8169/8136, 1186:4300):
//!   the "C+" descriptor rings most on-board Realtek Gigabit chips use. QEMU
//!   has no model of these, so this path is **untested on hardware**; chips
//!   that need Linux's per-revision PHY fix-ups or firmware may stay silent
//!   (`ipconfig` then shows the status).

use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};

use crate::net::{MacAddr, NetDevice};
use crate::pci::PciDevice;

const WAIT: u32 = 2_000_000;

// Registers shared by both families.
const IDR0: usize = 0x00;
const MAR0: usize = 0x08;
const CR: usize = 0x37;
const CR_RST: u8 = 0x10;
const CR_RE: u8 = 0x08;
const CR_TE: u8 = 0x04;
const CR_BUFE: u8 = 0x01;
const IMR: usize = 0x3C;
const ISR: usize = 0x3E;
const TCR: usize = 0x40;
const RCR: usize = 0x44;
/// Accept all physical (promiscuous off: AAP stays clear), physical match,
/// multicast (IPv6 neighbour discovery), broadcast.
const RCR_APM_AM_AB: u32 = 0x2 | 0x4 | 0x8;

struct Regs(*mut u8);

impl Regs {
    fn r8(&self, o: usize) -> u8 {
        unsafe { read_volatile(self.0.add(o)) }
    }
    fn r16(&self, o: usize) -> u16 {
        unsafe { read_volatile(self.0.add(o) as *const u16) }
    }
    fn r32(&self, o: usize) -> u32 {
        unsafe { read_volatile(self.0.add(o) as *const u32) }
    }
    fn w8(&self, o: usize, v: u8) {
        unsafe { write_volatile(self.0.add(o), v) }
    }
    fn w16(&self, o: usize, v: u16) {
        unsafe { write_volatile(self.0.add(o) as *mut u16, v) }
    }
    fn w32(&self, o: usize, v: u32) {
        unsafe { write_volatile(self.0.add(o) as *mut u32, v) }
    }
    fn mac(&self) -> MacAddr {
        let mut m = [0u8; 6];
        for (i, b) in m.iter_mut().enumerate() {
            *b = self.r8(IDR0 + i);
        }
        m
    }
    fn reset(&self) -> bool {
        self.w8(CR, CR_RST);
        for _ in 0..WAIT {
            if self.r8(CR) & CR_RST == 0 {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }
    /// Every multicast group: the hash filter fully open.
    fn all_multicast(&self) {
        self.w32(MAR0, u32::MAX);
        self.w32(MAR0 + 4, u32::MAX);
    }
}

/// The registers of `dev`: its first memory BAR.
fn map_regs(dev: &PciDevice) -> Result<Regs, &'static str> {
    let bar = (0..6).find_map(|i| dev.mem_bar(i)).ok_or("no memory BAR")?;
    dev.enable_mem_and_bus_master();
    let va = crate::memory::map_mmio(bar, 0x1000).ok_or("cannot map the registers")?;
    Ok(Regs(va as *mut u8))
}

// ---------------------------------------------------------------- RTL8139

const TSD0: usize = 0x10;
const TSAD0: usize = 0x20;
const RBSTART: usize = 0x30;
const CAPR: usize = 0x38;
const CONFIG1: usize = 0x52;
const TSD_OWN: u32 = 1 << 13;
const TSD_TOK: u32 = 1 << 15;
const RCR_WRAP: u32 = 1 << 7;
const RX_RING: usize = 8192;
/// The ring, its 16-byte overrun pad and room for a frame written past the
/// end (WRAP mode).
const RX_ALLOC_PAGES: usize = 3;

pub struct Rtl8139 {
    regs: Regs,
    mac: MacAddr,
    rx: *mut u8,
    rx_off: usize,
    tx: [(u64, *mut u8); 4],
    tx_next: usize,
}

unsafe impl Send for Rtl8139 {}

fn rtl8139_up(dev: &PciDevice) -> Result<Rtl8139, &'static str> {
    let regs = map_regs(dev)?;
    regs.w8(CONFIG1, 0); // power on (LWAKE + LWPTN low)
    if !regs.reset() {
        return Err("reset did not finish");
    }
    let (rx_phys, rx) = crate::memory::alloc_dma_contiguous(RX_ALLOC_PAGES).ok_or("out of memory")?;
    if rx_phys >> 32 != 0 {
        return Err("receive buffer above 4 GiB");
    }
    unsafe { core::ptr::write_bytes(rx, 0, RX_ALLOC_PAGES * 4096) };
    let mut tx = [(0u64, core::ptr::null_mut()); 4];
    for slot in tx.iter_mut() {
        let (p, v) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
        if p >> 32 != 0 {
            return Err("transmit buffer above 4 GiB");
        }
        *slot = (p, v);
    }
    regs.w32(RBSTART, rx_phys as u32);
    regs.w16(IMR, 0);
    regs.w16(ISR, 0xFFFF);
    regs.all_multicast();
    regs.w8(CR, CR_RE | CR_TE);
    // 8 KiB ring (RBLEN 00), unlimited DMA burst, no FIFO threshold.
    regs.w32(RCR, RCR_APM_AM_AB | RCR_WRAP | 7 << 8 | 7 << 13);
    regs.w32(TCR, 3 << 24 | 6 << 8); // standard IFG, 1 KiB DMA burst
    let mac = regs.mac();
    Ok(Rtl8139 { regs, mac, rx, rx_off: 0, tx, tx_next: 0 })
}

impl NetDevice for Rtl8139 {
    fn mac(&self) -> MacAddr {
        self.mac
    }

    fn transmit(&mut self, frame: &[u8]) {
        if frame.is_empty() || frame.len() > 1792 {
            return;
        }
        let i = self.tx_next;
        let (phys, va) = self.tx[i];
        let len = frame.len().max(60); // the card does not pad short frames
        unsafe {
            core::ptr::write_bytes(va, 0, len);
            core::ptr::copy_nonoverlapping(frame.as_ptr(), va, frame.len());
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.regs.w32(TSAD0 + i * 4, phys as u32);
        self.regs.w32(TSD0 + i * 4, len as u32); // clearing OWN starts it
        self.tx_next = (i + 1) % 4;
        for _ in 0..WAIT {
            let s = self.regs.r32(TSD0 + i * 4);
            if s & (TSD_OWN | TSD_TOK) != 0 {
                return;
            }
            core::hint::spin_loop();
        }
    }

    fn receive(&mut self) -> Option<Vec<u8>> {
        if self.regs.r8(CR) & CR_BUFE != 0 {
            return None;
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        let at = unsafe { self.rx.add(self.rx_off) };
        let status = unsafe { read_volatile(at as *const u16) };
        let len = unsafe { read_volatile(at.add(2) as *const u16) } as usize;
        // the packet (CRC included) follows its 4-byte header; WRAP mode lets
        // it run past the ring's end into the pad
        let ok = status & 1 != 0 && (64..=1792).contains(&len);
        let out = if ok {
            let mut v = alloc::vec![0u8; len - 4];
            unsafe { core::ptr::copy_nonoverlapping(at.add(4), v.as_mut_ptr(), v.len()) };
            Some(v)
        } else {
            None
        };
        if !ok && (len == 0 || len > 1792) {
            // a broken header: the ring position is lost, start over
            self.regs.w8(CR, CR_TE);
            self.regs.w8(CR, CR_RE | CR_TE);
            self.rx_off = 0;
            self.regs.w16(CAPR, 0u16.wrapping_sub(16));
            return None;
        }
        self.rx_off = (self.rx_off + len + 4 + 3) & !3;
        self.rx_off %= RX_RING;
        self.regs.w16(CAPR, (self.rx_off as u16).wrapping_sub(16));
        self.regs.w16(ISR, 0xFFFF);
        match out {
            Some(v) => Some(v),
            None => self.receive(), // a bad frame: skip it
        }
    }
}

// ------------------------------------------------- RTL8169 / 8168 / 8111

const TNPDS: usize = 0x20;
const TPPOLL: usize = 0x38;
const TPPOLL_NPQ: u8 = 0x40;
const CFG9346: usize = 0x50;
const RMS: usize = 0xDA;
const CPLUS: usize = 0xE0;
const RDSAR: usize = 0xE4;
const MTPS: usize = 0xEC;
const PHY_STATUS: usize = 0x6C;
const OWN: u32 = 1 << 31;
const EOR: u32 = 1 << 30;
const FS: u32 = 1 << 29;
const LS: u32 = 1 << 28;
const RX_DESCS: usize = 32;
const TX_DESCS: usize = 16;
const BUF: usize = 2048;

pub struct Rtl8169 {
    regs: Regs,
    mac: MacAddr,
    rx_ring: *mut u8,
    tx_ring: *mut u8,
    rx_bufs: Vec<(u64, *mut u8)>,
    tx_bufs: Vec<(u64, *mut u8)>,
    rx_next: usize,
    tx_next: usize,
}

unsafe impl Send for Rtl8169 {}

fn buffers(count: usize) -> Option<Vec<(u64, *mut u8)>> {
    let mut out = Vec::with_capacity(count);
    while out.len() < count {
        let (phys, va) = crate::memory::alloc_dma_frame()?;
        for k in 0..4096 / BUF {
            out.push((phys + (k * BUF) as u64, unsafe { va.add(k * BUF) }));
        }
    }
    out.truncate(count);
    Some(out)
}

fn desc(ring: *mut u8, i: usize) -> *mut u32 {
    unsafe { ring.add(i * 16) as *mut u32 }
}

fn rtl8169_up(dev: &PciDevice) -> Result<Rtl8169, &'static str> {
    let regs = map_regs(dev)?;
    if !regs.reset() {
        return Err("reset did not finish");
    }
    let (rx_phys, rx_ring) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
    let (tx_phys, tx_ring) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
    let rx_bufs = buffers(RX_DESCS).ok_or("out of memory")?;
    let tx_bufs = buffers(TX_DESCS).ok_or("out of memory")?;
    for (i, b) in rx_bufs.iter().enumerate() {
        let eor = if i == RX_DESCS - 1 { EOR } else { 0 };
        unsafe {
            write_volatile(desc(rx_ring, i), OWN | eor | BUF as u32);
            write_volatile(desc(rx_ring, i).add(1), 0);
            write_volatile(desc(rx_ring, i).add(2) as *mut u64, b.0);
        }
    }
    unsafe { write_volatile(desc(tx_ring, TX_DESCS - 1), EOR) };

    regs.w8(CFG9346, 0xC0); // unlock the configuration registers
    regs.w16(CPLUS, regs.r16(CPLUS) & !(1 << 5 | 1 << 6)); // no RX checksum / VLAN offload
    regs.w16(RMS, BUF as u16);
    regs.w8(MTPS, 0x3B); // max transmit packet size, in 128-byte units
    regs.w32(TNPDS, tx_phys as u32);
    regs.w32(TNPDS + 4, (tx_phys >> 32) as u32);
    regs.w32(RDSAR, rx_phys as u32);
    regs.w32(RDSAR + 4, (rx_phys >> 32) as u32);
    regs.w8(CR, CR_RE | CR_TE);
    regs.w32(TCR, 3 << 24 | 7 << 8);
    regs.w32(RCR, RCR_APM_AM_AB | 7 << 8 | 7 << 13);
    regs.all_multicast();
    regs.w16(IMR, 0);
    regs.w16(ISR, 0xFFFF);
    regs.w8(CFG9346, 0x00);
    let mac = regs.mac();
    Ok(Rtl8169 { regs, mac, rx_ring, tx_ring, rx_bufs, tx_bufs, rx_next: 0, tx_next: 0 })
}

impl NetDevice for Rtl8169 {
    fn mac(&self) -> MacAddr {
        self.mac
    }

    fn transmit(&mut self, frame: &[u8]) {
        if frame.is_empty() || frame.len() > BUF {
            return;
        }
        let i = self.tx_next;
        let (phys, va) = self.tx_bufs[i];
        let len = frame.len().max(60);
        let eor = if i == TX_DESCS - 1 { EOR } else { 0 };
        unsafe {
            core::ptr::write_bytes(va, 0, len);
            core::ptr::copy_nonoverlapping(frame.as_ptr(), va, frame.len());
            let d = desc(self.tx_ring, i);
            write_volatile(d.add(2) as *mut u64, phys);
            write_volatile(d.add(1), 0);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            write_volatile(d, OWN | eor | FS | LS | len as u32);
        }
        self.tx_next = (i + 1) % TX_DESCS;
        self.regs.w8(TPPOLL, TPPOLL_NPQ);
        for _ in 0..WAIT {
            if unsafe { read_volatile(desc(self.tx_ring, i)) } & OWN == 0 {
                return;
            }
            core::hint::spin_loop();
        }
    }

    fn receive(&mut self) -> Option<Vec<u8>> {
        let i = self.rx_next;
        let d = desc(self.rx_ring, i);
        let opts = unsafe { read_volatile(d) };
        if opts & OWN != 0 {
            return None;
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        let len = (opts & 0x3FFF) as usize;
        let whole = opts & (FS | LS) == FS | LS && opts & (1 << 21) == 0; // not RES (error)
        let out = if whole && (64..=BUF).contains(&len) {
            let mut v = alloc::vec![0u8; len - 4];
            unsafe { core::ptr::copy_nonoverlapping(self.rx_bufs[i].1, v.as_mut_ptr(), v.len()) };
            Some(v)
        } else {
            None
        };
        let eor = if i == RX_DESCS - 1 { EOR } else { 0 };
        unsafe { write_volatile(d, OWN | eor | BUF as u32) };
        self.rx_next = (i + 1) % RX_DESCS;
        match out {
            Some(v) => Some(v),
            None => self.receive(),
        }
    }

    fn diag(&self) -> Option<alloc::string::String> {
        let phy = self.regs.r8(PHY_STATUS);
        Some(alloc::format!("link {}, PHY status {:#04x}", if phy & 2 != 0 { "up" } else { "down" }, phy))
    }
}

// ---------------------------------------------------------------- probing

const GIGABIT: [(u16, u16); 6] = [(0x10EC, 0x8161), (0x10EC, 0x8168), (0x10EC, 0x8169), (0x10EC, 0x8136), (0x10EC, 0x8167), (0x1186, 0x4300)];

/// The first Realtek card on the bus that comes up.
pub fn probe(devices: &[PciDevice]) -> Option<(alloc::boxed::Box<dyn NetDevice>, &'static str)> {
    for dev in devices.iter().filter(|d| d.class == 0x02) {
        let id = (dev.vendor_id, dev.device_id);
        let (result, name): (Result<alloc::boxed::Box<dyn NetDevice>, &str>, &'static str) = if id == (0x10EC, 0x8139) {
            (rtl8139_up(dev).map(|n| alloc::boxed::Box::new(n) as alloc::boxed::Box<dyn NetDevice>), "rtl8139")
        } else if GIGABIT.contains(&id) {
            (rtl8169_up(dev).map(|n| alloc::boxed::Box::new(n) as alloc::boxed::Box<dyn NetDevice>), "rtl8169")
        } else {
            continue;
        };
        match result {
            Ok(n) => {
                let m = n.mac();
                crate::println!(
                    "[{}] {:04x}:{:04x}: MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    name, id.0, id.1, m[0], m[1], m[2], m[3], m[4], m[5]
                );
                return Some((n, name));
            }
            Err(e) => crate::println!("[{}] {:04x}:{:04x}: {}", name, id.0, id.1, e),
        }
    }
    None
}
