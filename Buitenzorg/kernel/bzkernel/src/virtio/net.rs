//! virtio-net (§5.1): an Ethernet NIC for the kernel's network stack
//! (`net::NetDevice`). Queue 0 receives, queue 1 transmits; every frame is
//! preceded by a 12-byte `virtio_net_hdr` (all zero: no offloads negotiated).
//! Receive buffers (2 KiB each, two per DMA page) stay posted on queue 0 and
//! are re-posted as frames are taken.

use alloc::{boxed::Box, vec::Vec};

use super::{Buf, Transport, Virtqueue};
use crate::net::{MacAddr, NetDevice};

const F_MAC: u64 = 1 << 5;
const HDR: usize = 12;
const BUF: usize = 2048;
const RX_BUFS: usize = 16;

pub struct VirtioNet {
    t: Box<dyn Transport>,
    rx: Virtqueue,
    tx: Virtqueue,
    mac: MacAddr,
    /// (physical, virtual) of each receive buffer.
    rx_bufs: Vec<(u64, *mut u8)>,
    slot_of: [u16; 256],
    tx_phys: u64,
    tx_buf: *mut u8,
    pub sent: u64,
    pub received: u64,
}

unsafe impl Send for VirtioNet {}

impl VirtioNet {
    fn new(mut t: Box<dyn Transport>) -> Result<VirtioNet, &'static str> {
        let features = super::begin_init(t.as_mut(), F_MAC)?;
        let rx = Virtqueue::new(t.as_mut(), 0)?;
        let tx = Virtqueue::new(t.as_mut(), 1)?;
        super::finish_init(t.as_mut());
        let mut mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        if features & F_MAC != 0 {
            for (i, m) in mac.iter_mut().enumerate() {
                *m = t.config_read8(i);
            }
        }
        let mut rx_bufs = Vec::new();
        for _ in 0..RX_BUFS / 2 {
            let (p, v) = crate::memory::alloc_dma_frame().ok_or("virtio-net: out of memory")?;
            rx_bufs.push((p, v));
            rx_bufs.push((p + BUF as u64, unsafe { v.add(BUF) }));
        }
        let (tx_phys, tx_buf) = crate::memory::alloc_dma_frame().ok_or("virtio-net: out of memory")?;
        let mut n = VirtioNet { t, rx, tx, mac, rx_bufs, slot_of: [0; 256], tx_phys, tx_buf, sent: 0, received: 0 };
        for i in 0..RX_BUFS {
            n.post_rx(i)?;
        }
        n.t.notify(0);
        crate::println!(
            "[virtio] net at {}: MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}, {} receive buffers",
            n.t.location(),
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5],
            RX_BUFS
        );
        Ok(n)
    }

    fn post_rx(&mut self, i: usize) -> Result<(), &'static str> {
        let (phys, _) = self.rx_bufs[i];
        let head = self.rx.push(&[Buf { phys, len: BUF as u32, device_writes: true }])?;
        self.slot_of[head as usize] = i as u16;
        Ok(())
    }
}

impl NetDevice for VirtioNet {
    fn mac(&self) -> MacAddr {
        self.mac
    }

    fn transmit(&mut self, frame: &[u8]) {
        let n = frame.len().min(BUF * 2 - HDR);
        unsafe {
            core::ptr::write_bytes(self.tx_buf, 0, HDR);
            core::ptr::copy_nonoverlapping(frame.as_ptr(), self.tx_buf.add(HDR), n);
        }
        let buf = Buf { phys: self.tx_phys, len: (HDR + n) as u32, device_writes: false };
        if self.tx.submit_wait(self.t.as_ref(), &[buf]).is_ok() {
            self.sent += 1;
        }
    }

    fn receive(&mut self) -> Option<Vec<u8>> {
        let (head, len) = self.rx.pop_used()?;
        let i = self.slot_of[head as usize] as usize;
        let (_, virt) = self.rx_bufs[i];
        let len = (len as usize).min(BUF);
        let frame = if len > HDR {
            unsafe { core::slice::from_raw_parts(virt.add(HDR), len - HDR) }.to_vec()
        } else {
            Vec::new()
        };
        if self.post_rx(i).is_ok() {
            self.t.notify(0);
        }
        self.received += 1;
        Some(frame)
    }
}

/// The first virtio NIC, if the machine has one.
pub fn probe() -> Option<VirtioNet> {
    for t in super::find(super::device::NET) {
        let loc = t.location();
        match VirtioNet::new(t) {
            Ok(n) => return Some(n),
            Err(e) => crate::println!("[virtio] net at {}: {}", loc, e),
        }
    }
    None
}
