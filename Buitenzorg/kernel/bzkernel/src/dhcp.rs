//! A DHCP client (RFC 2131) on a raw `net::NetDevice`, run before the device
//! joins the IP stack: DISCOVER -> OFFER -> REQUEST -> ACK, broadcast replies
//! (we have no address yet), retransmitted every 2 s. Gives the address, the
//! netmask, the router and a DNS server. Used for a real NIC on a real LAN
//! (the Raspberry Pi 4's Ethernet, roadmap R4); QEMU's user network answers
//! too, which is how it is tested without hardware.

use alloc::vec::Vec;

use crate::net::{Ipv4Addr, MacAddr, NetDevice};

#[derive(Clone, Copy, Debug)]
pub struct Lease {
    pub ip: Ipv4Addr,
    pub mask: Ipv4Addr,
    pub router: Ipv4Addr,
    pub dns: Ipv4Addr,
    pub server: Ipv4Addr,
    pub seconds: u32,
}

const DISCOVER: u8 = 1;
const OFFER: u8 = 2;
const REQUEST: u8 = 3;
const ACK: u8 = 5;
const NAK: u8 = 6;

fn checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    for c in data.chunks(2) {
        sum += u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]) as u32;
    }
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// One DHCP message in a broadcast Ethernet/IPv4/UDP frame.
fn frame(mac: &MacAddr, xid: u32, kind: u8, requested: Option<(Ipv4Addr, Ipv4Addr)>) -> Vec<u8> {
    let mut b = Vec::with_capacity(300);
    b.extend_from_slice(&[1, 1, 6, 0]); // BOOTREQUEST, Ethernet, 6-byte address, 0 hops
    b.extend_from_slice(&xid.to_be_bytes());
    b.extend_from_slice(&[0, 0, 0x80, 0]); // secs, flags: broadcast replies
    b.extend_from_slice(&[0; 16]); // ciaddr yiaddr siaddr giaddr
    b.extend_from_slice(mac);
    b.extend_from_slice(&[0; 10 + 64 + 128]); // chaddr padding, sname, file
    b.extend_from_slice(&[0x63, 0x82, 0x53, 0x63]);
    b.extend_from_slice(&[53, 1, kind]);
    b.extend_from_slice(&[61, 7, 1]);
    b.extend_from_slice(mac);
    if let Some((ip, server)) = requested {
        b.extend_from_slice(&[50, 4]);
        b.extend_from_slice(&ip);
        b.extend_from_slice(&[54, 4]);
        b.extend_from_slice(&server);
    }
    b.extend_from_slice(&[55, 4, 1, 3, 6, 51]); // ask for mask, router, DNS, lease time
    b.extend_from_slice(&[12, 10]);
    b.extend_from_slice(b"buitenzorg");
    b.push(255);
    while b.len() < 300 {
        b.push(0);
    }

    let udp_len = 8 + b.len();
    let ip_len = 20 + udp_len;
    let mut f = Vec::with_capacity(14 + ip_len);
    f.extend_from_slice(&[0xFF; 6]);
    f.extend_from_slice(mac);
    f.extend_from_slice(&[0x08, 0x00]);
    let ip_start = f.len();
    f.extend_from_slice(&[0x45, 0]);
    f.extend_from_slice(&(ip_len as u16).to_be_bytes());
    f.extend_from_slice(&[(xid >> 8) as u8, xid as u8, 0, 0, 64, 17, 0, 0]);
    f.extend_from_slice(&[0, 0, 0, 0, 255, 255, 255, 255]);
    let c = checksum(&f[ip_start..ip_start + 20]);
    f[ip_start + 10..ip_start + 12].copy_from_slice(&c.to_be_bytes());
    f.extend_from_slice(&68u16.to_be_bytes());
    f.extend_from_slice(&67u16.to_be_bytes());
    f.extend_from_slice(&(udp_len as u16).to_be_bytes());
    f.extend_from_slice(&[0, 0]); // no UDP checksum (allowed over IPv4)
    f.extend_from_slice(&b);
    f
}

/// (message type, offered address, lease) if `f` is a DHCP reply for `xid`.
fn parse(f: &[u8], xid: u32) -> Option<(u8, Lease)> {
    if f.len() < 14 + 20 + 8 + 240 || f[12..14] != [0x08, 0x00] {
        return None;
    }
    let ip = &f[14..];
    let ihl = ((ip[0] & 0xF) as usize) * 4;
    if ip[9] != 17 || ip.len() < ihl + 8 + 240 {
        return None;
    }
    let udp = &ip[ihl..];
    if u16::from_be_bytes([udp[2], udp[3]]) != 68 {
        return None;
    }
    let m = &udp[8..];
    if m[0] != 2 || u32::from_be_bytes([m[4], m[5], m[6], m[7]]) != xid || m[236..240] != [0x63, 0x82, 0x53, 0x63] {
        return None;
    }
    let mut lease = Lease { ip: [m[16], m[17], m[18], m[19]], mask: [255, 255, 255, 0], router: [0; 4], dns: [0; 4], server: [0; 4], seconds: 0 };
    let mut kind = 0;
    let mut i = 240;
    while i < m.len() {
        let code = m[i];
        if code == 255 {
            break;
        }
        if code == 0 {
            i += 1;
            continue;
        }
        if i + 1 >= m.len() {
            break;
        }
        let len = m[i + 1] as usize;
        let v = &m[(i + 2).min(m.len())..(i + 2 + len).min(m.len())];
        let addr = |v: &[u8]| if v.len() >= 4 { [v[0], v[1], v[2], v[3]] } else { [0; 4] };
        match code {
            53 if !v.is_empty() => kind = v[0],
            1 => lease.mask = addr(v),
            3 => lease.router = addr(v),
            6 => lease.dns = addr(v),
            54 => lease.server = addr(v),
            51 if v.len() >= 4 => lease.seconds = u32::from_be_bytes([v[0], v[1], v[2], v[3]]),
            _ => {}
        }
        i += 2 + len;
    }
    Some((kind, lease))
}

/// Ask for an address; gives up after `seconds`.
pub fn lease(dev: &mut dyn NetDevice, seconds: u64) -> Option<Lease> {
    let mac = dev.mac();
    let hz = crate::arch::cpu::counter_hz().max(1);
    let t0 = crate::arch::cpu::timestamp();
    let xid = 0xB2_0000 ^ (t0 as u32) ^ u32::from_be_bytes([mac[2], mac[3], mac[4], mac[5]]);
    let mut offer: Option<Lease> = None;
    let mut last_send = 0u64;
    let mut sent = false;
    loop {
        let now = crate::arch::cpu::timestamp();
        if now - t0 > seconds * hz {
            return None;
        }
        if !sent || now - last_send > 2 * hz {
            let f = match offer {
                None => frame(&mac, xid, DISCOVER, None),
                Some(o) => frame(&mac, xid, REQUEST, Some((o.ip, o.server))),
            };
            dev.transmit(&f);
            last_send = now;
            sent = true;
        }
        while let Some(f) = dev.receive() {
            match parse(&f, xid) {
                Some((OFFER, l)) if offer.is_none() => {
                    offer = Some(l);
                    sent = false; // send the REQUEST now
                }
                Some((ACK, l)) if offer.is_some() => {
                    let o = offer.unwrap();
                    return Some(Lease { server: if l.server == [0; 4] { o.server } else { l.server }, ..l });
                }
                Some((NAK, _)) => {
                    offer = None;
                    sent = false;
                }
                _ => {}
            }
        }
        // on a PC this runs on the background start-up task: with one CPU the
        // desktop must keep running for the seconds a LAN without a server takes
        crate::task::yield_now();
    }
}
