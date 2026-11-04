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
const IPPROTO_UDP: u8 = 17;

/// Largest UDP payload a socket will accept or deliver.
pub const UDP_MAX_PAYLOAD: usize = 1024;
/// Datagrams a socket queues before dropping the oldest.
const UDP_BACKLOG: usize = 16;

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

/// A bound UDP socket: a local port plus a small receive backlog.
struct UdpSocket {
    handle: u64,
    port: u16,
    bound: bool,
    /// (peer ip, peer port, payload)
    rx: VecDeque<(Ipv4Addr, u16, Vec<u8>)>,
}

struct Stack {
    device: alloc::boxed::Box<dyn NetDevice>,
    /// Frames addressed to our own MAC never leave the machine: with a real
    /// NIC attached, traffic to our own IP is still delivered locally here.
    local: VecDeque<Vec<u8>>,
    ip: Ipv4Addr,
    arp: Vec<(Ipv4Addr, MacAddr)>,
    icmp_replies: u64,
    arp_replies: u64,
    sockets: Vec<UdpSocket>,
    next_handle: u64,
    udp_tx: u64,
    udp_rx: u64,
    /// Default route: traffic outside `ip & mask` goes to this router's MAC
    /// (all zeros = no router: everything is sent on-link).
    gateway: Ipv4Addr,
    mask: Ipv4Addr,
    /// A real NIC is attached (else the loopback device).
    attached: bool,
    frames_tx: u64,
    frames_rx: u64,
}

static STACK: Mutex<Option<Stack>> = Mutex::new(None);

/// What bringing up a real NIC came to (shown by `ipconfig` while only the
/// loopback is attached: a PC has no serial cable to read the log from).
static STATUS: Mutex<Option<alloc::string::String>> = Mutex::new(None);
/// The platform's "bring the network up" routine, for the shell's `netup`.
static RETRY: Mutex<Option<fn()>> = Mutex::new(None);

#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
pub fn set_status(s: alloc::string::String) {
    *STATUS.lock() = Some(s);
}

pub fn status() -> Option<alloc::string::String> {
    STATUS.lock().clone()
}

#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
pub fn set_retry(f: fn()) {
    *RETRY.lock() = Some(f);
}

/// Start the platform's network bring-up again; false when there is none.
pub fn retry() -> bool {
    let f = *RETRY.lock();
    match f {
        Some(f) => {
            f();
            true
        }
        None => false,
    }
}

impl Stack {
    /// Send a frame: to ourselves through the local queue, otherwise out of
    /// the device.
    fn tx(&mut self, frame: &[u8]) {
        self.frames_tx += 1;
        if frame.len() >= 6 && frame[..6] == self.device.mac() {
            self.local.push_back(frame.to_vec());
        } else {
            self.device.transmit(frame);
        }
    }

    /// The MAC to send a packet for `dest` to: the router's for addresses
    /// outside our subnet (when a router is set), else `dest`'s own;
    /// broadcast while it is unknown.
    fn next_hop_mac(&self, dest: Ipv4Addr) -> MacAddr {
        let off_link = self.gateway != [0; 4] && (0..4).any(|i| dest[i] & self.mask[i] != self.ip[i] & self.mask[i]);
        let hop = if off_link { self.gateway } else { dest };
        self.arp.iter().find(|(ip, _)| *ip == hop).map(|(_, m)| *m).unwrap_or([0xFF; ETH_ALEN])
    }

    /// Learn (or refresh) an IPv4 -> MAC mapping.
    fn learn(&mut self, ip: Ipv4Addr, mac: MacAddr) {
        if let Some(e) = self.arp.iter_mut().find(|(i, _)| *i == ip) {
            e.1 = mac;
        } else {
            self.arp.push((ip, mac));
        }
    }
}

/// Replace the loopback device with a real NIC and take address `ip` (multi-
/// arch M2: virtio-net). Sockets and counters are kept.
pub fn attach(device: alloc::boxed::Box<dyn NetDevice>, ip: Ipv4Addr) {
    let mut guard = STACK.lock();
    let Some(stack) = guard.as_mut() else { return };
    let mac = device.mac();
    stack.device = device;
    stack.ip = ip;
    stack.arp = vec![(ip, mac)];
    stack.attached = true;
}

/// Set the default route (from DHCP): `gateway` for everything outside
/// `ip & mask`.
#[cfg_attr(not(target_arch = "aarch64"), allow(dead_code))]
pub fn set_route(mask: Ipv4Addr, gateway: Ipv4Addr) {
    if let Some(stack) = STACK.lock().as_mut() {
        stack.mask = mask;
        stack.gateway = gateway;
    }
}

/// Broadcast an ARP request for `target`.
pub fn arp_request(target: Ipv4Addr) {
    let mut guard = STACK.lock();
    let Some(stack) = guard.as_mut() else { return };
    let mac = stack.device.mac();
    let mut f = Vec::with_capacity(42);
    f.extend_from_slice(&[0xFF; ETH_ALEN]);
    f.extend_from_slice(&mac);
    f.extend_from_slice(&ETHERTYPE_ARP.to_be_bytes());
    f.extend_from_slice(&[0, 1, 0x08, 0, 6, 4, 0, 1]); // request
    f.extend_from_slice(&mac);
    f.extend_from_slice(&stack.ip.clone());
    f.extend_from_slice(&[0; ETH_ALEN]);
    f.extend_from_slice(&target);
    stack.tx(&f);
}

/// The MAC learned for `ip`, if any.
pub fn resolve(ip: Ipv4Addr) -> Option<MacAddr> {
    STACK.lock().as_ref()?.arp.iter().find(|(i, _)| *i == ip).map(|(_, m)| *m)
}

/// Bring up the loopback interface with the given IPv4 address.
pub fn init(ip: Ipv4Addr) {
    let device = Loopback::new();
    let mac = device.mac();
    *STACK.lock() = Some(Stack {
        device: alloc::boxed::Box::new(device),
        local: VecDeque::new(),
        ip,
        arp: vec![(ip, mac)], // self entry
        icmp_replies: 0,
        arp_replies: 0,
        sockets: Vec::new(),
        next_handle: 1,
        udp_tx: 0,
        udp_rx: 0,
        gateway: [0; 4],
        mask: [255, 255, 255, 0],
        attached: false,
        frames_tx: 0,
        frames_rx: 0,
    });
}

/// What `ipconfig` shows.
pub struct NetInfo {
    pub mac: MacAddr,
    pub ip: Ipv4Addr,
    pub mask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub attached: bool,
    pub frames_tx: u64,
    pub frames_rx: u64,
    pub udp_tx: u64,
    pub udp_rx: u64,
    pub icmp_replies: u64,
    pub arp: Vec<(Ipv4Addr, MacAddr)>,
    pub udp_ports: Vec<u16>,
}

pub fn info() -> Option<NetInfo> {
    let guard = STACK.lock();
    let s = guard.as_ref()?;
    Some(NetInfo {
        mac: s.device.mac(),
        ip: s.ip,
        mask: s.mask,
        gateway: s.gateway,
        attached: s.attached,
        frames_tx: s.frames_tx,
        frames_rx: s.frames_rx,
        udp_tx: s.udp_tx,
        udp_rx: s.udp_rx,
        icmp_replies: s.icmp_replies,
        arp: s.arp.clone(),
        udp_ports: s.sockets.iter().filter(|u| u.bound).map(|u| u.port).collect(),
    })
}

/// "52:54:00:12:34:56".
pub fn fmt_mac(m: MacAddr) -> alloc::string::String {
    alloc::format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", m[0], m[1], m[2], m[3], m[4], m[5])
}

/// "10.0.2.15" -> address.
pub fn parse_ip(s: &str) -> Option<Ipv4Addr> {
    let mut ip = [0u8; 4];
    let mut parts = s.trim().split('.');
    for b in ip.iter_mut() {
        *b = parts.next()?.parse().ok()?;
    }
    parts.next().is_none().then_some(ip)
}

/// One ICMP echo to `dest`; the round trip in microseconds, or None after
/// `timeout_ms` (ARPs for the next hop first when its MAC is unknown).
pub fn ping_once(dest: Ipv4Addr, seq: u16, timeout_ms: u64) -> Option<u64> {
    let hop_known = {
        let guard = STACK.lock();
        let s = guard.as_ref()?;
        dest == s.ip || s.next_hop_mac(dest) != [0xFF; ETH_ALEN]
    };
    if !hop_known {
        let (mask, gw, ip) = {
            let guard = STACK.lock();
            let s = guard.as_ref()?;
            (s.mask, s.gateway, s.ip)
        };
        let off_link = gw != [0; 4] && (0..4).any(|i| dest[i] & mask[i] != ip[i] & mask[i]);
        arp_request(if off_link { gw } else { dest });
        let t = crate::arch::cpu::nanos();
        while crate::arch::cpu::nanos() - t < 300_000_000 {
            poll();
        }
    }
    let before = counters().0;
    let t0 = crate::arch::cpu::nanos();
    send_ping(dest, 0xB2B2, seq);
    while crate::arch::cpu::nanos() - t0 < timeout_ms * 1_000_000 {
        poll();
        if counters().0 > before {
            return Some((crate::arch::cpu::nanos() - t0) / 1000);
        }
        core::hint::spin_loop();
    }
    None
}

/// The interface address, or `None` if the stack is down.
pub fn local_ip() -> Option<Ipv4Addr> {
    STACK.lock().as_ref().map(|s| s.ip)
}

/// (udp datagrams sent, udp datagrams delivered).
pub fn udp_counters() -> (u64, u64) {
    STACK
        .lock()
        .as_ref()
        .map(|s| (s.udp_tx, s.udp_rx))
        .unwrap_or((0, 0))
}

// ---- UDP sockets --------------------------------------------------------

/// Allocate a socket handle. Returns 0 if the stack is down.
pub fn udp_socket() -> u64 {
    let mut guard = STACK.lock();
    let Some(stack) = guard.as_mut() else { return 0 };
    let handle = stack.next_handle;
    stack.next_handle += 1;
    stack.sockets.push(UdpSocket {
        handle,
        port: 0,
        bound: false,
        rx: VecDeque::new(),
    });
    handle
}

/// Bind a socket to a local port. Fails if the port is already taken.
pub fn udp_bind(handle: u64, port: u16) -> Result<(), &'static str> {
    let mut guard = STACK.lock();
    let Some(stack) = guard.as_mut() else { return Err("net down") };
    if stack.sockets.iter().any(|s| s.bound && s.port == port) {
        return Err("port in use");
    }
    let Some(sock) = stack.sockets.iter_mut().find(|s| s.handle == handle) else {
        return Err("bad handle");
    };
    sock.port = port;
    sock.bound = true;
    Ok(())
}

/// Release a socket and drop its backlog.
pub fn udp_close(handle: u64) -> Result<(), &'static str> {
    let mut guard = STACK.lock();
    let Some(stack) = guard.as_mut() else { return Err("net down") };
    let before = stack.sockets.len();
    stack.sockets.retain(|s| s.handle != handle);
    if stack.sockets.len() == before {
        return Err("bad handle");
    }
    Ok(())
}

/// Send a UDP datagram. The source port is the socket's bound port (0 if it was
/// never bound). Returns the payload length queued for transmission.
pub fn udp_send(handle: u64, dest: Ipv4Addr, dest_port: u16, payload: &[u8]) -> Result<usize, &'static str> {
    if payload.len() > UDP_MAX_PAYLOAD {
        return Err("payload too large");
    }
    let mut guard = STACK.lock();
    let Some(stack) = guard.as_mut() else { return Err("net down") };
    let Some(sock) = stack.sockets.iter().find(|s| s.handle == handle) else {
        return Err("bad handle");
    };
    let src_port = sock.port;
    let src_ip = stack.ip;
    let src_mac = stack.device.mac();
    let dst_mac = stack.next_hop_mac(dest);

    let udp = build_udp(src_ip, dest, src_port, dest_port, payload);
    let frame = build_ipv4_frame(src_mac, dst_mac, src_ip, dest, IPPROTO_UDP, &udp);
    stack.tx(&frame);
    stack.udp_tx += 1;
    Ok(payload.len())
}

/// Take the next queued datagram for a socket, if any. Callers should
/// [`poll`] first so freshly looped-back frames are processed.
pub fn udp_recv(handle: u64) -> Option<(Ipv4Addr, u16, Vec<u8>)> {
    let mut guard = STACK.lock();
    let stack = guard.as_mut()?;
    let sock = stack.sockets.iter_mut().find(|s| s.handle == handle)?;
    sock.rx.pop_front()
}

/// Build a UDP header + payload with a correct checksum over the IPv4
/// pseudo-header (a zero checksum means "not computed", which we avoid).
fn build_udp(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, src_port: u16, dst_port: u16, payload: &[u8]) -> Vec<u8> {
    let len = 8 + payload.len();
    let mut udp = Vec::with_capacity(len);
    udp.extend_from_slice(&src_port.to_be_bytes());
    udp.extend_from_slice(&dst_port.to_be_bytes());
    udp.extend_from_slice(&(len as u16).to_be_bytes());
    udp.extend_from_slice(&[0, 0]); // checksum placeholder
    udp.extend_from_slice(payload);

    // Pseudo-header: src ip, dst ip, zero, protocol, UDP length.
    let mut pseudo = Vec::with_capacity(12 + len);
    pseudo.extend_from_slice(&src_ip);
    pseudo.extend_from_slice(&dst_ip);
    pseudo.extend_from_slice(&[0, IPPROTO_UDP]);
    pseudo.extend_from_slice(&(len as u16).to_be_bytes());
    pseudo.extend_from_slice(&udp);
    let mut csum = checksum16(&pseudo);
    if csum == 0 {
        csum = 0xFFFF; // 0 is reserved for "no checksum"
    }
    udp[6] = (csum >> 8) as u8;
    udp[7] = csum as u8;
    udp
}

/// Deliver a received UDP datagram to the socket bound to its destination port.
fn handle_udp(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, udp: &[u8]) {
    if udp.len() < 8 {
        return;
    }
    let src_port = be16(udp, 0);
    let dst_port = be16(udp, 2);
    let ulen = be16(udp, 4) as usize;
    if ulen < 8 || ulen > udp.len() {
        return;
    }
    let payload = &udp[8..ulen];

    let mut guard = STACK.lock();
    let Some(stack) = guard.as_mut() else { return };
    if dst_ip != stack.ip {
        return;
    }
    let Some(sock) = stack
        .sockets
        .iter_mut()
        .find(|s| s.bound && s.port == dst_port)
    else {
        return; // nothing listening: drop (no ICMP port-unreachable yet)
    };
    if sock.rx.len() >= UDP_BACKLOG {
        sock.rx.pop_front();
    }
    sock.rx.push_back((src_ip, src_port, payload.to_vec()));
    stack.udp_rx += 1;
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
    let dst_mac = stack.next_hop_mac(dest);

    // ICMP echo request.
    let mut icmp = vec![8u8, 0, 0, 0, (ident >> 8) as u8, ident as u8, (seq >> 8) as u8, seq as u8];
    icmp.extend_from_slice(b"buitenzorg-dahan");
    let csum = checksum16(&icmp);
    icmp[2] = (csum >> 8) as u8;
    icmp[3] = csum as u8;

    let frame = build_ipv4_frame(src_mac, dst_mac, stack.ip, dest, IPPROTO_ICMP, &icmp);
    stack.tx(&frame);
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

/// "10.0.2.15".
pub fn fmt_ip(ip: Ipv4Addr) -> alloc::string::String {
    alloc::format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
}

/// Send an IPv4 packet with `payload` (a transport segment) to `dest` (TCP).
pub fn send_ipv4(dest: Ipv4Addr, proto: u8, payload: &[u8]) {
    let mut guard = STACK.lock();
    let Some(stack) = guard.as_mut() else { return };
    let (src_mac, dst_mac, src_ip) = (stack.device.mac(), stack.next_hop_mac(dest), stack.ip);
    let dst_mac = if dest == src_ip { src_mac } else { dst_mac };
    let frame = build_ipv4_frame(src_mac, dst_mac, src_ip, dest, proto, payload);
    stack.tx(&frame);
}

/// Drive the stack: process all currently queued received frames. Returns the
/// number of frames handled.
pub fn poll() -> usize {
    crate::tcp::timers();
    let mut handled = 0;
    loop {
        let frame = {
            let mut guard = STACK.lock();
            let Some(stack) = guard.as_mut() else { return handled };
            match stack.local.pop_front() {
                Some(f) => Some(f),
                None => stack.device.receive(),
            }
        };
        let Some(frame) = frame else { break };
        if let Some(s) = STACK.lock().as_mut() {
            s.frames_rx += 1;
        }
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
    let mut guard = STACK.lock();
    let Some(stack) = guard.as_mut() else { return };
    // Learn the sender of every request / reply aimed at us.
    let target_ip: Ipv4Addr = [arp[24], arp[25], arp[26], arp[27]];
    if target_ip == stack.ip {
        stack.learn([arp[14], arp[15], arp[16], arp[17]], [arp[8], arp[9], arp[10], arp[11], arp[12], arp[13]]);
    }
    if oper != 1 || target_ip != stack.ip {
        return; // only requests for our address get a reply
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
    stack.tx(&reply);
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
    // learn an on-link sender's MAC, so replies (TCP) go to it, not broadcast
    if let Some(stack) = STACK.lock().as_mut() {
        let on_link = (0..4).all(|i| src_ip[i] & stack.mask[i] == stack.ip[i] & stack.mask[i]);
        if on_link && src_ip != [0; 4] && src_ip != stack.ip && frame.len() >= 12 {
            stack.learn(src_ip, [frame[6], frame[7], frame[8], frame[9], frame[10], frame[11]]);
        }
    }
    if proto == crate::tcp::IPPROTO_TCP {
        // the IP total length, not the frame: Ethernet pads short frames
        let total = (be16(ip, 2) as usize).min(ip.len());
        if total > ihl {
            crate::tcp::input(src_ip, dst_ip, &ip[ihl..total]);
        }
        return;
    }
    if proto == IPPROTO_UDP {
        handle_udp(src_ip, dst_ip, &ip[ihl..]);
        return;
    }
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
    stack.tx(&reply);
}
