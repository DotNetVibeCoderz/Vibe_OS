//! SSH server (SSH-2, RFC 4253/4252/4254) on TCP port 22: remote shell and
//! command execution for the accounts in `users.rs`.
//!
//! Algorithms (what current OpenSSH clients offer): key exchange
//! `curve25519-sha256` (RFC 8731), host key `ssh-ed25519` (kept in
//! `/disk/SSHHOST.KEY`, created on first start), cipher
//! `chacha20-poly1305@openssh.com` (no separate MAC), no compression.
//! Authentication: `password`. Channels: one `session` per connection with
//! `pty-req` / `shell` (a line-editing shell over `shell::run`) or `exec`.
//!
//! Off by default: `ssh on` in the terminal starts it (and remembers it in
//! `/disk/SSHD.CFG`), `ssh off` stops it. The desktop loop calls [`poll`].
//! Client: `ssh administrator@<ip>` (QEMU user network: `-netdev
//! user,hostfwd=tcp::2222-:22` and `ssh -p 2222 administrator@127.0.0.1`).

use alloc::string::String;
use alloc::vec::Vec;
use chacha20::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use poly1305::universal_hash::KeyInit;
use sha2::{Digest, Sha256};
use spin::Mutex;

pub const PORT: u16 = 22;
const HOST_KEY_FILE: &str = "/disk/SSHHOST.KEY";
const CONFIG_FILE: &str = "/disk/SSHD.CFG";
const VERSION: &str = "SSH-2.0-BuitenzorgSSH_1.0";
const MAX_SESSIONS: usize = 4;
const MAX_PACKET: usize = 35_000;
const MAX_AUTH_FAILURES: u32 = 5;

const MSG_DISCONNECT: u8 = 1;
const MSG_IGNORE: u8 = 2;
const MSG_UNIMPLEMENTED: u8 = 3;
const MSG_DEBUG: u8 = 4;
const MSG_SERVICE_REQUEST: u8 = 5;
const MSG_SERVICE_ACCEPT: u8 = 6;
const MSG_EXT_INFO: u8 = 7;
const MSG_KEXINIT: u8 = 20;
const MSG_NEWKEYS: u8 = 21;
const MSG_KEX_ECDH_INIT: u8 = 30;
const MSG_KEX_ECDH_REPLY: u8 = 31;
const MSG_USERAUTH_REQUEST: u8 = 50;
const MSG_USERAUTH_FAILURE: u8 = 51;
const MSG_USERAUTH_SUCCESS: u8 = 52;
const MSG_USERAUTH_BANNER: u8 = 53;
const MSG_GLOBAL_REQUEST: u8 = 80;
const MSG_REQUEST_FAILURE: u8 = 82;
const MSG_CHANNEL_OPEN: u8 = 90;
const MSG_CHANNEL_OPEN_CONFIRMATION: u8 = 91;
const MSG_CHANNEL_OPEN_FAILURE: u8 = 92;
const MSG_CHANNEL_WINDOW_ADJUST: u8 = 93;
const MSG_CHANNEL_DATA: u8 = 94;
const MSG_CHANNEL_EOF: u8 = 96;
const MSG_CHANNEL_CLOSE: u8 = 97;
const MSG_CHANNEL_REQUEST: u8 = 98;
const MSG_CHANNEL_SUCCESS: u8 = 99;
const MSG_CHANNEL_FAILURE: u8 = 100;

// ---- wire encoding -----------------------------------------------------------

struct W(Vec<u8>);

impl W {
    fn new(msg: u8) -> W {
        W(alloc::vec![msg])
    }
    fn u8(mut self, v: u8) -> W {
        self.0.push(v);
        self
    }
    fn u32(mut self, v: u32) -> W {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn bool(self, v: bool) -> W {
        self.u8(v as u8)
    }
    fn str(mut self, s: &[u8]) -> W {
        self.0.extend_from_slice(&(s.len() as u32).to_be_bytes());
        self.0.extend_from_slice(s);
        self
    }
}

fn put_str(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(&(s.len() as u32).to_be_bytes());
    out.extend_from_slice(s);
}

/// An unsigned big-endian number as an SSH mpint.
fn put_mpint(out: &mut Vec<u8>, be: &[u8]) {
    let mut b = be;
    while let [0, rest @ ..] = b {
        b = rest;
    }
    let pad = !b.is_empty() && b[0] & 0x80 != 0;
    out.extend_from_slice(&((b.len() + pad as usize) as u32).to_be_bytes());
    if pad {
        out.push(0);
    }
    out.extend_from_slice(b);
}

struct R<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> R<'a> {
    fn new(b: &'a [u8]) -> R<'a> {
        R { b, i: 0 }
    }
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.i)?;
        self.i += 1;
        Some(v)
    }
    fn u32(&mut self) -> Option<u32> {
        let s = self.b.get(self.i..self.i + 4)?;
        self.i += 4;
        Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn bool(&mut self) -> Option<bool> {
        Some(self.u8()? != 0)
    }
    fn str(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        let s = self.b.get(self.i..self.i + n)?;
        self.i += n;
        Some(s)
    }
    fn text(&mut self) -> Option<&'a str> {
        core::str::from_utf8(self.str()?).ok()
    }
}

// ---- chacha20-poly1305@openssh.com -------------------------------------------

struct Cipher {
    main: [u8; 32],
    header: [u8; 32],
}

impl Cipher {
    fn stream(key: &[u8; 32], seq: u32, block: u64) -> chacha20::ChaCha20Legacy {
        let nonce = (seq as u64).to_be_bytes();
        let mut c = chacha20::ChaCha20Legacy::new(key.into(), (&nonce).into());
        c.seek(block * 64);
        c
    }

    fn poly_key(&self, seq: u32) -> [u8; 32] {
        let mut k = [0u8; 32];
        Self::stream(&self.main, seq, 0).apply_keystream(&mut k);
        k
    }

    /// Encrypt `packet` (length field + body) in place and return the tag.
    fn seal(&self, seq: u32, packet: &mut [u8]) -> [u8; 16] {
        Self::stream(&self.header, seq, 0).apply_keystream(&mut packet[..4]);
        Self::stream(&self.main, seq, 1).apply_keystream(&mut packet[4..]);
        poly1305::Poly1305::new((&self.poly_key(seq)).into()).compute_unpadded(packet).into()
    }

    fn length(&self, seq: u32, enc: &[u8]) -> u32 {
        let mut l = [enc[0], enc[1], enc[2], enc[3]];
        Self::stream(&self.header, seq, 0).apply_keystream(&mut l);
        u32::from_be_bytes(l)
    }

    /// Check the tag of `enc` (length + body) and decrypt the body.
    fn open(&self, seq: u32, enc: &[u8], tag: &[u8]) -> Option<Vec<u8>> {
        let want: [u8; 16] = poly1305::Poly1305::new((&self.poly_key(seq)).into()).compute_unpadded(enc).into();
        if want.iter().zip(tag).fold(0u8, |a, (x, y)| a | (x ^ y)) != 0 {
            return None;
        }
        let mut body = enc[4..].to_vec();
        Self::stream(&self.main, seq, 1).apply_keystream(&mut body);
        Some(body)
    }
}

// ---- host key ------------------------------------------------------------------

static HOST_KEY: Mutex<Option<ed25519_dalek::SigningKey>> = Mutex::new(None);

fn host_key() -> ed25519_dalek::SigningKey {
    let mut hk = HOST_KEY.lock();
    if hk.is_none() {
        let seed: [u8; 32] = match crate::vfs::read(HOST_KEY_FILE) {
            Ok(b) if b.len() == 32 => b.as_slice().try_into().unwrap(),
            _ => {
                let s = crate::rng::bytes::<32>();
                let _ = crate::vfs::write(HOST_KEY_FILE, &s);
                s
            }
        };
        *hk = Some(ed25519_dalek::SigningKey::from_bytes(&seed));
    }
    hk.clone().unwrap()
}

fn host_key_blob(k: &ed25519_dalek::SigningKey) -> Vec<u8> {
    let mut b = Vec::new();
    put_str(&mut b, b"ssh-ed25519");
    put_str(&mut b, k.verifying_key().as_bytes());
    b
}

/// "SHA256:..." as `ssh-keygen -lf` prints it.
pub fn fingerprint() -> String {
    let d = Sha256::digest(host_key_blob(&host_key()));
    let mut s = String::from("SHA256:");
    s.push_str(&base64(&d));
    s
}

fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..c.len() + 1 {
            s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    s // ssh-keygen omits the '=' padding
}

// ---- sessions -------------------------------------------------------------------

#[derive(PartialEq, Eq)]
enum Phase {
    Version,
    Kex,
    Auth,
    Open,
}

struct Session {
    conn: u64,
    phase: Phase,
    inbuf: Vec<u8>,
    client_version: Vec<u8>,
    client_kexinit: Vec<u8>,
    server_kexinit: Vec<u8>,
    session_id: Option<[u8; 32]>,
    /// The client-to-server keys, used from the client's NEWKEYS on.
    pending_rx: Option<Cipher>,
    rx_cipher: Option<Cipher>,
    tx_cipher: Option<Cipher>,
    rx_seq: u32,
    tx_seq: u32,
    user: Option<String>,
    auth_failures: u32,
    peer_channel: Option<u32>,
    peer_max_packet: usize,
    shell: bool,
    line: String,
    escape: u8,
    closing: bool,
}

struct Server {
    sessions: Vec<Session>,
    logins: u64,
}

static SERVER: Mutex<Option<Server>> = Mutex::new(None);

impl Session {
    fn new(conn: u64) -> Session {
        Session {
            conn,
            phase: Phase::Version,
            inbuf: Vec::new(),
            client_version: Vec::new(),
            client_kexinit: Vec::new(),
            server_kexinit: Vec::new(),
            session_id: None,
            pending_rx: None,
            rx_cipher: None,
            tx_cipher: None,
            rx_seq: 0,
            tx_seq: 0,
            user: None,
            auth_failures: 0,
            peer_channel: None,
            peer_max_packet: 32768,
            shell: false,
            line: String::new(),
            escape: 0,
            closing: false,
        }
    }

    fn send(&mut self, payload: &[u8]) {
        let aead = self.tx_cipher.is_some();
        let unpadded = if aead { 1 + payload.len() } else { 5 + payload.len() };
        let mut pad = 8 - unpadded % 8;
        if pad < 4 {
            pad += 8;
        }
        let len = 1 + payload.len() + pad;
        let mut p = Vec::with_capacity(4 + len + 16);
        p.extend_from_slice(&(len as u32).to_be_bytes());
        p.push(pad as u8);
        p.extend_from_slice(payload);
        let mut padding = alloc::vec![0u8; pad];
        crate::rng::fill(&mut padding);
        p.extend_from_slice(&padding);
        if let Some(c) = &self.tx_cipher {
            let tag = c.seal(self.tx_seq, &mut p);
            p.extend_from_slice(&tag);
        }
        self.tx_seq = self.tx_seq.wrapping_add(1);
        crate::tcp::send(self.conn, &p);
    }

    fn disconnect(&mut self, why: &str) {
        self.send(&W::new(MSG_DISCONNECT).u32(11).str(why.as_bytes()).str(b"").0);
        self.closing = true;
    }

    /// Take one complete packet's payload out of `inbuf`, if there is one.
    fn next_packet(&mut self) -> Result<Option<Vec<u8>>, &'static str> {
        if self.inbuf.len() < 4 {
            return Ok(None);
        }
        let (len, extra) = match &self.rx_cipher {
            Some(c) => (c.length(self.rx_seq, &self.inbuf) as usize, 16),
            None => (u32::from_be_bytes([self.inbuf[0], self.inbuf[1], self.inbuf[2], self.inbuf[3]]) as usize, 0),
        };
        if len < 5 || len > MAX_PACKET {
            return Err("bad packet length");
        }
        if self.inbuf.len() < 4 + len + extra {
            return Ok(None);
        }
        let body = match &self.rx_cipher {
            Some(c) => c.open(self.rx_seq, &self.inbuf[..4 + len], &self.inbuf[4 + len..4 + len + 16]).ok_or("bad MAC")?,
            None => self.inbuf[4..4 + len].to_vec(),
        };
        self.inbuf.drain(..4 + len + extra);
        self.rx_seq = self.rx_seq.wrapping_add(1);
        let pad = body[0] as usize;
        if pad + 1 > body.len() {
            return Err("bad padding");
        }
        Ok(Some(body[1..body.len() - pad].to_vec()))
    }

    fn kexinit(&mut self) {
        let mut w = W::new(MSG_KEXINIT);
        w.0.extend_from_slice(&crate::rng::bytes::<16>());
        let w = w
            .str(b"curve25519-sha256,curve25519-sha256@libssh.org")
            .str(b"ssh-ed25519")
            .str(b"chacha20-poly1305@openssh.com")
            .str(b"chacha20-poly1305@openssh.com")
            .str(b"hmac-sha2-256")
            .str(b"hmac-sha2-256")
            .str(b"none")
            .str(b"none")
            .str(b"")
            .str(b"")
            .bool(false)
            .u32(0);
        self.server_kexinit = w.0.clone();
        self.send(&w.0);
    }

    fn handle(&mut self, p: &[u8], logins: &mut u64) -> Result<(), &'static str> {
        let mut r = R::new(p);
        let msg = r.u8().ok_or("empty packet")?;
        match msg {
            MSG_IGNORE | MSG_DEBUG | MSG_UNIMPLEMENTED | MSG_EXT_INFO => {}
            MSG_DISCONNECT => self.closing = true,
            MSG_KEXINIT => {
                self.client_kexinit = p.to_vec();
                let mut k = R::new(&p[17..]);
                let kex = k.text().ok_or("bad kexinit")?;
                let hostkey = k.text().ok_or("bad kexinit")?;
                let c2s = k.text().ok_or("bad kexinit")?;
                let s2c = k.text().ok_or("bad kexinit")?;
                let ok = kex.split(',').any(|a| a == "curve25519-sha256" || a == "curve25519-sha256@libssh.org")
                    && hostkey.split(',').any(|a| a == "ssh-ed25519")
                    && c2s.split(',').any(|a| a == "chacha20-poly1305@openssh.com")
                    && s2c.split(',').any(|a| a == "chacha20-poly1305@openssh.com");
                if !ok {
                    self.disconnect("no common algorithms: need curve25519-sha256, ssh-ed25519, chacha20-poly1305@openssh.com");
                    return Ok(());
                }
                if self.server_kexinit.is_empty() {
                    self.kexinit();
                }
            }
            MSG_KEX_ECDH_INIT => {
                let qc: [u8; 32] = r.str().ok_or("bad ecdh init")?.try_into().map_err(|_| "bad client key")?;
                let secret = x25519_dalek::StaticSecret::from(crate::rng::bytes::<32>());
                let qs = x25519_dalek::PublicKey::from(&secret);
                let shared = secret.diffie_hellman(&x25519_dalek::PublicKey::from(qc));
                if shared.as_bytes().iter().all(|&b| b == 0) {
                    return Err("degenerate key exchange");
                }
                let hk = host_key();
                let ks = host_key_blob(&hk);
                let mut k = Vec::new();
                put_mpint(&mut k, shared.as_bytes());
                let mut h = Vec::new();
                put_str(&mut h, &self.client_version);
                put_str(&mut h, VERSION.as_bytes());
                put_str(&mut h, &self.client_kexinit);
                put_str(&mut h, &self.server_kexinit);
                put_str(&mut h, &ks);
                put_str(&mut h, &qc);
                put_str(&mut h, qs.as_bytes());
                h.extend_from_slice(&k);
                let hash: [u8; 32] = Sha256::digest(&h).into();
                let sid = *self.session_id.get_or_insert(hash);
                use ed25519_dalek::Signer;
                let sig = hk.sign(&hash);
                let mut sig_blob = Vec::new();
                put_str(&mut sig_blob, b"ssh-ed25519");
                put_str(&mut sig_blob, &sig.to_bytes());
                self.send(&W::new(MSG_KEX_ECDH_REPLY).str(&ks).str(qs.as_bytes()).str(&sig_blob).0);
                let derive = |letter: u8| -> Cipher {
                    let mut a = Sha256::new();
                    a.update(&k);
                    a.update(hash);
                    a.update([letter]);
                    a.update(sid);
                    let k1: [u8; 32] = a.finalize().into();
                    let mut b = Sha256::new();
                    b.update(&k);
                    b.update(hash);
                    b.update(k1);
                    let k2: [u8; 32] = b.finalize().into();
                    Cipher { main: k1, header: k2 }
                };
                let (c2s, s2c) = (derive(b'C'), derive(b'D'));
                self.send(&[MSG_NEWKEYS]);
                self.tx_cipher = Some(s2c); // everything after our NEWKEYS
                self.pending_rx = Some(c2s);
            }
            MSG_NEWKEYS => {
                let rx = self.pending_rx.take().ok_or("NEWKEYS before key exchange")?;
                self.rx_cipher = Some(rx);
                self.server_kexinit.clear(); // a later re-key starts afresh
                if self.phase == Phase::Kex {
                    self.phase = Phase::Auth;
                }
            }
            MSG_SERVICE_REQUEST => {
                let name = r.str().ok_or("bad service request")?;
                if name == b"ssh-userauth" {
                    self.send(&W::new(MSG_SERVICE_ACCEPT).str(name).0);
                } else {
                    self.disconnect("service not available");
                }
            }
            MSG_USERAUTH_REQUEST => {
                let user = String::from(r.text().ok_or("bad auth")?);
                let _service = r.str();
                let method = r.text().unwrap_or("");
                let ok = method == "password" && {
                    let _change = r.bool();
                    let pw = r.text().unwrap_or("");
                    crate::users::verify(&user, pw)
                };
                if ok {
                    crate::println!("[ssh] {} logged in", user);
                    *logins += 1;
                    self.user = Some(user);
                    self.phase = Phase::Open;
                    self.send(&[MSG_USERAUTH_SUCCESS]);
                } else {
                    if method == "password" {
                        self.auth_failures += 1;
                        crate::println!("[ssh] login failed for '{}'", user);
                    } else if method == "none" && self.auth_failures == 0 {
                        let banner = "Buitenzorg OS -- masuk dengan akun dari Pengaturan > Pengguna.\r\n";
                        self.send(&W::new(MSG_USERAUTH_BANNER).str(banner.as_bytes()).str(b"").0);
                    }
                    if self.auth_failures >= MAX_AUTH_FAILURES {
                        self.disconnect("too many authentication failures");
                    } else {
                        self.send(&W::new(MSG_USERAUTH_FAILURE).str(b"password").bool(false).0);
                    }
                }
            }
            MSG_GLOBAL_REQUEST => {
                let _name = r.str();
                if r.bool() == Some(true) {
                    self.send(&[MSG_REQUEST_FAILURE]);
                }
            }
            MSG_CHANNEL_OPEN => {
                let kind = r.str().ok_or("bad open")?;
                let sender = r.u32().ok_or("bad open")?;
                let _window = r.u32();
                let max = r.u32().unwrap_or(32768) as usize;
                if self.phase != Phase::Open || kind != b"session" || self.peer_channel.is_some() {
                    self.send(&W::new(MSG_CHANNEL_OPEN_FAILURE).u32(sender).u32(1).str(b"only one session channel").str(b"").0);
                    return Ok(());
                }
                self.peer_channel = Some(sender);
                self.peer_max_packet = max.clamp(512, 32768);
                self.send(&W::new(MSG_CHANNEL_OPEN_CONFIRMATION).u32(sender).u32(0).u32(1 << 20).u32(32768).0);
            }
            MSG_CHANNEL_REQUEST => {
                let _ch = r.u32();
                let kind = r.text().unwrap_or("");
                let want = r.bool().unwrap_or(false);
                let ok = match kind {
                    "pty-req" | "env" | "window-change" | "keepalive@openssh.com" => true,
                    "shell" => {
                        self.shell = true;
                        let who = self.user.clone().unwrap_or_default();
                        self.data(alloc::format!(
                            "Selamat datang di Buitenzorg OS, {}.\r\nGravicode Studios -- ketik 'help' untuk perintah, 'exit' untuk keluar.\r\n\r\n",
                            who
                        )
                        .as_bytes());
                        self.prompt();
                        true
                    }
                    "exec" => {
                        let cmd = String::from(r.text().unwrap_or(""));
                        if want {
                            self.send(&W::new(MSG_CHANNEL_SUCCESS).u32(self.peer_channel.unwrap_or(0)).0);
                        }
                        self.run(&cmd);
                        self.finish(0);
                        return Ok(());
                    }
                    _ => false,
                };
                if want {
                    let m = if ok { MSG_CHANNEL_SUCCESS } else { MSG_CHANNEL_FAILURE };
                    self.send(&W::new(m).u32(self.peer_channel.unwrap_or(0)).0);
                }
            }
            MSG_CHANNEL_DATA => {
                let _ch = r.u32();
                let data = r.str().unwrap_or(&[]).to_vec();
                // keep the client's window for us open
                self.send(&W::new(MSG_CHANNEL_WINDOW_ADJUST).u32(self.peer_channel.unwrap_or(0)).u32(data.len() as u32).0);
                if self.shell {
                    for b in data {
                        self.key(b);
                        if self.closing {
                            break;
                        }
                    }
                }
            }
            MSG_CHANNEL_WINDOW_ADJUST => {}
            MSG_CHANNEL_EOF | MSG_CHANNEL_CLOSE => self.finish(0),
            _ => {
                self.send(&W::new(MSG_UNIMPLEMENTED).u32(self.rx_seq.wrapping_sub(1)).0);
            }
        }
        Ok(())
    }

    /// Channel data to the client, in packets the client accepts.
    fn data(&mut self, bytes: &[u8]) {
        let Some(ch) = self.peer_channel else { return };
        for chunk in bytes.chunks(self.peer_max_packet.min(16 * 1024)) {
            self.send(&W::new(MSG_CHANNEL_DATA).u32(ch).str(chunk).0);
        }
    }

    fn prompt(&mut self) {
        let user = self.user.clone().unwrap_or_default();
        let p = alloc::format!("{}@buitenzorg:{}$ ", user, crate::shell::cwd());
        self.data(p.as_bytes());
    }

    fn run(&mut self, line: &str) {
        let (out, effect) = crate::shell::run(line);
        if matches!(effect, crate::shell::Effect::Clear) {
            self.data(b"\x1b[2J\x1b[H");
        }
        let mut text = String::new();
        for l in out {
            text.push_str(&l);
            text.push_str("\r\n");
        }
        self.data(text.as_bytes());
    }

    fn finish(&mut self, status: u32) {
        if let Some(ch) = self.peer_channel.take() {
            self.send(&W::new(MSG_CHANNEL_REQUEST).u32(ch).str(b"exit-status").bool(false).u32(status).0);
            self.send(&W::new(MSG_CHANNEL_EOF).u32(ch).0);
            self.send(&W::new(MSG_CHANNEL_CLOSE).u32(ch).0);
        }
        self.closing = true;
    }

    /// A typed byte in the interactive shell.
    fn key(&mut self, b: u8) {
        // skip VT100 escape sequences (arrow keys etc.): ESC [ ... final
        match self.escape {
            1 => {
                self.escape = if b == b'[' || b == b'O' { 2 } else { 0 };
                return;
            }
            2 => {
                if (0x40..=0x7E).contains(&b) {
                    self.escape = 0;
                }
                return;
            }
            _ => {}
        }
        match b {
            0x1b => self.escape = 1,
            b'\r' | b'\n' => {
                self.data(b"\r\n");
                let line = core::mem::take(&mut self.line);
                let cmd = line.trim();
                if cmd == "exit" || cmd == "logout" {
                    self.data(b"Sampai jumpa.\r\n");
                    self.finish(0);
                    return;
                }
                if !cmd.is_empty() {
                    crate::println!("[ssh] {}: {}", self.user.clone().unwrap_or_default(), cmd);
                    self.run(cmd);
                }
                self.prompt();
            }
            0x7f | 0x08 => {
                if self.line.pop().is_some() {
                    self.data(b"\x08 \x08");
                }
            }
            0x03 => {
                self.line.clear();
                self.data(b"^C\r\n");
                self.prompt();
            }
            0x04 if self.line.is_empty() => {
                self.data(b"\r\n");
                self.finish(0);
            }
            0x15 => {
                // Ctrl-U: erase the line
                for _ in 0..self.line.chars().count() {
                    self.data(b"\x08 \x08");
                }
                self.line.clear();
            }
            0x20..=0x7e if self.line.len() < 512 => {
                self.line.push(b as char);
                self.data(&[b]);
            }
            _ => {}
        }
    }

    fn input(&mut self, bytes: &[u8], logins: &mut u64) {
        self.inbuf.extend_from_slice(bytes);
        if self.phase == Phase::Version {
            let Some(end) = self.inbuf.windows(2).position(|w| w == b"\r\n").or_else(|| self.inbuf.iter().position(|&b| b == b'\n')) else {
                if self.inbuf.len() > 255 {
                    self.closing = true;
                }
                return;
            };
            let line = self.inbuf[..end].to_vec();
            let skip = if self.inbuf.get(end) == Some(&b'\r') { end + 2 } else { end + 1 };
            self.inbuf.drain(..skip.min(self.inbuf.len()));
            if !line.starts_with(b"SSH-2.0-") {
                return; // a banner line before the version: ignore it
            }
            self.client_version = line;
            self.phase = Phase::Kex;
            if self.server_kexinit.is_empty() {
                self.kexinit();
            }
        }
        loop {
            match self.next_packet() {
                Ok(Some(p)) => {
                    if let Err(e) = self.handle(&p, logins) {
                        crate::println!("[ssh] protocol error: {}", e);
                        self.closing = true;
                    }
                    if self.closing {
                        return;
                    }
                }
                Ok(None) => return,
                Err(e) => {
                    crate::println!("[ssh] protocol error: {}", e);
                    self.closing = true;
                    return;
                }
            }
        }
    }
}

/// Start listening on port 22 (`remember`: also turn it on for next boots).
pub fn start(remember: bool) -> Result<(), &'static str> {
    let mut s = SERVER.lock();
    if s.is_none() {
        crate::tcp::listen(PORT)?;
        let _ = host_key();
        *s = Some(Server { sessions: Vec::new(), logins: 0 });
        crate::println!("[ssh] listening on port {} (host key {})", PORT, fingerprint());
    }
    if remember {
        let _ = crate::vfs::write(CONFIG_FILE, b"on\r\n");
    }
    Ok(())
}

pub fn stop() {
    if let Some(srv) = SERVER.lock().take() {
        crate::tcp::unlisten(PORT);
        for s in srv.sessions {
            crate::tcp::close(s.conn);
        }
    }
    let _ = crate::vfs::write(CONFIG_FILE, b"off\r\n");
}

/// Start it at boot when it was turned on before (`/disk/SSHD.CFG`).
pub fn autostart() {
    if let Ok(b) = crate::vfs::read(CONFIG_FILE) {
        if b.starts_with(b"on") {
            let _ = start(false);
        }
    }
}

/// (running, open sessions, successful logins).
pub fn status() -> (bool, usize, u64) {
    SERVER.lock().as_ref().map_or((false, 0, 0), |s| (true, s.sessions.len(), s.logins))
}

/// Serve the sessions (desktop loop).
pub fn poll() {
    if SERVER.lock().is_none() {
        return;
    }
    crate::net::poll();
    let Some(mut guard) = SERVER.try_lock() else { return };
    let Some(srv) = guard.as_mut() else { return };
    while srv.sessions.len() < MAX_SESSIONS {
        let Some(conn) = crate::tcp::accept(PORT) else { break };
        let mut s = Session::new(conn);
        let v = alloc::format!("{}\r\n", VERSION);
        crate::tcp::send(conn, v.as_bytes());
        if let Some((ip, _)) = crate::tcp::peer(conn) {
            crate::println!("[ssh] connection from {}", crate::net::fmt_ip(ip));
        }
        s.inbuf.clear();
        srv.sessions.push(s);
    }
    let mut logins = srv.logins;
    for s in srv.sessions.iter_mut() {
        let bytes = crate::tcp::recv(s.conn, 64 * 1024);
        if !bytes.is_empty() {
            s.input(&bytes, &mut logins);
        }
        if crate::tcp::at_eof(s.conn) {
            s.closing = true;
        }
    }
    srv.logins = logins;
    srv.sessions.retain(|s| {
        if s.closing {
            crate::tcp::close(s.conn);
            false
        } else {
            true
        }
    });
}
