//! Intel I217 / I218 / I219 ("PCH LAN": the Ethernet built into Intel chipsets
//! since Haswell, e.g. the I219-LM of a Dell Latitude 7290) as a
//! `net::NetDevice`. The MAC is an e1000 relative -- same registers, same
//! legacy descriptors -- but its PHY sits behind the chipset and is usually
//! left asleep: Windows and the firmware put it into Ultra Low Power mode and
//! SMBus mode, where MDIO does not answer. Bring-up follows Linux's e1000e
//! (`ich8lan.c`): leave ULP (through the Management Engine when it runs, else
//! by hand), toggle LANPHYPC, unforce SMBus, reset PHY and MAC, wait for the
//! LAN init, restart auto-negotiation. It cannot be tested under QEMU (no
//! model of this chip), so every step logs what it saw.

use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};

use crate::net::{MacAddr, NetDevice};
use crate::pci::PciDevice;

const CTRL: usize = 0x0000;
const CTRL_SLU: u32 = 1 << 6;
const CTRL_FRCSPD: u32 = 1 << 11;
const CTRL_FRCDPX: u32 = 1 << 12;
const CTRL_LANPHYPC_OVERRIDE: u32 = 1 << 16;
const CTRL_LANPHYPC_VALUE: u32 = 1 << 17;
const CTRL_RST: u32 = 1 << 26;
const CTRL_PHY_RST: u32 = 1 << 31;
const STATUS: usize = 0x0008;
const STATUS_LU: u32 = 1 << 1;
const STATUS_LAN_INIT_DONE: u32 = 1 << 9;
const CTRL_EXT: usize = 0x0018;
const CTRL_EXT_LPCD: u32 = 1 << 2;
const CTRL_EXT_FORCE_SMBUS: u32 = 1 << 11;
const MDIC: usize = 0x0020;
const FEXTNVM3: usize = 0x003C;
const FEXTNVM7: usize = 0x00E4;
const FEXTNVM7_DISABLE_SMB_PERST: u32 = 0x20;
const ICR: usize = 0x00C0;
const IMC: usize = 0x00D8;
const RCTL: usize = 0x0100;
const RCTL_EN: u32 = 1 << 1;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;
const TCTL: usize = 0x0400;
const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const TIPG: usize = 0x0410;
const EXTCNF_CTRL: usize = 0x0F00;
const EXTCNF_SWFLAG: u32 = 1 << 5;
const RDBAL: usize = 0x2800;
const RDBAH: usize = 0x2804;
const RDLEN: usize = 0x2808;
const RDH: usize = 0x2810;
const RDT: usize = 0x2818;
const TDBAL: usize = 0x3800;
const TDBAH: usize = 0x3804;
const TDLEN: usize = 0x3808;
const TDH: usize = 0x3810;
const TDT: usize = 0x3818;
const TXDCTL: usize = 0x3828;
const TARC0: usize = 0x3840;
const MTA: usize = 0x5200;
const RAL: usize = 0x5400;
const RAH: usize = 0x5404;
const H2ME: usize = 0x5B50;
const H2ME_ULP: u32 = 0x0800;
const H2ME_ENFORCE_SETTINGS: u32 = 0x1000;
const FWSM: usize = 0x5B54;
const FWSM_FW_VALID: u32 = 0x8000;
const FWSM_ULP_CFG_DONE: u32 = 0x0400;
const FEXTNVM11: usize = 0x5BBC;
const FEXTNVM11_DISABLE_MULR_FIX: u32 = 0x2000;

/// PHY registers: (page, register). Pages >= 768 live at PHY address 1.
const PHY_ID1: (u32, u32) = (0, 2);
const PHY_ID2: (u32, u32) = (0, 3);
const PHY_BMCR: (u32, u32) = (0, 0);
const PHY_BMSR: (u32, u32) = (0, 1);
const CV_SMB_CTRL: (u32, u32) = (769, 23);
const CV_SMB_CTRL_FORCE_SMBUS: u16 = 0x0001;
const I218_ULP_CONFIG1: (u32, u32) = (779, 16);

const RX_DESCS: usize = 32;
const TX_DESCS: usize = 16;
const BUF: usize = 2048;
const DD: u8 = 1;

/// I217, I218 and I219 (Lynx Point to Alder Point). From Ice Lake (0x15f*,
/// 0x0d5*, 0x1a1*, ...) the chips also boot like this, with extra care Linux
/// takes that this driver may lack: they are tried and report what happens.
const SUPPORTED: [u16; 34] = [
    0x153A, 0x153B, 0x155A, 0x1559, 0x15A0, 0x15A1, 0x15A2, 0x15A3, // I217/I218
    0x156F, 0x1570, 0x15B7, 0x15B8, 0x15B9, 0x15D6, 0x15D7, 0x15D8, 0x15E3, // I219 SPT/KBP
    0x15BB, 0x15BC, 0x15BD, 0x15BE, 0x15DF, 0x15E0, 0x15E1, 0x15E2, // CNP
    0x0D4C, 0x0D4D, 0x0D4E, 0x0D4F, 0x0D53, 0x0D55, // CMP
    0x15F9, 0x15FA, 0x15FB,
];

fn delay_us(us: u64) {
    let hz = crate::arch::cpu::counter_hz().max(1_000_000_000);
    let t0 = crate::arch::cpu::timestamp();
    while crate::arch::cpu::timestamp().wrapping_sub(t0) < us * hz / 1_000_000 {
        core::hint::spin_loop();
    }
}

pub struct E1000e {
    regs: *mut u8,
    mac: MacAddr,
    rx_ring: *mut u8,
    tx_ring: *mut u8,
    rx_bufs: Vec<(u64, *mut u8)>,
    tx_bufs: Vec<(u64, *mut u8)>,
    rx_next: usize,
    tx_next: usize,
    me: bool,
}

// The registers and DMA pages belong to this driver instance.
unsafe impl Send for E1000e {}

impl E1000e {
    fn rd(&self, o: usize) -> u32 {
        unsafe { read_volatile(self.regs.add(o) as *const u32) }
    }
    fn wr(&self, o: usize, v: u32) {
        unsafe { write_volatile(self.regs.add(o) as *mut u32, v) }
    }
    fn flush(&self) {
        let _ = self.rd(STATUS);
    }

    /// The software flag that keeps the Management Engine off the PHY.
    fn acquire(&self) -> bool {
        for _ in 0..100 {
            self.wr(EXTCNF_CTRL, self.rd(EXTCNF_CTRL) | EXTCNF_SWFLAG);
            if self.rd(EXTCNF_CTRL) & EXTCNF_SWFLAG != 0 {
                return true;
            }
            delay_us(10_000);
        }
        false
    }
    fn release(&self) {
        self.wr(EXTCNF_CTRL, self.rd(EXTCNF_CTRL) & !EXTCNF_SWFLAG);
    }

    fn mdic(&self, addr: u32, reg: u32, write: Option<u16>) -> Option<u16> {
        let op = if write.is_some() { 1 } else { 2 };
        self.wr(MDIC, write.unwrap_or(0) as u32 | (reg & 0x1F) << 16 | addr << 21 | op << 26);
        for _ in 0..640 {
            delay_us(50);
            let v = self.rd(MDIC);
            if v & (1 << 28) != 0 {
                return if v & (1 << 30) != 0 { None } else { Some(v as u16) };
            }
        }
        None
    }

    /// A PHY register (page, register), through the page select.
    fn phy(&self, (page, reg): (u32, u32), write: Option<u16>) -> Option<u16> {
        let addr = if page >= 768 { 1 } else { 2 };
        if page > 0 {
            self.mdic(addr, 31, Some((page << 5) as u16))?;
        }
        let r = self.mdic(addr, reg, write);
        if page > 0 {
            let _ = self.mdic(addr, 31, Some(0));
        }
        r
    }

    fn phy_id(&self) -> Option<u32> {
        let a = self.phy(PHY_ID1, None)?;
        let b = self.phy(PHY_ID2, None)?;
        let id = (a as u32) << 16 | b as u32;
        (id != 0 && id != 0xFFFF_FFFF).then_some(id)
    }

    /// Pull LANPHYPC low for a moment: powers the PHY up from ULP.
    fn toggle_lanphypc(&self) {
        // PHY config counter: 50 ms (bits 28:26 of FEXTNVM3 = 1)
        self.wr(FEXTNVM3, (self.rd(FEXTNVM3) & !(7 << 26)) | 1 << 26);
        self.wr(CTRL, (self.rd(CTRL) | CTRL_LANPHYPC_OVERRIDE) & !CTRL_LANPHYPC_VALUE);
        self.flush();
        delay_us(10);
        self.wr(CTRL, self.rd(CTRL) & !CTRL_LANPHYPC_OVERRIDE);
        self.flush();
        for _ in 0..20 {
            if self.rd(CTRL_EXT) & CTRL_EXT_LPCD != 0 {
                break;
            }
            delay_us(1000);
        }
        delay_us(30_000);
    }

    /// Leave Ultra Low Power mode (what Windows/the firmware left the PHY in).
    fn disable_ulp(&self) -> &'static str {
        if self.me {
            // the Management Engine owns the PHY: ask it
            self.wr(H2ME, (self.rd(H2ME) & !H2ME_ULP) | H2ME_ENFORCE_SETTINGS);
            let mut done = false;
            for _ in 0..250 {
                if self.rd(FWSM) & FWSM_ULP_CFG_DONE == 0 {
                    done = true;
                    break;
                }
                delay_us(10_000);
            }
            self.wr(H2ME, self.rd(H2ME) & !H2ME_ENFORCE_SETTINGS);
            return if done { "via the Management Engine" } else { "ME did not confirm" };
        }
        self.toggle_lanphypc();
        if !self.acquire() {
            return "no PHY semaphore";
        }
        // unforce SMBus in the PHY (if it does not answer, in the MAC first)
        let mut smb = self.phy(CV_SMB_CTRL, None);
        if smb.is_none() {
            self.wr(CTRL_EXT, self.rd(CTRL_EXT) & !CTRL_EXT_FORCE_SMBUS);
            delay_us(50_000);
            smb = self.phy(CV_SMB_CTRL, None);
        }
        if let Some(v) = smb {
            let _ = self.phy(CV_SMB_CTRL, Some(v & !CV_SMB_CTRL_FORCE_SMBUS));
        }
        self.wr(CTRL_EXT, self.rd(CTRL_EXT) & !CTRL_EXT_FORCE_SMBUS);
        if let Some(v) = self.phy(I218_ULP_CONFIG1, None) {
            // clear the ULP settings, then commit with START
            let v = v & !(0x0004 | 0x0010 | 0x0020 | 0x0040 | 0x0100 | 0x0400 | 0x0800 | 0x1000);
            let _ = self.phy(I218_ULP_CONFIG1, Some(v));
            let _ = self.phy(I218_ULP_CONFIG1, Some(v | 0x0001));
        }
        self.wr(FEXTNVM7, self.rd(FEXTNVM7) & !FEXTNVM7_DISABLE_SMB_PERST);
        self.release();
        if smb.is_some() { "by hand" } else { "by hand (PHY silent)" }
    }

    fn phy_reset(&self) {
        if self.acquire() {
            self.wr(CTRL, self.rd(CTRL) | CTRL_PHY_RST);
            self.flush();
            delay_us(100);
            self.wr(CTRL, self.rd(CTRL) & !CTRL_PHY_RST);
            self.flush();
            self.release();
        }
        delay_us(20_000);
        self.wr(CTRL_EXT, self.rd(CTRL_EXT) & !CTRL_EXT_FORCE_SMBUS);
    }

    pub fn link_up(&self) -> bool {
        self.rd(STATUS) & STATUS_LU != 0
    }
}

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

fn bring_up(dev: &PciDevice) -> Result<E1000e, &'static str> {
    let bar = dev.mem_bar(0).ok_or("no memory BAR")?;
    dev.enable_mem_and_bus_master();
    let va = crate::memory::map_mmio(bar, 0x20000).ok_or("cannot map the registers")?;
    let (rx_phys, rx_ring) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
    let (tx_phys, tx_ring) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
    let mut n = E1000e {
        regs: va as *mut u8,
        mac: [0; 6],
        rx_ring,
        tx_ring,
        rx_bufs: buffers(RX_DESCS).ok_or("out of memory")?,
        tx_bufs: buffers(TX_DESCS).ok_or("out of memory")?,
        rx_next: 0,
        tx_next: 0,
        me: false,
    };
    n.wr(IMC, u32::MAX);
    n.me = n.rd(FWSM) & FWSM_FW_VALID != 0;
    let id0 = n.phy_id();
    crate::println!(
        "[e1000e] 8086:{:04x}: Management Engine {}, PHY {}, STATUS {:#010x}, CTRL_EXT {:#010x}",
        dev.device_id,
        if n.me { "active" } else { "off" },
        id0.map_or(alloc::string::String::from("asleep"), |i| alloc::format!("{:#010x}", i)),
        n.rd(STATUS),
        n.rd(CTRL_EXT)
    );

    let ulp = n.disable_ulp();
    if n.phy_id().is_none() {
        n.toggle_lanphypc();
        n.wr(CTRL_EXT, n.rd(CTRL_EXT) & !CTRL_EXT_FORCE_SMBUS);
    }
    n.phy_reset();

    // MAC reset (the PHY stays up), then the chip reloads its NVM
    n.wr(RCTL, 0);
    n.wr(TCTL, TCTL_PSP);
    n.flush();
    delay_us(10_000);
    let swflag = n.acquire();
    n.wr(CTRL, n.rd(CTRL) | CTRL_RST);
    delay_us(20_000);
    if swflag {
        n.release();
    }
    for _ in 0..150 {
        if n.rd(STATUS) & STATUS_LAN_INIT_DONE != 0 {
            break;
        }
        delay_us(1000);
    }
    n.wr(STATUS, n.rd(STATUS) & !STATUS_LAN_INIT_DONE);
    n.wr(IMC, u32::MAX);
    let _ = n.rd(ICR);
    n.wr(CTRL_EXT, n.rd(CTRL_EXT) & !CTRL_EXT_FORCE_SMBUS);
    let id = n.phy_id();
    crate::println!(
        "[e1000e] ULP exit {}; after reset: PHY {}",
        ulp,
        id.map_or(alloc::string::String::from("still silent"), |i| alloc::format!("{:#010x}", i))
    );
    if id.is_none() {
        return Err("the PHY does not answer after reset (ULP/SMBus)");
    }

    // e1000e's per-chip bits (ich8lan: initialize_hw_bits) + the SPT fix
    n.wr(TXDCTL, n.rd(TXDCTL) | 1 << 22);
    n.wr(TARC0, n.rd(TARC0) | 1 << 23 | 1 << 24 | 1 << 26 | 1 << 27);
    n.wr(CTRL_EXT, n.rd(CTRL_EXT) | 1 << 22);
    n.wr(FEXTNVM11, n.rd(FEXTNVM11) | FEXTNVM11_DISABLE_MULR_FIX);

    let (lo, hi) = (n.rd(RAL), n.rd(RAH));
    n.mac = [lo as u8, (lo >> 8) as u8, (lo >> 16) as u8, (lo >> 24) as u8, hi as u8, (hi >> 8) as u8];
    if n.mac == [0; 6] || n.mac == [0xFF; 6] {
        return Err("no MAC address in receive address 0");
    }
    n.wr(RAH, hi | 1 << 31);
    for i in 0..32 {
        n.wr(MTA + i * 4, 0);
    }

    // link: auto-negotiation restarted on the PHY, the MAC follows it
    n.wr(CTRL, (n.rd(CTRL) | CTRL_SLU) & !(CTRL_FRCSPD | CTRL_FRCDPX));
    if n.acquire() {
        let _ = n.phy(PHY_BMCR, Some(0x1000 | 0x0200 | 0x0100 | 0x0040)); // autoneg on + restart, full duplex
        n.release();
    }

    for (i, b) in n.rx_bufs.iter().enumerate() {
        unsafe {
            write_volatile(rx_ring.add(i * 16) as *mut u64, b.0);
            write_volatile(rx_ring.add(i * 16 + 8) as *mut u64, 0);
        }
    }
    n.wr(RDBAL, rx_phys as u32);
    n.wr(RDBAH, (rx_phys >> 32) as u32);
    n.wr(RDLEN, (RX_DESCS * 16) as u32);
    n.wr(RDH, 0);
    n.wr(RDT, RX_DESCS as u32 - 1);
    n.wr(RCTL, RCTL_EN | RCTL_BAM | RCTL_SECRC);
    n.wr(TDBAL, tx_phys as u32);
    n.wr(TDBAH, (tx_phys >> 32) as u32);
    n.wr(TDLEN, (TX_DESCS * 16) as u32);
    n.wr(TDH, 0);
    n.wr(TDT, 0);
    n.wr(TIPG, 0x0060_2008);
    n.wr(TCTL, TCTL_EN | TCTL_PSP | 0x0F << 4 | 0x3F << 12);
    Ok(n)
}

/// The chipset's Ethernet, if there is one. Waits up to `link_ms` for a link
/// (auto-negotiation takes a few seconds).
pub fn probe(devices: &[PciDevice], link_ms: u64) -> Option<E1000e> {
    let dev = devices.iter().find(|d| d.vendor_id == 0x8086 && d.class == 0x02 && SUPPORTED.contains(&d.device_id))?;
    match bring_up(dev) {
        Ok(n) => {
            let t0 = crate::arch::cpu::timestamp();
            let hz = crate::arch::cpu::counter_hz().max(1);
            while !n.link_up() && crate::arch::cpu::timestamp().wrapping_sub(t0) < link_ms * hz / 1000 {
                crate::task::yield_now();
            }
            let m = n.mac;
            let bmsr = if n.acquire() {
                let v = n.phy(PHY_BMSR, None);
                n.release();
                v.unwrap_or(0)
            } else {
                0
            };
            crate::println!(
                "[e1000e] MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}, link {} (STATUS {:#010x}, PHY status {:#06x})",
                m[0],
                m[1],
                m[2],
                m[3],
                m[4],
                m[5],
                if n.link_up() { "up" } else { "down -- is a cable plugged in?" },
                n.rd(STATUS),
                bmsr
            );
            Some(n)
        }
        Err(e) => {
            crate::println!("[e1000e] 8086:{:04x}: {}", dev.device_id, e);
            None
        }
    }
}

impl NetDevice for E1000e {
    fn mac(&self) -> MacAddr {
        self.mac
    }

    fn transmit(&mut self, frame: &[u8]) {
        if frame.is_empty() || frame.len() > BUF {
            return;
        }
        let i = self.tx_next;
        let (phys, va) = self.tx_bufs[i];
        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), va, frame.len());
            let d = self.tx_ring.add(i * 16);
            write_volatile(d as *mut u64, phys);
            write_volatile(d.add(8) as *mut u16, frame.len() as u16);
            write_volatile(d.add(10), 0);
            write_volatile(d.add(11), 0x01 | 0x02 | 0x08); // EOP, IFCS, RS
            write_volatile(d.add(12), 0);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            self.tx_next = (i + 1) % TX_DESCS;
            self.wr(TDT, self.tx_next as u32);
            for _ in 0..2_000_000 {
                if read_volatile(d.add(12)) & DD != 0 {
                    return;
                }
                core::hint::spin_loop();
            }
        }
    }

    fn receive(&mut self) -> Option<Vec<u8>> {
        loop {
            let i = self.rx_next;
            let d = unsafe { self.rx_ring.add(i * 16) };
            let status = unsafe { read_volatile(d.add(12)) };
            if status & DD == 0 {
                return None;
            }
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            let len = unsafe { read_volatile(d.add(8) as *const u16) } as usize;
            let errors = unsafe { read_volatile(d.add(13)) };
            let mut out = alloc::vec![0u8; len.min(BUF)];
            unsafe { core::ptr::copy_nonoverlapping(self.rx_bufs[i].1, out.as_mut_ptr(), out.len()) };
            unsafe { write_volatile(d.add(12), 0) };
            self.wr(RDT, i as u32);
            self.rx_next = (i + 1) % RX_DESCS;
            if errors == 0 && status & 2 != 0 {
                return Some(out);
            }
        }
    }
}
