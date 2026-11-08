//! Remote package repository: `bz repo URL`, `bz update`, `bz install NAME`
//! (with dependencies), `bz upgrade`.
//!
//! A repository is a plain HTTP server (on a PC: `bz repo serve DIR`) with
//! `INDEX.TXT` and the package files (`.bzpkg`, see `bzpkg.rs`). The index:
//!
//! ```text
//! BZREPO 1
//! name<TAB>version<TAB>file<TAB>sha256 (hex)<TAB>depends (a,b or -)<TAB>title
//! ```
//!
//! `bz update` downloads the index into `/disk/REPOIDX.TXT`; `bz install`
//! resolves a package's dependencies (depth first, a cycle is an error),
//! downloads every one that is missing or older, checks its SHA-256 against
//! the index, and installs it through `bzpkg::install_file`; `bz upgrade`
//! does that for every installed file package the index has a newer version
//! of. Versions compare part by part (`1.10.0` > `1.9.2`), numerically where
//! both parts are numbers. HTTP is a minimal HTTP/1.0 GET over `tcp.rs`, to an
//! IPv4 address (there is no DNS yet).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cmp::Ordering;
use sha2::{Digest, Sha256};

const CONFIG: &str = "/disk/REPO.CFG";
const INDEX: &str = "/disk/REPOIDX.TXT";
const DOWNLOAD: &str = "/disk/REPODL.BZP";
const MAX_BODY: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub version: String,
    pub file: String,
    pub sha256: String,
    pub depends: Vec<String>,
    pub title: String,
}

/// `1.10.0` vs `1.9.2`: part by part, numerically where both are numbers.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let pa: Vec<&str> = a.split(['.', '-']).collect();
    let pb: Vec<&str> = b.split(['.', '-']).collect();
    for i in 0..pa.len().max(pb.len()) {
        let (x, y) = (pa.get(i).copied().unwrap_or("0"), pb.get(i).copied().unwrap_or("0"));
        let o = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(m), Ok(n)) => m.cmp(&n),
            _ => x.cmp(y),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    Ordering::Equal
}

/// Parse an index (lines that do not fit are skipped).
pub fn parse_index(text: &str) -> Result<Vec<Entry>, String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("BZREPO 1") {
        return Err(String::from("bukan indeks repo (baris pertama 'BZREPO 1')"));
    }
    let mut out = Vec::new();
    for l in lines {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() < 6 || f[0].is_empty() {
            continue;
        }
        let depends = if f[4] == "-" { Vec::new() } else { f[4].split(',').map(|d| d.trim().to_string()).filter(|d| !d.is_empty()).collect() };
        let e = Entry {
            name: f[0].to_string(),
            version: f[1].to_string(),
            file: f[2].to_string(),
            sha256: f[3].to_ascii_lowercase(),
            depends,
            title: f[5].to_string(),
        };
        // a repository may keep several versions of a package: the newest counts
        match out.iter().position(|x: &Entry| x.name == e.name) {
            Some(i) if compare_versions(&out[i].version, &e.version) == Ordering::Less => out[i] = e,
            Some(_) => {}
            None => out.push(e),
        }
    }
    Ok(out)
}

/// The order to install `name` in: its dependencies first (depth first).
pub fn resolve(index: &[Entry], name: &str) -> Result<Vec<Entry>, String> {
    fn visit(index: &[Entry], name: &str, path: &mut Vec<String>, out: &mut Vec<Entry>) -> Result<(), String> {
        if out.iter().any(|e| e.name == name) {
            return Ok(());
        }
        if path.iter().any(|p| p == name) {
            path.push(String::from(name));
            return Err(format!("dependensi melingkar: {}", path.join(" -> ")));
        }
        let e = index.iter().find(|e| e.name == name).ok_or_else(|| format!("'{}' tidak ada di repo", name))?;
        path.push(String::from(name));
        for d in &e.depends {
            visit(index, d, path, out)?;
        }
        path.pop();
        out.push(e.clone());
        Ok(())
    }
    let mut out = Vec::new();
    visit(index, name, &mut Vec::new(), &mut out)?;
    Ok(out)
}

/// `http://A.B.C.D[:PORT][/prefix]` -> (address, port, path prefix).
pub fn parse_url(url: &str) -> Result<([u8; 4], u16, String), String> {
    let rest = url.trim().strip_prefix("http://").ok_or("URL repo harus 'http://IP[:PORT]' (belum ada https)")?;
    let (hostport, prefix) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].trim_end_matches('/').to_string()),
        None => (rest, String::new()),
    };
    let (host, port) = match hostport.split_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| "port tidak sah")?),
        None => (hostport, 80),
    };
    let ip = crate::net::parse_ip(host).ok_or("alamat repo harus IPv4 (belum ada DNS), mis. http://10.0.2.2:8080")?;
    Ok((ip, port, prefix))
}

fn now_ms() -> u64 {
    crate::arch::cpu::nanos() / 1_000_000
}

/// HTTP/1.0 GET; the body of a 200 answer.
pub fn http_get(url: &str, path: &str) -> Result<Vec<u8>, String> {
    let (code, status, body) = http_request(url, "GET", path, &[], 10_000)?;
    if code != 200 {
        return Err(format!("{}: {}", path, status));
    }
    Ok(body)
}

/// One HTTP/1.0 request: (status code, status line, body). `stall_ms` = how
/// long the server may stay silent (a build server compiles before it answers).
pub fn http_request(url: &str, method: &str, path: &str, body: &[u8], stall_ms: u64) -> Result<(u16, String, Vec<u8>), String> {
    use crate::tcp::{self, State};
    let (ip, port, prefix) = parse_url(url)?;
    let full = format!("{}/{}", prefix, path.trim_start_matches('/'));
    let id = tcp::connect(ip, port).map_err(String::from)?;
    let start = now_ms();
    while now_ms() - start < 5000 && tcp::state(id) == State::SynSent {
        crate::net::poll();
        crate::task::yield_now();
    }
    if tcp::state(id) != State::Established {
        tcp::close(id);
        return Err(format!("tidak bisa tersambung ke {}", url));
    }
    let head = format!(
        "{} {} HTTP/1.0\r\nHost: {}\r\nUser-Agent: buitenzorg-bz\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        method,
        full,
        crate::net::fmt_ip(ip),
        body.len()
    );
    tcp::send(id, head.as_bytes());
    // the body in pieces: the send queue is the connection's memory
    let mut sent = 0;
    let mut stalled = now_ms();
    while sent < body.len() {
        crate::net::poll();
        if tcp::unsent(id) < 64 * 1024 {
            let n = (body.len() - sent).min(16 * 1024);
            if tcp::send(id, &body[sent..sent + n]) == 0 {
                tcp::close(id);
                return Err(String::from("sambungan terputus saat mengirim"));
            }
            sent += n;
            stalled = now_ms();
        } else if now_ms() - stalled > stall_ms {
            tcp::close(id);
            return Err(format!("{}: waktu habis saat mengirim", full));
        }
        crate::task::yield_now();
    }
    let mut data = Vec::new();
    let mut stalled = now_ms();
    loop {
        crate::net::poll();
        let chunk = tcp::recv(id, 64 * 1024);
        if !chunk.is_empty() {
            data.extend_from_slice(&chunk);
            stalled = now_ms();
            if data.len() > MAX_BODY + 4096 {
                tcp::close(id);
                return Err(String::from("jawaban terlalu besar"));
            }
            continue;
        }
        if tcp::at_eof(id) {
            break;
        }
        if now_ms() - stalled > stall_ms {
            tcp::close(id);
            return Err(format!("{}: waktu habis", full));
        }
        crate::task::yield_now();
    }
    tcp::close(id);
    let split = data.windows(4).position(|w| w == b"\r\n\r\n").ok_or("jawaban HTTP tanpa header")?;
    let head = core::str::from_utf8(&data[..split]).map_err(|_| "header HTTP bukan teks")?;
    let status = String::from(head.lines().next().unwrap_or(""));
    let code = status.split_whitespace().nth(1).and_then(|c| c.parse::<u16>().ok()).unwrap_or(0);
    Ok((code, status, data[split + 4..].to_vec()))
}

fn repo_url() -> Option<String> {
    let t = crate::vfs::read(CONFIG).ok()?;
    let s = String::from_utf8(t).ok()?;
    s.lines().find_map(|l| l.strip_prefix("url ").map(|u| u.trim().to_string()))
}

fn load_index() -> Result<Vec<Entry>, String> {
    let bytes = crate::vfs::read(INDEX).map_err(|_| String::from("belum ada indeks: jalankan 'bz update'"))?;
    parse_index(&String::from_utf8_lossy(&bytes))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

/// The installed version of a file package (None = not installed).
fn installed_version(name: &str) -> Option<String> {
    crate::bzpkg::find(name).map(|p| p.version)
}

/// Download, verify and install one index entry.
fn fetch_install(url: &str, e: &Entry) -> Result<String, String> {
    let body = http_get(url, &e.file)?;
    let got = hex(&Sha256::digest(&body));
    if got != e.sha256 {
        return Err(format!("{}: SHA-256 tidak cocok dengan indeks (berkas rusak atau diganti)", e.file));
    }
    crate::vfs::write(DOWNLOAD, &body).map_err(|err| format!("menyimpan unduhan: {}", err))?;
    let r = crate::bzpkg::install_file(DOWNLOAD);
    let _ = crate::vfs::remove(DOWNLOAD);
    match r? {
        (n, v, Some(old)) => Ok(format!("diperbarui: {} {} -> {}", n, old, v)),
        (n, v, None) => Ok(format!("terpasang: {} v{} (dari repo)", n, v)),
    }
}

/// Install `name` and whatever it needs from the repository.
fn install(name: &str) -> Vec<String> {
    let Some(url) = repo_url() else { return alloc::vec![String::from("bz install: belum ada repo ('bz repo http://IP:PORT', lalu 'bz update')")] };
    let index = match load_index() {
        Ok(i) => i,
        Err(e) => return alloc::vec![format!("bz install: {}", e)],
    };
    let plan = match resolve(&index, name) {
        Ok(p) => p,
        Err(e) => return alloc::vec![format!("bz install: {}", e)],
    };
    let mut out = Vec::new();
    for e in plan {
        let have = installed_version(&e.name);
        if have.as_ref().is_some_and(|v| compare_versions(v, &e.version) != Ordering::Less) {
            if e.name == name {
                out.push(format!("{} v{} sudah terpasang", e.name, have.unwrap_or_default()));
            }
            continue;
        }
        if e.name != name {
            out.push(format!("dependensi: {} v{}", e.name, e.version));
        }
        match fetch_install(&url, &e) {
            Ok(m) => out.push(m),
            Err(err) => {
                out.push(format!("bz install: {}: {}", e.name, err));
                return out;
            }
        }
    }
    out
}

/// True when `name` is a package of the downloaded index (the shell's
/// `bz install` asks before the built-in registry says no).
pub fn knows(name: &str) -> bool {
    load_index().is_ok_and(|i| i.iter().any(|e| e.name == name))
}

/// `bz repo [URL|off]`, `bz update`, `bz upgrade`, `bz install NAME`.
pub fn command(args: &[&str]) -> Vec<String> {
    let one = |s: String| alloc::vec![s];
    match args {
        ["repo"] => match repo_url() {
            Some(u) => one(format!("repo: {} (indeks: {})", u, if crate::vfs::stat(INDEX).is_ok() { INDEX } else { "belum ada, 'bz update'" })),
            None => one(String::from("repo: belum diatur ('bz repo http://IP:PORT'; di QEMU user-net PC = 10.0.2.2)")),
        },
        ["repo", "off"] => {
            let _ = crate::vfs::remove(CONFIG);
            let _ = crate::vfs::remove(INDEX);
            one(String::from("repo: dilepas"))
        }
        ["repo", url] => match parse_url(url) {
            Ok(_) => match crate::vfs::write(CONFIG, format!("url {}\n", url).as_bytes()) {
                Ok(()) => one(format!("repo: {} -- lanjutkan dengan 'bz update'", url)),
                Err(e) => one(format!("repo: {}: {}", CONFIG, e)),
            },
            Err(e) => one(format!("repo: {}", e)),
        },
        ["update"] => {
            let Some(url) = repo_url() else { return one(String::from("bz update: belum ada repo ('bz repo http://IP:PORT')")) };
            match http_get(&url, "INDEX.TXT").and_then(|b| {
                let text = String::from_utf8_lossy(&b).into_owned();
                parse_index(&text).map(|i| (b, i))
            }) {
                Ok((bytes, index)) => {
                    if let Err(e) = crate::vfs::write(INDEX, &bytes) {
                        return one(format!("bz update: {}: {}", INDEX, e));
                    }
                    let newer: Vec<String> = index
                        .iter()
                        .filter_map(|e| installed_version(&e.name).filter(|v| compare_versions(v, &e.version) == Ordering::Less).map(|v| format!("{} {} -> {}", e.name, v, e.version)))
                        .collect();
                    let mut out = alloc::vec![format!("bz update: {} paket di {}", index.len(), url)];
                    if newer.is_empty() {
                        out.push(String::from("semua paket terpasang sudah versi terbaru"));
                    } else {
                        out.push(format!("bisa diperbarui ({}): {} -- 'bz upgrade'", newer.len(), newer.join(", ")));
                    }
                    out
                }
                Err(e) => one(format!("bz update: {}", e)),
            }
        }
        ["upgrade"] => {
            let index = match load_index() {
                Ok(i) => i,
                Err(e) => return one(format!("bz upgrade: {}", e)),
            };
            let todo: Vec<&Entry> = index
                .iter()
                .filter(|e| installed_version(&e.name).is_some_and(|v| compare_versions(&v, &e.version) == Ordering::Less))
                .collect();
            if todo.is_empty() {
                return one(String::from("bz upgrade: semua sudah versi terbaru"));
            }
            let mut out = Vec::new();
            for e in todo {
                out.extend(install(&e.name));
            }
            out
        }
        ["install", name] => install(name),
        ["search", filter @ ..] => match load_index() {
            Ok(index) => {
                let f = filter.first().copied().unwrap_or("");
                let mut out = alloc::vec![String::from("repo:")];
                for e in index.iter().filter(|e| e.name.contains(f) || e.title.contains(f)) {
                    let mark = match installed_version(&e.name) {
                        Some(v) if compare_versions(&v, &e.version) == Ordering::Less => format!("[^ {}]", v),
                        Some(_) => String::from("[x]"),
                        None => String::from("[ ]"),
                    };
                    let deps = if e.depends.is_empty() { String::new() } else { format!(" (butuh {})", e.depends.join(", ")) };
                    out.push(format!("{} {:<10} {:<7} {}{}", mark, e.name, e.version, e.title, deps));
                }
                out
            }
            Err(_) => Vec::new(),
        },
        _ => one(String::from("bz repo [URL|off] | bz update | bz upgrade | bz install NAMA")),
    }
}

/// Boot self-test (no network needed): versions, the index format, the
/// dependency order and a cycle.
pub fn self_test() -> bool {
    let idx = "BZREPO 1\nalpha\t1.2.0\tALPHA.BZP\tab\tbeta,gamma\tAlpha\nbeta\t0.9\tBETA.BZP\tcd\tgamma\tBeta\ngamma\t2.0\tGAMMA.BZP\tef\t-\tGamma\nbad line\nloop1\t1\tL1.BZP\t00\tloop2\tL\nloop2\t1\tL2.BZP\t00\tloop1\tL\n";
    let checks = [
        compare_versions("1.10.0", "1.9.2") == Ordering::Greater,
        compare_versions("1.0", "1.0.0") == Ordering::Equal,
        compare_versions("2.0-beta", "2.0-rc") == Ordering::Less,
        parse_index(idx).is_ok_and(|i| i.len() == 5 && i[0].depends == ["beta", "gamma"] && i[2].depends.is_empty()),
        parse_index(idx).and_then(|i| resolve(&i, "alpha")).is_ok_and(|p| p.iter().map(|e| e.name.as_str()).collect::<Vec<_>>() == ["gamma", "beta", "alpha"]),
        parse_index(idx).and_then(|i| resolve(&i, "loop1")).is_err_and(|e| e.contains("melingkar")),
        parse_index("nope\n").is_err(),
        parse_index("BZREPO 1\nx\t1.2\tA\t00\t-\tX\nx\t1.10\tB\t00\t-\tX\nx\t1.9\tC\t00\t-\tX\n").is_ok_and(|i| i.len() == 1 && i[0].file == "B"),
        parse_url("http://10.0.2.2:8080/repo").is_ok_and(|(ip, p, pre)| ip == [10, 0, 2, 2] && p == 8080 && pre == "/repo"),
        parse_url("https://x").is_err(),
    ];
    let ok = checks.iter().all(|c| *c);
    if ok {
        crate::println!("[kernel] MILESTONE: REPO OK (versions, index, dependency order gamma -> beta -> alpha, cycle refused, URLs)");
    } else {
        crate::println!("[repo] self-test failed: {:?}", checks);
    }
    ok
}
