//! IPv6, the essentials: addresses (link-local from the MAC, a global one
//! from a router advertisement: SLAAC with EUI-64 identifiers), neighbour
//! discovery (solicit / advertise, a neighbour cache, router solicitation),
//! and ICMPv6 echo (`ping6`). It shares the Ethernet device with `net.rs`:
//! `net::handle_frame` hands over ethertype 0x86DD, and frames go out through
//! `net::send_frame`.
//!
//! Not done: duplicate address detection, UDP/TCP over IPv6, DNS AAAA,
//! extension headers (a packet with any is ignored), fragmentation, DHCPv6.
//!
//! Locking: `STATE` is never held while calling into `net` (which takes its
//! own `STACK` lock); frames are built under `STATE` and sent after it drops.

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::net::MacAddr;

pub type Ipv6Addr = [u8; 16];

pub const ETHERTYPE_IPV6: u16 = 0x86DD;
const NEXT_ICMPV6: u8 = 58;
const HOP_LIMIT: u8 = 64;

const ECHO_REQUEST: u8 = 128;
const ECHO_REPLY: u8 = 129;
const ROUTER_SOLICIT: u8 = 133;
const ROUTER_ADVERT: u8 = 134;
const NEIGHBOR_SOLICIT: u8 = 135;
const NEIGHBOR_ADVERT: u8 = 136;

const ALL_NODES: Ipv6Addr = [0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
const ALL_ROUTERS: Ipv6Addr = [0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];
const UNSPECIFIED: Ipv6Addr = [0; 16];

struct State {
    /// A global address from SLAAC, with its prefix length.
    global: Option<(Ipv6Addr, u8)>,
    /// The router that advertised it (link-local address).
    router: Option<Ipv6Addr>,
    neighbours: Vec<(Ipv6Addr, MacAddr)>,
    echo_replies: u64,
    last_reply_seq: u16,
    adverts: u64,
    /// Neighbour advertisements we sent (answers to solicitations).
    na_sent: u64,
}

static STATE: Mutex<State> = Mutex::new(State { global: None, router: None, neighbours: Vec::new(), echo_replies: 0, last_reply_seq: 0, adverts: 0, na_sent: 0 });

/// Forget everything learned on the previous device (`net::attach`).
pub fn reset() {
    let mut s = STATE.lock();
    s.global = None;
    s.router = None;
    s.neighbours.clear();
}

/// fe80::/64 with the modified EUI-64 identifier of `mac`.
pub fn link_local_of(mac: MacAddr) -> Ipv6Addr {
    let mut a = [0u8; 16];
    a[0] = 0xfe;
    a[1] = 0x80;
    a[8..].copy_from_slice(&eui64(mac));
    a
}

fn eui64(mac: MacAddr) -> [u8; 8] {
    [mac[0] ^ 0x02, mac[1], mac[2], 0xff, 0xfe, mac[3], mac[4], mac[5]]
}

pub fn link_local() -> Option<Ipv6Addr> {
    crate::net::device_mac().map(link_local_of)
}

/// Our addresses: link-local first, then the global one (with prefix length).
pub fn addresses() -> Vec<(Ipv6Addr, u8)> {
    let mut v = Vec::new();
    if let Some(ll) = link_local() {
        v.push((ll, 64));
    }
    if let Some(g) = STATE.lock().global {
        v.push(g);
    }
    v
}

pub fn router() -> Option<Ipv6Addr> {
    STATE.lock().router
}

pub fn neighbours() -> Vec<(Ipv6Addr, MacAddr)> {
    STATE.lock().neighbours.clone()
}

fn is_ours(a: &Ipv6Addr, mac: MacAddr, s: &State) -> bool {
    *a == link_local_of(mac) || s.global.is_some_and(|(g, _)| g == *a)
}

fn solicited_node(a: &Ipv6Addr) -> Ipv6Addr {
    [0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0xff, a[13], a[14], a[15]]
}

fn multicast_mac(a: &Ipv6Addr) -> MacAddr {
    [0x33, 0x33, a[12], a[13], a[14], a[15]]
}

fn accepts(dst: &Ipv6Addr, mac: MacAddr, s: &State) -> bool {
    if is_ours(dst, mac, s) || *dst == ALL_NODES {
        return true;
    }
    let ll = link_local_of(mac);
    *dst == solicited_node(&ll) || s.global.is_some_and(|(g, _)| *dst == solicited_node(&g))
}

// ---- packets --------------------------------------------------------------

fn sum16(data: &[u8], mut acc: u32) -> u32 {
    let mut chunks = data.chunks_exact(2);
    for c in &mut chunks {
        acc += u16::from_be_bytes([c[0], c[1]]) as u32;
    }
    if let [b] = chunks.remainder() {
        acc += (*b as u32) << 8;
    }
    acc
}

fn fold(mut acc: u32) -> u16 {
    while acc >> 16 != 0 {
        acc = (acc & 0xFFFF) + (acc >> 16);
    }
    !(acc as u16)
}

/// The ICMPv6 checksum (RFC 4443 2.3: over the pseudo-header).
fn icmp_checksum(src: &Ipv6Addr, dst: &Ipv6Addr, msg: &[u8]) -> u16 {
    let mut acc = sum16(src, 0);
    acc = sum16(dst, acc);
    acc += msg.len() as u32;
    acc += NEXT_ICMPV6 as u32;
    fold(sum16(msg, acc))
}

/// Ethernet + IPv6 + an ICMPv6 message whose checksum field is filled in here.
fn frame(src_mac: MacAddr, dst_mac: MacAddr, src: &Ipv6Addr, dst: &Ipv6Addr, hop: u8, mut msg: Vec<u8>) -> Vec<u8> {
    msg[2] = 0;
    msg[3] = 0;
    let c = icmp_checksum(src, dst, &msg);
    msg[2..4].copy_from_slice(&c.to_be_bytes());
    let mut f = Vec::with_capacity(54 + msg.len());
    f.extend_from_slice(&dst_mac);
    f.extend_from_slice(&src_mac);
    f.extend_from_slice(&ETHERTYPE_IPV6.to_be_bytes());
    f.extend_from_slice(&[0x60, 0, 0, 0]);
    f.extend_from_slice(&(msg.len() as u16).to_be_bytes());
    f.push(NEXT_ICMPV6);
    f.push(hop);
    f.extend_from_slice(src);
    f.extend_from_slice(dst);
    f.extend_from_slice(&msg);
    f
}

/// A neighbour solicitation for `target`, sent to its solicited-node group.
fn solicit(mac: MacAddr, src: &Ipv6Addr, target: &Ipv6Addr) -> Vec<u8> {
    let mut m = alloc::vec![NEIGHBOR_SOLICIT, 0, 0, 0, 0, 0, 0, 0];
    m.extend_from_slice(target);
    m.extend_from_slice(&[1, 1]); // source link-layer address option
    m.extend_from_slice(&mac);
    let dst = solicited_node(target);
    frame(mac, multicast_mac(&dst), src, &dst, 255, m)
}

/// Where a packet for `dst` goes on the link: a multicast MAC, a known
/// neighbour (or the router for an off-link destination), or None while it
/// has to be solicited first (then also the address to solicit).
fn next_hop(dst: &Ipv6Addr, mac: MacAddr, s: &State) -> Result<MacAddr, Ipv6Addr> {
    if dst[0] == 0xff {
        return Ok(multicast_mac(dst));
    }
    if is_ours(dst, mac, s) {
        return Ok(mac);
    }
    let link_local = dst[0] == 0xfe && dst[1] & 0xc0 == 0x80;
    let on_link = link_local || s.global.is_some_and(|(g, len)| same_prefix(&g, dst, len));
    let hop = if on_link { *dst } else { s.router.unwrap_or(*dst) };
    s.neighbours.iter().find(|(a, _)| *a == hop).map(|(_, m)| *m).ok_or(hop)
}

fn same_prefix(a: &Ipv6Addr, b: &Ipv6Addr, len: u8) -> bool {
    let full = (len / 8) as usize;
    if a[..full] != b[..full] {
        return false;
    }
    let rest = len % 8;
    rest == 0 || (a[full] ^ b[full]) & (0xFFu8 << (8 - rest)) == 0
}

/// The address to send from towards `dst`: the global one off the link-local
/// scope when we have it.
fn source_for(dst: &Ipv6Addr, mac: MacAddr, s: &State) -> Ipv6Addr {
    let link_scope = dst[0] == 0xfe && dst[1] & 0xc0 == 0x80 || dst[0] == 0xff && dst[1] & 0x0f <= 2;
    match s.global {
        Some((g, _)) if !link_scope => g,
        _ => link_local_of(mac),
    }
}

fn learn(s: &mut State, a: Ipv6Addr, m: MacAddr) {
    if a == UNSPECIFIED || a[0] == 0xff {
        return;
    }
    match s.neighbours.iter_mut().find(|(x, _)| *x == a) {
        Some(e) => e.1 = m,
        None => {
            if s.neighbours.len() >= 64 {
                s.neighbours.remove(0);
            }
            s.neighbours.push((a, m));
        }
    }
}

/// The link-layer address option (`kind` 1 = source, 2 = target) of a
/// neighbour discovery message, from `opts` on.
fn lladdr_option(msg: &[u8], mut at: usize, kind: u8) -> Option<MacAddr> {
    while at + 2 <= msg.len() {
        let (t, len) = (msg[at], msg[at + 1] as usize * 8);
        if len == 0 || at + len > msg.len() {
            return None;
        }
        if t == kind && len >= 8 {
            return Some([msg[at + 2], msg[at + 3], msg[at + 4], msg[at + 5], msg[at + 6], msg[at + 7]]);
        }
        at += len;
    }
    None
}

/// An Ethernet frame with ethertype 0x86DD (from `net::handle_frame`).
pub fn input(f: &[u8]) {
    if f.len() < 14 + 40 || f[14] >> 4 != 6 {
        return;
    }
    let Some(mac) = crate::net::device_mac() else { return };
    let ip = &f[14..];
    let plen = (u16::from_be_bytes([ip[4], ip[5]]) as usize).min(ip.len() - 40);
    let (next, hop) = (ip[6], ip[7]);
    let mut src = [0u8; 16];
    let mut dst = [0u8; 16];
    src.copy_from_slice(&ip[8..24]);
    dst.copy_from_slice(&ip[24..40]);
    let msg = &ip[40..40 + plen];
    let src_mac: MacAddr = [f[6], f[7], f[8], f[9], f[10], f[11]];
    if next != NEXT_ICMPV6 || msg.len() < 4 || icmp_checksum(&src, &dst, msg) != 0 {
        return;
    }
    let mut out: Vec<Vec<u8>> = Vec::new();
    {
        let mut s = STATE.lock();
        if !accepts(&dst, mac, &s) {
            return;
        }
        match msg[0] {
            ECHO_REQUEST if is_ours(&dst, mac, &s) && msg.len() >= 8 => {
                learn(&mut s, src, src_mac);
                let mut r = msg.to_vec();
                r[0] = ECHO_REPLY;
                out.push(frame(mac, src_mac, &dst, &src, HOP_LIMIT, r));
            }
            ECHO_REPLY if msg.len() >= 8 => {
                s.echo_replies += 1;
                s.last_reply_seq = u16::from_be_bytes([msg[6], msg[7]]);
            }
            // Neighbour discovery must arrive with hop limit 255 (RFC 4861).
            NEIGHBOR_SOLICIT if hop == 255 && msg.len() >= 24 => {
                let mut target = [0u8; 16];
                target.copy_from_slice(&msg[8..24]);
                if !is_ours(&target, mac, &s) {
                    return;
                }
                let reply_to = if src == UNSPECIFIED { ALL_NODES } else { src };
                if let Some(m) = lladdr_option(msg, 24, 1) {
                    learn(&mut s, src, m);
                }
                let to_mac = if reply_to == ALL_NODES { multicast_mac(&ALL_NODES) } else { lladdr_option(msg, 24, 1).unwrap_or(src_mac) };
                let solicited = if src == UNSPECIFIED { 0 } else { 0x40 };
                let mut m = alloc::vec![NEIGHBOR_ADVERT, 0, 0, 0, solicited | 0x20, 0, 0, 0]; // S + O flags
                m.extend_from_slice(&target);
                m.extend_from_slice(&[2, 1]);
                m.extend_from_slice(&mac);
                out.push(frame(mac, to_mac, &target, &reply_to, 255, m));
                s.na_sent += 1;
            }
            NEIGHBOR_ADVERT if hop == 255 && msg.len() >= 24 => {
                let mut target = [0u8; 16];
                target.copy_from_slice(&msg[8..24]);
                let m = lladdr_option(msg, 24, 2).unwrap_or(src_mac);
                learn(&mut s, target, m);
            }
            ROUTER_ADVERT if hop == 255 && msg.len() >= 16 => {
                s.adverts += 1;
                let lifetime = u16::from_be_bytes([msg[6], msg[7]]);
                if let Some(m) = lladdr_option(msg, 16, 1) {
                    learn(&mut s, src, m);
                } else {
                    learn(&mut s, src, src_mac);
                }
                if lifetime != 0 {
                    s.router = Some(src);
                }
                // prefix information options with the autonomous flag: SLAAC
                let mut at = 16;
                while at + 2 <= msg.len() {
                    let (t, len) = (msg[at], msg[at + 1] as usize * 8);
                    if len == 0 || at + len > msg.len() {
                        break;
                    }
                    if t == 3 && len == 32 {
                        let (plen, flags) = (msg[at + 2], msg[at + 3]);
                        let valid = u32::from_be_bytes([msg[at + 4], msg[at + 5], msg[at + 6], msg[at + 7]]);
                        let link_local = msg[at + 16] == 0xfe && msg[at + 17] & 0xc0 == 0x80;
                        if flags & 0x40 != 0 && plen == 64 && valid != 0 && !link_local && msg[at + 16] != 0xff && s.global.is_none() {
                            let mut g = [0u8; 16];
                            g[..8].copy_from_slice(&msg[at + 16..at + 24]);
                            g[8..].copy_from_slice(&eui64(mac));
                            s.global = Some((g, plen));
                            crate::println!("[ipv6] SLAAC: {}/{} from router {}", fmt(&g), plen, fmt(&src));
                        }
                    }
                    at += len;
                }
            }
            _ => {}
        }
    }
    for f in out {
        crate::net::send_frame(&f);
    }
}

/// Ask the routers on the link to advertise (we learn a prefix from the
/// answer).
pub fn solicit_routers() {
    let Some(mac) = crate::net::device_mac() else { return };
    let src = link_local_of(mac);
    let mut m = alloc::vec![ROUTER_SOLICIT, 0, 0, 0, 0, 0, 0, 0];
    m.extend_from_slice(&[1, 1]);
    m.extend_from_slice(&mac);
    let f = frame(mac, multicast_mac(&ALL_ROUTERS), &src, &ALL_ROUTERS, 255, m);
    crate::net::send_frame(&f);
}

/// Wait (polling the stack) until `done` or `ms` milliseconds pass.
fn wait(ms: u64, done: &dyn Fn() -> bool) -> bool {
    let t0 = crate::arch::cpu::nanos();
    while crate::arch::cpu::nanos() - t0 < ms * 1_000_000 {
        crate::net::poll();
        if done() {
            return true;
        }
        core::hint::spin_loop();
    }
    done()
}

/// Solicit routers and wait up to `ms` for an advertisement; true if a
/// global address was configured.
pub fn autoconfigure(ms: u64) -> bool {
    if STATE.lock().global.is_some() {
        return true;
    }
    solicit_routers();
    wait(ms, &|| STATE.lock().global.is_some())
}

/// One ICMPv6 echo to `dst`; the round trip in microseconds, or None after
/// `timeout_ms` (resolving the next hop's MAC first).
pub fn ping(dst: &Ipv6Addr, seq: u16, timeout_ms: u64) -> Option<u64> {
    let mac = crate::net::device_mac()?;
    let hop = {
        let s = STATE.lock();
        next_hop(dst, mac, &s)
    };
    let dst_mac = match hop {
        Ok(m) => m,
        Err(target) => {
            let src = source_for(&target, mac, &STATE.lock());
            crate::net::send_frame(&solicit(mac, &src, &target));
            if !wait(1000, &|| STATE.lock().neighbours.iter().any(|(a, _)| *a == target)) {
                return None;
            }
            next_hop(dst, mac, &STATE.lock()).ok()?
        }
    };
    let (src, before) = {
        let s = STATE.lock();
        (source_for(dst, mac, &s), s.echo_replies)
    };
    let mut m = alloc::vec![ECHO_REQUEST, 0, 0, 0, 0xB2, 0x06];
    m.extend_from_slice(&seq.to_be_bytes());
    m.extend_from_slice(b"buitenzorg ping6 ");
    let t0 = crate::arch::cpu::nanos();
    crate::net::send_frame(&frame(mac, dst_mac, &src, dst, HOP_LIMIT, m));
    if wait(timeout_ms, &|| {
        let s = STATE.lock();
        s.echo_replies > before && s.last_reply_seq == seq
    }) {
        Some((crate::arch::cpu::nanos() - t0) / 1000)
    } else {
        None
    }
}

// ---- text -------------------------------------------------------------------

/// RFC 5952 text: lower-case hex, the longest run (>= 2) of zero groups as "::".
pub fn fmt(a: &Ipv6Addr) -> String {
    let g: Vec<u16> = (0..8).map(|i| u16::from_be_bytes([a[i * 2], a[i * 2 + 1]])).collect();
    let (mut best, mut best_len, mut i) = (8usize, 1usize, 0usize);
    while i < 8 {
        if g[i] == 0 {
            let start = i;
            while i < 8 && g[i] == 0 {
                i += 1;
            }
            if i - start > best_len {
                best = start;
                best_len = i - start;
            }
        } else {
            i += 1;
        }
    }
    let part = |r: core::ops::Range<usize>| g[r].iter().map(|x| alloc::format!("{:x}", x)).collect::<Vec<_>>().join(":");
    if best == 8 {
        part(0..8)
    } else {
        alloc::format!("{}::{}", part(0..best), part(best + best_len..8))
    }
}

/// "fe80::1", "fec0::2", "2001:db8::a:b" -> address.
pub fn parse(s: &str) -> Option<Ipv6Addr> {
    let s = s.trim();
    let groups = |t: &str| -> Option<Vec<u16>> {
        if t.is_empty() {
            return Some(Vec::new());
        }
        t.split(':').map(|x| if x.is_empty() || x.len() > 4 { None } else { u16::from_str_radix(x, 16).ok() }).collect()
    };
    let words: Vec<u16> = match s.split_once("::") {
        Some((head, tail)) => {
            let (h, t) = (groups(head)?, groups(tail)?);
            if h.len() + t.len() > 7 || tail.contains("::") {
                return None;
            }
            let mut w = h;
            w.resize(8 - t.len(), 0);
            w.extend(t);
            w
        }
        None => groups(s)?,
    };
    if words.len() != 8 {
        return None;
    }
    let mut a = [0u8; 16];
    for (i, w) in words.iter().enumerate() {
        a[i * 2..i * 2 + 2].copy_from_slice(&w.to_be_bytes());
    }
    Some(a)
}

/// Text form, address parsing and the ICMPv6 path end to end on whatever
/// device is attached (the loopback during the boot): ping our own
/// link-local address, answer a neighbour solicitation for it.
pub fn self_test() -> Result<String, String> {
    let cases: [(&str, &str); 5] = [
        ("fe80::1", "fe80::1"),
        ("2001:db8:0:0:1:0:0:1", "2001:db8::1:0:0:1"),
        ("::", "::"),
        ("fec0::2", "fec0::2"),
        ("1:2:3:4:5:6:7:8", "1:2:3:4:5:6:7:8"),
    ];
    for (input, want) in cases {
        let a = parse(input).ok_or_else(|| alloc::format!("cannot parse {}", input))?;
        if fmt(&a) != want {
            return Err(alloc::format!("{} printed as {}, want {}", input, fmt(&a), want));
        }
    }
    for bad in ["1::2::3", "12345::", "1:2:3", "g::1"] {
        if parse(bad).is_some() {
            return Err(alloc::format!("{} parsed", bad));
        }
    }
    let mac = crate::net::device_mac().ok_or("the network stack is down")?;
    let ll = link_local_of(mac);
    if ll[8] != mac[0] ^ 2 || ll[11] != 0xff || ll[12] != 0xfe {
        return Err(String::from("wrong EUI-64 identifier"));
    }
    // the checksum of a message carrying its own checksum is 0
    let mut m = alloc::vec![ECHO_REQUEST, 0, 0, 0, 1, 2, 3, 4, 5];
    let c = icmp_checksum(&ll, &ALL_NODES, &m);
    m[2..4].copy_from_slice(&c.to_be_bytes());
    if icmp_checksum(&ll, &ALL_NODES, &m) != 0 {
        return Err(String::from("checksum does not verify"));
    }
    let rtt = ping(&ll, 0x6006, 500).ok_or("no echo reply from our own link-local address")?;
    // a neighbour solicitation for our address must be answered (to ourselves)
    // a neighbour solicitation for our address is answered, and its sender
    // (a made-up neighbour) is learned from its link-layer option
    let other = parse("fe80::bad").unwrap();
    STATE.lock().neighbours.retain(|(a, _)| *a != other);
    let na0 = STATE.lock().na_sent;
    crate::net::send_frame(&solicit(mac, &other, &ll));
    let answered = wait(500, &|| {
        let s = STATE.lock();
        s.na_sent > na0 && s.neighbours.iter().any(|(a, m)| *a == other && *m == mac)
    });
    STATE.lock().neighbours.retain(|(a, _)| *a != other);
    if !answered {
        return Err(String::from("a neighbour solicitation got no advertisement"));
    }
    Ok(alloc::format!("link-local {}, echo to self in {} us, NS -> NA, text form", fmt(&ll), rtt))
}
