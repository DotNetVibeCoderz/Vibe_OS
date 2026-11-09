//! Network natives for managed programs (`Buitenzorg.Net` in BZAPP.DLL):
//! DNS and TCP over the kernel's DNS_RESOLVE / TCP_OP syscalls, and TLS
//! (HTTPS) done here in the host with rustls -- `no_std`, the RustCrypto
//! provider, Mozilla's root certificates (webpki-roots) plus any roots a
//! program adds, the real-time clock for certificate dates and the kernel's
//! RANDOM syscall for key material. Nothing blocks: the managed side polls
//! (`TlsPump` moves bytes between the socket and rustls).

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use rustclr_core::{ArrayStorage, ClrArray, ExecResult, Interpreter, Value};
use rustls::client::UnbufferedClientConnection;
use rustls::unbuffered::{ConnectionState, EncodeError, EncryptError};

use crate::bzapi::{int, string};
use crate::sys;

const TCP_OP: u64 = 64;
const DNS_RESOLVE: u64 = 65;
const RANDOM: u64 = 57;
const OP_CONNECT: u64 = 0;
const OP_SEND: u64 = 1;
const OP_RECV: u64 = 2;
const OP_STATE: u64 = 3;
const OP_CLOSE: u64 = 4;
const OP_LISTEN: u64 = 5;
const OP_ACCEPT: u64 = 6;

fn ret_i64(v: i64) -> ExecResult<Option<Value>> {
    Ok(Some(Value::I64(v)))
}
fn ret_i32(v: i64) -> ExecResult<Option<Value>> {
    Ok(Some(Value::I32(v as i32)))
}

fn cstr(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s.as_bytes());
    v.push(0);
    v
}

/// `count` bytes of the byte[] argument `i` from `off`.
fn byte_range(interp: &Interpreter, a: &[Value], i: usize, off: usize, count: usize) -> Vec<u8> {
    let Some(Value::Obj(h)) = a.get(i) else { return Vec::new() };
    if h.is_null() {
        return Vec::new();
    }
    interp
        .heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::U8(v) => v.get(off.min(v.len())..off.saturating_add(count).min(v.len())).map(|s| s.to_vec()).unwrap_or_default(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

/// The capacity of byte[] argument `i` from `off` (at most `count`).
fn room(interp: &Interpreter, a: &[Value], i: usize, off: usize, count: usize) -> usize {
    let Some(Value::Obj(h)) = a.get(i) else { return 0 };
    if h.is_null() {
        return 0;
    }
    interp
        .heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::U8(v) => v.len().saturating_sub(off).min(count),
            _ => 0,
        })
        .unwrap_or(0)
}

fn store_at(interp: &mut Interpreter, a: &[Value], i: usize, off: usize, data: &[u8]) -> bool {
    let Some(Value::Obj(h)) = a.get(i) else { return false };
    interp
        .heap
        .with_mut::<ClrArray, _>(*h, |arr| match &mut arr.storage {
            ArrayStorage::U8(v) if off + data.len() <= v.len() => {
                v[off..off + data.len()].copy_from_slice(data);
                true
            }
            _ => false,
        })
        .unwrap_or(false)
}

// ---- DNS + TCP ------------------------------------------------------------

/// `DnsResolve(string name)` -> IPv4 address (a << 24 | b << 16 | c << 8 | d), -1.
fn dns_resolve(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let name = cstr(&string(i, a, 0));
    let r = sys::syscall(DNS_RESOLVE, name.as_ptr() as u64, 0, 0);
    ret_i64(if sys::is_err(r) { -1 } else { r as i64 })
}

fn tcp(op: u64, handle: u64, a1: u64, a2: u64) -> u64 {
    sys::syscall(TCP_OP, op | handle << 8, a1, a2)
}

/// `TcpConnect(long ip, int port)` -> handle, -1.
fn tcp_connect(_: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = tcp(OP_CONNECT, 0, int(a, 0) as u32 as u64, int(a, 1) as u64);
    ret_i64(if sys::is_err(r) { -1 } else { r as i64 })
}

/// `TcpListen(int port)` -> handle, -1.
fn tcp_listen(_: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = tcp(OP_LISTEN, 0, 0, int(a, 0) as u64);
    ret_i64(if sys::is_err(r) { -1 } else { r as i64 })
}

/// `TcpAccept(long listener)` -> a connection's handle, 0 = none yet, -1.
fn tcp_accept(_: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = tcp(OP_ACCEPT, int(a, 0) as u64, 0, 0);
    ret_i64(if sys::is_err(r) { -1 } else { r as i64 })
}

fn raw_send(handle: u64, data: &[u8]) -> i64 {
    let r = tcp(OP_SEND, handle, data.as_ptr() as u64, data.len() as u64);
    if sys::is_err(r) { -1 } else { r as i64 }
}

fn raw_recv(handle: u64, buf: &mut [u8]) -> i64 {
    let r = tcp(OP_RECV, handle, buf.as_mut_ptr() as u64, buf.len() as u64);
    if sys::is_err(r) { -1 } else { r as i64 }
}

fn raw_state(handle: u64) -> i64 {
    let r = tcp(OP_STATE, handle, 0, 0);
    if sys::is_err(r) { 3 } else { r as i64 }
}

/// `TcpSend(long h, byte[] data, int offset, int count)` -> bytes queued, -1.
fn tcp_send(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let data = byte_range(i, a, 1, int(a, 2).max(0) as usize, int(a, 3).max(0) as usize);
    ret_i32(raw_send(int(a, 0) as u64, &data))
}

/// `TcpRecv(long h, byte[] buffer, int offset, int count)` -> bytes read (0 = none yet), -1.
fn tcp_recv(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let off = int(a, 2).max(0) as usize;
    let cap = room(i, a, 1, off, int(a, 3).max(0) as usize).min(64 * 1024);
    if cap == 0 {
        return ret_i32(-1);
    }
    let mut buf = alloc::vec![0u8; cap];
    let n = raw_recv(int(a, 0) as u64, &mut buf);
    if n > 0 {
        store_at(i, a, 1, off, &buf[..n as usize]);
    }
    ret_i32(n)
}

/// `TcpState(long h)` -> 0 connecting, 1 open, 2 peer closed, 3 closed.
fn tcp_state(_: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    ret_i32(raw_state(int(a, 0) as u64))
}

/// `TcpClose(long h)`.
fn tcp_close(_: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = tcp(OP_CLOSE, int(a, 0) as u64, 0, 0);
    ret_i32(if sys::is_err(r) { -1 } else { 0 })
}

// ---- TLS ------------------------------------------------------------------

fn random_fill(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    for chunk in buf.chunks_mut(4096) {
        let r = sys::syscall(RANDOM, chunk.as_mut_ptr() as u64, chunk.len() as u64, 0);
        if sys::is_err(r) {
            return Err(getrandom::Error::UNSUPPORTED);
        }
    }
    Ok(())
}
getrandom::register_custom_getrandom!(random_fill);

#[derive(Debug)]
struct Clock;
impl rustls::time_provider::TimeProvider for Clock {
    fn current_time(&self) -> Option<rustls::pki_types::UnixTime> {
        let ms = sys::unix_millis();
        (ms > 0).then(|| rustls::pki_types::UnixTime::since_unix_epoch(core::time::Duration::from_millis(ms as u64)))
    }
}

struct Tls {
    id: i64,
    tcp: u64,
    conn: UnbufferedClientConnection,
    incoming: Vec<u8>,
    outgoing: Vec<u8>,
    plain_in: Vec<u8>,
    plain_out: Vec<u8>,
    open: bool,
    close_asked: bool,
    closed: bool,
    error: String,
}

struct TlsTable {
    next: i64,
    conns: Vec<Tls>,
    extra_roots: Vec<Vec<u8>>,
    config: Option<Arc<rustls::ClientConfig>>,
}

/// A spin lock (natives run on whichever managed thread calls them).
pub(crate) struct Lock<T> {
    held: core::sync::atomic::AtomicBool,
    value: core::cell::UnsafeCell<T>,
}
unsafe impl<T: Send> Sync for Lock<T> {}
pub(crate) struct Guard<'a, T>(&'a Lock<T>);
impl<T> Lock<T> {
    pub(crate) const fn new(v: T) -> Self {
        Lock { held: core::sync::atomic::AtomicBool::new(false), value: core::cell::UnsafeCell::new(v) }
    }
    pub(crate) fn lock(&self) -> Guard<'_, T> {
        use core::sync::atomic::Ordering;
        while self.held.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        Guard(self)
    }
}
impl<T> core::ops::Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.0.value.get() }
    }
}
impl<T> core::ops::DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.0.value.get() }
    }
}
impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        self.0.held.store(false, core::sync::atomic::Ordering::Release);
    }
}

/// Natives run on whichever managed thread calls them; one lock guards the table.
static TLS: Lock<TlsTable> = Lock::new(TlsTable { next: 1, conns: Vec::new(), extra_roots: Vec::new(), config: None });

fn config(t: &mut TlsTable) -> Result<Arc<rustls::ClientConfig>, String> {
    if let Some(c) = &t.config {
        return Ok(c.clone());
    }
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    for der in &t.extra_roots {
        roots.add(rustls::pki_types::CertificateDer::from(der.clone())).map_err(|e| alloc::format!("{}", e))?;
    }
    let c = rustls::ClientConfig::builder_with_details(Arc::new(rustls_rustcrypto::provider()), Arc::new(Clock))
        .with_safe_default_protocol_versions()
        .map_err(|e| alloc::format!("{}", e))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let c = Arc::new(c);
    t.config = Some(c.clone());
    Ok(c)
}

impl Tls {
    fn fail(&mut self, e: impl core::fmt::Display) {
        if self.error.is_empty() {
            self.error = alloc::format!("{}", e);
        }
        self.closed = true;
    }

    /// Move everything that can move: socket -> rustls -> plaintext, and
    /// queued plaintext -> rustls -> socket.
    fn pump(&mut self) {
        if self.closed {
            return;
        }
        // whatever the socket has
        let mut buf = alloc::vec![0u8; 16 * 1024];
        loop {
            let n = raw_recv(self.tcp, &mut buf);
            if n <= 0 {
                break;
            }
            self.incoming.extend_from_slice(&buf[..n as usize]);
        }
        let mut rounds = 0;
        loop {
            rounds += 1;
            if rounds > 64 {
                break;
            }
            let status = self.conn.process_tls_records(&mut self.incoming);
            let mut discard = status.discard;
            let mut stop = false;
            match status.state {
                Err(e) => {
                    let msg = alloc::format!("{}", e);
                    self.incoming.drain(..discard.min(self.incoming.len()));
                    self.fail(msg);
                    return;
                }
                Ok(ConnectionState::ReadTraffic(mut r)) => {
                    while let Some(rec) = r.next_record() {
                        match rec {
                            Ok(rec) => {
                                discard += rec.discard;
                                self.plain_in.extend_from_slice(rec.payload);
                            }
                            Err(_) => break,
                        }
                    }
                }
                Ok(ConnectionState::EncodeTlsData(mut e)) => {
                    let mut out = alloc::vec![0u8; 4096];
                    loop {
                        match e.encode(&mut out) {
                            Ok(n) => {
                                self.outgoing.extend_from_slice(&out[..n]);
                                break;
                            }
                            Err(EncodeError::InsufficientSize(s)) => out.resize(s.required_size, 0),
                            Err(_) => break,
                        }
                    }
                }
                Ok(ConnectionState::TransmitTlsData(t)) => {
                    if !flush_out(self.tcp, &mut self.outgoing) {
                        self.error = String::from("sambungan terputus");
                        self.closed = true;
                    }
                    t.done();
                }
                Ok(ConnectionState::BlockedHandshake) => stop = true,
                Ok(ConnectionState::WriteTraffic(mut w)) => {
                    self.open = true;
                    if !self.plain_out.is_empty() {
                        let data = core::mem::take(&mut self.plain_out);
                        let mut out = alloc::vec![0u8; data.len() + 1024];
                        loop {
                            match w.encrypt(&data, &mut out) {
                                Ok(n) => {
                                    self.outgoing.extend_from_slice(&out[..n]);
                                    break;
                                }
                                Err(EncryptError::InsufficientSize(s)) => out.resize(s.required_size, 0),
                                Err(e) => {
                                    self.error = alloc::format!("{}", e);
                                    self.closed = true;
                                    break;
                                }
                            }
                        }
                    }
                    if self.close_asked {
                        let mut out = alloc::vec![0u8; 256];
                        if let Ok(n) = w.queue_close_notify(&mut out) {
                            self.outgoing.extend_from_slice(&out[..n]);
                        }
                        self.closed = true;
                    }
                    stop = true;
                }
                Ok(ConnectionState::PeerClosed) | Ok(ConnectionState::Closed) => {
                    self.closed = true;
                    stop = true;
                }
                Ok(_) => stop = true,
            }
            self.incoming.drain(..discard.min(self.incoming.len()));
            if stop {
                break;
            }
        }
        self.flush();
    }

    fn flush(&mut self) {
        if !flush_out(self.tcp, &mut self.outgoing) {
            self.fail("sambungan terputus");
        }
    }
}

/// Send what is queued (as much as the kernel takes now); false = the connection is gone.
fn flush_out(tcp: u64, outgoing: &mut Vec<u8>) -> bool {
    while !outgoing.is_empty() {
        let n = raw_send(tcp, &outgoing[..outgoing.len().min(64 * 1024)]);
        if n < 0 {
            return false;
        }
        if n == 0 {
            break; // the kernel's send queue is full: next pump
        }
        outgoing.drain(..n as usize);
    }
    true
}

/// `TlsAddRoot(byte[] der)` -> 0, -1: trust one more root certificate (DER)
/// for connections opened after it.
fn tls_add_root(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let der = byte_range(i, a, 0, 0, usize::MAX);
    if der.is_empty() {
        return ret_i32(-1);
    }
    let mut t = TLS.lock();
    t.extra_roots.push(der);
    t.config = None;
    ret_i32(0)
}

/// `TlsOpen(long tcp, string host)` -> TLS id, -1. The TCP connection should be open.
fn tls_open(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let host = string(i, a, 1);
    let tcp = int(a, 0) as u64;
    let mut t = TLS.lock();
    let Ok(cfg) = config(&mut t) else { return ret_i64(-1) };
    let Ok(name) = rustls::pki_types::ServerName::try_from(host) else { return ret_i64(-1) };
    let Ok(conn) = UnbufferedClientConnection::new(cfg, name) else { return ret_i64(-1) };
    let id = t.next;
    t.next += 1;
    let mut c = Tls {
        id,
        tcp,
        conn,
        incoming: Vec::new(),
        outgoing: Vec::new(),
        plain_in: Vec::new(),
        plain_out: Vec::new(),
        open: false,
        close_asked: false,
        closed: false,
        error: String::new(),
    };
    c.pump();
    t.conns.push(c);
    ret_i64(id)
}

fn with_tls<R>(id: i64, f: impl FnOnce(&mut Tls) -> R) -> Option<R> {
    let mut t = TLS.lock();
    t.conns.iter_mut().find(|c| c.id == id).map(f)
}

/// `TlsSend(long id, byte[] data, int offset, int count)` -> bytes taken, -1.
fn tls_send(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let data = byte_range(i, a, 1, int(a, 2).max(0) as usize, int(a, 3).max(0) as usize);
    let r = with_tls(int(a, 0), |c| {
        if c.closed {
            return -1;
        }
        c.plain_out.extend_from_slice(&data);
        c.pump();
        data.len() as i64
    });
    ret_i32(r.unwrap_or(-1))
}

/// `TlsRecv(long id, byte[] buffer, int offset, int count)` -> bytes, 0 = none yet, -1 = closed.
fn tls_recv(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let off = int(a, 2).max(0) as usize;
    let cap = room(i, a, 1, off, int(a, 3).max(0) as usize);
    let got = with_tls(int(a, 0), |c| {
        c.pump();
        if c.plain_in.is_empty() {
            let peer_gone = raw_state(c.tcp) >= 2;
            return if c.closed || peer_gone { None } else { Some(Vec::new()) };
        }
        let n = c.plain_in.len().min(cap);
        Some(c.plain_in.drain(..n).collect::<Vec<u8>>())
    });
    match got {
        Some(Some(data)) => {
            if !data.is_empty() {
                store_at(i, a, 1, off, &data);
            }
            ret_i32(data.len() as i64)
        }
        _ => ret_i32(-1),
    }
}

/// `TlsState(long id)` -> 0 handshaking, 1 open, 2 closed, 3 failed (see TlsError).
fn tls_state(_: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = with_tls(int(a, 0), |c| {
        c.pump();
        if !c.error.is_empty() {
            3
        } else if c.closed {
            2
        } else if c.open {
            1
        } else if raw_state(c.tcp) >= 2 {
            c.error = String::from("server menutup sambungan saat jabat tangan TLS");
            3
        } else {
            0
        }
    });
    ret_i32(r.unwrap_or(3))
}

/// `TlsError(long id)` -> why the connection failed ("" when it did not).
fn tls_error(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let e = with_tls(int(a, 0), |c| c.error.clone()).unwrap_or_else(|| String::from("tidak ada sambungan TLS itu"));
    Ok(Some(Value::Obj(i.alloc_string(&e))))
}

/// `TlsClose(long id)`: send close_notify, forget the connection (the TCP
/// handle stays with the caller).
fn tls_close(_: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let id = int(a, 0);
    let mut t = TLS.lock();
    if let Some(pos) = t.conns.iter().position(|c| c.id == id) {
        let mut c = t.conns.remove(pos);
        c.close_asked = true;
        c.pump();
    }
    ret_i32(0)
}

pub fn register(interp: &mut Interpreter) {
    let natives: [(&str, rustclr_core::interp::NativeFn); 16] = [
        ("DnsResolve", dns_resolve),
        ("TcpConnect", tcp_connect),
        ("TcpListen", tcp_listen),
        ("TcpAccept", tcp_accept),
        ("TcpSend", tcp_send),
        ("TcpRecv", tcp_recv),
        ("TcpState", tcp_state),
        ("TcpClose", tcp_close),
        ("TlsAddRoot", tls_add_root),
        ("TlsOpen", tls_open),
        ("TlsSend", tls_send),
        ("TlsRecv", tls_recv),
        ("TlsState", tls_state),
        ("TlsError", tls_error),
        ("TlsClose", tls_close),
        ("TlsVersion", tls_version),
    ];
    for (name, f) in natives {
        interp.register_native(alloc::format!("pinvoke:buitenzorg!{}", name), f);
    }
    crate::htmlpage::register(interp);
}

/// `TlsVersion(long id)` -> "TLSv1_3 TLS13_AES_128_GCM_SHA256" once open.
fn tls_version(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let s = with_tls(int(a, 0), |c| {
        let v = c.conn.protocol_version().map(|v| alloc::format!("{:?}", v)).unwrap_or_default();
        let s = c.conn.negotiated_cipher_suite().map(|s| alloc::format!("{:?}", s.suite())).unwrap_or_default();
        alloc::format!("{} {}", v, s)
    })
    .unwrap_or_default();
    Ok(Some(Value::Obj(i.alloc_string(&s))))
}
