//! The Raspberry Pi 4's Gigabit Ethernet MAC (`brcm,bcm2711-genet-v5`,
//! roadmap R4) and its external PHY (BCM54213PE on MDIO address 1).
//!
//! Modelled on the U-Boot and Linux `bcmgenet` drivers, polled, one queue
//! (the default ring 16): the DMA descriptors (256 per direction, 12 bytes:
//! length/status, address low, address high) live inside the MAC's register
//! block, so only the packet buffers are in RAM, and those go through
//! `arch::cpu::dma_sync` (the SoC bus does not snoop the CPU caches). The
//! receiver puts two bytes in front of each frame (RBUF_ALIGN_2B); transmit
//! appends the CRC in hardware. The PHY is reset, given the RGMII clock delays
//! the Device Tree's `phy-mode` asks for (`rgmii-rxid`: the PHY delays RX, the
//! MAC delays TX), auto-negotiates, and the MAC is set to the result.

use alloc::vec::Vec;

use crate::arch::cpu;
use crate::net::{MacAddr, NetDevice};

const SYS_PORT_CTRL: u64 = 0x0004;
const SYS_RBUF_FLUSH_CTRL: u64 = 0x0008;
const EXT_RGMII_OOB_CTRL: u64 = 0x008c;
const RBUF_CTRL: u64 = 0x0300;
const RBUF_TBUF_SIZE_CTRL: u64 = 0x03b4;
const UMAC_CMD: u64 = 0x0808;
const UMAC_MAC0: u64 = 0x080c;
const UMAC_MAC1: u64 = 0x0810;
const UMAC_MAX_FRAME_LEN: u64 = 0x0814;
const UMAC_TX_FLUSH: u64 = 0x0b34;
const UMAC_MIB_CTRL: u64 = 0x0d80;
const MDIO_CMD: u64 = 0x0e14;

const DESCS: usize = 256;
const DESC_SIZE: u64 = 12;
const RX_OFF: u64 = 0x2000;
const TX_OFF: u64 = 0x4000;
const RING_REGS: u64 = DESCS as u64 * DESC_SIZE; // ring registers follow the descriptors
const Q: u64 = 16; // the default queue
const RDMA_RING: u64 = RX_OFF + RING_REGS + Q * 0x40;
const TDMA_RING: u64 = TX_OFF + RING_REGS + Q * 0x40;
const RDMA_REGS: u64 = RX_OFF + RING_REGS + 17 * 0x40;
const TDMA_REGS: u64 = TX_OFF + RING_REGS + 17 * 0x40;
// ring registers
const RDMA_WRITE_PTR: u64 = RDMA_RING;
const RDMA_PROD_INDEX: u64 = RDMA_RING + 0x08;
const RDMA_CONS_INDEX: u64 = RDMA_RING + 0x0c;
const RDMA_XON_XOFF_THRESH: u64 = RDMA_RING + 0x28;
const RDMA_READ_PTR: u64 = RDMA_RING + 0x2c;
const TDMA_READ_PTR: u64 = TDMA_RING;
const TDMA_CONS_INDEX: u64 = TDMA_RING + 0x08;
const TDMA_PROD_INDEX: u64 = TDMA_RING + 0x0c;
const TDMA_FLOW_PERIOD: u64 = TDMA_RING + 0x28;
const TDMA_WRITE_PTR: u64 = TDMA_RING + 0x2c;
const RING_BUF_SIZE: u64 = 0x10;
const RING_START_ADDR: u64 = 0x14;
const RING_END_ADDR: u64 = 0x1c;
const RING_MBUF_DONE_THRESH: u64 = 0x24;
// DMA block registers
const DMA_RING_CFG: u64 = 0x00;
const DMA_CTRL: u64 = 0x04;
const DMA_SCB_BURST_SIZE: u64 = 0x0c;

const BUF_LEN: u32 = 2048;
const DMA_OWN: u32 = 0x8000;
const DMA_EOP: u32 = 0x4000;
const DMA_SOP: u32 = 0x2000;
const DMA_TX_APPEND_CRC: u32 = 0x0040;
const MAX_FRAME: u32 = 1536;

// PHY registers (IEEE 802.3 clause 22 + Broadcom shadows)
const BMCR: u32 = 0;
const BMSR: u32 = 1;
const PHYID1: u32 = 2;
const PHYID2: u32 = 3;
const ADVERTISE: u32 = 4;
const LPA: u32 = 5;
const CTRL1000: u32 = 9;
const STAT1000: u32 = 10;
const BCM_AUX_CTL: u32 = 0x18;
const BCM_SHADOW: u32 = 0x1c;

fn delay_us(us: u64) {
    let hz = cpu::counter_hz().max(1);
    let t0 = cpu::timestamp();
    while cpu::timestamp() - t0 < us * hz / 1_000_000 {
        core::hint::spin_loop();
    }
}

struct Buf {
    phys: u64,
    va: *mut u8,
}

pub struct Genet {
    base: u64,
    mac: MacAddr,
    rx: Vec<Buf>,
    tx: Buf,
    rx_idx: usize,
    rx_cons: u32,
    tx_idx: usize,
    pub speed: u32,
    pub full_duplex: bool,
    pub rx_frames: u64,
    pub tx_frames: u64,
}

unsafe impl Send for Genet {}

impl Genet {
    fn r(&self, off: u64) -> u32 {
        unsafe { ((self.base + off) as *const u32).read_volatile() }
    }
    fn w(&self, off: u64, v: u32) {
        unsafe { ((self.base + off) as *mut u32).write_volatile(v) }
    }
    fn set(&self, off: u64, bits: u32) {
        self.w(off, self.r(off) | bits);
    }
    fn clear(&self, off: u64, bits: u32) {
        self.w(off, self.r(off) & !bits);
    }

    fn mdio(&self, cmd: u32) -> Option<u32> {
        self.w(MDIO_CMD, cmd);
        self.set(MDIO_CMD, 1 << 29); // start
        for _ in 0..2000 {
            let v = self.r(MDIO_CMD);
            if v & (1 << 29) == 0 {
                return if v & (1 << 28) != 0 { None } else { Some(v & 0xFFFF) };
            }
            delay_us(10);
        }
        None
    }
    fn phy_read(&self, phy: u32, reg: u32) -> Option<u32> {
        self.mdio((2 << 26) | (phy << 21) | (reg << 16))
    }
    fn phy_write(&self, phy: u32, reg: u32, v: u32) {
        let _ = self.mdio((1 << 26) | (phy << 21) | (reg << 16) | (v & 0xFFFF));
    }

    /// Bring up the MAC at `base` (mapped) with address `mac`; `phy` = MDIO
    /// address of the PHY, `rx_delay_in_phy` / `tx_delay_in_phy` from the DT
    /// `phy-mode`. Fails when the PHY does not answer.
    pub fn new(base: u64, mac: MacAddr, phy: u32, rx_delay_in_phy: bool, tx_delay_in_phy: bool) -> Result<Genet, &'static str> {
        let tx = alloc_buf()?;
        let mut rx = Vec::with_capacity(DESCS);
        for _ in 0..DESCS / 2 {
            let b = alloc_buf()?;
            rx.push(Buf { phys: b.phys + BUF_LEN as u64, va: unsafe { b.va.add(BUF_LEN as usize) } });
            rx.insert(rx.len() - 1, b);
        }
        let mut g = Genet { base, mac, rx, tx, rx_idx: 0, rx_cons: 0, tx_idx: 0, speed: 0, full_duplex: true, rx_frames: 0, tx_frames: 0 };
        let rev = g.r(0);
        if rev == 0 || rev == 0xFFFF_FFFF {
            return Err("GENET: no MAC at that address");
        }

        // External gigabit PHY over RGMII.
        g.w(SYS_PORT_CTRL, 3);
        // UniMAC reset (as U-Boot's bcmgenet_umac_reset).
        g.set(SYS_RBUF_FLUSH_CTRL, 1 << 1);
        delay_us(10);
        g.clear(SYS_RBUF_FLUSH_CTRL, 1 << 1);
        delay_us(10);
        g.w(SYS_RBUF_FLUSH_CTRL, 0);
        delay_us(10);
        g.w(UMAC_CMD, 0);
        g.w(UMAC_CMD, (1 << 13) | (1 << 15)); // software reset, local loopback
        delay_us(2);
        g.w(UMAC_CMD, 0);
        g.w(UMAC_MIB_CTRL, 0b111);
        g.w(UMAC_MIB_CTRL, 0);
        g.w(UMAC_MAX_FRAME_LEN, MAX_FRAME);
        g.set(RBUF_CTRL, 1 << 1); // 2-byte receive alignment
        g.w(RBUF_TBUF_SIZE_CTRL, 1);
        // Station address.
        g.w(UMAC_MAC0, u32::from_be_bytes([mac[0], mac[1], mac[2], mac[3]]));
        g.w(UMAC_MAC1, u32::from_be_bytes([0, 0, mac[4], mac[5]]));

        // DMA off, transmit FIFO flushed.
        g.clear(TDMA_REGS + DMA_CTRL, 1);
        g.clear(RDMA_REGS + DMA_CTRL, 1);
        g.w(UMAC_TX_FLUSH, 1);
        delay_us(10);
        g.w(UMAC_TX_FLUSH, 0);

        // Receive ring 16: every descriptor owns a 2 KiB buffer.
        g.w(RDMA_REGS + DMA_SCB_BURST_SIZE, 8);
        g.w(RDMA_RING + RING_START_ADDR, 0);
        g.w(RDMA_READ_PTR, 0);
        g.w(RDMA_WRITE_PTR, 0);
        g.w(RDMA_RING + RING_END_ADDR, (DESCS as u64 * DESC_SIZE / 4 - 1) as u32);
        g.rx_cons = g.r(RDMA_PROD_INDEX) & 0xFFFF; // PROD cannot be reset: follow it
        g.w(RDMA_CONS_INDEX, g.rx_cons);
        g.w(RDMA_RING + RING_BUF_SIZE, ((DESCS as u32) << 16) | BUF_LEN);
        g.w(RDMA_XON_XOFF_THRESH, (5 << 16) | (DESCS as u32 >> 4));
        g.w(RDMA_REGS + DMA_RING_CFG, 1 << Q);
        for (i, b) in g.rx.iter().enumerate() {
            let d = RX_OFF + i as u64 * DESC_SIZE;
            g.w(d + 4, b.phys as u32);
            g.w(d + 8, (b.phys >> 32) as u32);
            g.w(d, (BUF_LEN << 16) | DMA_OWN);
        }
        // Transmit ring 16.
        g.w(TDMA_REGS + DMA_SCB_BURST_SIZE, 8);
        g.w(TDMA_RING + RING_START_ADDR, 0);
        g.w(TDMA_READ_PTR, 0);
        g.w(TDMA_WRITE_PTR, 0);
        g.w(TDMA_RING + RING_END_ADDR, (DESCS as u64 * DESC_SIZE / 4 - 1) as u32);
        let cons = g.r(TDMA_CONS_INDEX) & 0xFFFF; // CONS cannot be reset: follow it
        g.w(TDMA_PROD_INDEX, cons);
        g.tx_idx = (cons as usize) % DESCS;
        g.w(TDMA_RING + RING_MBUF_DONE_THRESH, 1);
        g.w(TDMA_FLOW_PERIOD, 0);
        g.w(TDMA_RING + RING_BUF_SIZE, ((DESCS as u32) << 16) | BUF_LEN);
        g.w(TDMA_REGS + DMA_RING_CFG, 1 << Q);
        // Ring 16 + DMA on, both directions.
        g.set(RDMA_REGS + DMA_CTRL, (1 << (Q + 1)) | 1);
        g.set(TDMA_REGS + DMA_CTRL, (1 << (Q + 1)) | 1);

        g.phy_up(phy, rx_delay_in_phy, tx_delay_in_phy)?;
        Ok(g)
    }

    fn phy_up(&mut self, phy: u32, rx_delay: bool, tx_delay: bool) -> Result<(), &'static str> {
        let id1 = self.phy_read(phy, PHYID1).ok_or("GENET: the PHY does not answer on MDIO")?;
        let id2 = self.phy_read(phy, PHYID2).unwrap_or(0);
        if id1 == 0xFFFF || id1 == 0 {
            return Err("GENET: no PHY at that MDIO address");
        }
        self.phy_write(phy, BMCR, 1 << 15); // reset
        for _ in 0..100 {
            delay_us(1000);
            if self.phy_read(phy, BMCR).map_or(false, |v| v & (1 << 15) == 0) {
                break;
            }
        }
        // RGMII clock delays (Broadcom: RXC-RXD skew in the AUX "misc" shadow,
        // GTXCLK delay in shadow 0x03), as Linux's bcm54xx_config_clock_delay.
        self.phy_write(phy, BCM_AUX_CTL, 0x7 | (0x7 << 12));
        let mut misc = self.phy_read(phy, BCM_AUX_CTL).unwrap_or(0) | 0x8000;
        if rx_delay {
            misc |= 1 << 8;
        } else {
            misc &= !(1 << 8);
        }
        self.phy_write(phy, BCM_AUX_CTL, 0x7 | misc);
        self.phy_write(phy, BCM_SHADOW, 0x3 << 10);
        let mut clk = self.phy_read(phy, BCM_SHADOW).unwrap_or(0) & 0x3FF;
        if tx_delay {
            clk |= 1 << 9;
        } else {
            clk &= !(1 << 9);
        }
        self.phy_write(phy, BCM_SHADOW, 0x8000 | (0x3 << 10) | clk);
        // Advertise 10/100/1000, restart auto-negotiation.
        self.phy_write(phy, ADVERTISE, 0x01E1);
        self.phy_write(phy, CTRL1000, 0x0300);
        self.phy_write(phy, BMCR, (1 << 12) | (1 << 9));
        crate::println!("[genet] PHY {:04x}:{:04x} at MDIO {}: auto-negotiating", id1, id2, phy);
        let mut up = false;
        for _ in 0..60 {
            delay_us(100_000);
            let _ = self.phy_read(phy, BMSR); // latched-low link bit: read twice
            if self.phy_read(phy, BMSR).map_or(false, |s| s & 0x24 == 0x24) {
                up = true;
                break;
            }
        }
        if !up {
            return Err("GENET: no Ethernet link (is the cable plugged in?)");
        }
        let lpa = self.phy_read(phy, LPA).unwrap_or(0);
        let stat1000 = self.phy_read(phy, STAT1000).unwrap_or(0);
        let (speed, code, full) = if stat1000 & (1 << 11) != 0 {
            (1000, 2, true)
        } else if stat1000 & (1 << 10) != 0 {
            (1000, 2, false)
        } else if lpa & (1 << 8) != 0 {
            (100, 1, true)
        } else if lpa & (1 << 7) != 0 {
            (100, 1, false)
        } else {
            (10, 0, lpa & (1 << 6) != 0)
        };
        self.speed = speed;
        self.full_duplex = full;
        // MAC side of the link: RGMII on, link up, TX delay in the MAC unless the PHY does it.
        let mut oob = self.r(EXT_RGMII_OOB_CTRL);
        oob &= !(1 << 5); // OOB_DISABLE
        oob |= (1 << 4) | (1 << 6); // RGMII_LINK, RGMII_MODE_EN
        if tx_delay {
            oob |= 1 << 16; // ID_MODE_DIS
        } else {
            oob &= !(1 << 16);
        }
        self.w(EXT_RGMII_OOB_CTRL, oob);
        self.w(UMAC_CMD, (code << 2) | if full { 0 } else { 1 << 10 });
        self.set(UMAC_CMD, 0b11); // TX + RX enable
        Ok(())
    }
}

fn alloc_buf() -> Result<Buf, &'static str> {
    let (phys, va) = crate::memory::alloc_dma_frame().ok_or("GENET: out of memory")?;
    cpu::dma_sync(va as u64, 4096);
    Ok(Buf { phys, va })
}

impl NetDevice for Genet {
    fn mac(&self) -> MacAddr {
        self.mac
    }

    fn transmit(&mut self, frame: &[u8]) {
        let len = frame.len().min(MAX_FRAME as usize - 4);
        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), self.tx.va, len);
            if len < 60 {
                core::ptr::write_bytes(self.tx.va.add(len), 0, 60 - len); // minimum frame
            }
        }
        let len = len.max(60);
        cpu::dma_sync(self.tx.va as u64, len);
        let d = TX_OFF + self.tx_idx as u64 * DESC_SIZE;
        self.w(d + 4, self.tx.phys as u32);
        self.w(d + 8, (self.tx.phys >> 32) as u32);
        self.w(d, ((len as u32) << 16) | (0x3F << 7) | DMA_TX_APPEND_CRC | DMA_SOP | DMA_EOP);
        self.tx_idx = (self.tx_idx + 1) % DESCS;
        let prod = (self.r(TDMA_PROD_INDEX) + 1) & 0xFFFF;
        self.w(TDMA_PROD_INDEX, prod);
        // one buffer: wait until it has gone out
        for _ in 0..10_000 {
            if self.r(TDMA_CONS_INDEX) & 0xFFFF == prod {
                self.tx_frames += 1;
                return;
            }
            delay_us(1);
        }
    }

    fn receive(&mut self) -> Option<alloc::vec::Vec<u8>> {
        loop {
            let prod = self.r(RDMA_PROD_INDEX) & 0xFFFF;
            if prod == self.rx_cons {
                return None;
            }
            let d = RX_OFF + self.rx_idx as u64 * DESC_SIZE;
            let ls = self.r(d);
            let len = ((ls >> 16) & 0xFFF) as usize;
            let b = &self.rx[self.rx_idx];
            cpu::dma_sync(b.va as u64, len.min(BUF_LEN as usize));
            let frame = if ls & (DMA_SOP | DMA_EOP) == (DMA_SOP | DMA_EOP) && len > 2 + 14 {
                let mut v = alloc::vec![0u8; len - 2];
                unsafe { core::ptr::copy_nonoverlapping(b.va.add(2), v.as_mut_ptr(), len - 2) };
                Some(v)
            } else {
                None
            };
            self.rx_cons = (self.rx_cons + 1) & 0xFFFF;
            self.w(RDMA_CONS_INDEX, self.rx_cons);
            self.rx_idx = (self.rx_idx + 1) % DESCS;
            if let Some(f) = frame {
                self.rx_frames += 1;
                return Some(f);
            }
        }
    }
}
