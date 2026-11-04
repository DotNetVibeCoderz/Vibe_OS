//! Intel e1000 family network cards (82540EM and friends: what QEMU, VirtualBox
//! and VMware emulate, and many older PCs carry; 82574L too) as a
//! `net::NetDevice`: polled, legacy descriptors, 32 receive + 16 transmit
//! buffers of 2 KiB in DMA pages. Newer on-board Intel chips (I217/I219) need
//! more than this and are reported, not driven.

use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};

use crate::net::{MacAddr, NetDevice};
use crate::pci::PciDevice;

const CTRL: usize = 0x0000;
const CTRL_ASDE: u32 = 1 << 5;
const CTRL_SLU: u32 = 1 << 6;
const CTRL_RST: u32 = 1 << 26;
const STATUS: usize = 0x0008;
const STATUS_LU: u32 = 1 << 1;
const EERD: usize = 0x0014;
const ICR: usize = 0x00C0;
const IMC: usize = 0x00D8;
const RCTL: usize = 0x0100;
const RCTL_EN: u32 = 1 << 1;
const RCTL_BAM: u32 = 1 << 15; // accept broadcast (a DHCP reply, ARP)
const RCTL_SECRC: u32 = 1 << 26;
const TCTL: usize = 0x0400;
const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const TIPG: usize = 0x0410;
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
const MTA: usize = 0x5200;
const RAL: usize = 0x5400;
const RAH: usize = 0x5404;

const RX_DESCS: usize = 32;
const TX_DESCS: usize = 16;
const BUF: usize = 2048;
const DD: u8 = 1;
const CMD_EOP: u8 = 1;
const CMD_IFCS: u8 = 2;
const CMD_RS: u8 = 8;
/// Polls of a register or a descriptor before giving up.
const WAIT: u32 = 2_000_000;

const SUPPORTED: [u16; 8] = [0x100E, 0x100F, 0x1004, 0x1010, 0x1011, 0x1026, 0x1076, 0x10D3];

pub struct E1000 {
    regs: *mut u8,
    mac: MacAddr,
    rx_ring: *mut u8,
    tx_ring: *mut u8,
    rx_bufs: Vec<(u64, *mut u8)>,
    tx_bufs: Vec<(u64, *mut u8)>,
    rx_next: usize,
    tx_next: usize,
}

// The registers and DMA pages belong to this driver instance.
unsafe impl Send for E1000 {}

impl E1000 {
    fn rd(&self, off: usize) -> u32 {
        unsafe { read_volatile(self.regs.add(off) as *const u32) }
    }
    fn wr(&self, off: usize, v: u32) {
        unsafe { write_volatile(self.regs.add(off) as *mut u32, v) }
    }

    fn read_eeprom(&self, word: u32) -> Option<u16> {
        self.wr(EERD, 1 | word << 8);
        for _ in 0..WAIT {
            let v = self.rd(EERD);
            if v & (1 << 4) != 0 {
                return Some((v >> 16) as u16);
            }
            core::hint::spin_loop();
        }
        None
    }

    pub fn link_up(&self) -> bool {
        self.rd(STATUS) & STATUS_LU != 0
    }
}

/// A buffer page cut into 2 KiB buffers.
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

fn bring_up(dev: &PciDevice) -> Result<E1000, &'static str> {
    let bar = dev.mem_bar(0).ok_or("no memory BAR")?;
    dev.enable_mem_and_bus_master();
    let va = crate::memory::map_mmio(bar, 0x20000).ok_or("cannot map the registers")?;
    let (rx_phys, rx_ring) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
    let (tx_phys, tx_ring) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
    let rx_bufs = buffers(RX_DESCS).ok_or("out of memory")?;
    let tx_bufs = buffers(TX_DESCS).ok_or("out of memory")?;
    let mut n = E1000 { regs: va as *mut u8, mac: [0; 6], rx_ring, tx_ring, rx_bufs, tx_bufs, rx_next: 0, tx_next: 0 };

    n.wr(IMC, u32::MAX);
    n.wr(CTRL, n.rd(CTRL) | CTRL_RST);
    let mut reset = false;
    for _ in 0..WAIT {
        if n.rd(CTRL) & CTRL_RST == 0 {
            reset = true;
            break;
        }
        core::hint::spin_loop();
    }
    if !reset {
        return Err("reset did not finish");
    }
    n.wr(IMC, u32::MAX);
    let _ = n.rd(ICR);
    n.wr(CTRL, n.rd(CTRL) | CTRL_SLU | CTRL_ASDE);

    // The MAC address: receive address 0 (loaded from the EEPROM), else the EEPROM.
    let (lo, hi) = (n.rd(RAL), n.rd(RAH));
    n.mac = [lo as u8, (lo >> 8) as u8, (lo >> 16) as u8, (lo >> 24) as u8, hi as u8, (hi >> 8) as u8];
    if n.mac == [0; 6] || n.mac == [0xFF; 6] {
        for w in 0..3u32 {
            let v = n.read_eeprom(w).ok_or("no MAC address")?;
            n.mac[w as usize * 2] = v as u8;
            n.mac[w as usize * 2 + 1] = (v >> 8) as u8;
        }
        n.wr(RAL, u32::from_le_bytes([n.mac[0], n.mac[1], n.mac[2], n.mac[3]]));
        n.wr(RAH, n.mac[4] as u32 | (n.mac[5] as u32) << 8 | 1 << 31);
    }
    for i in 0..128 {
        n.wr(MTA + i * 4, 0);
    }

    // Receive: every descriptor owned by the card (tail = last).
    for (i, b) in n.rx_bufs.iter().enumerate() {
        unsafe { write_volatile(rx_ring.add(i * 16) as *mut u64, b.0) };
    }
    n.wr(RDBAL, rx_phys as u32);
    n.wr(RDBAH, (rx_phys >> 32) as u32);
    n.wr(RDLEN, (RX_DESCS * 16) as u32);
    n.wr(RDH, 0);
    n.wr(RDT, RX_DESCS as u32 - 1);
    n.wr(RCTL, RCTL_EN | RCTL_BAM | RCTL_SECRC); // 2 KiB buffers (BSIZE 0)

    n.wr(TDBAL, tx_phys as u32);
    n.wr(TDBAH, (tx_phys >> 32) as u32);
    n.wr(TDLEN, (TX_DESCS * 16) as u32);
    n.wr(TDH, 0);
    n.wr(TDT, 0);
    n.wr(TIPG, 0x0060_200A);
    n.wr(TCTL, TCTL_EN | TCTL_PSP | 0x10 << 4 | 0x40 << 12);
    Ok(n)
}

/// The first supported card on the bus.
pub fn probe(devices: &[PciDevice]) -> Option<E1000> {
    for dev in devices.iter().filter(|d| d.vendor_id == 0x8086 && d.class == 0x02) {
        if !SUPPORTED.contains(&dev.device_id) {
            crate::println!("[e1000] Intel network card 8086:{:04x}: no driver for this model", dev.device_id);
            continue;
        }
        match bring_up(dev) {
            Ok(n) => {
                let m = n.mac;
                crate::println!(
                    "[e1000] 8086:{:04x}: MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}, link {}",
                    dev.device_id,
                    m[0],
                    m[1],
                    m[2],
                    m[3],
                    m[4],
                    m[5],
                    if n.link_up() { "up" } else { "down" }
                );
                return Some(n);
            }
            Err(e) => crate::println!("[e1000] 8086:{:04x}: {}", dev.device_id, e),
        }
    }
    None
}

impl NetDevice for E1000 {
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
            write_volatile(d.add(11), CMD_EOP | CMD_IFCS | CMD_RS);
            write_volatile(d.add(12), 0);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            self.tx_next = (i + 1) % TX_DESCS;
            self.wr(TDT, self.tx_next as u32);
            for _ in 0..WAIT {
                if read_volatile(d.add(12)) & DD != 0 {
                    return;
                }
                core::hint::spin_loop();
            }
        }
    }

    fn receive(&mut self) -> Option<Vec<u8>> {
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
        if errors != 0 || status & 2 == 0 {
            return self.receive(); // a bad frame, or part of one: skip it
        }
        Some(out)
    }
}
