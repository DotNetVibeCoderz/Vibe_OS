//! TCP (RFC 793/9293, the parts a small server and client need): passive
//! open (listen / accept) and active open (connect), sequence numbers and
//! acknowledgements, the receive window, MSS-sized segments, retransmission
//! of unacknowledged data with exponential back-off, FIN / RST close.
//! Deliberately simple: out-of-order segments are dropped and re-acked (the
//! peer retransmits), no SACK / window scaling / Nagle / congestion control,
//! no TIME_WAIT (a closed connection is freed at once).
//!
//! Locking: `TCP` is never taken while `net::STACK` is held. Input arrives
//! from `net::handle_ipv4` (no lock held); segments to send are collected
//! under `TCP` and handed to `net::send_ipv4` after it is released.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;

use crate::net::Ipv4Addr;

pub const IPPROTO_TCP: u8 = 6;
const MSS: usize = 1460;
const RCV_WINDOW: usize = 32 * 1024;
const RTO_NS: u64 = 400_000_000;
const MAX_RETRIES: u32 = 8;

const FIN: u8 = 0x01;
const SYN: u8 = 0x02;
const RST: u8 = 0x04;
const PSH: u8 = 0x08;
const ACK: u8 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    SynSent,
    SynReceived,
    Established,
    /// We sent FIN, waiting for its ACK / the peer's FIN.
    FinWait,
    /// The peer sent FIN; we may still send.
    CloseWait,
    /// Both sides sent FIN, waiting for the ACK of ours.
    LastAck,
    Closed,
}

struct Conn {
    id: u64,
    local_ip: Ipv4Addr,
    local_port: u16,
    remote_ip: Ipv4Addr,
    remote_port: u16,
    state: State,
    /// Oldest unacknowledged sequence number / next one to send.
    snd_una: u32,
    snd_nxt: u32,
    /// Bytes sent but not acknowledged (starting at snd_una), then unsent.
    unacked: VecDeque<u8>,
    pending: VecDeque<u8>,
    peer_window: usize,
    rcv_nxt: u32,
    rx: VecDeque<u8>,
    /// We owe a FIN (close requested, sent after the pending data).
    fin_queued: bool,
    fin_sent: bool,
    peer_fin: bool,
    last_tx_ns: u64,
    retries: u32,
    /// From a listener and not yet returned by `accept`.
    listener: Option<u16>,
    accepted: bool,
    /// The application closed its handle: free once the connection is closed.
    released: bool,
}

struct Tcp {
    conns: Vec<Conn>,
    listeners: Vec<u16>,
    next_id: u64,
    iss: u32,
    ephemeral: u16,
}

static TCP: Mutex<Tcp> = Mutex::new(Tcp { conns: Vec::new(), listeners: Vec::new(), next_id: 1, iss: 0x1000_0000, ephemeral: 49152 });

/// A segment to send: (destination, TCP header + payload).
type Out = (Ipv4Addr, Vec<u8>);

fn now() -> u64 {
    crate::arch::cpu::nanos()
}

fn seq_lt(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}

fn seq_le(a: u32, b: u32) -> bool {
    a == b || seq_lt(a, b)
}

fn checksum(src: Ipv4Addr, dst: Ipv4Addr, seg: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut add = |b: &[u8]| {
        let mut i = 0;
        while i + 1 < b.len() {
            sum += u16::from_be_bytes([b[i], b[i + 1]]) as u32;
            i += 2;
        }
        if i < b.len() {
            sum += (b[i] as u32) << 8;
        }
    };
    add(&src);
    add(&dst);
    add(&[0, IPPROTO_TCP]);
    add(&(seg.len() as u16).to_be_bytes());
    add(seg);
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

fn segment(c: &Conn, seq: u32, flags: u8, data: &[u8]) -> Out {
    let syn = flags & SYN != 0;
    let hlen = if syn { 24 } else { 20 };
    let mut s = Vec::with_capacity(hlen + data.len());
    s.extend_from_slice(&c.local_port.to_be_bytes());
    s.extend_from_slice(&c.remote_port.to_be_bytes());
    s.extend_from_slice(&seq.to_be_bytes());
    s.extend_from_slice(&(if flags & ACK != 0 { c.rcv_nxt } else { 0 }).to_be_bytes());
    s.push(((hlen / 4) as u8) << 4);
    s.push(flags);
    let window = RCV_WINDOW.saturating_sub(c.rx.len()).min(0xFFFF) as u16;
    s.extend_from_slice(&window.to_be_bytes());
    s.extend_from_slice(&[0, 0, 0, 0]); // checksum, urgent pointer
    if syn {
        s.extend_from_slice(&[2, 4]); // MSS option
        s.extend_from_slice(&(MSS as u16).to_be_bytes());
    }
    s.extend_from_slice(data);
    let cs = checksum(c.local_ip, c.remote_ip, &s);
    s[16..18].copy_from_slice(&cs.to_be_bytes());
    (c.remote_ip, s)
}

fn rst_for(src: Ipv4Addr, dst: Ipv4Addr, seg: &[u8], len: usize) -> Option<Out> {
    let flags = seg[13];
    if flags & RST != 0 {
        return None;
    }
    let (sport, dport) = (u16::from_be_bytes([seg[0], seg[1]]), u16::from_be_bytes([seg[2], seg[3]]));
    let seq = u32::from_be_bytes([seg[4], seg[5], seg[6], seg[7]]);
    let ack = u32::from_be_bytes([seg[8], seg[9], seg[10], seg[11]]);
    let mut s = Vec::with_capacity(20);
    s.extend_from_slice(&dport.to_be_bytes());
    s.extend_from_slice(&sport.to_be_bytes());
    let (my_seq, my_ack, f) = if flags & ACK != 0 {
        (ack, 0u32, RST)
    } else {
        let l = len as u32 + (flags & SYN != 0) as u32 + (flags & FIN != 0) as u32;
        (0, seq.wrapping_add(l), RST | ACK)
    };
    s.extend_from_slice(&my_seq.to_be_bytes());
    s.extend_from_slice(&my_ack.to_be_bytes());
    s.push(5 << 4);
    s.push(f);
    s.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    let cs = checksum(dst, src, &s);
    s[16..18].copy_from_slice(&cs.to_be_bytes());
    Some((src, s))
}

impl Tcp {
    fn conn(&mut self, id: u64) -> Option<&mut Conn> {
        self.conns.iter_mut().find(|c| c.id == id)
    }

    fn new_conn(&mut self, local_ip: Ipv4Addr, local_port: u16, remote_ip: Ipv4Addr, remote_port: u16, state: State) -> Conn {
        self.iss = self.iss.wrapping_add(0x0010_3f05 ^ (now() as u32 & 0xFFFF));
        let id = self.next_id;
        self.next_id += 1;
        Conn {
            id,
            local_ip,
            local_port,
            remote_ip,
            remote_port,
            state,
            snd_una: self.iss,
            snd_nxt: self.iss,
            unacked: VecDeque::new(),
            pending: VecDeque::new(),
            peer_window: MSS,
            rcv_nxt: 0,
            rx: VecDeque::new(),
            fin_queued: false,
            fin_sent: false,
            peer_fin: false,
            last_tx_ns: now(),
            retries: 0,
            listener: None,
            accepted: false,
            released: false,
        }
    }
}

/// Send what the window allows: pending data, then a queued FIN.
fn push(c: &mut Conn, out: &mut Vec<Out>) {
    if !matches!(c.state, State::Established | State::CloseWait | State::FinWait | State::LastAck) {
        return;
    }
    loop {
        let in_flight = c.unacked.len();
        let room = c.peer_window.saturating_sub(in_flight);
        if c.pending.is_empty() || room == 0 {
            break;
        }
        let n = c.pending.len().min(MSS).min(room);
        let data: Vec<u8> = c.pending.drain(..n).collect();
        out.push(segment(c, c.snd_nxt, ACK | PSH, &data));
        c.snd_nxt = c.snd_nxt.wrapping_add(n as u32);
        c.unacked.extend(data);
        c.last_tx_ns = now();
    }
    if c.fin_queued && !c.fin_sent && c.pending.is_empty() {
        out.push(segment(c, c.snd_nxt, ACK | FIN, &[]));
        c.snd_nxt = c.snd_nxt.wrapping_add(1);
        c.fin_sent = true;
        c.last_tx_ns = now();
        c.state = if c.peer_fin { State::LastAck } else { State::FinWait };
    }
}

/// A TCP segment arrived (`seg` = header + payload, `len` = payload length
/// from the IP header). Called by `net` with no lock held.
pub fn input(src: Ipv4Addr, dst: Ipv4Addr, seg: &[u8]) {
    let out = {
        let mut t = TCP.lock();
        input_locked(&mut t, src, dst, seg)
    };
    for (dest, s) in out {
        crate::net::send_ipv4(dest, IPPROTO_TCP, &s);
    }
}

fn input_locked(t: &mut Tcp, src: Ipv4Addr, dst: Ipv4Addr, seg: &[u8]) -> Vec<Out> {
    let mut out = Vec::new();
    if seg.len() < 20 {
        return out;
    }
    let doff = ((seg[12] >> 4) as usize) * 4;
    if doff < 20 || doff > seg.len() || checksum(src, dst, seg) != 0 {
        return out;
    }
    let (sport, dport) = (u16::from_be_bytes([seg[0], seg[1]]), u16::from_be_bytes([seg[2], seg[3]]));
    let seq = u32::from_be_bytes([seg[4], seg[5], seg[6], seg[7]]);
    let ack = u32::from_be_bytes([seg[8], seg[9], seg[10], seg[11]]);
    let flags = seg[13];
    let window = u16::from_be_bytes([seg[14], seg[15]]) as usize;
    let data = &seg[doff..];

    let idx = t.conns.iter().position(|c| c.local_port == dport && c.remote_port == sport && c.remote_ip == src && c.state != State::Closed);
    let Some(i) = idx else {
        // a new connection to a listening port
        if flags & SYN != 0 && flags & ACK == 0 && t.listeners.contains(&dport) {
            let mut c = t.new_conn(dst, dport, src, sport, State::SynReceived);
            c.rcv_nxt = seq.wrapping_add(1);
            c.peer_window = window.max(1);
            c.listener = Some(dport);
            let syn_ack = segment(&c, c.snd_nxt, SYN | ACK, &[]);
            c.snd_nxt = c.snd_nxt.wrapping_add(1);
            out.push(syn_ack);
            t.conns.push(c);
        } else if let Some(r) = rst_for(src, dst, seg, data.len()) {
            out.push(r);
        }
        return out;
    };
    let c = &mut t.conns[i];
    if flags & RST != 0 {
        c.state = State::Closed;
        return out;
    }
    c.peer_window = window;

    if c.state == State::SynSent {
        if flags & SYN != 0 && flags & ACK != 0 && ack == c.snd_nxt {
            c.rcv_nxt = seq.wrapping_add(1);
            c.snd_una = ack;
            c.state = State::Established;
            c.retries = 0;
            out.push(segment(c, c.snd_nxt, ACK, &[]));
            push(c, &mut out);
        }
        return out;
    }

    // acknowledgement of what we sent
    if flags & ACK != 0 && seq_lt(c.snd_una, ack) && seq_le(ack, c.snd_nxt) {
        let mut acked = ack.wrapping_sub(c.snd_una) as usize;
        if c.state == State::SynReceived {
            acked = acked.saturating_sub(1); // our SYN
            c.state = State::Established;
        }
        let data_acked = acked.min(c.unacked.len());
        c.unacked.drain(..data_acked);
        c.snd_una = ack;
        c.retries = 0;
        c.last_tx_ns = now();
        if c.fin_sent && ack == c.snd_nxt {
            if c.state == State::LastAck || (c.state == State::FinWait && c.peer_fin) {
                c.state = State::Closed;
            }
        }
    }
    if c.state == State::SynReceived {
        return out; // still waiting for the handshake's ACK
    }

    // data, in order only
    let mut need_ack = false;
    if !data.is_empty() || flags & FIN != 0 {
        if seq == c.rcv_nxt {
            let room = RCV_WINDOW.saturating_sub(c.rx.len());
            let take = data.len().min(room);
            c.rx.extend(&data[..take]);
            c.rcv_nxt = c.rcv_nxt.wrapping_add(take as u32);
            if flags & FIN != 0 && take == data.len() && !c.peer_fin {
                c.peer_fin = true;
                c.rcv_nxt = c.rcv_nxt.wrapping_add(1);
                c.state = match c.state {
                    State::Established => State::CloseWait,
                    State::FinWait if c.fin_sent && c.snd_una == c.snd_nxt => State::Closed,
                    State::FinWait => State::FinWait,
                    s => s,
                };
            }
        }
        need_ack = true; // ack in-order data, re-ack a gap
    }
    let before = out.len();
    push(c, &mut out);
    if need_ack && out.len() == before {
        out.push(segment(c, c.snd_nxt, ACK, &[]));
    }
    out
}

/// Retransmit unacknowledged data / SYN / FIN whose timer ran out; drop
/// connections that stopped answering; free closed, released ones. Called
/// from `net::poll`.
pub fn timers() {
    let out = {
        let Some(mut t) = TCP.try_lock() else { return };
        let mut out = Vec::new();
        let n = now();
        for c in t.conns.iter_mut() {
            let waiting = !c.unacked.is_empty()
                || (c.fin_sent && c.snd_una != c.snd_nxt)
                || matches!(c.state, State::SynSent | State::SynReceived);
            if !waiting || c.state == State::Closed {
                continue;
            }
            let rto = RTO_NS << c.retries.min(6);
            if n.saturating_sub(c.last_tx_ns) < rto {
                continue;
            }
            if c.retries >= MAX_RETRIES {
                c.state = State::Closed;
                continue;
            }
            c.retries += 1;
            c.last_tx_ns = n;
            match c.state {
                State::SynSent => out.push(segment(c, c.snd_una, SYN, &[])),
                State::SynReceived => out.push(segment(c, c.snd_una, SYN | ACK, &[])),
                _ => {
                    if !c.unacked.is_empty() {
                        let len = c.unacked.len().min(MSS);
                        let data: Vec<u8> = c.unacked.iter().take(len).copied().collect();
                        out.push(segment(c, c.snd_una, ACK | PSH, &data));
                    } else if c.fin_sent {
                        out.push(segment(c, c.snd_nxt.wrapping_sub(1), ACK | FIN, &[]));
                    }
                }
            }
        }
        t.conns.retain(|c| !(c.state == State::Closed && (c.released || (c.listener.is_some() && !c.accepted))));
        out
    };
    for (dest, s) in out {
        crate::net::send_ipv4(dest, IPPROTO_TCP, &s);
    }
}

// ---- the application side ---------------------------------------------------

/// Accept connections on `port`.
pub fn listen(port: u16) -> Result<(), &'static str> {
    let mut t = TCP.lock();
    if t.listeners.contains(&port) {
        return Err("port already listening");
    }
    t.listeners.push(port);
    Ok(())
}

pub fn unlisten(port: u16) {
    TCP.lock().listeners.retain(|&p| p != port);
}

/// The next established connection on a listening `port`, if any.
pub fn accept(port: u16) -> Option<u64> {
    let mut t = TCP.lock();
    let c = t.conns.iter_mut().find(|c| c.listener == Some(port) && !c.accepted && matches!(c.state, State::Established | State::CloseWait))?;
    c.accepted = true;
    Some(c.id)
}

/// Open a connection to `ip:port` (returns at once; see [`state`]).
pub fn connect(ip: Ipv4Addr, port: u16) -> Result<u64, &'static str> {
    let local_ip = crate::net::local_ip().ok_or("no network")?;
    let (id, out) = {
        let mut t = TCP.lock();
        t.ephemeral = if t.ephemeral >= 65000 { 49152 } else { t.ephemeral + 1 };
        let lp = t.ephemeral;
        let mut c = t.new_conn(local_ip, lp, ip, port, State::SynSent);
        c.accepted = true;
        let syn = segment(&c, c.snd_nxt, SYN, &[]);
        c.snd_nxt = c.snd_nxt.wrapping_add(1);
        let id = c.id;
        t.conns.push(c);
        (id, syn)
    };
    crate::net::send_ipv4(out.0, IPPROTO_TCP, &out.1);
    Ok(id)
}

pub fn state(id: u64) -> State {
    TCP.lock().conn(id).map_or(State::Closed, |c| c.state)
}

/// (remote ip, remote port) of a connection.
#[allow(dead_code)]
pub fn peer(id: u64) -> Option<(Ipv4Addr, u16)> {
    TCP.lock().conn(id).map(|c| (c.remote_ip, c.remote_port))
}

/// Queue `data` for sending. Returns the bytes queued (0 once closed).
pub fn send(id: u64, data: &[u8]) -> usize {
    let out = {
        let mut t = TCP.lock();
        let Some(c) = t.conn(id) else { return 0 };
        if !matches!(c.state, State::Established | State::CloseWait | State::SynSent | State::SynReceived) || c.fin_queued {
            return 0;
        }
        c.pending.extend(data);
        let mut out = Vec::new();
        push(c, &mut out);
        out
    };
    for (dest, s) in out {
        crate::net::send_ipv4(dest, IPPROTO_TCP, &s);
    }
    data.len()
}

/// Bytes waiting to be acknowledged or sent.
pub fn unsent(id: u64) -> usize {
    TCP.lock().conn(id).map_or(0, |c| c.pending.len() + c.unacked.len())
}

/// Take up to `max` received bytes (empty: nothing yet, or the peer closed
/// -- see [`at_eof`]).
pub fn recv(id: u64, max: usize) -> Vec<u8> {
    let (data, out) = {
        let mut t = TCP.lock();
        let Some(c) = t.conn(id) else { return Vec::new() };
        let was_full = c.rx.len() >= RCV_WINDOW / 2;
        let n = c.rx.len().min(max);
        let data: Vec<u8> = c.rx.drain(..n).collect();
        // a window that reopens is announced (the peer may be waiting on it)
        let out = if was_full && n > 0 { Some(segment(c, c.snd_nxt, ACK, &[])) } else { None };
        (data, out)
    };
    if let Some((dest, s)) = out {
        crate::net::send_ipv4(dest, IPPROTO_TCP, &s);
    }
    data
}

/// Received bytes not read yet.
pub fn recv_pending(id: u64) -> usize {
    TCP.lock().conn(id).map_or(0, |c| c.rx.len())
}

/// The peer closed its side and every byte was read.
pub fn at_eof(id: u64) -> bool {
    TCP.lock().conn(id).map_or(true, |c| (c.peer_fin || c.state == State::Closed) && c.rx.is_empty())
}

/// Close our side (after the queued data); the handle is released.
pub fn close(id: u64) {
    let out = {
        let mut t = TCP.lock();
        let Some(c) = t.conn(id) else { return };
        c.released = true;
        let mut out = Vec::new();
        match c.state {
            State::SynSent | State::Closed => c.state = State::Closed,
            _ => {
                c.fin_queued = true;
                push(c, &mut out);
            }
        }
        out
    };
    for (dest, s) in out {
        crate::net::send_ipv4(dest, IPPROTO_TCP, &s);
    }
}

/// (local port, remote ip, remote port, state) of every connection, and the
/// listening ports (`netstat`).
pub fn snapshot() -> (Vec<(u16, Ipv4Addr, u16, State)>, Vec<u16>) {
    let t = TCP.lock();
    (t.conns.iter().map(|c| (c.local_port, c.remote_ip, c.remote_port, c.state)).collect(), t.listeners.clone())
}

/// Number of open connections (diagnostics).
#[allow(dead_code)]
pub fn connections() -> usize {
    TCP.lock().conns.iter().filter(|c| c.state != State::Closed).count()
}

/// Loopback check (`TCP OK`): listen, connect to our own address, exchange
/// 64 KiB each way (many segments, the window), close both sides.
pub fn self_test() -> Result<alloc::string::String, &'static str> {
    let ip = crate::net::local_ip().ok_or("no network")?;
    const PORT: u16 = 7071;
    listen(PORT)?;
    let result = (|| {
        let client = connect(ip, PORT)?;
        let mut server = None;
        let spin = |cond: &mut dyn FnMut() -> bool| -> bool {
            let start = now();
            while now().saturating_sub(start) < 3_000_000_000 {
                crate::net::poll();
                if cond() {
                    return true;
                }
            }
            false
        };
        if !spin(&mut || {
            if server.is_none() {
                server = accept(PORT);
            }
            server.is_some() && state(client) == State::Established
        }) {
            return Err("handshake did not complete");
        }
        let server = server.unwrap();
        let msg: Vec<u8> = (0..65536u32).map(|i| (i * 7 + 3) as u8).collect();
        let mut got = Vec::new();
        let mut echoed = Vec::new();
        let mut sent = 0;
        if !spin(&mut || {
            if sent < msg.len() && unsent(client) < 8192 {
                let n = (msg.len() - sent).min(4096);
                sent += send(client, &msg[sent..sent + n]);
            }
            let part = recv(server, 8192);
            if !part.is_empty() {
                send(server, &part); // echo
                got.extend(part);
            }
            echoed.extend(recv(client, 8192));
            echoed.len() >= msg.len()
        }) {
            return Err("data exchange stalled");
        }
        if got != msg || echoed != msg {
            return Err("data corrupted");
        }
        close(client);
        if !spin(&mut || at_eof(server)) {
            return Err("FIN not delivered");
        }
        close(server);
        if !spin(&mut || state(client) == State::Closed && state(server) == State::Closed) {
            return Err("close did not finish");
        }
        Ok(alloc::format!("loopback {}:{}: handshake, 64 KiB echoed both ways, FIN close", crate::net::fmt_ip(ip), PORT))
    })();
    unlisten(PORT);
    timers();
    result
}
