//! Networking, early (v0.5 "Dahan": "networking awal"). A minimal protocol
//! stack — Ethernet + ARP + IPv4 + ICMP echo — driven over a loopback device.
//! This exercises real frame parsing/building end to end without needing a
//! hardware NIC driver yet (e1000 is later roadmap work); the same
//! `NetDevice` trait will back the e1000 when it lands.

use alloc::collections::VecDeque;
use alloc::{vec, vec::Vec};
use spin::Mutex;

pub const ETH_ALEN: usize = 6;
pub type MacAddr = [u8; ETH_ALEN];
pub type Ipv4Addr = [u8; 4];

const ETHERTYPE_ARP: u16 = 0x0806;
const ETHERTYPE_IPV4: u16 = 0x0800;
const IPPROTO_ICMP: u8 = 1;

/// A link-layer device the stack can transmit through. Received frames are
/// delivered back to the stack by the poller.
pub trait NetDevice: Send {
    fn mac(&self) -> MacAddr;
    fn transmit(&mut self, frame: &[u8]);
    fn receive(&mut self) -> Option<Vec<u8>>;
}

/// Loopback: every transmitted frame is queued straight back for receive.
pub struct Loopback {
    mac: MacAddr,
    queue: VecDeque<Vec<u8>>,
}

impl Loopback {
    pub fn new() -> Self {
        Self {
            mac: [0x02, 0x00, 0x00, 0x00, 0x00, 0x01],
            queue: VecDeque::new(),
        }
    }
}

impl NetDevice for Loopback {
    fn mac(&self) -> MacAddr {
        self.mac
    }
    fn transmit(&mut self, frame: &[u8]) {
        self.queue.push_back(frame.to_vec());
    }
    fn receive(&mut self) -> Option<Vec<u8>> {
        self.queue.pop_front()
    }
}

struct Stack {
    device: Loopback,
    ip: Ipv4Addr,
    arp: Vec<(Ipv4Addr, MacAddr)>,
    icmp_replies: u64,
    arp_replies: u64,
}

static STACK: Mutex<Option<Stack>> = Mutex::new(None);

/// Bring up the loopback interface with the given IPv4 address.
pub fn init(ip: Ipv4Addr) {
    let device = Loopback::new();
    let mac = device.mac();
    *STACK.lock() = Some(Stack {
        device,
        ip,
        arp: vec![(ip, mac)], // self entry
        icmp_replies: 0,
        arp_replies: 0,
    });
}

/// (icmp_replies, arp_replies) — for diagnostics/tests.
pub fn counters() -> (u64, u64) {
    STACK
        .lock()
        .as_ref()
        .map(|s| (s.icmp_replies, s.arp_replies))
        .unwrap_or((0, 0))
}

fn be16(b: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([b[o], b[o + 1]])
}

fn checksum16(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += be16(data, i) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Send an ICMP echo request from this host to `dest` (over loopback, `dest`
/// is our own IP). Returns immediately; [`poll`] drives the exchange.
pub fn send_ping(dest: Ipv4Addr, ident: u16, seq: u16) {
    let mut guard = STACK.lock();
    let Some(stack) = guard.as_mut() else { return };
    let src_mac = stack.device.mac();
    let dst_mac = stack
        .arp
        .iter()
        .find(|(ip, _)| *ip == dest)
        .map(|(_, m)| *m)
        .unwrap_or([0xFF; ETH_ALEN]);

    // ICMP echo request.
    let mut icmp = vec![8u8, 0, 0, 0, (ident >> 8) as u8, ident as u8, (seq >> 8) as u8, seq as u8];
    icmp.extend_from_slice(b"buitenzorg-dahan");
    let csum = checksum16(&icmp);
    icmp[2] = (csum >> 8) as u8;
    icmp[3] = csum as u8;

    let frame = build_ipv4_frame(src_mac, dst_mac, stack.ip, dest, IPPROTO_ICMP, &icmp);
    stack.device.transmit(&frame);
}

fn build_ipv4_frame(
    src_mac: MacAddr,
    dst_mac: MacAddr,
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    proto: u8,
    payload: &[u8],
) -> Vec<u8> {
    let total_len = 20 + payload.len();
    let mut ip = Vec::with_capacity(total_len);
    ip.extend_from_slice(&[0x45, 0x00]);
    ip.extend_from_slice(&(total_len as u16).to_be_bytes());
    ip.extend_from_slice(&[0, 0, 0x40, 0x00, 64, proto, 0, 0]); // id, flags, ttl, proto, csum=0
    ip.extend_from_slice(&src_ip);
    ip.extend_from_slice(&dst_ip);
    let csum = checksum16(&ip[..20]);
    ip[10] = (csum >> 8) as u8;
    ip[11] = csum as u8;
    ip.extend_from_slice(payload);

    let mut frame = Vec::with_capacity(14 + ip.len());
    frame.extend_from_slice(&dst_mac);
    frame.extend_from_slice(&src_mac);
    frame.extend_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
    frame.extend_from_slice(&ip);
    frame
}

/// Drive the stack: process all currently queued received frames. Returns the
/// number of frames handled.
pub fn poll() -> usize {
    let mut handled = 0;
    loop {
        let frame = {
            let mut guard = STACK.lock();
            let Some(stack) = guard.as_mut() else { return handled };
            stack.device.receive()
        };
        let Some(frame) = frame else { break };
        handle_frame(&frame);
        handled += 1;
    }
    handled
}

fn handle_frame(frame: &[u8]) {
    if frame.len() < 14 {
        return;
    }
    let ethertype = be16(frame, 12);
    match ethertype {
        ETHERTYPE_ARP => handle_arp(&frame[14..]),
        ETHERTYPE_IPV4 => handle_ipv4(frame, &frame[14..]),
        _ => {}
    }
}

fn handle_arp(arp: &[u8]) {
    if arp.len() < 28 {
        return;
    }
    let oper = be16(arp, 6);
    if oper != 1 {
        return; // only replies to requests
    }
    let mut guard = STACK.lock();
    let Some(stack) = guard.as_mut() else { return };
    let target_ip: Ipv4Addr = [arp[24], arp[25], arp[26], arp[27]];
    if target_ip != stack.ip {
        return;
    }
    // Build ARP reply (sender/target swapped, our MAC as sender hw).
    let sender_mac: MacAddr = [arp[8], arp[9], arp[10], arp[11], arp[12], arp[13]];
    let sender_ip: Ipv4Addr = [arp[14], arp[15], arp[16], arp[17]];
    let our_mac = stack.device.mac();

    let mut reply = Vec::with_capacity(42);
    reply.extend_from_slice(&sender_mac); // eth dst
    reply.extend_from_slice(&our_mac); // eth src
    reply.extend_from_slice(&ETHERTYPE_ARP.to_be_bytes());
    reply.extend_from_slice(&[0, 1, 0x08, 0, 6, 4, 0, 2]); // htype, ptype, hlen, plen, oper=reply
    reply.extend_from_slice(&our_mac);
    reply.extend_from_slice(&stack.ip);
    reply.extend_from_slice(&sender_mac);
    reply.extend_from_slice(&sender_ip);
    stack.device.transmit(&reply);
    stack.arp_replies += 1;
}

fn handle_ipv4(frame: &[u8], ip: &[u8]) {
    if ip.len() < 20 || (ip[0] >> 4) != 4 {
        return;
    }
    let ihl = (ip[0] & 0x0F) as usize * 4;
    if ip.len() < ihl {
        return;
    }
    let proto = ip[9];
    let src_ip: Ipv4Addr = [ip[12], ip[13], ip[14], ip[15]];
    let dst_ip: Ipv4Addr = [ip[16], ip[17], ip[18], ip[19]];
    if proto != IPPROTO_ICMP {
        return;
    }
    let icmp = &ip[ihl..];
    if icmp.len() < 8 {
        return;
    }
    match icmp[0] {
        8 => reply_echo(frame, src_ip, dst_ip, icmp), // echo request -> reply
        0 => {
            // echo reply received: count it (the ping round-trip completed).
            if let Some(stack) = STACK.lock().as_mut() {
                stack.icmp_replies += 1;
            }
        }
        _ => {}
    }
}

fn reply_echo(frame: &[u8], src_ip: Ipv4Addr, dst_ip: Ipv4Addr, icmp_req: &[u8]) {
    let mut guard = STACK.lock();
    let Some(stack) = guard.as_mut() else { return };
    if dst_ip != stack.ip {
        return;
    }
    let src_mac: MacAddr = [frame[6], frame[7], frame[8], frame[9], frame[10], frame[11]];
    let our_mac = stack.device.mac();

    // Build echo reply: type 0, copy identifier/seq/payload, recompute csum.
    let mut icmp = icmp_req.to_vec();
    icmp[0] = 0; // echo reply
    icmp[2] = 0;
    icmp[3] = 0;
    let csum = checksum16(&icmp);
    icmp[2] = (csum >> 8) as u8;
    icmp[3] = csum as u8;

    let reply = build_ipv4_frame(our_mac, src_mac, dst_ip, src_ip, IPPROTO_ICMP, &icmp);
    stack.device.transmit(&reply);
}
