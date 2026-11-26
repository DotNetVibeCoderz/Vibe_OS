//! A packet filter for the network stack (generic, every kernel).
//!
//! Rules live in `/disk/FIREWALL.CFG`, one per line, first match wins:
//!
//! ```text
//! enabled=on
//! default in allow            # policy when no rule matches (allow / deny)
//! default out allow
//! deny in tcp 22              # nobody reaches the SSH server
//! allow in tcp 22 from 192.168.68.0/24
//! deny in udp 7070-7071
//! deny out 10.0.0.0/8         # any protocol, any port
//! deny out tcp 25
//! deny out app mweb           # per program: Peramban gets no network
//! allow out app mweb 93.184.216.34/32 tcp 443
//! ```
//!
//! Inbound rules see new TCP connections (a SYN without ACK) and every UDP
//! datagram; replies to our own connections pass. Outbound rules see TCP
//! connects and UDP sends; `app NAME` matches the calling program (its
//! process name, or its package). Traffic between this machine and itself
//! (loopback, our own address) is never filtered. Address masks apply to
//! IPv4; an IPv6 peer matches only rules without an address.
//!
//! Per-app *permission* (may this program use the network at all) is the
//! permission broker's (`perm.rs`, `bz perm`); this filter decides *where*.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

const CONFIG: &str = "/disk/FIREWALL.CFG";
const MAX_RULES: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Proto {
    Tcp,
    Udp,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Rule {
    allow: bool,
    inbound: bool,
    proto: Option<Proto>,
    ports: Option<(u16, u16)>,
    /// IPv4 network and prefix length (source for in, destination for out).
    net: Option<([u8; 4], u8)>,
    /// Outbound only: the program's name.
    app: Option<String>,
    hits: u64,
}

struct Table {
    rules: Vec<Rule>,
    default_in: bool,
    default_out: bool,
}

static TABLE: Mutex<Table> = Mutex::new(Table { rules: Vec::new(), default_in: true, default_out: true });
static ENABLED: AtomicBool = AtomicBool::new(false);
static DROPPED_IN: AtomicU64 = AtomicU64::new(0);
static DROPPED_OUT: AtomicU64 = AtomicU64::new(0);
static LOGGED: AtomicU64 = AtomicU64::new(0);

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

fn parse_net(s: &str) -> Option<([u8; 4], u8)> {
    let (ip, bits) = match s.split_once('/') {
        Some((a, b)) => (a, b.parse::<u8>().ok()?),
        None => (s, 32),
    };
    if bits > 32 {
        return None;
    }
    let mut out = [0u8; 4];
    let mut parts = ip.split('.');
    for b in out.iter_mut() {
        *b = parts.next()?.parse().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some((out, bits))
}

fn parse_ports(s: &str) -> Option<(u16, u16)> {
    match s.split_once('-') {
        Some((a, b)) => {
            let (a, b) = (a.parse().ok()?, b.parse().ok()?);
            (a <= b).then_some((a, b))
        }
        None => s.parse().ok().map(|p| (p, p)),
    }
}

/// Parse one rule: `allow|deny in|out [app NAME] [tcp|udp|any] [PORTS] [from|to] [A.B.C.D[/N]]`
/// (words in any order after the direction).
fn parse_rule(line: &str) -> Result<Rule, String> {
    let w: Vec<&str> = line.split_whitespace().collect();
    if w.len() < 2 {
        return Err("terlalu pendek".to_string());
    }
    let allow = match w[0] {
        "allow" | "izinkan" => true,
        "deny" | "tolak" | "block" => false,
        x => return Err(format!("'{}': harus allow atau deny", x)),
    };
    let inbound = match w[1] {
        "in" | "masuk" => true,
        "out" | "keluar" => false,
        x => return Err(format!("'{}': harus in atau out", x)),
    };
    let mut r = Rule { allow, inbound, proto: None, ports: None, net: None, app: None, hits: 0 };
    let mut i = 2;
    while i < w.len() {
        match w[i] {
            "tcp" => r.proto = Some(Proto::Tcp),
            "udp" => r.proto = Some(Proto::Udp),
            "any" | "semua" => r.proto = None,
            "from" | "to" | "dari" | "ke" => {}
            "app" | "aplikasi" => {
                if inbound {
                    return Err("'app' hanya untuk aturan out".to_string());
                }
                i += 1;
                let name = w.get(i).ok_or("nama aplikasi?")?;
                r.app = Some(name.to_ascii_lowercase());
            }
            x if x.contains('.') => r.net = Some(parse_net(x).ok_or(format!("alamat '{}' tidak sah", x))?),
            x if x.as_bytes()[0].is_ascii_digit() => r.ports = Some(parse_ports(x).ok_or(format!("port '{}' tidak sah", x))?),
            x => return Err(format!("kata '{}' tidak dikenal", x)),
        }
        i += 1;
    }
    Ok(r)
}

fn rule_text(r: &Rule) -> String {
    let mut s = String::from(if r.allow { "allow" } else { "deny" });
    s.push_str(if r.inbound { " in" } else { " out" });
    if let Some(a) = &r.app {
        s.push_str(" app ");
        s.push_str(a);
    }
    match r.proto {
        Some(Proto::Tcp) => s.push_str(" tcp"),
        Some(Proto::Udp) => s.push_str(" udp"),
        None => {}
    }
    if let Some((a, b)) = r.ports {
        s.push_str(&if a == b { format!(" {}", a) } else { format!(" {}-{}", a, b) });
    }
    if let Some((ip, bits)) = r.net {
        s.push_str(&format!(" {} {}.{}.{}.{}/{}", if r.inbound { "from" } else { "to" }, ip[0], ip[1], ip[2], ip[3], bits));
    }
    s
}

fn in_net(ip: Option<[u8; 4]>, net: Option<([u8; 4], u8)>) -> bool {
    let Some((n, bits)) = net else { return true };
    let Some(ip) = ip else { return false };
    if bits == 0 {
        return true;
    }
    let mask = u32::MAX << (32 - bits as u32);
    u32::from_be_bytes(ip) & mask == u32::from_be_bytes(n) & mask
}

fn matches(r: &Rule, inbound: bool, proto: Proto, ip: Option<[u8; 4]>, port: u16, app: Option<&str>) -> bool {
    r.inbound == inbound
        && r.proto.is_none_or(|p| p == proto)
        && r.ports.is_none_or(|(a, b)| port >= a && port <= b)
        && in_net(ip, r.net)
        && match &r.app {
            None => true,
            Some(name) => app.is_some_and(|a| a.eq_ignore_ascii_case(name)),
        }
}

fn decide(inbound: bool, proto: Proto, ip: Option<[u8; 4]>, port: u16, app: Option<&str>) -> (bool, Option<String>) {
    let mut t = TABLE.lock();
    for r in t.rules.iter_mut() {
        if matches(r, inbound, proto, ip, port, app) {
            r.hits += 1;
            let why = (!r.allow).then(|| rule_text(r));
            return (r.allow, why);
        }
    }
    let allow = if inbound { t.default_in } else { t.default_out };
    (allow, (!allow).then(|| String::from(if inbound { "default in deny" } else { "default out deny" })))
}

fn local(ip: Option<[u8; 4]>) -> bool {
    match ip {
        Some(ip) => ip[0] == 127 || Some(ip) == crate::net::local_ip(),
        None => false,
    }
}

fn note_drop(inbound: bool, proto: Proto, ip: Option<[u8; 4]>, port: u16, why: &str) {
    if inbound { &DROPPED_IN } else { &DROPPED_OUT }.fetch_add(1, Ordering::Relaxed);
    // the first drops are logged; after that only counted
    if LOGGED.fetch_add(1, Ordering::Relaxed) < 20 {
        let peer = match ip {
            Some(i) => format!("{}.{}.{}.{}", i[0], i[1], i[2], i[3]),
            None => String::from("IPv6"),
        };
        crate::println!(
            "[firewall] dropped {} {} {} port {} ({})",
            if inbound { "inbound" } else { "outbound" },
            if proto == Proto::Tcp { "tcp" } else { "udp" },
            peer,
            port,
            why
        );
    }
}

/// Inbound: a new TCP connection or a UDP datagram from `src` (None for an
/// IPv6 peer) to our port `port`. Called from packet input: takes only the
/// table lock.
pub fn allow_in(proto: Proto, src: Option<[u8; 4]>, port: u16) -> bool {
    if !enabled() || local(src) {
        return true;
    }
    let (ok, why) = decide(true, proto, src, port, None);
    if !ok {
        note_drop(true, proto, src, port, why.as_deref().unwrap_or(""));
    }
    ok
}

/// Inbound TCP segment `seg` (header first): only connection requests (SYN
/// without ACK) are filtered.
pub fn allow_tcp_segment(src: Option<[u8; 4]>, seg: &[u8]) -> bool {
    if !enabled() || seg.len() < 14 {
        return true;
    }
    let flags = seg[13];
    if flags & 0x02 == 0 || flags & 0x10 != 0 {
        return true;
    }
    allow_in(Proto::Tcp, src, u16::from_be_bytes([seg[2], seg[3]]))
}

/// Outbound: the running program connects / sends to `dst` (None = IPv6) `port`.
pub fn allow_out(proto: Proto, dst: Option<[u8; 4]>, port: u16) -> bool {
    if !enabled() || local(dst) {
        return true;
    }
    let pid = crate::aspace::current_id();
    let app = if pid == 0 { None } else { crate::process::name_of(pid) };
    // a program file name (CLRHOST.ELF runs managed apps under their DLL name)
    let app = app.map(|a| {
        let base = a.rsplit('/').next().unwrap_or(&a).to_ascii_lowercase();
        String::from(base.split('.').next().unwrap_or(&base))
    });
    let (ok, why) = decide(false, proto, dst, port, app.as_deref());
    if !ok {
        note_drop(false, proto, dst, port, why.as_deref().unwrap_or(""));
    }
    ok
}

fn apply(text: &str) -> Result<usize, String> {
    let mut rules = Vec::new();
    let (mut din, mut dout, mut on) = (true, true, false);
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(v) = line.strip_prefix("enabled=") {
            on = matches!(v.trim(), "on" | "1" | "yes" | "ya");
            continue;
        }
        let w: Vec<&str> = line.split_whitespace().collect();
        if w.first() == Some(&"default") && w.len() == 3 {
            let allow = match w[2] {
                "allow" => true,
                "deny" => false,
                _ => return Err(format!("baris {}: default in|out allow|deny", n + 1)),
            };
            match w[1] {
                "in" => din = allow,
                "out" => dout = allow,
                _ => return Err(format!("baris {}: default in|out allow|deny", n + 1)),
            }
            continue;
        }
        if rules.len() >= MAX_RULES {
            return Err(format!("lebih dari {} aturan", MAX_RULES));
        }
        rules.push(parse_rule(line).map_err(|e| format!("baris {}: {}", n + 1, e))?);
    }
    let count = rules.len();
    *TABLE.lock() = Table { rules, default_in: din, default_out: dout };
    ENABLED.store(on, Ordering::Relaxed);
    Ok(count)
}

fn config_text() -> String {
    let t = TABLE.lock();
    let mut s = format!(
        "# Buitenzorg firewall (shell: firewall)\nenabled={}\ndefault in {}\ndefault out {}\n",
        if enabled() { "on" } else { "off" },
        if t.default_in { "allow" } else { "deny" },
        if t.default_out { "allow" } else { "deny" }
    );
    for r in &t.rules {
        s.push_str(&rule_text(r));
        s.push('\n');
    }
    s
}

fn save() -> Result<(), &'static str> {
    crate::vfs::write(CONFIG, config_text().as_bytes()).map(|_| ())
}

/// Read `/disk/FIREWALL.CFG` (boot, after /disk is mounted).
pub fn load() {
    let Ok(bytes) = crate::vfs::read(CONFIG) else { return };
    match apply(&String::from_utf8_lossy(&bytes)) {
        Ok(n) => crate::println!("[firewall] {} rule(s) loaded, {}", n, if enabled() { "on" } else { "off" }),
        Err(e) => crate::println!("[firewall] {}: {} -- firewall off", CONFIG, e),
    }
}

/// Shell `firewall ...`.
pub fn command(args: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    match args.first().copied() {
        None | Some("status") => {
            let t = TABLE.lock();
            out.push(format!(
                "firewall: {} (default in {}, out {}); dibuang: {} masuk, {} keluar",
                if enabled() { "AKTIF" } else { "mati" },
                if t.default_in { "allow" } else { "deny" },
                if t.default_out { "allow" } else { "deny" },
                DROPPED_IN.load(Ordering::Relaxed),
                DROPPED_OUT.load(Ordering::Relaxed)
            ));
            for (i, r) in t.rules.iter().enumerate() {
                out.push(format!("  {:>2}. {:<48} {} kena", i + 1, rule_text(r), r.hits));
            }
            if t.rules.is_empty() {
                out.push(String::from("  (belum ada aturan -- contoh: firewall deny in tcp 22)"));
            }
        }
        Some("on") | Some("off") => {
            ENABLED.store(args[0] == "on", Ordering::Relaxed);
            out.push(format!("firewall {}{}", if args[0] == "on" { "aktif" } else { "mati" }, save_note()));
        }
        Some("allow") | Some("deny") => match parse_rule(&args.join(" ")) {
            Ok(r) => {
                let text = rule_text(&r);
                let mut t = TABLE.lock();
                if t.rules.len() >= MAX_RULES {
                    out.push(String::from("firewall: aturan sudah penuh"));
                } else {
                    t.rules.push(r);
                    drop(t);
                    out.push(format!("firewall: + {}{}", text, save_note()));
                }
            }
            Err(e) => out.push(format!("firewall: {}", e)),
        },
        Some("insert") if args.len() > 2 => match (args[1].parse::<usize>(), parse_rule(&args[2..].join(" "))) {
            (Ok(n), Ok(r)) => {
                let mut t = TABLE.lock();
                let at = n.saturating_sub(1).min(t.rules.len());
                t.rules.insert(at, r);
                drop(t);
                out.push(format!("firewall: disisipkan di {}{}", at + 1, save_note()));
            }
            (_, Err(e)) => out.push(format!("firewall: {}", e)),
            _ => out.push(String::from("firewall insert N RULE")),
        },
        Some("default") if args.len() == 3 => {
            let allow = args[2] == "allow";
            let mut t = TABLE.lock();
            match args[1] {
                "in" => t.default_in = allow,
                "out" => t.default_out = allow,
                _ => {}
            }
            drop(t);
            out.push(format!("firewall: default {} {}{}", args[1], args[2], save_note()));
        }
        Some("remove") | Some("hapus") if args.len() == 2 => {
            let mut t = TABLE.lock();
            match args[1].parse::<usize>() {
                Ok(n) if n >= 1 && n <= t.rules.len() => {
                    let r = t.rules.remove(n - 1);
                    drop(t);
                    out.push(format!("firewall: - {}{}", rule_text(&r), save_note()));
                }
                _ => out.push(String::from("firewall remove N (nomor dari 'firewall')")),
            }
        }
        Some("reset") => {
            *TABLE.lock() = Table { rules: Vec::new(), default_in: true, default_out: true };
            ENABLED.store(false, Ordering::Relaxed);
            out.push(format!("firewall: semua aturan dihapus, mati{}", save_note()));
        }
        Some("reload") => {
            load();
            out.push(format!("firewall: dimuat ulang ({} aturan)", TABLE.lock().rules.len()));
        }
        Some("check") if args.len() >= 4 => {
            // firewall check in|out tcp|udp PORT [IP]
            let proto = if args[2] == "udp" { Proto::Udp } else { Proto::Tcp };
            let port = args[3].parse().unwrap_or(0);
            let ip = args.get(4).and_then(|s| parse_net(s)).map(|n| n.0);
            let (ok, why) = decide(args[1] == "in", proto, ip, port, None);
            out.push(format!("{}{}", if ok { "diizinkan" } else { "ditolak" }, why.map(|w| format!(" ({})", w)).unwrap_or_default()));
        }
        _ => {
            out.push(String::from("firewall [status|on|off|reload|reset]"));
            out.push(String::from("firewall allow|deny in tcp|udp PORT[-PORT] [from A.B.C.D/N]"));
            out.push(String::from("firewall allow|deny out [app NAME] [tcp|udp] [PORT] [A.B.C.D/N]"));
            out.push(String::from("firewall insert N RULE | remove N | default in|out allow|deny"));
            out.push(String::from("firewall check in|out tcp|udp PORT [IP]"));
        }
    }
    out
}

fn save_note() -> &'static str {
    match save() {
        Ok(()) => "",
        Err(_) => " (tidak tersimpan: /disk tidak ada)",
    }
}

/// Self-test: rule parsing, first match, ports/masks/apps, defaults, the
/// segment filter (SYN only), and real loopback traffic is never filtered.
/// The user's rules and file are put back.
pub fn self_test() -> bool {
    let saved = config_text();
    let was_on = enabled();
    let (in0, out0, log0) = (DROPPED_IN.load(Ordering::Relaxed), DROPPED_OUT.load(Ordering::Relaxed), LOGGED.load(Ordering::Relaxed));
    LOGGED.store(u64::MAX / 2, Ordering::Relaxed); // the test's drops are not logged
    let mut fails: Vec<&str> = Vec::new();
    let mut check = |ok: bool, what: &'static str| {
        if !ok {
            fails.push(what);
        }
    };
    let text = "enabled=on\ndefault in allow\ndefault out allow\n\
                allow in tcp 2222 from 10.0.2.0/24\n\
                deny in tcp 2222\n\
                deny in udp 7000-7010\n\
                deny out 203.0.113.0/24\n\
                deny out app uji tcp 443\n";
    check(apply(text) == Ok(5), "parse");
    check(!allow_in(Proto::Tcp, Some([192, 0, 2, 1]), 2222), "deny in tcp");
    check(allow_in(Proto::Tcp, Some([10, 0, 2, 2]), 2222), "allow before deny (first match)");
    check(allow_in(Proto::Tcp, Some([192, 0, 2, 1]), 80), "other port passes");
    check(!allow_in(Proto::Udp, Some([192, 0, 2, 1]), 7005), "port range");
    check(allow_in(Proto::Udp, Some([192, 0, 2, 1]), 7011), "outside range");
    check(allow_in(Proto::Udp, Some([127, 0, 0, 1]), 7005), "loopback never filtered");
    check(!allow_out(Proto::Tcp, Some([203, 0, 113, 9]), 80), "deny out network");
    check(allow_out(Proto::Tcp, Some([198, 51, 100, 9]), 80), "other network passes");
    check(decide(false, Proto::Tcp, Some([1, 1, 1, 1]), 443, Some("uji")).0 == false, "per-app out rule");
    check(decide(false, Proto::Tcp, Some([1, 1, 1, 1]), 443, Some("lain")).0, "other app passes");
    // a SYN to 2222 is dropped, an ACK (reply of a connection) is not
    let mut syn = [0u8; 20];
    syn[2..4].copy_from_slice(&2222u16.to_be_bytes());
    syn[13] = 0x02;
    check(!allow_tcp_segment(Some([192, 0, 2, 1]), &syn), "SYN filtered");
    syn[13] = 0x12;
    check(allow_tcp_segment(Some([192, 0, 2, 1]), &syn), "SYN+ACK passes");
    check(parse_rule("deny in app x tcp 1").is_err() && parse_rule("allow sideways tcp 1").is_err(), "bad rules refused");
    check(apply("default in deny\nenabled=on\n") == Ok(0) && !allow_in(Proto::Tcp, Some([192, 0, 2, 1]), 80), "default in deny");
    // put back the user's table (not the file: it was never written)
    let _ = apply(&saved);
    ENABLED.store(was_on, Ordering::Relaxed);
    let n = DROPPED_IN.load(Ordering::Relaxed) - in0 + DROPPED_OUT.load(Ordering::Relaxed) - out0;
    DROPPED_IN.store(in0, Ordering::Relaxed);
    DROPPED_OUT.store(out0, Ordering::Relaxed);
    LOGGED.store(log0, Ordering::Relaxed);
    if fails.is_empty() {
        crate::println!("[kernel] MILESTONE: FIREWALL OK (in/out rules, ports, masks, per-app, SYN-only, loopback exempt; {} test drops)", n);
        true
    } else {
        crate::println!("[kernel] firewall self-test FAILED: {:?}", fails);
        false
    }
}
