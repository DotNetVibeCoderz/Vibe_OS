//! Remote deploy + debug service: the device side of the **Buitenzorg Deploy
//! Protocol (BZDP v1)** that the Magic App Generator (tools/MagicAppGen)
//! speaks, over UDP port 7070 (there is no TCP yet).
//!
//! Off by default: `deploy on [code]` in the shell starts it with a 4-digit
//! pairing code that every request must carry. The desktop loop calls
//! [`poll`]. Protocol (little-endian, stop-and-wait; replies echo the seq):
//!
//! * datagram = `"BZDP"` | ver u8 = 1 | op u8 | seq u32 | code u16 | payload
//! * reply    = same header with op | 0x80, code 0, payload = status u8 | data
//!   (status 0 ok, 1 bad code, 2 bad request, 3 I/O error, 4 not found, 5 busy)
//! * 1 HELLO -> "Buitenzorg OS <arch> <host>; max_chunk=1024"
//! * 2 BEGIN size u32 | path_len u16 | path   (starts an upload)
//! * 3 DATA offset u32 | bytes                -> next_offset u32
//! * 4 END crc32 u32                          (writes the file)
//! * 5 RUN command line (a shell command)     -> pid u32 (0 = ran in the shell)
//! * 6 LOG cursor u32                         -> next_cursor u32 | text
//! * 7 KILL pid u32
//! * 8 LIST path                              -> names, '\n'-separated
//! * 9 READ offset u32 | path                 -> total u32 | up to 992 bytes
//!   (a file from the device, chunk by chunk: `bz pull`)
//! * 10 EXEC command line                     -> failed u8 | total u32 | output
//!   (runs the line in the shell like RUN, but returns what it printed; the
//!   first 992 bytes come with the reply, the rest through 11)
//! * 11 OUTPUT offset u32                     -> total u32 | bytes (the last EXEC's output)
//!
//! A retransmitted request (same seq as the last one) gets the saved reply
//! again instead of running twice -- a RUN must not start two copies because
//! a reply was lost.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

pub const PORT: u16 = 7070;
const MAX_CHUNK: usize = 1024;
/// Data bytes in one READ / EXEC / OUTPUT reply: a datagram carries at most
/// `net::UDP_MAX_PAYLOAD` (1024) bytes, and the reply header, status, size
/// and flags take 18 of them (a larger reply is refused by `udp_send`).
const REPLY_CHUNK: usize = crate::net::UDP_MAX_PAYLOAD - 32;
/// Largest file accepted (a managed app with its libraries fits easily).
const MAX_FILE: usize = 8 * 1024 * 1024;

const OK: u8 = 0;
const BAD_CODE: u8 = 1;
const BAD_REQUEST: u8 = 2;
const IO_ERROR: u8 = 3;
const NOT_FOUND: u8 = 4;

static ENABLED: AtomicBool = AtomicBool::new(false);
static CODE: AtomicU32 = AtomicU32::new(0);

struct Upload {
    path: String,
    size: usize,
    data: Vec<u8>,
}

struct State {
    socket: u64,
    upload: Option<Upload>,
    last: Option<(u32, Vec<u8>)>,
    requests: u64,
    /// The last EXEC's output (op 11 reads it in chunks).
    output: Vec<u8>,
    /// The file READ is sending (read from the VFS at offset 0, then served
    /// from here: reading it again per 1 KiB chunk was too slow).
    reading: Option<(String, Vec<u8>)>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Start the service with pairing code `code` (0-9999). Returns false when
/// the port cannot be bound.
pub fn enable(code: u16) -> bool {
    let mut st = STATE.lock();
    if st.is_none() {
        let socket = crate::net::udp_socket();
        if crate::net::udp_bind(socket, PORT).is_err() {
            let _ = crate::net::udp_close(socket);
            return false;
        }
        *st = Some(State { socket, upload: None, last: None, requests: 0, output: Vec::new(), reading: None });
    }
    CODE.store(code as u32 % 10_000, Ordering::Relaxed);
    ENABLED.store(true, Ordering::Release);
    true
}

pub fn disable() {
    ENABLED.store(false, Ordering::Release);
    if let Some(st) = STATE.lock().take() {
        let _ = crate::net::udp_close(st.socket);
    }
}

/// Remembers the service across reboots: "on NNNN" or "off" (`deploy on|off`).
const CONFIG_FILE: &str = "/disk/DEPLOY.CFG";
/// QEMU fw_cfg file whose text is a pairing code: starts the service at boot
/// without a keyboard (the smoke test's `[bzlink]` step), x86 only.
#[cfg(target_arch = "x86_64")]
const FW_CFG_FILE: &str = "opt/buitenzorg/deploy";

/// Start the service at boot when the user left it on (`DEPLOY.CFG`), or
/// when QEMU passes a code through fw_cfg.
pub fn autostart() {
    let mut code: Option<u16> = None;
    #[cfg(target_arch = "x86_64")]
    {
        let mut b = [0u8; 8];
        if let Some(n) = crate::arch::x86::fwcfg::file_read(FW_CFG_FILE, &mut b) {
            code = core::str::from_utf8(&b[..n]).ok().and_then(|t| t.trim().parse::<u16>().ok());
        }
    }
    if code.is_none() {
        if let Ok(b) = crate::vfs::read(CONFIG_FILE) {
            let t = core::str::from_utf8(&b).unwrap_or("");
            if let Some(rest) = t.trim().strip_prefix("on") {
                code = Some(rest.trim().parse::<u16>().unwrap_or_else(|_| random_code()));
            }
        }
    }
    if let Some(c) = code.filter(|c| *c < 10_000) {
        if enable(c) {
            crate::println!("[deploy] service on at boot: udp/{}, pairing code {:04}", PORT, c);
        }
    }
}

/// Remember the switch for the next boot (best effort: no /disk, no memory).
pub fn remember(on: bool, code: u16) {
    let text = if on { alloc::format!("on {:04}\n", code) } else { String::from("off\n") };
    let _ = crate::vfs::write(CONFIG_FILE, text.as_bytes());
}

/// (enabled, pairing code, requests served).
static SERVED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn status() -> (bool, u16, u64) {
    // try_lock: RUN executes a shell line while `STATE` is held.
    let served = STATE.try_lock().map_or(SERVED.load(Ordering::Relaxed), |g| g.as_ref().map_or(0, |s| s.requests));
    (ENABLED.load(Ordering::Relaxed), CODE.load(Ordering::Relaxed) as u16, served)
}

/// A pairing code from the clock (when the user did not choose one).
pub fn random_code() -> u16 {
    let t = crate::arch::cpu::timestamp();
    ((t ^ (t >> 17) ^ (t >> 31)) % 9000 + 1000) as u16
}

/// Serve pending requests (desktop loop).
pub fn poll() {
    if !ENABLED.load(Ordering::Acquire) {
        return;
    }
    crate::net::poll();
    let Some(mut guard) = STATE.try_lock() else { return };
    let Some(st) = guard.as_mut() else { return };
    for _ in 0..32 {
        let Some((ip, port, pkt)) = crate::net::udp_recv(st.socket) else { break };
        if let Some(reply) = handle(st, &pkt) {
            if let Err(e) = crate::net::udp_send(st.socket, ip, port, &reply) {
                crate::println!("[deploy] reply of {} bytes not sent: {}", reply.len(), e);
            }
        }
    }
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}

fn reply(op: u8, seq: u32, status: u8, data: &[u8]) -> Vec<u8> {
    let mut r = Vec::with_capacity(13 + data.len());
    r.extend_from_slice(b"BZDP");
    r.push(1);
    r.push(op | 0x80);
    r.extend_from_slice(&seq.to_le_bytes());
    r.extend_from_slice(&0u16.to_le_bytes());
    r.push(status);
    r.extend_from_slice(data);
    r
}

fn handle(st: &mut State, pkt: &[u8]) -> Option<Vec<u8>> {
    if pkt.len() < 12 || &pkt[0..4] != b"BZDP" || pkt[4] != 1 {
        return None; // not ours: no reply
    }
    let op = pkt[5];
    let seq = u32_at(pkt, 6)?;
    let code = u16_at(pkt, 10)?;
    let body = &pkt[12..];
    if code as u32 != CODE.load(Ordering::Relaxed) {
        return Some(reply(op, seq, BAD_CODE, b"wrong pairing code (see 'deploy' in the shell)"));
    }
    if let Some((last_seq, ref saved)) = st.last {
        if last_seq == seq && seq != 0 {
            return Some(saved.clone());
        }
    }
    st.requests += 1;
    SERVED.store(st.requests, Ordering::Relaxed);
    let r = match op {
        1 => {
            let text = alloc::format!(
                "Buitenzorg OS {} {}; max_chunk={}",
                arch_name(),
                crate::net::local_ip().map_or(String::from("-"), |ip| alloc::format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])),
                MAX_CHUNK
            );
            reply(op, seq, OK, text.as_bytes())
        }
        2 => begin(st, op, seq, body),
        3 => data(st, op, seq, body),
        4 => end(st, op, seq, body),
        5 => run(op, seq, body),
        6 => log(op, seq, body),
        7 => match u32_at(body, 0) {
            Some(pid) if crate::process::kill(pid as u64) => reply(op, seq, OK, &[]),
            Some(_) => reply(op, seq, NOT_FOUND, &[]),
            None => reply(op, seq, BAD_REQUEST, &[]),
        },
        8 => {
            let path = core::str::from_utf8(body).unwrap_or("").trim();
            match crate::vfs::list(if path.is_empty() { "/disk" } else { path }) {
                Ok(names) => reply(op, seq, OK, names.join("\n").as_bytes()),
                Err(_) => reply(op, seq, NOT_FOUND, &[]),
            }
        }
        9 => read(st, op, seq, body),
        10 => exec(st, op, seq, body),
        11 => {
            let from = (u32_at(body, 0).unwrap_or(0) as usize).min(st.output.len());
            let to = (from + REPLY_CHUNK).min(st.output.len());
            let mut d = Vec::with_capacity(4 + to - from);
            d.extend_from_slice(&(st.output.len() as u32).to_le_bytes());
            d.extend_from_slice(&st.output[from..to]);
            reply(op, seq, OK, &d)
        }
        _ => reply(op, seq, BAD_REQUEST, &[]),
    };
    st.last = Some((seq, r.clone()));
    Some(r)
}

fn begin(st: &mut State, op: u8, seq: u32, b: &[u8]) -> Vec<u8> {
    let (Some(size), Some(len)) = (u32_at(b, 0), u16_at(b, 4)) else { return reply(op, seq, BAD_REQUEST, &[]) };
    let Some(path) = b.get(6..6 + len as usize).and_then(|p| core::str::from_utf8(p).ok()) else {
        return reply(op, seq, BAD_REQUEST, &[]);
    };
    if size as usize > MAX_FILE || !path.starts_with('/') {
        return reply(op, seq, BAD_REQUEST, b"file too large or path not absolute");
    }
    crate::println!("[deploy] receiving {} ({} bytes)", path, size);
    st.upload = Some(Upload { path: String::from(path), size: size as usize, data: Vec::with_capacity(size as usize) });
    reply(op, seq, OK, &[])
}

fn data(st: &mut State, op: u8, seq: u32, b: &[u8]) -> Vec<u8> {
    let Some(up) = st.upload.as_mut() else { return reply(op, seq, BAD_REQUEST, b"no upload in progress") };
    let Some(offset) = u32_at(b, 0) else { return reply(op, seq, BAD_REQUEST, &[]) };
    let chunk = &b[4..];
    // only the next contiguous chunk is taken; the client resends from next_offset
    if offset as usize == up.data.len() && up.data.len() + chunk.len() <= up.size && chunk.len() <= MAX_CHUNK {
        up.data.extend_from_slice(chunk);
    }
    reply(op, seq, OK, &(up.data.len() as u32).to_le_bytes())
}

fn end(st: &mut State, op: u8, seq: u32, b: &[u8]) -> Vec<u8> {
    let Some(up) = st.upload.take() else { return reply(op, seq, BAD_REQUEST, b"no upload in progress") };
    let Some(want) = u32_at(b, 0) else { return reply(op, seq, BAD_REQUEST, &[]) };
    if up.data.len() != up.size || crc32(&up.data) != want {
        crate::println!("[deploy] {}: incomplete or CRC mismatch", up.path);
        return reply(op, seq, IO_ERROR, b"incomplete upload or CRC mismatch");
    }
    match crate::vfs::write(&up.path, &up.data) {
        Ok(()) => {
            crate::println!("[deploy] wrote {} ({} bytes)", up.path, up.size);
            reply(op, seq, OK, &[])
        }
        Err(e) => {
            crate::println!("[deploy] writing {} failed: {}", up.path, e);
            reply(op, seq, IO_ERROR, e.as_bytes())
        }
    }
}

fn run(op: u8, seq: u32, b: &[u8]) -> Vec<u8> {
    let Ok(line) = core::str::from_utf8(b) else { return reply(op, seq, BAD_REQUEST, &[]) };
    if line.trim_start().starts_with("deploy") {
        // the service's own switch is not remote-controllable (and its lock is held here)
        return reply(op, seq, BAD_REQUEST, b"'deploy' can only be changed on the device");
    }
    crate::println!("[deploy] run: {}", line.trim());
    let (out, _) = crate::shell::run(line);
    let mut pid = 0u32;
    for l in &out {
        crate::println!("{}", l); // into the log ring as well
        if let Some(i) = l.find("(pid ") {
            pid = l[i + 5..].trim_end_matches(')').parse().unwrap_or(0);
        }
    }
    reply(op, seq, OK, &pid.to_le_bytes())
}

fn read(st: &mut State, op: u8, seq: u32, b: &[u8]) -> Vec<u8> {
    let (Some(offset), Ok(path)) = (u32_at(b, 0), core::str::from_utf8(b.get(4..).unwrap_or(&[]))) else {
        return reply(op, seq, BAD_REQUEST, &[]);
    };
    let path = path.trim();
    let cached = matches!(&st.reading, Some((p, _)) if p == path);
    if offset == 0 || !cached {
        st.reading = None;
        match crate::vfs::read(path) {
            Ok(data) if data.len() <= MAX_FILE => st.reading = Some((String::from(path), data)),
            Ok(_) => return reply(op, seq, BAD_REQUEST, b"file too large"),
            Err(_) => return reply(op, seq, NOT_FOUND, &[]),
        }
    }
    let data = &st.reading.as_ref().unwrap().1;
    let from = (offset as usize).min(data.len());
    let to = (from + REPLY_CHUNK).min(data.len());
    let mut d = Vec::with_capacity(4 + to - from);
    d.extend_from_slice(&(data.len() as u32).to_le_bytes());
    d.extend_from_slice(&data[from..to]);
    let done = to == data.len();
    let r = reply(op, seq, OK, &d);
    if done {
        st.reading = None; // the last chunk: free the copy
    }
    r
}

fn exec(st: &mut State, op: u8, seq: u32, b: &[u8]) -> Vec<u8> {
    let Ok(line) = core::str::from_utf8(b) else { return reply(op, seq, BAD_REQUEST, &[]) };
    if line.trim_start().starts_with("deploy") {
        return reply(op, seq, BAD_REQUEST, b"'deploy' can only be changed on the device");
    }
    crate::println!("[deploy] exec: {}", line.trim());
    let (out, _) = crate::shell::run(line);
    let failed = !crate::shell::last_ok();
    st.output.clear();
    for l in &out {
        st.output.extend_from_slice(l.as_bytes());
        st.output.push(b'\n');
    }
    let n = st.output.len().min(REPLY_CHUNK);
    let mut d = Vec::with_capacity(5 + n);
    d.push(failed as u8);
    d.extend_from_slice(&(st.output.len() as u32).to_le_bytes());
    d.extend_from_slice(&st.output[..n]);
    reply(op, seq, OK, &d)
}

fn log(op: u8, seq: u32, b: &[u8]) -> Vec<u8> {
    let from = u32_at(b, 0).unwrap_or(0) as u64;
    let mut text = [0u8; 1000];
    let (n, next) = crate::logring::read(from, &mut text);
    let mut data = Vec::with_capacity(4 + n);
    data.extend_from_slice(&(next as u32).to_le_bytes());
    data.extend_from_slice(&text[..n]);
    reply(op, seq, OK, &data)
}

fn arch_name() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86-64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "riscv64"
    }
}

/// IEEE CRC-32 (the one zlib / .NET's System.IO.Hashing.Crc32 compute).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}
