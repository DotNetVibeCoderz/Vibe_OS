//! TCP for ring-3 programs (syscall `TCP_OP`): a per-process handle table
//! over the kernel's `tcp.rs` connections and listeners. A handle belongs to
//! the process that made it; `on_exit` closes whatever a program left open.
//! Nothing here blocks: programs poll STATE / RECV (the syscalls run
//! `net::poll` first, so the stack moves while they wait).

use alloc::vec::Vec;
use bz_abi::tcp_state;
use spin::Mutex;

use crate::tcp::{self, State};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Conn(u64),
    Listen(u16),
}

struct Handle {
    id: u64,
    pid: u64,
    kind: Kind,
}

struct Table {
    next: u64,
    handles: Vec<Handle>,
}

static TABLE: Mutex<Table> = Mutex::new(Table { next: 1, handles: Vec::new() });

/// Most a program may have queued and unsent on one connection.
const MAX_UNSENT: usize = 256 * 1024;
/// Largest single SEND / RECV.
pub const MAX_IO: usize = 64 * 1024;

fn add(pid: u64, kind: Kind) -> u64 {
    let mut t = TABLE.lock();
    let id = t.next;
    t.next += 1;
    t.handles.push(Handle { id, pid, kind });
    id
}

fn lookup(pid: u64, id: u64) -> Option<Kind> {
    TABLE.lock().handles.iter().find(|h| h.id == id && h.pid == pid).map(|h| h.kind)
}

pub fn connect(pid: u64, ip: [u8; 4], port: u16) -> Result<u64, &'static str> {
    if port == 0 {
        return Err("port 0");
    }
    crate::net::poll();
    let c = tcp::connect(ip, port)?;
    Ok(add(pid, Kind::Conn(c)))
}

pub fn listen(pid: u64, port: u16) -> Result<u64, &'static str> {
    if port == 0 {
        return Err("port 0");
    }
    tcp::listen(port)?;
    Ok(add(pid, Kind::Listen(port)))
}

pub fn accept(pid: u64, id: u64) -> Option<u64> {
    let Some(Kind::Listen(port)) = lookup(pid, id) else { return None };
    crate::net::poll();
    tcp::accept(port).map(|c| add(pid, Kind::Conn(c)))
}

/// Bytes queued (0 when the send queue is full: try again later), or None
/// for a bad handle / a connection that can no longer send.
pub fn send(pid: u64, id: u64, data: &[u8]) -> Option<usize> {
    let Some(Kind::Conn(c)) = lookup(pid, id) else { return None };
    crate::net::poll();
    if !matches!(tcp::state(c), State::Established | State::CloseWait) {
        return None;
    }
    if tcp::unsent(c) > MAX_UNSENT {
        return Some(0);
    }
    Some(tcp::send(c, data))
}

pub fn recv(pid: u64, id: u64, max: usize) -> Option<Vec<u8>> {
    let Some(Kind::Conn(c)) = lookup(pid, id) else { return None };
    crate::net::poll();
    Some(tcp::recv(c, max.min(MAX_IO)))
}

pub fn state(pid: u64, id: u64) -> Option<u64> {
    let kind = lookup(pid, id)?;
    crate::net::poll();
    Some(match kind {
        Kind::Listen(_) => tcp_state::OPEN,
        Kind::Conn(c) => match tcp::state(c) {
            State::SynSent | State::SynReceived => tcp_state::CONNECTING,
            State::Established | State::FinWait => tcp_state::OPEN,
            State::CloseWait | State::LastAck if tcp::at_eof(c) => tcp_state::PEER_CLOSED,
            State::CloseWait | State::LastAck => tcp_state::OPEN,
            State::Closed if tcp::at_eof(c) || tcp::recv_pending(c) == 0 => tcp_state::CLOSED,
            State::Closed => tcp_state::OPEN, // closed, but data is still to be read
        },
    })
}

fn release(kind: Kind) {
    match kind {
        Kind::Conn(c) => tcp::close(c),
        Kind::Listen(port) => tcp::unlisten(port),
    }
}

pub fn close(pid: u64, id: u64) -> bool {
    let kind = {
        let mut t = TABLE.lock();
        let Some(i) = t.handles.iter().position(|h| h.id == id && h.pid == pid) else { return false };
        t.handles.remove(i).kind
    };
    release(kind);
    true
}

/// A program ended: close what it left open.
pub fn on_exit(pid: u64) {
    let gone: Vec<Kind> = {
        let mut t = TABLE.lock();
        let (mine, keep): (Vec<Handle>, Vec<Handle>) = core::mem::take(&mut t.handles).into_iter().partition(|h| h.pid == pid);
        t.handles = keep;
        mine.into_iter().map(|h| h.kind).collect()
    };
    for k in gone {
        release(k);
    }
}

/// How many handles a process holds (tests, `netstat`).
#[allow(dead_code)]
pub fn count(pid: u64) -> usize {
    TABLE.lock().handles.iter().filter(|h| h.pid == pid).count()
}

