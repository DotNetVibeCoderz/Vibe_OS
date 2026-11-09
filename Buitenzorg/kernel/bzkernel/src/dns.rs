//! DNS resolver: A records over UDP port 53.
//!
//! The server comes from the DHCP lease (`net::set_dns`); without one, a
//! QEMU user-network address (10.0.2.x) uses QEMU's built-in resolver at
//! 10.0.2.3. Answers are cached for their TTL (at least 30 s, at most an
//! hour). `resolve` blocks its caller (it polls the network and yields, like
//! `repo::http_request`): a query every 2 s, 8 s in all.

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::net::{self, Ipv4Addr};

struct Entry {
    name: String,
    ip: Ipv4Addr,
    expires_ms: u64,
}

static CACHE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static NEXT_ID: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0x4b5a);

fn now_ms() -> u64 {
    crate::arch::cpu::nanos() / 1_000_000
}

/// Dotted-quad text -> address.
pub fn parse_ip(s: &str) -> Option<Ipv4Addr> {
    net::parse_ip(s)
}

/// The resolver in use, if any.
pub fn server() -> Option<Ipv4Addr> {
    let info = net::info()?;
    if let Some(d) = net::dns_server() {
        return Some(d);
    }
    (info.attached && info.ip[0] == 10 && info.ip[1] == 0 && info.ip[2] == 2).then_some([10, 0, 2, 3])
}

/// Build a query for the A record of `name`.
pub fn query(id: u16, name: &str) -> Option<Vec<u8>> {
    let mut q = Vec::with_capacity(18 + name.len());
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]); // RD, 1 question
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]); // root, type A, class IN
    Some(q)
}

/// Skip a (possibly compressed) name; the offset after it.
fn skip_name(m: &[u8], mut p: usize) -> Option<usize> {
    loop {
        let len = *m.get(p)? as usize;
        if len == 0 {
            return Some(p + 1);
        }
        if len & 0xC0 == 0xC0 {
            return Some(p + 2);
        }
        p += 1 + len;
    }
}

/// The first A record of an answer to query `id`: (address, ttl seconds).
/// Err(text) for an answer that says the name does not exist.
pub fn parse_answer(m: &[u8], id: u16) -> Option<Result<(Ipv4Addr, u32), &'static str>> {
    if m.len() < 12 || u16::from_be_bytes([m[0], m[1]]) != id || m[2] & 0x80 == 0 {
        return None;
    }
    let rcode = m[3] & 0x0F;
    if rcode == 3 {
        return Some(Err("nama tidak ditemukan"));
    }
    if rcode != 0 {
        return Some(Err("server DNS menolak"));
    }
    let qd = u16::from_be_bytes([m[4], m[5]]) as usize;
    let an = u16::from_be_bytes([m[6], m[7]]) as usize;
    let mut p = 12;
    for _ in 0..qd {
        p = skip_name(m, p)? + 4;
    }
    for _ in 0..an {
        p = skip_name(m, p)?;
        let h = m.get(p..p + 10)?;
        let (ty, class) = (u16::from_be_bytes([h[0], h[1]]), u16::from_be_bytes([h[2], h[3]]));
        let ttl = u32::from_be_bytes([h[4], h[5], h[6], h[7]]);
        let len = u16::from_be_bytes([h[8], h[9]]) as usize;
        p += 10;
        if ty == 1 && class == 1 && len == 4 {
            let d = m.get(p..p + 4)?;
            return Some(Ok(([d[0], d[1], d[2], d[3]], ttl)));
        }
        p += len; // CNAME and others: the A record follows
    }
    Some(Err("tidak ada alamat IPv4"))
}

/// Resolve a host name (or dotted quad) to an IPv4 address.
pub fn resolve(name: &str) -> Result<Ipv4Addr, String> {
    let name = name.trim().trim_end_matches('.');
    if let Some(ip) = parse_ip(name) {
        return Ok(ip);
    }
    if name.eq_ignore_ascii_case("localhost") {
        return net::local_ip().ok_or_else(|| String::from("jaringan mati"));
    }
    let lower = name.to_ascii_lowercase();
    {
        let now = now_ms();
        let mut c = CACHE.lock();
        c.retain(|e| e.expires_ms > now);
        if let Some(e) = c.iter().find(|e| e.name == lower) {
            return Ok(e.ip);
        }
    }
    let server = server().ok_or_else(|| String::from("tidak ada server DNS (jaringan belum tersambung)"))?;
    let sock = net::udp_socket();
    if sock == 0 {
        return Err(String::from("jaringan mati"));
    }
    let result = (|| {
        let mut bound = false;
        for i in 0..64u16 {
            let port = 53000 + (NEXT_ID.load(core::sync::atomic::Ordering::Relaxed).wrapping_add(i) % 2000);
            if net::udp_bind(sock, port).is_ok() {
                bound = true;
                break;
            }
        }
        if !bound {
            return Err(String::from("tidak ada port bebas"));
        }
        // the first lookup of a name can take seconds upstream (QEMU's user
        // network asks the host's resolver): resend every 2 s, accept an
        // answer to any of our queries, give up after 8 s
        let mut ids: Vec<u16> = Vec::new();
        let start = now_ms();
        let mut next_send = start;
        while now_ms() - start < 8000 {
            if now_ms() >= next_send && ids.len() < 4 {
                let id = NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                let q = query(id, &lower).ok_or_else(|| String::from("nama tidak sah"))?;
                net::udp_send(sock, server, 53, &q).map_err(String::from)?;
                ids.push(id);
                next_send = now_ms() + 2000;
            }
            net::poll();
            while let Some((_, _, reply)) = net::udp_recv(sock) {
                for &id in &ids {
                    match parse_answer(&reply, id) {
                        Some(Ok((ip, ttl))) => {
                            let keep = (ttl.clamp(30, 3600) as u64) * 1000;
                            let mut c = CACHE.lock();
                            if c.len() >= 64 {
                                c.remove(0);
                            }
                            c.push(Entry { name: lower.clone(), ip, expires_ms: now_ms() + keep });
                            return Ok(ip);
                        }
                        Some(Err(e)) => return Err(alloc::format!("{}: {}", name, e)),
                        None => {}
                    }
                }
            }
            crate::task::yield_now();
        }
        Err(alloc::format!("{}: server DNS {} tidak menjawab", name, net::fmt_ip(server)))
    })();
    let _ = net::udp_close(sock);
    result
}

/// What `dns` lists: cached names.
pub fn cached() -> Vec<(String, Ipv4Addr)> {
    CACHE.lock().iter().map(|e| (e.name.clone(), e.ip)).collect()
}

/// Boot check (no network needed): build a query, parse a canned answer with
/// a compressed CNAME before the A record, and NXDOMAIN.
pub fn self_test() -> Result<(), &'static str> {
    let q = query(0x1234, "www.example.com").ok_or("query")?;
    if q.len() != 12 + 17 + 4 || q[12] != 3 || &q[13..16] != b"www" {
        return Err("query layout");
    }
    let mut a = q.clone();
    a[2] = 0x81;
    a[3] = 0x80;
    a[7] = 2; // two answers
    // CNAME www -> pointer to offset 12, TTL 60, rdata = pointer to 16 ("example.com")
    a.extend_from_slice(&[0xC0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 2, 0xC0, 16]);
    a.extend_from_slice(&[0xC0, 16, 0, 1, 0, 1, 0, 0, 0x0E, 0x10, 0, 4, 93, 184, 215, 14]);
    match parse_answer(&a, 0x1234) {
        Some(Ok(([93, 184, 215, 14], 3600))) => {}
        _ => return Err("answer with CNAME"),
    }
    if parse_answer(&a, 0x1235).is_some() {
        return Err("wrong id accepted");
    }
    a[3] = 0x83;
    if !matches!(parse_answer(&a, 0x1234), Some(Err(_))) {
        return Err("NXDOMAIN");
    }
    if parse_ip("10.0.2.2") != Some([10, 0, 2, 2]) || parse_ip("10.0.2") .is_some() || query(1, "a..b").is_some() {
        return Err("parse_ip / labels");
    }
    println!("[kernel] MILESTONE: DNS OK (query, CNAME + A answer, NXDOMAIN, cache; server {})",
        server().map(net::fmt_ip).unwrap_or_else(|| String::from("none yet")));
    Ok(())
}
