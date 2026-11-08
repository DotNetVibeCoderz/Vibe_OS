//! GDB remote stub for ring-3 programs: `gdbserver PORT APP [args]`.
//!
//! Starts APP under the debugger (`debug.rs`, stopped at its entry) and
//! serves the GDB Remote Serial Protocol on TCP PORT, so a GDB on a PC
//! debugs a program running on real hardware:
//!
//! ```text
//! (device)  gdbserver 1234 crashme
//! (PC)      gdb crashme.elf -ex "target remote 192.168.1.30:1234"
//! ```
//!
//! Packets: `?`, `g`/`p` (registers in GDB's order: amd64 rax..r15, rip,
//! eflags; aarch64 x0..x30, sp, pc, cpsr; riscv64 x0..x31, pc), `P` (set),
//! `m` (memory, breakpoints hidden), `Z0`/`z0` (software breakpoints), `c`,
//! `s`, `k`, `D`, thread queries (one thread), `QStartNoAckMode`, and
//! `qRcmd` (`monitor ...`: `monitor sym NAME` = a symbol's address, anything
//! else goes to the shell's `dbg`). Memory writes (`M`/`X`), watchpoints and
//! pause (Ctrl+C) are not supported: the program stops at breakpoints, after
//! a step, or at a fault. One session at a time; the stub is a kernel task
//! polling the network, so the desktop stays live meanwhile.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static PORT: AtomicU64 = AtomicU64::new(0);
static PID: AtomicU64 = AtomicU64::new(0);
/// `handle` already answered (the reply had to go out before a mode change).
const ALREADY_SENT: &str = "\u{0}";

/// GDB's register order for this architecture, as (frame name, bytes).
#[cfg(target_arch = "x86_64")]
const LAYOUT: &[(&str, usize)] = &[
    ("rax", 8), ("rbx", 8), ("rcx", 8), ("rdx", 8), ("rsi", 8), ("rdi", 8), ("rbp", 8), ("rsp", 8),
    ("r8", 8), ("r9", 8), ("r10", 8), ("r11", 8), ("r12", 8), ("r13", 8), ("r14", 8), ("r15", 8),
    ("rip", 8), ("rflags", 4),
];
#[cfg(target_arch = "aarch64")]
const LAYOUT: &[(&str, usize)] = &[
    ("x0", 8), ("x1", 8), ("x2", 8), ("x3", 8), ("x4", 8), ("x5", 8), ("x6", 8), ("x7", 8), ("x8", 8), ("x9", 8),
    ("x10", 8), ("x11", 8), ("x12", 8), ("x13", 8), ("x14", 8), ("x15", 8), ("x16", 8), ("x17", 8), ("x18", 8),
    ("x19", 8), ("x20", 8), ("x21", 8), ("x22", 8), ("x23", 8), ("x24", 8), ("x25", 8), ("x26", 8), ("x27", 8),
    ("x28", 8), ("x29", 8), ("x30", 8), ("sp", 8), ("pc", 8), ("pstate", 4),
];
#[cfg(target_arch = "riscv64")]
const LAYOUT: &[(&str, usize)] = &[
    ("zero", 8), ("ra", 8), ("sp", 8), ("gp", 8), ("tp", 8), ("t0", 8), ("t1", 8), ("t2", 8), ("s0", 8), ("s1", 8),
    ("a0", 8), ("a1", 8), ("a2", 8), ("a3", 8), ("a4", 8), ("a5", 8), ("a6", 8), ("a7", 8), ("s2", 8), ("s3", 8),
    ("s4", 8), ("s5", 8), ("s6", 8), ("s7", 8), ("s8", 8), ("s9", 8), ("s10", 8), ("s11", 8), ("t3", 8), ("t4", 8),
    ("t5", 8), ("t6", 8), ("pc", 8),
];

fn hex_le(v: u64, bytes: usize) -> String {
    (0..bytes).map(|i| format!("{:02x}", (v >> (8 * i)) & 0xff)).collect()
}

fn parse_le(h: &str) -> Option<u64> {
    if h.len() % 2 != 0 || h.len() > 16 {
        return None;
    }
    let mut v = 0u64;
    for i in 0..h.len() / 2 {
        v |= (u8::from_str_radix(&h[2 * i..2 * i + 2], 16).ok()? as u64) << (8 * i);
    }
    Some(v)
}

fn hex_text(s: &str) -> String {
    s.bytes().map(|b| format!("{:02x}", b)).collect()
}

fn unhex_text(h: &str) -> String {
    let b: Vec<u8> = (0..h.len() / 2).filter_map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).ok()).collect();
    String::from_utf8_lossy(&b).into_owned()
}

fn now_ms() -> u64 {
    crate::arch::cpu::nanos() / 1_000_000
}

/// `gdbserver PORT APP [args]`.
pub fn command(args: &[&str]) -> Vec<String> {
    let one = |s: String| alloc::vec![s];
    let [port, app, rest @ ..] = args else {
        return one(String::from("gdbserver PORT APP [arg...]   (lalu di PC: gdb APP.ELF -ex 'target remote IP:PORT')"));
    };
    let Ok(port) = port.parse::<u16>() else { return one(format!("gdbserver: port tidak sah: {}", port)) };
    if ACTIVE.swap(true, Ordering::SeqCst) {
        return one(String::from("gdbserver: sudah ada sesi (satu sesi sekaligus)"));
    }
    if crate::net::local_ip().is_none() {
        ACTIVE.store(false, Ordering::SeqCst);
        return one(String::from("gdbserver: tidak ada jaringan"));
    }
    let mut run: Vec<&str> = alloc::vec!["run", app];
    run.extend_from_slice(rest);
    let started = crate::debug::command(&run).join(" ");
    let Some(pid) = started.strip_prefix("pid ").and_then(|r| r.split_whitespace().next()).and_then(|p| p.parse::<u64>().ok()).filter(|_| started.contains("stopped")) else {
        ACTIVE.store(false, Ordering::SeqCst);
        return one(format!("gdbserver: {}", started));
    };
    if let Err(e) = crate::tcp::listen(port) {
        crate::debug::command(&["kill"]);
        ACTIVE.store(false, Ordering::SeqCst);
        return one(format!("gdbserver: port {}: {}", port, e));
    }
    PORT.store(port as u64, Ordering::SeqCst);
    PID.store(pid, Ordering::SeqCst);
    crate::task::spawn_named("gdbstub", serve);
    let ip = crate::net::local_ip().map(crate::net::fmt_ip).unwrap_or_default();
    one(format!("gdbserver: {} -- menunggu GDB di {}:{} (target remote {}:{})", started, ip, port, ip, port))
}

struct Conn {
    id: u64,
    buf: Vec<u8>,
    ack: bool,
}

impl Conn {
    fn send_packet(&mut self, data: &str) {
        let sum = data.bytes().fold(0u8, |a, b| a.wrapping_add(b));
        let p = format!("${}#{:02x}", data, sum);
        crate::tcp::send(self.id, p.as_bytes());
        crate::net::poll();
    }

    /// The next packet (None = the connection closed).
    fn next_packet(&mut self) -> Option<String> {
        loop {
            // a complete packet in the buffer?
            while let Some(&b) = self.buf.first() {
                match b {
                    b'+' | b'-' | 0x03 => {
                        self.buf.remove(0);
                    }
                    b'$' => break,
                    _ => {
                        self.buf.remove(0);
                    }
                }
            }
            if let Some(hash) = self.buf.iter().position(|&b| b == b'#') {
                if self.buf.len() >= hash + 3 {
                    let data = String::from_utf8_lossy(&self.buf[1..hash]).into_owned();
                    self.buf.drain(..hash + 3);
                    if self.ack {
                        crate::tcp::send(self.id, b"+");
                    }
                    return Some(data);
                }
            }
            crate::net::poll();
            let chunk = crate::tcp::recv(self.id, 4096);
            if chunk.is_empty() {
                if crate::tcp::at_eof(self.id) {
                    return None;
                }
                crate::task::yield_now();
            } else {
                self.buf.extend_from_slice(&chunk);
            }
        }
    }
}

fn dbg(pid: u64, args: &[&str]) -> String {
    let p = pid.to_string();
    crate::debug::command(&["pid", &p]);
    crate::debug::command(args).join("\n")
}

/// The stop reply for a `dbg` status line; None = still running.
fn stop_reply(status: &str) -> Option<String> {
    if status.contains("is running") {
        return None;
    }
    if let Some(i) = status.find("exited with code 0x") {
        let code = u64::from_str_radix(status[i + 19..].split_whitespace().next().unwrap_or("0"), 16).unwrap_or(0);
        return Some(format!("W{:02x}", code & 0xff));
    }
    if status.contains("stopped") {
        return Some(String::from("S05"));
    }
    Some(String::from("X09")) // gone (killed / not debugged)
}

fn registers(pid: u64) -> Option<Vec<u64>> {
    let p = pid.to_string();
    crate::debug::command(&["pid", &p]);
    let regs = crate::debug::raw_regs(pid)?;
    Some(LAYOUT.iter().map(|(n, _)| regs.iter().find(|(r, _)| r == n).map_or(0, |(_, v)| *v)).collect())
}

/// One request; the reply (None = end the session).
fn handle(pid: u64, p: &str, conn: &mut Conn) -> Option<String> {
    let reply = match p {
        "?" => String::from("S05"),
        "qAttached" => String::from("1"),
        "qC" => String::from("QC1"),
        "qfThreadInfo" => String::from("m1"),
        "qsThreadInfo" => String::from("l"),
        "QStartNoAckMode" => {
            conn.send_packet("OK");
            conn.ack = false;
            return Some(String::from(ALREADY_SENT));
        }
        "g" => match registers(pid) {
            Some(v) => LAYOUT.iter().zip(v).map(|((_, n), x)| hex_le(x, *n)).collect(),
            None => String::from("E01"),
        },
        "k" => {
            dbg(pid, &["kill"]);
            return None;
        }
        "D" | "D;1" => {
            dbg(pid, &["detach"]);
            conn.send_packet("OK");
            return None;
        }
        "c" | "s" => {
            let mut status = dbg(pid, &[if p == "c" { "cont" } else { "step" }]);
            loop {
                if let Some(r) = stop_reply(&status) {
                    break r;
                }
                crate::task::yield_now();
                status = dbg(pid, &["wait", "2000"]);
            }
        }
        _ if p.starts_with("qSupported") => String::from("PacketSize=1000;QStartNoAckMode+"),
        _ if p.starts_with('H') || p.starts_with('T') => String::from("OK"),
        _ if p.starts_with("qSymbol") => String::from("OK"),
        _ if p.starts_with('p') => {
            let n = usize::from_str_radix(&p[1..], 16).unwrap_or(usize::MAX);
            match (registers(pid), LAYOUT.get(n)) {
                (Some(v), Some((_, bytes))) => hex_le(v[n], *bytes),
                _ => String::from("E01"),
            }
        }
        _ if p.starts_with('P') => {
            let parsed = p[1..].split_once('=').and_then(|(n, v)| Some((usize::from_str_radix(n, 16).ok()?, parse_le(v)?)));
            match parsed.and_then(|(n, v)| LAYOUT.get(n).map(|(name, _)| (*name, v))) {
                Some((name, v)) => {
                    let out = dbg(pid, &["set", name, &format!("{:#x}", v)]);
                    if out.starts_with("dbg:") { String::from("E01") } else { String::from("OK") }
                }
                None => String::from("E01"),
            }
        }
        _ if p.starts_with('m') => {
            let parsed = p[1..].split_once(',').and_then(|(a, n)| Some((u64::from_str_radix(a, 16).ok()?, usize::from_str_radix(n, 16).ok()?)));
            match parsed {
                Some((a, n)) => {
                    let pid_s = pid.to_string();
                    crate::debug::command(&["pid", &pid_s]);
                    match crate::debug::raw_mem(pid, a, n) {
                        Some(b) => b.iter().map(|x| format!("{:02x}", x)).collect(),
                        None => String::from("E14"),
                    }
                }
                None => String::from("E01"),
            }
        }
        _ if p.starts_with("Z0,") || p.starts_with("z0,") => {
            let addr = p[3..].split(',').next().and_then(|a| u64::from_str_radix(a, 16).ok());
            match addr {
                Some(a) => {
                    let out = dbg(pid, &[if p.starts_with('Z') { "break" } else { "delete" }, &format!("{:#x}", a)]);
                    if out.starts_with("dbg:") { String::from("E01") } else { String::from("OK") }
                }
                None => String::from("E01"),
            }
        }
        _ if p.starts_with("qRcmd,") => {
            let cmd = unhex_text(&p[6..]);
            let words: Vec<&str> = cmd.split_whitespace().collect();
            let out = match words.as_slice() {
                ["sym", name] => match crate::crash::find_symbol(pid, name) {
                    Ok(a) => format!("{:#x}\n", a),
                    Err(e) => format!("{}\n", e),
                },
                [] => String::from("monitor sym NAME | monitor <perintah dbg> (bt, regs, mem ADDR N, breaks)\n"),
                w => format!("{}\n", dbg(pid, w)),
            };
            hex_text(&out)
        }
        _ => String::new(), // not supported: GDB falls back
    };
    Some(reply)
}

/// The stub's kernel task: accept one GDB, serve it, end the program.
fn serve() {
    let port = PORT.load(Ordering::SeqCst) as u16;
    let pid = PID.load(Ordering::SeqCst);
    let t0 = now_ms();
    let id = loop {
        crate::net::poll();
        if let Some(id) = crate::tcp::accept(port) {
            break Some(id);
        }
        if now_ms() - t0 > 600_000 || !crate::process::is_alive(pid) {
            break None;
        }
        crate::task::yield_now();
    };
    crate::tcp::unlisten(port);
    if let Some(id) = id {
        crate::println!("[gdbstub] GDB connected (pid {})", pid);
        let mut conn = Conn { id, buf: Vec::new(), ack: true };
        while let Some(p) = conn.next_packet() {
            match handle(pid, &p, &mut conn) {
                Some(r) if r == ALREADY_SENT => {}
                Some(r) => conn.send_packet(&r),
                None => break,
            }
        }
        let t = now_ms();
        while crate::tcp::unsent(id) > 0 && now_ms() - t < 2000 {
            crate::net::poll();
            crate::task::yield_now();
        }
        crate::tcp::close(id);
    } else {
        crate::println!("[gdbstub] no GDB within 10 minutes (or the program ended)");
    }
    // a program GDB left stopped (or never attached to) ends with the session
    if crate::process::is_alive(pid) && dbg(pid, &["list"]).contains(&format!("pid {}", pid)) {
        dbg(pid, &["kill"]);
    }
    crate::println!("[gdbstub] session ended (pid {})", pid);
    ACTIVE.store(false, Ordering::SeqCst);
}
