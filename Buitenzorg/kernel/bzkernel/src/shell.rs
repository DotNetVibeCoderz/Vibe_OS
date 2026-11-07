//! Built-in shell (v0.7 "Kanopi"): popular cross-ecosystem commands (Unix +
//! Windows aliases) over the VFS, the theme, and the window manager. Pure over
//! its input line: `run` returns output lines plus an [`Effect`].
//!
//! Supported: help, ls/dir, cat/type, echo, pwd, cd, clear/cls, ver, uname,
//! theme, ws, mounts, bz.

use alloc::{format, string::String, string::ToString, vec, vec::Vec};
use spin::Mutex;

use crate::{ai, app, model, pkg, power, screensaver, script, theme, vfs, vmm, vmx, wallpaper, wm};

pub struct ShellState {
    pub cwd: String,
    /// Shell variables (`NAME=value`, `set`, `export`; `$NAME` expands them).
    pub vars: alloc::collections::BTreeMap<String, String>,
    /// Whether the last command succeeded (`$?`, `&&`, `||`).
    pub ok: bool,
}

static STATE: Mutex<ShellState> = Mutex::new(ShellState { cwd: String::new(), vars: alloc::collections::BTreeMap::new(), ok: true });

/// The shell variables (set ones, then the built-in PWD/HOME/USER/SHELL).
pub fn vars() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = STATE.lock().vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for k in ["PWD", "HOME", "USER", "SHELL", "PATH"] {
        if !v.iter().any(|(n, _)| n == k) {
            v.push((String::from(k), var(k).unwrap_or_default()));
        }
    }
    v.sort();
    v
}

fn var(name: &str) -> Option<String> {
    if let Some(v) = STATE.lock().vars.get(name) {
        return Some(v.clone());
    }
    match name {
        "PWD" => Some(cwd()),
        "HOME" => Some(String::from("/disk")),
        "USER" => Some(String::from("administrator")),
        "SHELL" => Some(String::from("bzsh")),
        "PATH" => Some(String::from("/disk:/ram")),
        _ => None,
    }
}

/// A variable's value (set, else built-in), for other modules.
pub fn var_value(name: &str) -> Option<String> {
    var(name)
}

/// Whether the last command succeeded (`$?` = 0).
pub fn last_ok() -> bool {
    STATE.lock().ok
}

/// Whether `name` is one of the shell's own (one-line) commands.
pub fn is_legacy_command(name: &str) -> bool {
    LEGACY_COMMANDS.contains(&name)
}

/// Expand a word list (variables, quotes, globs) the way a command line's
/// arguments are expanded; operators are an error. Used by `for X in ...`.
pub fn expand_words(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for t in tokenize(text)? {
        match t {
            Tok::Word(w) => out.push(w),
            Tok::Op(op) => return Err(format!("{} tidak boleh di sini", op)),
        }
    }
    Ok(out)
}

/// Set a variable; false when `name` is not a valid name.
pub fn set_var(name: &str, value: &str) -> bool {
    let valid = !name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if valid {
        STATE.lock().vars.insert(String::from(name), String::from(value));
    }
    valid
}

pub fn unset_var(name: &str) {
    STATE.lock().vars.remove(name);
}

/// Every command name (the shell's own and the core utilities), for Tab.
pub fn command_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = LEGACY_COMMANDS.to_vec();
    v.extend_from_slice(crate::coreutils::NAMES);
    v.extend(crate::bzsh::NAMES.iter().filter(|n| **n != "."));
    v.sort();
    v.dedup();
    v
}

const LEGACY_COMMANDS: &[&str] = &[
    "help", "ls", "dir", "echo", "rm", "del", "df", "pwd", "cd", "clear", "cls", "ver", "uname", "about", "mounts",
    "dmesg", "lspci", "gpu", "theme", "ws", "run", "ps", "crash", "dbg", "tutup", "tile", "cpu", "kill", "deploy", "ssh", "ipconfig",
    "netup", "ping", "arp", "route", "netstat", "hostname", "nslookup", "bg", "saver", "cursor", "anim", "tone",
    "settings", "ask", "vm", "script", "js", "ts", "py", "csx", "prof", "gfx3d", "shutdown", "restart", "sleep", "bz",
];

// ---- command lines: quoting, variables, globs, pipes, redirection ----------
//
// A line is commands joined by `;`, `&&` (run if the previous one succeeded)
// and `||` (run if it failed); a command is a pipeline `a | b | c` whose
// stages pass their output lines on as the next one's input; any stage may
// redirect: `> FILE` (write), `>> FILE` (append), `< FILE` (input). Words may
// be quoted ('...' literal, "..." with $ expansion) or escaped with \;
// `$NAME`, `${NAME}` and `$?` expand; an unquoted word with * or ? expands to
// the matching file names. A command that is only `NAME=value` sets a
// variable. The core utilities (coreutils.rs) read their input from the
// pipe; the shell's other commands ignore it.

#[derive(Clone, PartialEq)]
enum Tok {
    Word(String),
    Op(&'static str),
}

/// Expand `$...` at `chars[i]` (a '$') into `out`; returns the index after it.
fn expand_var(chars: &[char], i: usize, out: &mut String) -> usize {
    let mut j = i + 1;
    if j < chars.len() && chars[j] == '?' {
        out.push(if STATE.lock().ok { '0' } else { '1' });
        return j + 1;
    }
    // script parameters: $0..$9, $#, $@
    if j < chars.len() && chars[j].is_ascii_digit() {
        let n = chars[j] as usize - '0' as usize;
        out.push_str(&crate::bzsh::positional(n).unwrap_or_default());
        return j + 1;
    }
    if j < chars.len() && chars[j] == '#' {
        out.push_str(&crate::bzsh::arg_count().to_string());
        return j + 1;
    }
    if j < chars.len() && chars[j] == '@' {
        out.push_str(&crate::bzsh::all_args());
        return j + 1;
    }
    let braced = j < chars.len() && chars[j] == '{';
    if braced {
        j += 1;
    }
    let start = j;
    while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
        j += 1;
    }
    if j == start {
        out.push('$');
        return i + 1;
    }
    let name: String = chars[start..j].iter().collect();
    if braced && j < chars.len() && chars[j] == '}' {
        j += 1;
    }
    out.push_str(&var(&name).unwrap_or_default());
    j
}

/// Expand a glob word against its folder; the word itself when nothing matches.
fn expand_glob(word: &str) -> Vec<String> {
    let (dir, pattern) = match word.rfind('/') {
        Some(i) => (&word[..=i], &word[i + 1..]),
        None => ("", word),
    };
    let folder = resolve(if dir.is_empty() { None } else { Some(dir) });
    let mut hits: Vec<String> = vfs::list_entries(&folder)
        .map(|es| {
            es.into_iter()
                .filter(|e| e.name != "." && e.name != ".." && crate::coreutils::wildcard(pattern, &e.name))
                .map(|e| format!("{}{}", dir, e.name))
                .collect()
        })
        .unwrap_or_default();
    hits.sort();
    if hits.is_empty() { vec![String::from(word)] } else { hits }
}

fn tokenize(line: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = line.chars().collect();
    let mut toks = Vec::new();
    let mut cur = String::new();
    let (mut has, mut glob) = (false, false);
    fn flush(toks: &mut Vec<Tok>, cur: &mut String, has: &mut bool, glob: &mut bool) {
        if *has {
            if *glob {
                toks.extend(expand_glob(cur).into_iter().map(Tok::Word));
            } else {
                toks.push(Tok::Word(core::mem::take(cur)));
            }
        }
        cur.clear();
        *has = false;
        *glob = false;
    }
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            ' ' | '\t' => flush(&mut toks, &mut cur, &mut has, &mut glob),
            '\'' => {
                has = true;
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    cur.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err(String::from("tanda kutip ' tidak ditutup"));
                }
            }
            '"' => {
                has = true;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    // "$@": one word per script argument
                    if chars[i] == '$' && chars.get(i + 1) == Some(&'@') {
                        for (k, a) in crate::bzsh::arg_list().iter().enumerate() {
                            if k > 0 {
                                flush(&mut toks, &mut cur, &mut has, &mut glob);
                                has = true;
                            }
                            cur.push_str(a);
                        }
                        i += 2;
                        continue;
                    }
                    if chars[i] == '$' {
                        i = expand_var(&chars, i, &mut cur);
                        continue;
                    }
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                    }
                    cur.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err(String::from("tanda kutip \" tidak ditutup"));
                }
            }
            '\\' => {
                if let Some(n) = next {
                    cur.push(n);
                    has = true;
                    i += 1;
                }
            }
            '$' if next == Some('@') => {
                // $@ unquoted: the script's arguments as separate words
                for (k, a) in crate::bzsh::arg_list().iter().enumerate() {
                    if k > 0 {
                        flush(&mut toks, &mut cur, &mut has, &mut glob);
                    }
                    cur.push_str(a);
                    has = true;
                }
                i += 2;
                continue;
            }
            '$' => {
                has = true;
                i = expand_var(&chars, i, &mut cur);
                continue;
            }
            ';' => {
                flush(&mut toks, &mut cur, &mut has, &mut glob);
                toks.push(Tok::Op(";"));
            }
            '&' if next == Some('&') => {
                flush(&mut toks, &mut cur, &mut has, &mut glob);
                toks.push(Tok::Op("&&"));
                i += 1;
            }
            '|' => {
                flush(&mut toks, &mut cur, &mut has, &mut glob);
                if next == Some('|') {
                    toks.push(Tok::Op("||"));
                    i += 1;
                } else {
                    toks.push(Tok::Op("|"));
                }
            }
            '>' => {
                flush(&mut toks, &mut cur, &mut has, &mut glob);
                if next == Some('>') {
                    toks.push(Tok::Op(">>"));
                    i += 1;
                } else {
                    toks.push(Tok::Op(">"));
                }
            }
            '<' => {
                flush(&mut toks, &mut cur, &mut has, &mut glob);
                toks.push(Tok::Op("<"));
            }
            '*' | '?' => {
                glob = true;
                has = true;
                cur.push(c);
            }
            _ => {
                has = true;
                cur.push(c);
            }
        }
        i += 1;
    }
    flush(&mut toks, &mut cur, &mut has, &mut glob);
    Ok(toks)
}

/// Split a line at `;`, `&&` and `||` outside quotes: (command, the
/// connector before it). Each command is tokenized only when its turn comes,
/// so `X=1; echo $X` and `false || echo $?` see the values of the moment.
fn split_commands(line: &str) -> Result<Vec<(String, &'static str)>, String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut connector = ";";
    let mut quote: Option<char> = None;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match quote {
            Some(q) => {
                if c == '\\' && q == '"' && next.is_some() {
                    cur.push(c);
                    cur.push(next.unwrap());
                    i += 2;
                    continue;
                }
                if c == q {
                    quote = None;
                }
                cur.push(c);
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    cur.push(c);
                }
                '\\' => {
                    cur.push(c);
                    if let Some(n) = next {
                        cur.push(n);
                        i += 1;
                    }
                }
                ';' => {
                    out.push((core::mem::take(&mut cur), connector));
                    connector = ";";
                }
                '&' if next == Some('&') => {
                    out.push((core::mem::take(&mut cur), connector));
                    connector = "&&";
                    i += 1;
                }
                '|' if next == Some('|') => {
                    out.push((core::mem::take(&mut cur), connector));
                    connector = "||";
                    i += 1;
                }
                _ => cur.push(c),
            },
        }
        i += 1;
    }
    if let Some(q) = quote {
        return Err(format!("tanda kutip {} tidak ditutup", q));
    }
    out.push((cur, connector));
    Ok(out)
}

/// Run one command line; returns (output lines, effect).
pub fn run(line: &str) -> (Vec<String>, Effect) {
    let commands = match split_commands(line) {
        Ok(c) => c,
        Err(e) => {
            STATE.lock().ok = false;
            return (vec![format!("sh: {}", e)], Effect::None);
        }
    };
    let mut out = Vec::new();
    let mut effect = Effect::None;
    let mut ok = true;
    for (text, connector) in commands {
        if text.trim().is_empty() {
            continue;
        }
        let run_it = match connector {
            "&&" => ok,
            "||" => !ok,
            _ => true,
        };
        if !run_it {
            continue;
        }
        let toks = match tokenize(&text) {
            Ok(t) => t,
            Err(e) => {
                out.push(format!("sh: {}", e));
                ok = false;
                STATE.lock().ok = false;
                continue;
            }
        };
        if toks.is_empty() {
            continue;
        }
        let (o, e, good) = run_pipeline(&toks);
        out.extend(o);
        if e == Effect::Clear || effect == Effect::None {
            effect = e;
        }
        ok = good;
        STATE.lock().ok = ok;
    }
    (out, effect)
}

/// One pipeline: `a | b | c`, each stage with its redirections.
fn run_pipeline(toks: &[Tok]) -> (Vec<String>, Effect, bool) {
    let mut stdin: Option<Vec<String>> = None;
    let mut effect = Effect::None;
    let mut ok = true;
    let stages: Vec<&[Tok]> = toks.split(|t| *t == Tok::Op("|")).collect();
    let last = stages.len() - 1;
    for (k, stage) in stages.iter().enumerate() {
        let mut argv: Vec<String> = Vec::new();
        let mut out_file: Option<(String, bool)> = None;
        let mut j = 0;
        while j < stage.len() {
            match &stage[j] {
                Tok::Word(w) => argv.push(w.clone()),
                Tok::Op(op) => {
                    let Some(Tok::Word(target)) = stage.get(j + 1) else {
                        return (vec![format!("sh: butuh nama file setelah {}", op)], effect, false);
                    };
                    match *op {
                        ">" | ">>" => out_file = Some((target.clone(), *op == ">>")),
                        "<" => match crate::coreutils::run(&[String::from("cat"), target.clone()], None) {
                            (lines, true) => stdin = Some(lines),
                            (msg, false) => return (msg, effect, false),
                        },
                        _ => {}
                    }
                    j += 1;
                }
            }
            j += 1;
        }
        // aliases: the first word of a stage (repeatedly, a few times)
        for _ in 0..8 {
            let Some(expansion) = argv.first().and_then(|w| crate::bzsh::alias_of(w)) else { break };
            match expand_words(&expansion) {
                Ok(mut words) if !words.is_empty() && words[0] != argv[0] => {
                    words.extend(argv.drain(1..));
                    argv = words;
                }
                Ok(mut words) if !words.is_empty() => {
                    words.extend(argv.drain(1..));
                    argv = words;
                    break;
                }
                _ => break,
            }
        }
        if argv.is_empty() && stage.is_empty() {
            return (vec![String::from("sh: perintah kosong di pipe")], effect, false);
        }
        // only NAME=value words: set variables
        if !argv.is_empty() && argv.iter().all(|a| a.split_once('=').is_some_and(|(k, _)| !k.is_empty() && !k.contains('/'))) {
            for a in &argv {
                let (k, v) = a.split_once('=').unwrap();
                if !set_var(k, v) {
                    return (vec![format!("sh: nama variabel tidak sah: {}", k)], effect, false);
                }
            }
            stdin = Some(Vec::new());
            continue;
        }
        let (mut lines, e, good) = exec(&argv, stdin.as_deref());
        if e != Effect::None {
            effect = e;
        }
        ok = good;
        if let Some((file, append)) = out_file {
            if let Err(err) = crate::coreutils::write_lines(&file, &lines, append) {
                return (vec![format!("sh: {}", err)], effect, false);
            }
            lines = Vec::new();
        }
        if k == last {
            return (lines, effect, ok);
        }
        stdin = Some(lines);
    }
    (Vec::new(), effect, ok)
}

/// Run one command with its arguments (and the pipe's input, if any).
fn exec(argv: &[String], stdin: Option<&[String]>) -> (Vec<String>, Effect, bool) {
    let Some(name) = argv.first() else { return (Vec::new(), Effect::None, true) };
    if crate::coreutils::handles(name) {
        let (lines, ok) = crate::coreutils::run(argv, stdin);
        return (lines, Effect::None, ok);
    }
    if crate::bzsh::is_builtin(name) {
        return crate::bzsh::builtin(argv);
    }
    if !LEGACY_COMMANDS.contains(&name.as_str()) {
        // a package whose entry is a script, a script (path to a .SH, or
        // NAME.SH in $PATH), else a program
        if let Some(entry) = crate::bzpkg::entry_for(name).filter(|e| e.ends_with(".SH")) {
            return crate::bzsh::run_file(&format!("/disk/{}", entry), &argv[1..]);
        }
        if let Some(script) = crate::bzsh::find_script(name) {
            return crate::bzsh::run_file(&script, &argv[1..]);
        }
        if app::is_app(name) {
            let mut line = String::from("run");
            for a in argv {
                line.push(' ');
                line.push_str(a);
            }
            let (lines, effect) = run_legacy(&line);
            let failed = lines.first().is_some_and(|l| l.starts_with("run: "));
            return (lines, effect, !failed);
        }
    }
    let (lines, effect) = run_legacy(&argv.join(" "));
    // the shell's own commands report errors as "<command>: ..."
    let failed = lines.first().is_some_and(|l| l.contains("command not found") || l.starts_with(&format!("{}: ", name)) && !l.contains(": ok"));
    (lines, effect, !failed)
}

/// Side effect a command asks the terminal/desktop to perform.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    Clear,
    Redraw, // theme/workspace changed; recompose the desktop
}

// ---- network commands -------------------------------------------------------

fn ipconfig() -> Vec<String> {
    use crate::net::{fmt_ip, fmt_mac};
    let Some(i) = crate::net::info() else { return vec![String::from("jaringan: tidak aktif")] };
    let prefix: u32 = i.mask.iter().map(|b| b.count_ones()).sum();
    let mut out = vec![
        format!("{}  {}", if i.attached { "eth0" } else { "lo  " }, if i.attached { "(kartu jaringan)" } else { "(loopback -- tidak ada kartu jaringan)" }),
        format!("      MAC          {}", fmt_mac(i.mac)),
        format!("      IPv4         {}/{}", fmt_ip(i.ip), prefix),
        format!("      netmask      {}", fmt_ip(i.mask)),
        format!("      gateway      {}", if i.gateway == [0; 4] { String::from("-") } else { fmt_ip(i.gateway) }),
        String::from("      DNS          - (belum ada resolver)"),
        format!("      frame        {} terkirim, {} diterima", i.frames_tx, i.frames_rx),
        format!("      UDP          {} terkirim, {} diterima; ICMP balasan {}", i.udp_tx, i.udp_rx, i.icmp_replies),
    ];
    if !i.attached {
        if let Some(s) = crate::net::status() {
            for (k, part) in s.split("; ").enumerate() {
                out.push(format!("      {}  {}", if k == 0 { "status     " } else { "           " }, part));
            }
        }
        out.push(String::from("      (colok kabel lalu ketik 'netup' untuk mencoba lagi; rincian: dmesg e1000e)"));
    }
    let (ssh_on, _, _) = crate::ssh::status();
    let (dep_on, code, _) = crate::deploy::status();
    out.push(format!(
        "      layanan      ssh {}, deploy {}",
        if ssh_on { "aktif (tcp/22)" } else { "mati" },
        if dep_on { format!("aktif (udp/{}, kode {:04})", crate::deploy::PORT, code) } else { String::from("mati") }
    ));
    out
}

fn ping(args: &[&str]) -> Vec<String> {
    let Some(dest) = args.first().and_then(|a| crate::net::parse_ip(a)) else {
        return vec![String::from("pakai: ping <alamat-ip> [jumlah]   (mis. ping 10.0.2.2)")];
    };
    let n = args.get(1).and_then(|c| c.parse::<u16>().ok()).unwrap_or(4).clamp(1, 20);
    let ip = crate::net::fmt_ip(dest);
    let mut out = vec![format!("PING {} ({} kali)", ip, n)];
    let (mut ok, mut sum, mut min, mut max) = (0u32, 0u64, u64::MAX, 0u64);
    for seq in 1..=n {
        match crate::net::ping_once(dest, seq, 1000) {
            Some(us) => {
                ok += 1;
                sum += us;
                min = min.min(us);
                max = max.max(us);
                out.push(format!("balasan dari {}: seq={} waktu={}.{:03} ms", ip, seq, us / 1000, us % 1000));
            }
            None => out.push(format!("seq={}: tidak ada balasan (1 detik)", seq)),
        }
    }
    let loss = (n as u32 - ok) * 100 / n as u32;
    out.push(format!("--- {}: {} terkirim, {} diterima, {}% hilang", ip, n, ok, loss));
    if ok > 0 {
        out.push(format!("waktu min/rata/maks = {}.{:03} / {}.{:03} / {}.{:03} ms",
            min / 1000, min % 1000, sum / ok as u64 / 1000, sum / ok as u64 % 1000, max / 1000, max % 1000));
    }
    out
}

fn arp() -> Vec<String> {
    let Some(i) = crate::net::info() else { return vec![String::from("jaringan: tidak aktif")] };
    let mut out = vec![String::from("alamat IP          alamat MAC")];
    for (ip, mac) in i.arp {
        out.push(format!("{:<18} {}{}", crate::net::fmt_ip(ip), crate::net::fmt_mac(mac), if ip == i.ip { "  (perangkat ini)" } else { "" }));
    }
    out
}

fn route() -> Vec<String> {
    let Some(i) = crate::net::info() else { return vec![String::from("jaringan: tidak aktif")] };
    let net: [u8; 4] = core::array::from_fn(|k| i.ip[k] & i.mask[k]);
    let mut out = vec![
        String::from("tujuan             netmask            gateway            antarmuka"),
        format!("{:<18} {:<18} {:<18} {}", crate::net::fmt_ip(net), crate::net::fmt_ip(i.mask), "langsung", if i.attached { "eth0" } else { "lo" }),
    ];
    if i.gateway != [0; 4] {
        out.push(format!("{:<18} {:<18} {:<18} {}", "0.0.0.0", "0.0.0.0", crate::net::fmt_ip(i.gateway), if i.attached { "eth0" } else { "lo" }));
    }
    out
}

fn netstat() -> Vec<String> {
    let (conns, listeners) = crate::tcp::snapshot();
    let mut out = vec![String::from("proto  lokal            jarak jauh           status")];
    for p in listeners {
        out.push(format!("tcp    *:{:<14} *:*                  LISTEN", p));
    }
    for (lp, rip, rp, st) in conns {
        let remote = format!("{}:{}", crate::net::fmt_ip(rip), rp);
        out.push(format!("tcp    *:{:<14} {:<20} {:?}", lp, remote, st));
    }
    if let Some(i) = crate::net::info() {
        for p in i.udp_ports {
            out.push(format!("udp    *:{:<14} *:*", p));
        }
    }
    out
}

/// `ssh [on | off]`: the SSH server (port 22, accounts from Pengaturan > Pengguna).
fn ssh_cmd(arg: Option<&str>) -> Vec<String> {
    match arg {
        Some("on") => {
            if let Err(e) = crate::ssh::start(true) {
                return vec![format!("ssh: {}", e)];
            }
        }
        Some("off") => crate::ssh::stop(),
        None | Some("status") => {}
        Some(_) => return vec![String::from("pakai: ssh [on | off]")],
    }
    let (on, sessions, logins) = crate::ssh::status();
    let ip = crate::net::local_ip().map_or(String::from("(tanpa jaringan)"), crate::net::fmt_ip);
    if on {
        vec![
            format!("ssh: AKTIF di {}:{} -- {} sesi, {} login berhasil", ip, crate::ssh::PORT, sessions, logins),
            format!("     kunci host {}", crate::ssh::fingerprint()),
            String::from("     masuk: ssh administrator@<ip> (ganti kata sandi bawaan di Pengaturan > Pengguna)"),
        ]
    } else {
        vec![String::from("ssh: mati ('ssh on' untuk menyalakan server SSH di port 22)")]
    }
}

/// `crash [PID|last]`: list the crash reports of ring-3 programs, or show one.
fn crash_cmd(arg: Option<&str>) -> Vec<String> {
    let dir = crate::crash::dir();
    match arg {
        None => {
            let mut out = Vec::new();
            match crate::vfs::list(dir) {
                Ok(names) if !names.is_empty() => {
                    out.push(format!("laporan crash di {}:", dir));
                    for n in names {
                        out.push(format!("  {}", n));
                    }
                    out.push(String::from("'crash PID' atau 'crash last' untuk melihat isinya"));
                }
                _ => out.push(String::from("belum ada laporan crash")),
            }
            out
        }
        Some("last") => match crate::crash::last() {
            Some((_, t)) => t.lines().map(String::from).collect(),
            None => vec![String::from("belum ada laporan crash")],
        },
        Some(pid) => match crate::vfs::read(&format!("{}/P{}.TXT", dir, pid.trim_start_matches('P').trim_end_matches(".TXT"))) {
            Ok(b) => String::from_utf8_lossy(&b).lines().map(String::from).collect(),
            Err(_) => vec![format!("crash: tidak ada laporan untuk {}", pid)],
        },
    }
}

/// `deploy [on [code] | off]`: the remote deploy service for the Magic App
/// Generator (BZDP over UDP 7070, `deploy.rs`).
fn deploy_cmd(args: &[&str]) -> Vec<String> {
    match args.first().copied() {
        Some("on") => {
            let code = args.get(1).and_then(|c| c.parse::<u16>().ok()).filter(|c| *c < 10_000).unwrap_or_else(crate::deploy::random_code);
            if !crate::deploy::enable(code) {
                return vec![String::from("deploy: port UDP 7070 tidak bisa dibuka")];
            }
            crate::deploy::remember(true, code); // stays on after a reboot until 'deploy off'
        }
        Some("off") => {
            crate::deploy::disable();
            crate::deploy::remember(false, 0);
        }
        None | Some("status") => {}
        Some(_) => return vec![String::from("pakai: deploy [on [kode] | off]")],
    }
    let (on, code, served) = crate::deploy::status();
    let ip = crate::net::local_ip().map_or(String::from("(tanpa jaringan)"), |ip| format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]));
    if on {
        vec![
            format!("deploy: AKTIF di {}:{} (UDP), kode pairing {:04}", ip, crate::deploy::PORT, code),
            format!("         {} permintaan dilayani; dari PC: 'bz device connect {}:{} {:04}'", served, ip, crate::deploy::PORT, code),
            String::from("         tetap aktif setelah restart sampai 'deploy off'"),
        ]
    } else {
        vec![String::from("deploy: mati ('deploy on' untuk menerima app dari Magic App Generator)")]
    }
}

pub fn cwd() -> String {
    let s = STATE.lock();
    if s.cwd.is_empty() { String::from("/disk") } else { s.cwd.clone() }
}

pub fn prompt() -> String {
    format!("buitenzorg:{}$ ", cwd())
}

/// Run one of the shell's own commands (a plain, already expanded line).
fn run_legacy(line: &str) -> (Vec<String>, Effect) {
    let line = line.trim();
    if line.is_empty() {
        return (Vec::new(), Effect::None);
    }
    let mut parts = line.split_whitespace();
    let cmd = parts.next().unwrap();
    let args: Vec<&str> = parts.collect();

    match cmd {
        "help" => (help(), Effect::None),
        "ls" | "dir" if args.len() > 1 => {
            let mut out = Vec::new();
            for a in &args {
                let lines = list_dir(Some(a));
                if !(lines.len() == 1 && lines[0] == *a) {
                    out.push(format!("{}:", a));
                }
                out.extend(lines);
            }
            (out, Effect::None)
        }
        "ls" | "dir" => (list_dir(args.first().copied()), Effect::None),
        "cat" | "type" => (cat(args.first().copied()), Effect::None),
        "echo" => (echo(&args), Effect::None),
        "rm" | "del" => (rm(args.first().copied()), Effect::None),
        "df" => (df(), Effect::None),
        "pwd" => (vec![cwd()], Effect::None),
        "cd" => (cd(args.first().copied()), Effect::None),
        "clear" | "cls" => (Vec::new(), Effect::Clear),
        "ver" | "uname" => (vec![version()], Effect::None),
        "about" | "credits" => (about(), Effect::None),
        "mounts" => (vfs::mounts(), Effect::None),
        "dmesg" | "log" => (dmesg(args.first().copied()), Effect::None),
        "lspci" => (lspci(), Effect::None),
        "gpu" => (gpu_cmd(args.first().copied()), Effect::None),
        "theme" => theme_cmd(args.first().copied()),
        "ws" => ws_cmd(args.first().copied()),
        "run" => run_cmd(args.first().copied(), &args[args.len().min(1)..]),
        "ps" => (ps_cmd(), Effect::None),
        "crash" | "crashes" => (crash_cmd(args.first().copied()), Effect::None),
        "dbg" | "debug" => (crate::debug::command(&args), Effect::None),
        "tutup" | "closeall" => {
            let n = crate::wm::close_workspace_windows();
            (vec![format!("{} jendela ditutup", n)], Effect::Redraw)
        }
        "tile" | "susun" => {
            let n = crate::wm::tile();
            (vec![format!("{} jendela disusun", n)], Effect::Redraw)
        }
        "cpu" | "cpus" => (cpu_cmd(args.first().copied()), Effect::None),
        "kill" => (kill_cmd(args.first().copied()), Effect::Redraw),
        "deploy" => (deploy_cmd(&args), Effect::None),
        "ssh" | "sshd" => (ssh_cmd(args.first().copied()), Effect::None),
        "ipconfig" | "ifconfig" | "ip" => (ipconfig(), Effect::None),
        "netup" | "dhcp" => (
            vec![String::from(if crate::net::retry() {
                "jaringan: mencoba lagi di latar belakang (10-15 detik), lalu 'ipconfig'"
            } else {
                "jaringan: tidak ada yang bisa dicoba di platform ini"
            })],
            Effect::None,
        ),
        "ping" => (ping(&args), Effect::None),
        "arp" => (arp(), Effect::None),
        "route" => (route(), Effect::None),
        "netstat" => (netstat(), Effect::None),
        "hostname" => (vec![String::from("buitenzorg")], Effect::None),
        "nslookup" | "dig" | "host" => (
            vec![String::from("nslookup: belum ada DNS resolver -- pakai alamat IP (mis. ping 10.0.2.2)")],
            Effect::None,
        ),
        "bg" | "wallpaper" => bg_cmd(args.first().copied()),
        "saver" | "screensaver" => saver_cmd(args.first().copied()),
        "cursor" => cursor_cmd(args.first().copied()),
        "anim" => anim_cmd(args.first().copied()),
        "tone" | "nada" => {
            // tone <Hz> <ms>: a sine tone on every audio output (sound card / jack / HDMI)
            let hz = args.first().and_then(|a| a.parse::<u32>().ok()).unwrap_or(440);
            let ms = args.get(1).and_then(|a| a.parse::<u32>().ok()).unwrap_or(1000).min(10_000);
            let ok = crate::audio::play_tone(hz, ms);
            (vec![if ok { format!("nada {} Hz, {} ms", hz, ms) } else { String::from("tidak ada perangkat suara") }], Effect::None)
        }
        "settings" | "personalize" => (settings_show(), Effect::None),
        "ask" | "ai" => (ai_ask(&args), Effect::Redraw),
        "vm" => vm_cmd(&args),
        "script" | "js" | "ts" | "py" => script_cmd(cmd, &args),
        "csx" | "csharp" => csx_cmd(&args),
        "prof" | "profile" => prof_cmd(args.first().copied()),
        "gfx3d" | "3d" => {
            // gfx3d [test]: the 3D API's resources, or its self-test
            let mut out = Vec::new();
            if args.first() == Some(&"test") {
                let ok = crate::gfx3d::self_test();
                out.push(String::from(if ok { "self-test 3D: OK (lihat log serial)" } else { "self-test 3D GAGAL (lihat log serial)" }));
            }
            match args.first().copied() {
                Some("sw") => {
                    crate::virtio::virgl::set_enabled(false);
                    crate::gfx3d::set_native(false);
                }
                Some("gpu") => {
                    crate::virtio::virgl::set_enabled(true);
                    crate::gfx3d::set_native(true);
                }
                _ => {}
            }
            let gpu = crate::virtio::virgl::enabled() && crate::virtio::virgl::available();
            if let Some((name, on)) = crate::gfx3d::native_state() {
                out.push(format!(
                    "backend native: {} ({}) -- 'gfx3d sw' / 'gfx3d gpu' untuk konteks baru",
                    name,
                    if on { "aktif" } else { "mati" }
                ));
            }
            out.push(String::from(if gpu {
                "backend: GPU (virgl, virtio-gpu 3D) untuk konteks baru -- 'gfx3d sw' untuk software"
            } else {
                "backend: rasterizer software -- 'gfx3d gpu' memakai GPU bila virtio-gpu-gl ada"
            }));
            #[cfg(target_arch = "aarch64")]
            {
                let v = crate::platform::virt::v3d::version();
                if v != 0 {
                    out.push(format!("GPU Raspberry Pi: V3D {}.{} terdeteksi (render lewat V3D belum ada -- rasterizer software)", v / 10, v % 10));
                }
            }
            let (c, m, t, used, draws, tris, px) = crate::gfx3d::summary();
            out.push(format!("3D: {} konteks, {} mesh, {} tekstur, {} KiB dari {} KiB", c, m, t, used / 1024, crate::gfx3d::BUDGET / 1024));
            out.push(format!("   {} draw, {} segitiga, {} piksel sejak boot", draws, tris, px));
            (out, Effect::None)
        }
        "shutdown" | "poweroff" => power::shutdown(),
        "restart" | "reboot" => power::restart(),
        "sleep" | "suspend" => {
            power::sleep();
            (vec![String::from("bangun dari sleep")], Effect::Redraw)
        }
        "bz" => bz_cmd(&args),
        other => (vec![format!("{}: command not found (try 'help')", other)], Effect::None),
    }
}

/// The OS version: one place, shown by `ver`, `about`, `bz version` and the
/// boot log. A release candidate until v1.0 has booted on a physical PC.
pub const VERSION: &str = "v1.0 'Buitenzorg'";

/// The kernel log (the last 16 KiB), optionally only lines containing `filter`
/// -- on a PC without a serial cable, the way to see what the boot did.
fn dmesg(filter: Option<&str>) -> Vec<String> {
    let mut buf = vec![0u8; 16 * 1024];
    let (n, _) = crate::logring::read(0, &mut buf);
    let text = String::from_utf8_lossy(&buf[..n]);
    let lines: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty() && filter.map_or(true, |f| l.to_lowercase().contains(&f.to_lowercase())))
        .map(String::from)
        .collect();
    let skip = lines.len().saturating_sub(40);
    let mut out: Vec<String> = lines.into_iter().skip(skip).collect();
    if out.is_empty() {
        out.push(String::from("dmesg: no matching lines"));
    }
    out
}

/// What the GPU is and how the firmware set it up (Intel, PCs).
fn gpu_info() -> Vec<String> {
    #[cfg(target_arch = "x86_64")]
    {
        crate::intel_gpu::info()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        vec![String::from("gpu: PCs only (Raspberry Pi: V3D, see the boot log)")]
    }
}

/// `gpu blit`: the Intel blitter self-test (step 2 of GPU support).
/// `gpu on` / `gpu cpu`: present through the blitter or by CPU (step 2b).
fn gpu_cmd(arg: Option<&str>) -> Vec<String> {
    #[cfg(target_arch = "x86_64")]
    {
        use crate::platform::pc::intel_blt;
        match arg {
            Some("blit") => intel_blt::self_test(),
            Some("render") => crate::platform::pc::intel_render::self_test(),
            Some("on") => vec![format!("gpu: {}", intel_blt::enable_present())],
            Some("soft") => vec![format!("gpu: {}", intel_blt::soft_present())],
            Some("soft-stats") => vec![format!("gpu soft: {} rectangles copied", intel_blt::soft_rects())],
            Some("cpu") | Some("off") => {
                intel_blt::disable_present();
                vec![String::from("gpu: the desktop is presented by the CPU")]
            }
            _ => gpu_info(),
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        match arg {
            None => gpu_info(),
            Some(_) => vec![String::from("gpu blit/on/cpu: PCs with Intel graphics only")],
        }
    }
}

/// PCI devices: which controllers a PC has (and so which drivers it needs).
fn lspci() -> Vec<String> {
    #[cfg(target_arch = "x86_64")]
    {
        crate::pci::scan()
            .iter()
            .map(|d| {
                format!(
                    "{:02x}:{:02x}.{} {:04x}:{:04x} {:02x}{:02x}{:02x} {}",
                    d.bus, d.slot, d.function, d.vendor_id, d.device_id, d.class, d.subclass, d.prog_if, d.class_name()
                )
            })
            .collect()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        vec![String::from("lspci: PCs only")]
    }
}

fn version() -> String {
    format!("Buitenzorg OS {} -- Gravicode Studios (Kang Fadhil)", VERSION)
}

fn about() -> Vec<String> {
    let mut lines: Vec<String> = ["Buitenzorg OS",
     "  Sistem operasi hibrida & AI-native (kernel Rust, userland C#).",
     "  \"zonder zorg\" - tanpa kekhawatiran. Kebun Raya Bogor edition.",
     "",
     "  Dibuat oleh : Gravicode Studios",
     "  Dipimpin oleh: Kang Fadhil"]
        .iter()
        .map(|s| String::from(*s))
        .collect();
    lines.push(format!("  Versi       : {}", VERSION));
    lines
}

fn help() -> Vec<String> {
    let mut v = help_lines();
    v.extend(crate::bzsh::describe());
    v
}

fn help_lines() -> Vec<String> {
    ["Perintah:",
     "  ls|dir [path]     daftar isi direktori (VFS)",
     "  cat|type <file>   tampilkan isi file",
     "  cd <path>         pindah direktori",
     "  pwd               direktori sekarang",
     "  dmesg [kata]      log kernel (40 baris terakhir, opsional disaring)",
     "  lspci             perangkat PCI (kontroler disk/USB/jaringan)",
     "  gpu [blit|on|cpu] GPU; blit = uji blitter Intel, on/cpu = tampilkan lewat GPU/CPU",
     "  gpu render        uji render engine Intel (3D, tahap 3a)",
     "  netup             coba lagi menyalakan jaringan (DHCP)",
     "  echo <teks>       cetak teks",
     "  mounts            daftar mount VFS",
     "  theme [nama|cycle|list]     ganti tema (8 style + dark/light)",
     "  bg [nama|list|/path.bmp]    ganti wallpaper (bawaan / gambar user)",
     "  saver [nama|list|off]       screensaver (starfield, mystify, pipes, ...)",
     "  cursor [normal|besar]       ukuran kursor",
     "  anim [on|off]               micro-interaction on/off",
     "  settings                    tampilkan pengaturan (personalization)",
     "  ws [1-4]          pindah virtual desktop",
     "  tile|susun        susun jendela desktop ini berdampingan",
     "  tutup|closeall    tutup semua jendela desktop ini (kecuali terminal)",
     "  run <app> [arg..] jalankan app sebagai proses (xox, paint, taskmgr)",
     "  run <app>.dll     app C# managed (RustCLR), mis. 'run hellogui'",
     "  ps                daftar proses (task kernel + app)",
     "  cpu [on|off]      daftar CPU; on/off = proses ring-3 boleh jalan di AP",
     "  echo <teks> > f   tulis teks ke file (>> = tambahkan); /disk persisten",
     "  rm <file>         hapus file",
     "  df                ruang kosong tiap mount",
     "  kill <pid>        hentikan proses app",
     "  crash [PID|last]  laporan crash app (backtrace bersimbol)",
     "  dbg run <app>     debugger: break/cont/step/regs/mem/bt ('dbg help')",
     "  deploy on|off     terima app dari Magic App Generator (UDP 7070)",
     "  ssh on|off        server SSH (port 22) untuk akses jarak jauh",
     "  ipconfig          alamat IP, MAC, gateway, statistik jaringan",
     "  ping <ip> [n]     uji koneksi (ICMP echo)",
     "  arp, route, netstat, hostname   tabel ARP, rute, koneksi",
     "  ask <prompt>      AI: lengkapi teks (LLM lokal)",
     "  vm <sub>          virtualisasi (list/create/start/snapshot/restore)",
     "  script <lang> [f] polyglot: js|ts|py (jalankan demo/file)",
     "  csx [FILE] [arg]  C# script (.csx) lewat engine bytecode; UI: run FILE.CSX",
     "  prof [self|on|off|reset|report]  profiler zona (TSC)",
     "  shutdown|restart|sleep      power management",
     "  bz <sub>          CLI (install/remove/list, model, power, vm, virt, script)",
     "  clear|cls         bersihkan layar",
     "  ver|uname         versi OS",
     "  about|credits     tentang & pembuat"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Resolve `arg` (optional) against the cwd into an absolute VFS path, with
/// `.` and `..` folded (never above the mount: `/disk/..` stays `/disk`).
pub fn resolve(arg: Option<&str>) -> String {
    let raw = match arg {
        Some(p) if p.starts_with('/') => p.to_string(),
        Some(p) => format!("{}/{}", cwd().trim_end_matches('/'), p),
        None => cwd(),
    };
    let mut parts: Vec<&str> = Vec::new();
    for c in raw.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                if parts.len() > 1 {
                    parts.pop();
                }
            }
            x => parts.push(x),
        }
    }
    format!("/{}", parts.join("/"))
}

/// `echo text`, `echo text > FILE` (create/overwrite) and `echo text >> FILE`
/// (append). Only echo redirects for now; general pipes/redirection are on the
/// Developer Workstation roadmap.
fn echo(args: &[&str]) -> Vec<String> {
    let pos = args.iter().position(|a| *a == ">" || *a == ">>");
    let Some(pos) = pos else {
        return vec![args.join(" ")];
    };
    let append = args[pos] == ">>";
    let Some(target) = args.get(pos + 1).copied() else {
        return vec![String::from("echo: butuh nama file setelah >")];
    };
    let path = resolve(Some(target));
    let mut data = if append { vfs::read(&path).unwrap_or_default() } else { Vec::new() };
    data.extend_from_slice(args[..pos].join(" ").as_bytes());
    data.push(b'\n');
    match vfs::write(&path, &data) {
        Ok(()) => vec![format!("{} byte ditulis ke {}", data.len(), path)],
        Err(e) => vec![format!("echo: {}: {}", path, e)],
    }
}

fn rm(arg: Option<&str>) -> Vec<String> {
    let Some(_) = arg else {
        return vec![String::from("rm: butuh nama file")];
    };
    let path = resolve(arg);
    match vfs::remove(&path) {
        Ok(()) => vec![format!("{} dihapus", path)],
        Err(e) => vec![format!("rm: {}: {}", path, e)],
    }
}

fn df() -> Vec<String> {
    let mut out = vec![String::from("MOUNT    BEBAS      MODE")];
    for m in vfs::mounts() {
        match vfs::free_space(&m) {
            Ok((free, ro)) => out.push(format!("{:<8} {:>6} KiB  {}", m, free / 1024, if ro { "ro" } else { "rw" })),
            Err(e) => out.push(format!("{:<8} ?  ({})", m, e)),
        }
    }
    out
}

fn list_dir(arg: Option<&str>) -> Vec<String> {
    let path = resolve(arg);
    // a file operand (e.g. from a glob: `ls /disk/H*.ELF`) lists itself
    if let Some(a) = arg {
        if let Ok(e) = vfs::stat(&path) {
            if !e.is_dir {
                return vec![String::from(a)];
            }
        }
    }
    match vfs::list(&path) {
        Ok(names) if names.is_empty() => vec![format!("{}: (kosong)", path)],
        Ok(names) => names,
        Err(e) => vec![format!("ls: {}: {}", path, e)],
    }
}

fn cat(arg: Option<&str>) -> Vec<String> {
    let Some(_) = arg else {
        return vec![String::from("cat: butuh nama file")];
    };
    let path = resolve(arg);
    match vfs::read(&path) {
        Ok(bytes) => match core::str::from_utf8(&bytes) {
            Ok(text) => text.lines().map(String::from).collect(),
            Err(_) => vec![format!("cat: {}: bukan teks UTF-8 ({} byte)", path, bytes.len())],
        },
        Err(e) => vec![format!("cat: {}: {}", path, e)],
    }
}

fn cd(arg: Option<&str>) -> Vec<String> {
    let path = resolve(arg);
    // Accept a path whose mount exists.
    if vfs::mounts().iter().any(|m| path == *m || path.starts_with(&format!("{}/", m))) {
        STATE.lock().cwd = path;
        Vec::new()
    } else {
        vec![format!("cd: {}: tidak ada", path)]
    }
}

fn theme_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    match arg {
        None | Some("toggle") => {
            (vec![format!("tema: {}", theme::toggle())], Effect::Redraw)
        }
        Some("cycle") | Some("next") => {
            (vec![format!("tema: {}", theme::cycle())], Effect::Redraw)
        }
        Some("list") => {
            let mut out = vec![String::from("tema tersedia:")];
            for t in theme::names().iter() {
                let mark = if t.name == theme::name() { "* " } else { "  " };
                out.push(format!("{}{}", mark, t.name));
            }
            (out, Effect::None)
        }
        Some(name) => {
            if theme::set_by_name(name) {
                (vec![format!("tema: {}", name)], Effect::Redraw)
            } else {
                (
                    vec![format!("theme: '{}' tidak dikenal (coba 'theme list')", name)],
                    Effect::None,
                )
            }
        }
    }
}

fn ws_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    match arg {
        None => (vec![format!("workspace: {}", wm::current_workspace() + 1)], Effect::None),
        Some(n) => match n.parse::<u8>() {
            Ok(v) if (1..=wm::WORKSPACES).contains(&v) => {
                wm::switch_workspace(v - 1);
                (vec![format!("pindah ke desktop {}", v)], Effect::Redraw)
            }
            _ => (vec![format!("ws: nomor 1..{}", wm::WORKSPACES)], Effect::None),
        },
    }
}

fn run_cmd(arg: Option<&str>, app_args: &[&str]) -> (Vec<String>, Effect) {
    let Some(name) = arg else {
        return (vec![String::from("run: butuh nama app (mis. 'run xox')")], Effect::None);
    };
    // a UI script by any path: `run MAIN.JS`, `run /disk/PROYEK/X/MAIN.PY`
    let lower = name.to_ascii_lowercase();
    let resolved;
    let name = if lower.ends_with(".js") || lower.ends_with(".ts") || lower.ends_with(".py") || lower.ends_with(".csx") {
        resolved = resolve(Some(name));
        resolved.as_str()
    } else {
        name
    };
    if !app::is_app(name) {
        return (vec![format!("run: '{}' tidak dikenal (lihat 'bz list')", name)], Effect::None);
    }
    // Apps in the registry must be installed first; others (svc) run directly.
    if pkg::find(name).is_some() && !pkg::is_installed(name) {
        return (
            vec![format!("run: '{}' belum terpasang — jalankan 'bz install {}'", name, name)],
            Effect::None,
        );
    }
    // Start it as its own process: the desktop and shell stay responsive while
    // it runs, and several apps can run side by side.
    match app::spawn_with_args(name, &app_args.join(" ")) {
        Ok(pid) => (vec![format!("app '{}' berjalan (pid {})", name, pid)], Effect::Redraw),
        Err(e) => (vec![format!("run: {}: {}", name, e)], Effect::None),
    }
}

fn cpu_cmd(arg: Option<&str>) -> Vec<String> {
    match arg {
        Some("on") => {
            crate::task::set_ap_scheduling(true);
            vec![String::from("proses ring-3 kini dibagi ke semua CPU")]
        }
        Some("threads-on") => {
            crate::task::set_parallel_threads(true);
            vec![String::from("thread dalam satu proses boleh jalan paralel")]
        }
        Some("threads-off") => {
            crate::task::set_parallel_threads(false);
            vec![String::from("thread dalam satu proses kini bergiliran")]
        }
        Some("off") => {
            crate::task::set_ap_scheduling(false);
            vec![String::from("proses ring-3 kini hanya di CPU #0 (BSP)")]
        }
        _ => crate::smp::report(),
    }
}

fn ps_cmd() -> Vec<String> {
    let mut buf: Vec<bz_abi::ProcInfo> = Vec::new();
    buf.resize_with(64, || unsafe { core::mem::zeroed() });
    let n = crate::process::list(&mut buf);
    let mut out = vec![String::from("  PID  JENIS   CPU  NAMA")];
    for p in &buf[..n] {
        let len = p.name.iter().position(|&b| b == 0).unwrap_or(p.name.len());
        let name = core::str::from_utf8(&p.name[..len]).unwrap_or("?");
        let kind = if p.kind == 1 { "app" } else { "kernel" };
        out.push(format!("{:>5}  {:<6} {:>5}  {}", p.pid, kind, p.cpu_ticks, name));
    }
    out
}

fn kill_cmd(arg: Option<&str>) -> Vec<String> {
    let Some(pid) = arg.and_then(|a| a.parse::<u64>().ok()) else {
        return vec![String::from("kill: butuh pid (lihat 'ps')")];
    };
    if crate::process::kill(pid) {
        vec![format!("proses {} dihentikan", pid)]
    } else {
        vec![format!("kill: {}: tidak bisa dihentikan (tidak ada, app sinkron, atau task inti)", pid)]
    }
}

fn bg_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    match arg {
        None => (vec![format!("wallpaper: {}", wallpaper::label())], Effect::None),
        Some("list") => {
            let mut out = vec![String::from("wallpaper resmi Buitenzorg OS:")];
            for (n, _, desc) in wallpaper::OFFICIAL {
                out.push(format!("  {:<11} {}", n, desc));
            }
            out.push(String::from("pola bawaan:"));
            for n in wallpaper::BUILTINS {
                out.push(format!("  {}", n));
            }
            out.push(String::from("  /disk/<file>.bmp (gambar user 24-bit)"));
            (out, Effect::None)
        }
        Some(path) if path.starts_with('/') => match vfs::read(path) {
            Ok(bytes) => match wallpaper::load_bmp(&bytes, path) {
                Ok((w, h)) => (vec![format!("wallpaper: {} ({}x{})", path, w, h)], Effect::Redraw),
                Err(e) => (vec![format!("bg: {}: {}", path, e)], Effect::None),
            },
            Err(e) => (vec![format!("bg: {}: {}", path, e)], Effect::None),
        },
        Some(name) if wallpaper::OFFICIAL.iter().any(|(n, _, _)| *n == name) => {
            match wallpaper::set_official(name) {
                Ok(_) => (vec![format!("wallpaper: {} (resmi)", name)], Effect::Redraw),
                Err(e) => (vec![format!("bg: {}: {}", name, e)], Effect::None),
            }
        }
        Some(name) => {
            if wallpaper::set_builtin(name) {
                (vec![format!("wallpaper: {}", name)], Effect::Redraw)
            } else {
                (vec![format!("bg: '{}' tidak dikenal (coba 'bg list')", name)], Effect::None)
            }
        }
    }
}

fn saver_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    match arg {
        None => (vec![format!("screensaver: {}", screensaver::name())], Effect::None),
        Some("list") => {
            let mut out = vec![String::from("screensaver:")];
            for n in screensaver::NAMES {
                out.push(format!("  {}", n));
            }
            out.push(String::from("  off"));
            (out, Effect::None)
        }
        Some(name) => {
            if screensaver::set(name) {
                (vec![format!("screensaver: {}", screensaver::name())], Effect::None)
            } else {
                (vec![format!("saver: '{}' tidak dikenal (coba 'saver list')", name)], Effect::None)
            }
        }
    }
}

fn cursor_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    match arg {
        Some("besar") | Some("large") | Some("2") => {
            wm::set_cursor_scale(2);
            (vec![String::from("kursor: besar")], Effect::Redraw)
        }
        Some("normal") | Some("1") => {
            wm::set_cursor_scale(1);
            (vec![String::from("kursor: normal")], Effect::Redraw)
        }
        _ => (vec![String::from("cursor: normal | besar")], Effect::None),
    }
}

fn anim_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    let (_, rounded) = wm::options();
    match arg {
        Some("off") => {
            wm::set_options(false, rounded);
            (vec![String::from("animasi: off")], Effect::Redraw)
        }
        Some("on") => {
            wm::set_options(true, rounded);
            (vec![String::from("animasi: on")], Effect::Redraw)
        }
        _ => (vec![String::from("anim: on | off")], Effect::None),
    }
}

fn settings_show() -> Vec<String> {
    let (anim, rounded) = wm::options();
    vec![
        String::from("== Personalization =="),
        format!("  tema      : {}", theme::name()),
        format!("  wallpaper : {}", wallpaper::label()),
        format!("  saver     : {}", screensaver::name()),
        format!("  animasi   : {}", if anim { "on" } else { "off" }),
        format!("  rounded   : {}", if rounded { "on" } else { "off" }),
        String::from("  ubah: theme/bg/saver/cursor/anim <nilai>"),
    ]
}

fn ai_ask(args: &[&str]) -> Vec<String> {
    if args.len() < 2 {
        return vec![String::from("ask: butuh prompt (mis. 'ask kernel')")];
    }
    let prompt = args[1..].join(" ");
    let full = format!("{} ", prompt);
    let out = ai::llm_complete(&full, 48);
    vec![
        String::from("[nalar/LLM lokal - buitenzorg/nalar-bigram]"),
        out,
    ]
}

fn model_cmd(args: &[&str]) -> (Vec<String>, Effect) {
    match args.first().copied() {
        None | Some("list") => {
            let mut out = vec![String::from("galeri model (Hugging Face-style):")];
            for m in model::GALLERY {
                let mark = if model::is_pulled(m.id) { "[v]" } else { "[ ]" };
                out.push(format!(
                    "{} {:<28} {:<15} {}MB {}",
                    mark, m.id, m.task, m.size_mb, m.format
                ));
            }
            (out, Effect::None)
        }
        Some("pull") | Some("download") => {
            let Some(id) = args.get(1) else {
                return (vec![String::from("bz model pull <id>")], Effect::None);
            };
            match model::pull(id) {
                Ok(m) => (
                    vec![format!("model diunduh: {} ({} MB, {})", m.id, m.size_mb, m.license)],
                    Effect::None,
                ),
                Err(e) => (vec![format!("bz model: {}: {}", id, e)], Effect::None),
            }
        }
        Some("info") => {
            let Some(id) = args.get(1) else {
                return (vec![String::from("bz model info <id>")], Effect::None);
            };
            match model::find(id) {
                Some(m) => (
                    vec![
                        format!("id      : {}", m.id),
                        format!("task    : {}", m.task),
                        format!("size    : {} MB (VRAM ~{} MB)", m.size_mb, m.vram_mb),
                        format!("license : {}   format: {}", m.license, m.format),
                        format!("status  : {}", if model::is_pulled(m.id) { "tersedia offline" } else { "belum diunduh" }),
                    ],
                    Effect::None,
                ),
                None => (vec![format!("bz model: '{}' tidak ada di galeri", id)], Effect::None),
            }
        }
        Some(sub) => (vec![format!("bz model {}: list|pull|info", sub)], Effect::None),
    }
}

fn power_cmd(args: &[&str]) -> (Vec<String>, Effect) {
    match args.first().copied() {
        Some("off") | Some("shutdown") => power::shutdown(),
        Some("restart") | Some("reboot") => power::restart(),
        Some("sleep") => {
            power::sleep();
            (vec![String::from("bangun dari sleep")], Effect::Redraw)
        }
        _ => {
            let (acpi, pm1a, slp, reset) = power::summary();
            (
                vec![
                    format!("power: acpi={} pm1a_cnt={:#x} slp_typ_s5={} reset_port={:#x}", acpi, pm1a, slp, reset),
                    String::from("bz power off | restart | sleep"),
                ],
                Effect::None,
            )
        }
    }
}

fn vm_cmd(args: &[&str]) -> (Vec<String>, Effect) {
    match args.first().copied() {
        None | Some("list") | Some("ls") => {
            let vms = vmm::list();
            if vms.is_empty() {
                return (vec![String::from("belum ada VM (buat: 'vm create <nama>')")], Effect::None);
            }
            let mut out = vec![String::from("virtual machines:")];
            for v in vms {
                out.push(format!(
                    "  #{} {:<10} {:>4}KiB {}vcpu disk={}KiB {:<8} steps={}{}",
                    v.id, v.name, v.mem_kib, v.vcpus, v.disk_kib, v.state.name(), v.steps,
                    if v.has_snapshot { " [snap]" } else { "" }
                ));
            }
            (out, Effect::None)
        }
        Some("create") | Some("new") => {
            let name = args.get(1).copied().unwrap_or("nanovm");
            let id = vmm::create(name, 64, 1);
            (vec![format!("VM dibuat: #{} '{}' (guest: NanoOS, 64KiB, 1 vcpu)", id, name)], Effect::None)
        }
        Some("start") | Some("run") | Some("boot") => {
            let Some(sel) = args.get(1).copied() else {
                return (vec![String::from("vm start <nama|id>")], Effect::None);
            };
            match vmm::start(sel) {
                Ok(r) => {
                    let mut out = vec![format!("== VM '{}' -> {} ({} instruksi ==", r.name, r.state.name(), r.steps)];
                    for l in r.console.lines() {
                        out.push(format!("  | {}", l));
                    }
                    (out, Effect::Redraw)
                }
                Err(e) => (vec![format!("vm start: {}", e)], Effect::None),
            }
        }
        Some("snapshot") | Some("snap") => {
            let Some(sel) = args.get(1).copied() else {
                return (vec![String::from("vm snapshot <nama|id>")], Effect::None);
            };
            match vmm::snapshot(sel) {
                Ok(()) => (vec![format!("snapshot dibuat untuk '{}'", sel)], Effect::None),
                Err(e) => (vec![format!("vm snapshot: {}", e)], Effect::None),
            }
        }
        Some("restore") => {
            let Some(sel) = args.get(1).copied() else {
                return (vec![String::from("vm restore <nama|id>")], Effect::None);
            };
            match vmm::restore(sel) {
                Ok(()) => (vec![format!("'{}' dipulihkan dari snapshot", sel)], Effect::None),
                Err(e) => (vec![format!("vm restore: {}", e)], Effect::None),
            }
        }
        Some("remove") | Some("rm") | Some("delete") => {
            let Some(sel) = args.get(1).copied() else {
                return (vec![String::from("vm remove <nama|id>")], Effect::None);
            };
            match vmm::remove(sel) {
                Ok(()) => (vec![format!("VM '{}' dihapus", sel)], Effect::None),
                Err(e) => (vec![format!("vm remove: {}", e)], Effect::None),
            }
        }
        Some(sub) => (
            vec![format!("vm {}: list|create|start|snapshot|restore|remove", sub)],
            Effect::None,
        ),
    }
}

fn lang_of(s: &str) -> Option<script::Lang> {
    match s {
        "js" | "javascript" => Some(script::Lang::Js),
        "ts" | "typescript" => Some(script::Lang::Ts),
        "py" | "python" => Some(script::Lang::Python),
        _ => None,
    }
}

/// The script debugger's run: the output as `run_script` prints it, then one
/// record per stop for Buitenzorg Studio:
/// `@dbg LINE<TAB>OUTPUT_LINES<TAB>STACK<TAB>LOCALS<TAB>GLOBALS`, the stack as
/// `fn:line` innermost first separated by `,`, variables as `name=value`
/// separated by U+001F.
fn debug_script(lang: script::Lang, src: &str, breaks: Vec<u32>, every: bool) -> Vec<String> {
    let out = script::run_debug(lang, src, breaks, every, 400);
    let snaps = out.snaps.iter().map(|s| (s.line, s.out_lines, &s.stack, &s.locals, &s.globals));
    debug_lines(lang.name(), &out.lines, &out.error, out.steps, out.snaps.len(), snaps)
}

type VarList = Vec<(String, String)>;

/// The output of a debugged run, then the `@dbg` records (see debug_script).
fn debug_lines<'a>(
    lang: &str,
    out: &[String],
    error: &Option<String>,
    steps: u64,
    count: usize,
    snaps: impl Iterator<Item = (u32, usize, &'a Vec<(String, u32)>, &'a VarList, &'a VarList)>,
) -> Vec<String> {
    let mut lines = vec![format!("[{}] ({} langkah, {} henti)", lang, steps, count)];
    for l in out {
        lines.push(format!("  {}", l));
    }
    if let Some(e) = error {
        lines.push(format!("  error: {}", e));
    }
    let clean = |t: &str| t.replace(['\t', '\u{1f}', '\n', '\r'], " ");
    let vars = |v: &VarList| v.iter().map(|(k, x)| format!("{}={}", k, clean(x))).collect::<Vec<_>>().join("\u{1f}");
    for (line, out_lines, stack, locals, globals) in snaps {
        let stack = stack.iter().map(|(f, l)| format!("{}:{}", f, l)).collect::<Vec<_>>().join(",");
        lines.push(format!("@dbg {}\t{}\t{}\t{}\t{}", line, out_lines, stack, vars(locals), vars(globals)));
    }
    lines
}

fn run_script(lang: script::Lang, src: &str) -> Vec<String> {
    let out = script::run(lang, src);
    let mut lines = vec![format!("[{}] ({} langkah)", lang.name(), out.steps)];
    for l in &out.lines {
        lines.push(format!("  {}", l));
    }
    if let Some(e) = out.error {
        // a UI script (window/button/...) needs its own process and window
        let ui = ["window", "button", "present", "running", "clear", "rect", "text"]
            .iter()
            .any(|f| e.contains(&format!("fungsi '{}' tak dikenal", f)));
        lines.push(format!("  error: {}", e));
        if ui {
            lines.push(String::from("  (skrip UI: jalankan sebagai aplikasi dengan 'run FILE', mis. 'run MAIN.JS')"));
        }
    }
    lines
}

/// `prof [on|off|reset|report|self]` — drive the instrumented profiler.
/// With no argument, run a quick self-profile (compose a desktop frame under
/// profiling) and show where the cycles went. Profiling is off by default so it
/// never perturbs normal timing.
fn prof_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    match arg {
        Some("on") => {
            crate::profile::enable();
            (vec![String::from("profiler: on")], Effect::None)
        }
        Some("off") => {
            crate::profile::disable();
            (vec![String::from("profiler: off")], Effect::None)
        }
        Some("hw") | Some("sw") => {
            let hw = arg == Some("hw");
            crate::wm::set_hw_cursor(hw);
            (vec![format!("cursor: {}", if hw { "hardware where an output has one" } else { "software everywhere" })], Effect::None)
        }
        Some("seq") | Some("par") => {
            let par = arg == Some("par");
            crate::wm::set_parallel_compose(par);
            (vec![format!("compose: {}", if par { "two bands in parallel" } else { "one CPU" })], Effect::None)
        }
        Some("reset") => {
            crate::profile::reset();
            (vec![String::from("profiler: cleared")], Effect::None)
        }
        Some("report") => {
            crate::profile::report();
            (
                vec![format!(
                    "profiler: {} zone(s) — full report on the serial log",
                    crate::profile::zone_count()
                )],
                Effect::None,
            )
        }
        None | Some("self") => {
            // Profile a real, representative workload: recompose the desktop a
            // few times (the WIN_PRESENT hot path) and report.
            let was = crate::profile::is_enabled();
            crate::profile::reset();
            crate::profile::enable();
            for _ in 0..8 {
                let _z = crate::profile::Guard::new("shell:present");
                crate::wm::present_now();
            }
            if !was {
                crate::profile::disable();
            }
            crate::profile::report();
            let (wr, sk) = crate::framebuffer::present_stats();
            crate::println!("[profile] present tiles since boot: {} written, {} unchanged and skipped", wr, sk);
            let (hits, misses) = crate::wm::layer_stats();
            crate::println!("[profile] shell layers (icons, taskbar) since boot: {} reused, {} redrawn", hits, misses);
            crate::println!("[profile] cursor-only frames since boot: {}", crate::wm::cursor_only_frames());
            crate::println!("[profile] frames composed in two bands (parallel): {}", crate::wm::parallel_frames());
            let (gpu_hw, fb_hw) = crate::wm::hw_cursor_outputs();
            let (moves, bands) = crate::virtio::gpu::stats();
            crate::println!("[profile] hardware cursor: virtio-gpu {}, board framebuffer {}; gpu cursor moves {}, gpu damage presents {}", gpu_hw, fb_hw, moves, bands);
            let (df, dr) = crate::wm::damage_stats();
            crate::println!("[profile] damage-presented frames since boot: {} ({} rows in all)", df, dr);
            (
                vec![
                    format!("profiler: sampled {} zone(s)", crate::profile::zone_count()),
                    String::from("full report on the serial log; `prof on|off|reset|report`"),
                ],
                Effect::None,
            )
        }
        Some(other) => (
            vec![format!("prof: unknown option '{}' (on|off|reset|report|self)", other)],
            Effect::None,
        ),
    }
}

/// `csx [FILE] [args]`: a C# script through the bytecode engine (`csx.rs`).
fn csx_cmd(args: &[&str]) -> (Vec<String>, Effect) {
    let (src, label) = match args.first() {
        None => (String::from(crate::csx::DEMO), String::from("demo")),
        Some(path) => match vfs::read(&resolve(Some(path))) {
            Ok(bytes) => (String::from_utf8_lossy(&bytes).into_owned(), String::from(*path)),
            Err(e) => return (vec![format!("csx: {}: {}", path, e)], Effect::None),
        },
    };
    let script_args: Vec<String> = args.iter().skip(1).map(|a| String::from(*a)).collect();
    let t0 = crate::arch::cpu::nanos();
    let out = crate::csx::run(&src, &script_args);
    let ms = (crate::arch::cpu::nanos().saturating_sub(t0)) / 1_000_000;
    let mut lines = vec![format!("[C#] {} ({} instruksi, {} ms)", label, out.steps, ms)];
    for l in &out.lines {
        lines.push(format!("  {}", l));
    }
    if let Some(e) = out.error {
        let ui = e.contains("'Ui.");
        lines.push(format!("  error: {}", e));
        if ui {
            lines.push(String::from("  (skrip UI: jalankan sebagai aplikasi dengan 'run FILE', mis. 'run MAIN.CSX')"));
        }
    }
    (lines, Effect::None)
}

fn script_cmd(cmd: &str, args: &[&str]) -> (Vec<String>, Effect) {
    // `script cs FILE` / `script csx FILE`: C# script
    if cmd == "script" && matches!(args.first().copied(), Some("cs") | Some("csx") | Some("csharp")) {
        return csx_cmd(&args[1..]);
    }
    // Direct form: `js|ts|py [path]`
    if let Some(lang) = lang_of(cmd) {
        return match args.first().copied() {
            None => (run_script(lang, script::demo_source(lang)), Effect::Redraw),
            Some(path) => match vfs::read(&resolve(Some(path))) {
                Ok(bytes) => match core::str::from_utf8(&bytes) {
                    Ok(text) => (run_script(lang, text), Effect::Redraw),
                    Err(_) => (vec![format!("script: {}: bukan teks UTF-8", path)], Effect::None),
                },
                Err(e) => (vec![format!("script: {}: {}", path, e)], Effect::None),
            },
        };
    }
    // `script <sub>`
    match args.first().copied() {
        None | Some("list") | Some("help") => (
            vec![
                String::from("runtime polyglot (v0.14 'Babel') -- interpreter bersama:"),
                String::from("  js | javascript   JavaScript"),
                String::from("  ts | typescript   TypeScript (transpile: strip tipe -> JS)"),
                String::from("  py | python       Python (subset, indentasi)"),
                String::from("pakai: script <lang> [file]   |   js|ts|py [file]"),
                String::from("  tanpa file -> jalankan demo bawaan (fib) untuk bahasa itu"),
            ],
            Effect::None,
        ),
        Some(l) if lang_of(l).is_some() => {
            let lang = lang_of(l).unwrap();
            match args.get(1).copied() {
                None => (run_script(lang, script::demo_source(lang)), Effect::Redraw),
                Some(path) => match vfs::read(&resolve(Some(path))) {
                    Ok(bytes) => match core::str::from_utf8(&bytes) {
                        Ok(text) => (run_script(lang, text), Effect::Redraw),
                        Err(_) => (vec![format!("script: {}: bukan teks UTF-8", path)], Effect::None),
                    },
                    Err(e) => (vec![format!("script: {}: {}", path, e)], Effect::None),
                },
            }
        }
        Some("run") => {
            let Some(l) = args.get(1).copied().and_then(lang_of) else {
                return (vec![String::from("script run <js|ts|py> <file>")], Effect::None);
            };
            let Some(path) = args.get(2).copied() else {
                return (vec![String::from("script run <js|ts|py> <file>")], Effect::None);
            };
            match vfs::read(&resolve(Some(path))) {
                Ok(bytes) => match core::str::from_utf8(&bytes) {
                    Ok(text) => (run_script(l, text), Effect::Redraw),
                    Err(_) => (vec![format!("script: {}: bukan teks UTF-8", path)], Effect::None),
                },
                Err(e) => (vec![format!("script: {}: {}", path, e)], Effect::None),
            }
        }
        Some("debug") => {
            // script debug <js|ts|py|csx> <file> <line,line,...|all>: run under
            // the script debugger; after the output, one `@dbg` record per stop
            let csx = matches!(args.get(1).copied(), Some("cs") | Some("csx"));
            let lang = args.get(1).copied().and_then(lang_of);
            let (true, Some(path)) = (csx || lang.is_some(), args.get(2).copied()) else {
                return (vec![String::from("script debug <js|ts|py|csx> <file> <baris,baris,...|all>")], Effect::None);
            };
            let spec = args.get(3).copied().unwrap_or("all");
            let every = spec == "all";
            let breaks: Vec<u32> = spec.split(',').filter_map(|n| n.trim().parse().ok()).collect();
            match vfs::read(&resolve(Some(path))) {
                Ok(bytes) => match core::str::from_utf8(&bytes) {
                    Ok(text) if csx => {
                        let out = crate::csx::run_debug(text, breaks, every, 400);
                        let snaps = out.snaps.iter().map(|s| (s.line, s.out_lines, &s.stack, &s.locals, &s.globals));
                        (debug_lines("cs", &out.lines, &out.error, out.steps, out.snaps.len(), snaps), Effect::None)
                    }
                    Ok(text) => (debug_script(lang.unwrap(), text, breaks, every), Effect::None),
                    Err(_) => (vec![format!("script: {}: bukan teks UTF-8", path)], Effect::None),
                },
                Err(e) => (vec![format!("script: {}: {}", path, e)], Effect::None),
            }
        }
        Some(sub) => (vec![format!("script {}: list|js|ts|py|run|debug", sub)], Effect::None),
    }
}

fn bz_cmd(args: &[&str]) -> (Vec<String>, Effect) {
    match args.first().copied() {
        Some("theme") => theme_cmd(args.get(1).copied()),
        Some("ws") => ws_cmd(args.get(1).copied()),
        Some("version") | None => (vec![format!("bz {} - Buitenzorg CLI", VERSION)], Effect::None),
        Some("list") | Some("search") => (pkg_list(args.get(1).copied()), Effect::None),
        Some("install") | Some("add") => (pkg_install(args.get(1).copied()), Effect::None),
        Some("remove") | Some("uninstall") => (pkg_remove(args.get(1).copied()), Effect::None),
        Some("info") => (pkg_info(args.get(1).copied()), Effect::None),
        Some("run") => run_cmd(args.get(1).copied(), &args[args.len().min(2)..]),
        Some("model") => model_cmd(&args[1..]),
        Some("power") => power_cmd(&args[1..]),
        Some("vm") => vm_cmd(&args[1..]),
        Some("virt") => (vmx::summary(), Effect::None),
        Some("script") => script_cmd("script", &args[1..]),
        Some(sub) => (
            vec![format!("bz {}: belum tersedia (lihat roadmap requirements.md §16)", sub)],
            Effect::None,
        ),
    }
}

fn pkg_list(filter: Option<&str>) -> Vec<String> {
    let mut out = vec![String::from("registry paket:")];
    for p in pkg::REGISTRY {
        if let Some(f) = filter {
            if !p.name.contains(f) && !p.description.contains(f) {
                continue;
            }
        }
        let mark = if pkg::is_installed(p.name) { "[x]" } else { "[ ]" };
        out.push(format!("{} {:<10} {:<7} {} ({})", mark, p.name, p.version, p.description, p.kind));
    }
    let files = crate::bzpkg::installed();
    if !files.is_empty() {
        out.push(String::from("terpasang dari berkas .bzpkg:"));
        for p in files {
            if filter.is_some_and(|f| !p.name.contains(f) && !p.title.contains(f)) {
                continue;
            }
            out.push(format!("[x] {:<10} {:<7} {} ({} berkas, entry {})", p.name, p.version, p.title, p.files.len(), p.entry));
        }
    }
    out
}

/// A package-file argument: a path, or a name with a .BZP/.BZPKG extension.
fn is_package_file(arg: &str) -> bool {
    let u = arg.to_ascii_uppercase();
    arg.contains('/') || u.ends_with(".BZP") || u.ends_with(".BZPKG")
}

fn pkg_info(arg: Option<&str>) -> Vec<String> {
    match arg {
        Some(a) if is_package_file(a) => match crate::bzpkg::info_file(&resolve(Some(a))) {
            Ok(lines) => lines,
            Err(e) => vec![format!("bz info: {}", e)],
        },
        Some(a) => match crate::bzpkg::find(a) {
            Some(p) => vec![
                format!("{} {} - {}", p.name, p.version, p.title),
                format!("  entry {}, berkas: {}", p.entry, p.files.join(" ")),
            ],
            None => match pkg::find(a) {
                Some(p) => vec![format!("{} {} - {} (paket bawaan, {})", p.name, p.version, p.description, if pkg::is_installed(a) { "terpasang" } else { "belum terpasang" })],
                None => vec![format!("bz info: {}: tidak ada", a)],
            },
        },
        None => vec![String::from("pakai: bz info NAMA | bz info ./PAKET.BZP")],
    }
}

fn pkg_install(name: Option<&str>) -> Vec<String> {
    let Some(name) = name else {
        return vec![String::from("bz install: butuh nama paket (lihat 'bz list') atau berkas ./PAKET.BZP")];
    };
    if is_package_file(name) {
        return match crate::bzpkg::install_file(&resolve(Some(name))) {
            Ok((n, v, Some(old))) => vec![format!("diperbarui: {} {} -> {}", n, old, v)],
            Ok((n, v, None)) => vec![format!("terpasang: {} v{} (dari berkas; jalankan '{}')", n, v, n)],
            Err(e) => vec![format!("bz install: {}", e)],
        };
    }
    match pkg::install(name) {
        Ok(ver) => vec![format!("terpasang: {} v{}", name, ver)],
        Err(e) => vec![format!("bz install: {}: {}", name, e)],
    }
}

fn pkg_remove(name: Option<&str>) -> Vec<String> {
    let Some(name) = name else {
        return vec![String::from("bz remove: butuh nama paket")];
    };
    if crate::bzpkg::find(name).is_some() {
        return match crate::bzpkg::remove(name) {
            Ok(n) => vec![format!("dihapus: {} ({} berkas)", name, n)],
            Err(e) => vec![format!("bz remove: {}", e)],
        };
    }
    match pkg::remove(name) {
        Ok(()) => vec![format!("dihapus: {}", name)],
        Err(e) => vec![format!("bz remove: {}: {}", name, e)],
    }
}
