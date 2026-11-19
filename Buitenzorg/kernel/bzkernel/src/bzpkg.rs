//! Package files (`.bzpkg`, Developer Workstation phase 2): install an app
//! from a file instead of only from the built-in registry, keep what is
//! installed across reboots, and remove it again.
//!
//! **Format, version 1** (little-endian; `bz pack` on the host writes it):
//!
//! ```text
//! "BZPK"  u16 version (1)  u16 file count  u32 manifest length
//! manifest: UTF-8 "key=value" lines -- name, version, title, kind,
//!           category, description, entry (the file `run <name>` starts)
//! per file: u8 name length, 8.3 name (A-Z 0-9 _ - .) -- or a path of
//!           up to 4 such names separated by '/' (sub-folders, 2026-11-19),
//!           u32 size,
//!           32-byte SHA-256 of the data, the data
//! trailer:  32-byte SHA-256 of every byte before it
//! ```
//!
//! **Signatures** (optional, 2026-11-19): a signed archive is the archive
//! above followed by `"BZSG"`, the signer's 32-byte Ed25519 public key and a
//! 64-byte Ed25519 signature over every byte of the archive (`bz pack
//! --sign` on the host, key from `bz keygen`). An archive whose signature
//! does not verify is always refused. `/disk/TRUST.CFG` lists trusted keys
//! (`HEX NAME` lines) and `require=on` makes unsigned and untrusted packages
//! refused (`bz trust ...`); without it they install with a note.
//!
//! On a FAT 8.3 volume the file is named `NAME.BZP` (the extension has three
//! letters); `bz install ./NAME.BZP` (or any path) installs it. Every hash is
//! checked before anything is written; files go to `/disk` (a path's
//! folders are created; folders a remove or upgrade leaves empty go); a
//! file that already exists is refused unless it belongs to an older version
//! of the same package (an upgrade). `/disk/PKGS.CFG` lists installed
//! packages, one per line: `pkg<TAB>name<TAB>version<TAB>title<TAB>entry
//! <TAB>FILE,FILE<TAB>category` for packages from files, `reg<TAB>name` for
//! registry packages installed with `bz install NAME`.

use alloc::{format, string::String, vec, vec::Vec};
use sha2::{Digest, Sha256};
use spin::Mutex;

pub const STATE_FILE: &str = "/disk/PKGS.CFG";
const MAGIC: &[u8; 4] = b"BZPK";
const MAX_TOTAL: usize = 32 * 1024 * 1024;
const MAX_FILES: usize = 64;

#[derive(Clone)]
pub struct Installed {
    pub name: String,
    pub version: String,
    pub title: String,
    pub entry: String,
    pub files: Vec<String>,
    pub category: String,
    /// Granted permissions (`perm.rs` bits).
    pub perms: u32,
    /// What the manifest asked for.
    pub requested: u32,
}

pub struct Archive {
    pub name: String,
    pub version: String,
    pub title: String,
    pub kind: String,
    pub category: String,
    pub description: String,
    pub entry: String,
    pub files: Vec<(String, Vec<u8>)>,
    /// `permissions=` of the manifest.
    pub permissions: u32,
}

static STATE: Mutex<Option<Vec<Installed>>> = Mutex::new(None);

// ---- signatures ---------------------------------------------------------------

pub const TRUST_FILE: &str = "/disk/TRUST.CFG";
const SIG_MAGIC: &[u8; 4] = b"BZSG";
const SIG_LEN: usize = 4 + 32 + 64;
/// Who signed the archive installed last (shown by `bz install`).
static LAST_SIGNER: Mutex<String> = Mutex::new(String::new());

/// Who signed a package.
pub enum Signer {
    Unsigned,
    Trusted(String),
    Untrusted([u8; 32]),
}

impl Signer {
    pub fn describe(&self) -> String {
        match self {
            Signer::Unsigned => String::from("tidak bertanda tangan"),
            Signer::Trusted(n) => format!("ditandatangani '{}' (terpercaya)", n),
            Signer::Untrusted(k) => format!("ditandatangani kunci tak dikenal {} ('bz trust add' untuk mempercayainya)", hex(&k[..8])),
        }
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

fn unhex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(k)
}

/// (trusted keys with their names, require signatures)
fn trust() -> (Vec<([u8; 32], String)>, bool) {
    let mut keys = Vec::new();
    let mut require = false;
    if let Ok(b) = crate::vfs::read(TRUST_FILE) {
        for line in String::from_utf8_lossy(&b).lines() {
            let line = line.trim();
            if line == "require=on" {
                require = true;
            } else if let Some((k, n)) = line.split_once(' ') {
                if let Some(key) = unhex32(k) {
                    keys.push((key, String::from(n.trim())));
                }
            }
        }
    }
    (keys, require)
}

fn save_trust(keys: &[([u8; 32], String)], require: bool) -> Result<(), &'static str> {
    let mut t = String::new();
    if require {
        t.push_str("require=on\n");
    }
    for (k, n) in keys {
        t.push_str(&format!("{} {}\n", hex(k), n));
    }
    crate::vfs::write(TRUST_FILE, t.as_bytes())
}

/// Parse and verify an archive, signed or not: every hash, and the signature
/// when there is one.
pub fn verify(bytes: &[u8]) -> Result<(Archive, Signer), &'static str> {
    let n = bytes.len();
    if n > SIG_LEN + 44 && &bytes[n - SIG_LEN..n - SIG_LEN + 4] == SIG_MAGIC {
        let body = &bytes[..n - SIG_LEN];
        let key: [u8; 32] = bytes[n - 96..n - 64].try_into().unwrap();
        let sig: [u8; 64] = bytes[n - 64..].try_into().unwrap();
        let ok = ed25519_dalek::VerifyingKey::from_bytes(&key)
            .is_ok_and(|vk| vk.verify_strict(body, &ed25519_dalek::Signature::from_bytes(&sig)).is_ok());
        if ok {
            let a = parse(body)?;
            let (keys, _) = trust();
            let signer = match keys.iter().find(|(k, _)| *k == key) {
                Some((_, name)) => Signer::Trusted(name.clone()),
                None => Signer::Untrusted(key),
            };
            return Ok((a, signer));
        }
        // the marker can only be there by chance in an unsigned archive
        return match parse(bytes) {
            Ok(a) => Ok((a, Signer::Unsigned)),
            Err(_) => Err("tanda tangan paket tidak sah (berkas diubah setelah ditandatangani?)"),
        };
    }
    Ok((parse(bytes)?, Signer::Unsigned))
}

/// Sign an archive (the self-test; `bz pack --sign` does the same on the host).
pub fn sign(archive: &[u8], key: &ed25519_dalek::SigningKey) -> Vec<u8> {
    use ed25519_dalek::Signer as _;
    let sig = key.sign(archive);
    let mut v = archive.to_vec();
    v.extend_from_slice(SIG_MAGIC);
    v.extend_from_slice(key.verifying_key().as_bytes());
    v.extend_from_slice(&sig.to_bytes());
    v
}

/// The signer of the package installed last.
pub fn last_signer() -> String {
    LAST_SIGNER.lock().clone()
}

/// `bz trust [add HEX NAME | remove NAME|HEX | require on|off]`.
pub fn trust_command(args: &[&str]) -> Vec<String> {
    let (mut keys, mut require) = trust();
    let changed = match args {
        ["add", k, name @ ..] if !name.is_empty() => match unhex32(k) {
            Some(key) => {
                keys.retain(|(x, _)| *x != key);
                keys.push((key, name.join(" ")));
                true
            }
            None => return alloc::vec![String::from("bz trust add: kunci = 64 digit hex (dari 'bz keygen' di PC)")],
        },
        ["remove", who] => {
            let before = keys.len();
            keys.retain(|(k, n)| n != who && hex(k) != *who);
            if keys.len() == before {
                return alloc::vec![format!("bz trust: '{}' tidak ada", who)];
            }
            true
        }
        ["require", v] => {
            require = *v == "on";
            true
        }
        [] => false,
        _ => return alloc::vec![String::from("bz trust [add HEX NAMA | remove NAMA | require on|off]")],
    };
    if changed {
        if let Err(e) = save_trust(&keys, require) {
            return alloc::vec![format!("bz trust: {}", e)];
        }
    }
    let mut out = alloc::vec![format!(
        "tanda tangan paket: {}",
        if require { "WAJIB (paket tanpa tanda tangan terpercaya ditolak)" } else { "opsional ('bz trust require on' untuk mewajibkan)" }
    )];
    if keys.is_empty() {
        out.push(String::from("  belum ada kunci terpercaya"));
    }
    for (k, n) in &keys {
        out.push(format!("  {:<16} {}", n, hex(k)));
    }
    out
}
/// Registry packages the user installed or removed (persisted with the rest).
static REGISTRY_INSTALLED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn valid_pkg_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 23 && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// An 8.3 name: 1-8 characters, optionally '.' and 1-3 more; A-Z 0-9 _ -.
pub fn valid_file_name(n: &str) -> bool {
    let (base, ext) = match n.split_once('.') {
        Some((b, e)) => (b, Some(e)),
        None => (n, None),
    };
    let ok = |s: &str, max: usize| !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    ok(base, 8) && ext.map_or(true, |e| ok(e, 3))
}

/// A file's place in a package: an 8.3 name, or up to 4 of them joined by '/'.
pub fn valid_file_path(n: &str) -> bool {
    let parts: Vec<&str> = n.split('/').collect();
    parts.len() <= 4 && parts.iter().all(|p| valid_file_name(p))
}

/// Create the folders above `file` (a /disk-relative path) that are missing.
fn make_parents(file: &str) -> Result<(), String> {
    let parts: Vec<&str> = file.split('/').collect();
    let mut dir = String::from("/disk");
    for p in &parts[..parts.len() - 1] {
        dir.push('/');
        dir.push_str(p);
        match crate::vfs::stat(&dir) {
            Ok(e) if e.is_dir => {}
            Ok(_) => return Err(format!("{} adalah berkas, bukan folder", dir)),
            Err(_) => crate::vfs::mkdir(&dir).map_err(|e| format!("membuat {}: {}", dir, e))?,
        }
    }
    Ok(())
}

/// Remove the folders of `files` (deepest first) that are empty now.
fn prune_folders(files: &[String]) {
    let mut dirs: Vec<String> = Vec::new();
    for f in files {
        let mut p = f.as_str();
        while let Some(i) = p.rfind('/') {
            p = &p[..i];
            if !dirs.iter().any(|d| d == p) {
                dirs.push(String::from(p));
            }
        }
    }
    dirs.sort_by_key(|d| core::cmp::Reverse(d.matches('/').count()));
    for d in dirs {
        let path = format!("/disk/{}", d);
        if crate::vfs::list_entries(&path).is_ok_and(|e| e.is_empty()) {
            let _ = crate::vfs::remove(&path);
        }
    }
}

/// Files the system needs: never replaced by a package.
fn protected(file: &str) -> bool {
    matches!(file, "PKGS.CFG" | "USERS.CFG" | "BZDISK.ID" | "SSHHOST.KEY" | "SSHD.CFG" | "REGION.CFG" | "RANDOM.SEE" | "PROFILE.SH" | "HISTORY.TXT" | "CLRHOST.ELF" | "BZAPP.DLL" | "BZUI.DLL")
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], &'static str> {
        if self.p + n > self.b.len() {
            return Err("arsip terpotong");
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, &'static str> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, &'static str> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
}

/// Parse and verify an archive (every hash), without writing anything.
pub fn parse(bytes: &[u8]) -> Result<Archive, &'static str> {
    if bytes.len() < 12 + 32 || &bytes[..4] != MAGIC {
        return Err("bukan berkas .bzpkg (magic BZPK tidak ada)");
    }
    if bytes.len() > MAX_TOTAL {
        return Err("arsip terlalu besar");
    }
    let body = &bytes[..bytes.len() - 32];
    let digest: [u8; 32] = Sha256::digest(body).into();
    if digest != bytes[bytes.len() - 32..] {
        return Err("checksum arsip salah (berkas rusak atau diubah)");
    }
    let mut r = Reader { b: body, p: 4 };
    if r.u16()? != 1 {
        return Err("versi format tidak didukung");
    }
    let count = r.u16()? as usize;
    if count == 0 || count > MAX_FILES {
        return Err("jumlah berkas tidak sah");
    }
    let mlen = r.u32()? as usize;
    let manifest = core::str::from_utf8(r.take(mlen)?).map_err(|_| "manifest bukan UTF-8")?;
    let mut a = Archive {
        name: String::new(),
        version: String::new(),
        title: String::new(),
        kind: String::from("desktop"),
        category: String::from("LAINNYA"),
        description: String::new(),
        entry: String::new(),
        files: Vec::new(),
        permissions: 0,
    };
    for line in manifest.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = String::from(v.trim());
        match k.trim() {
            "name" => a.name = v,
            "version" => a.version = v,
            "title" => a.title = v,
            "kind" => a.kind = v,
            "category" => a.category = v.to_uppercase(),
            "description" => a.description = v,
            "entry" => a.entry = v.to_uppercase(),
            "permissions" => a.permissions = crate::perm::parse(&v).map_err(|_| "izin tidak dikenal di manifest (permissions=)")?,
            _ => {}
        }
    }
    if !valid_pkg_name(&a.name) {
        return Err("nama paket tidak sah (a-z 0-9 -, maks 23)");
    }
    if a.version.is_empty() || a.version.len() > 23 {
        return Err("versi paket tidak sah");
    }
    if a.title.is_empty() {
        a.title = a.name.clone();
    }
    for _ in 0..count {
        let n = r.u8()? as usize;
        let name = core::str::from_utf8(r.take(n)?).map_err(|_| "nama berkas bukan ASCII")?;
        if !valid_file_path(name) {
            return Err("nama berkas bukan 8.3 (A-Z 0-9 _ -; folder: maks 4 tingkat dipisah /)");
        }
        let size = r.u32()? as usize;
        let hash: [u8; 32] = r.take(32)?.try_into().unwrap();
        let data = r.take(size)?;
        let got: [u8; 32] = Sha256::digest(data).into();
        if got != hash {
            return Err("SHA-256 sebuah berkas tidak cocok");
        }
        if a.files.iter().any(|(f, _)| f == name) {
            return Err("berkas ganda di dalam arsip");
        }
        a.files.push((String::from(name), data.to_vec()));
    }
    if r.p != body.len() {
        return Err("data berlebih setelah berkas terakhir");
    }
    if !a.entry.is_empty() && !a.files.iter().any(|(f, _)| *f == a.entry) {
        return Err("entry tidak ada di dalam arsip");
    }
    Ok(a)
}

// ---- state --------------------------------------------------------------------

fn with_state<R>(f: impl FnOnce(&mut Vec<Installed>) -> R) -> R {
    let mut s = STATE.lock();
    if s.is_none() {
        let mut v = Vec::new();
        let mut reg = Vec::new();
        if let Ok(b) = crate::vfs::read(STATE_FILE) {
            for line in String::from_utf8_lossy(&b).lines() {
                let p: Vec<&str> = line.split('\t').collect();
                match p.as_slice() {
                    ["reg", name] => reg.push(String::from(*name)),
                    ["pkg", name, version, title, entry, files, rest @ ..] => v.push(Installed {
                        name: String::from(*name),
                        version: String::from(*version),
                        title: String::from(*title),
                        entry: String::from(*entry),
                        files: files.split(',').filter(|f| !f.is_empty()).map(String::from).collect(),
                        category: String::from(rest.first().copied().unwrap_or("LAINNYA")),
                        // granted / requested permission bits (none for records from before them)
                        perms: rest.get(1).and_then(|v| u32::from_str_radix(v, 16).ok()).unwrap_or(0),
                        requested: rest.get(2).and_then(|v| u32::from_str_radix(v, 16).ok()).unwrap_or(0),
                    }),
                    _ => {}
                }
            }
        }
        *REGISTRY_INSTALLED.lock() = reg;
        *s = Some(v);
    }
    f(s.as_mut().unwrap())
}

fn save(state: &[Installed]) -> Result<(), &'static str> {
    let mut text = String::new();
    for r in REGISTRY_INSTALLED.lock().iter() {
        text.push_str(&format!("reg\t{}\n", r));
    }
    for p in state {
        text.push_str(&format!(
            "pkg\t{}\t{}\t{}\t{}\t{}\t{}\t{:x}\t{:x}\n",
            p.name,
            p.version,
            p.title,
            p.entry,
            p.files.join(","),
            p.category,
            p.perms,
            p.requested
        ));
    }
    crate::vfs::write(STATE_FILE, text.as_bytes())
}

/// Load the saved state at boot (after `/disk` is mounted) and re-apply the
/// registry packages the user installed.
pub fn load() {
    let n = with_state(|s| s.len());
    let reg = REGISTRY_INSTALLED.lock().clone();
    for r in &reg {
        let _ = crate::pkg::install(r);
    }
    if n + reg.len() > 0 {
        crate::println!("[pkg] {} package(s) from files, {} registry package(s) restored from {}", n, reg.len(), STATE_FILE);
    }
}

/// A registry package was installed / removed with `bz`: remember it.
pub fn note_registry(name: &str, installed: bool) {
    with_state(|s| {
        {
            let mut reg = REGISTRY_INSTALLED.lock();
            reg.retain(|r| r != name);
            if installed {
                reg.push(String::from(name));
            }
        }
        let _ = save(s);
    });
}

pub fn installed() -> Vec<Installed> {
    with_state(|s| s.clone())
}

pub fn find(name: &str) -> Option<Installed> {
    with_state(|s| s.iter().find(|p| p.name == name).cloned())
}

/// The file `run <name>` starts for a package installed from a file.
/// The package a launch target belongs to, with its grant: a package name,
/// or a path to one of its files (`/disk/GAME/MAIN.DLL`).
pub fn package_of(target: &str) -> Option<(String, u32)> {
    if let Some(p) = find(target) {
        return Some((p.name, p.perms));
    }
    let rel = target.strip_prefix("/disk/")?.to_uppercase();
    with_state(|s| s.iter().find(|p| p.files.iter().any(|f| *f == rel)).map(|p| (p.name.clone(), p.perms)))
}

/// `bz perm [NAME [+id|-id ...]]`: show or change what packages may do.
pub fn perm_command(args: &[&str]) -> Vec<String> {
    let Some(name) = args.first() else {
        let mut out = vec![String::from("izin paket (bz perm NAMA +izin / -izin):")];
        for p in installed() {
            out.push(format!("  {:<16} {}", p.name, crate::perm::names(p.perms)));
        }
        if out.len() == 1 {
            out.push(String::from("  (belum ada paket dari berkas)"));
        }
        return out;
    };
    let changes = &args[1..];
    with_state(|state| {
        let Some(p) = state.iter_mut().find(|p| p.name == *name) else {
            return vec![format!("bz perm: '{}' tidak terpasang dari berkas", name)];
        };
        for c in changes {
            let (add, id) = match c.split_at(1) {
                ("+", id) => (true, id),
                ("-", id) => (false, id),
                _ => return vec![format!("bz perm: pakai +izin atau -izin, bukan '{}'", c)],
            };
            match crate::perm::parse(id) {
                Ok(b) if add => p.perms |= b,
                Ok(b) => p.perms &= !b,
                Err(e) => return vec![format!("bz perm: {}", e)],
            }
        }
        let line = format!("{}: {} (diminta: {})", p.name, crate::perm::names(p.perms), crate::perm::names(p.requested));
        if !changes.is_empty() {
            if let Err(e) = save(state) {
                return vec![format!("bz perm: menyimpan {}: {}", STATE_FILE, e)];
            }
        }
        let mut out = vec![line];
        if !changes.is_empty() {
            out.push(String::from("  berlaku untuk program yang dijalankan setelah ini"));
        }
        out
    })
}

pub fn entry_for(name: &str) -> Option<String> {
    find(name).filter(|p| !p.entry.is_empty()).map(|p| p.entry)
}

// ---- install / remove -----------------------------------------------------------

/// Install a package file. Returns (name, version, upgraded-from).
pub fn install_file(path: &str) -> Result<(String, String, Option<String>), String> {
    let bytes = crate::vfs::read(path).map_err(|e| format!("{}: {}", path, e))?;
    let r = install_bytes(&bytes);
    if let Ok((name, _, _)) = &r {
        keep_history(name, &bytes);
    }
    r
}

/// Where installed archives are kept for `bz rollback`.
const HISTORY_DIR: &str = "/disk/PKGHIST";

fn history_stem(name: &str) -> String {
    let s: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>().to_ascii_uppercase();
    if s.is_empty() { String::from("PKG") } else { s }
}

/// Keep the archive just installed (`NAME.BZP`); the one before it becomes
/// `NAME.OLD` -- what `bz rollback` installs (best effort: /disk may be full).
fn keep_history(name: &str, bytes: &[u8]) {
    if crate::vfs::stat(HISTORY_DIR).is_err() {
        let _ = crate::vfs::mkdir(HISTORY_DIR);
    }
    let stem = history_stem(name);
    let cur = format!("{}/{}.BZP", HISTORY_DIR, stem);
    let old = format!("{}/{}.OLD", HISTORY_DIR, stem);
    if let Ok(prev) = crate::vfs::read(&cur) {
        if prev != bytes {
            let _ = crate::vfs::write(&old, &prev);
        }
    }
    let _ = crate::vfs::write(&cur, bytes);
}

/// `bz rollback NAME`: install the version before the current one again
/// (a second rollback goes forward again).
pub fn rollback(name: &str) -> Result<(String, String), String> {
    let cur = find(name).ok_or_else(|| format!("'{}' tidak terpasang dari berkas/repo", name))?;
    let old = format!("{}/{}.OLD", HISTORY_DIR, history_stem(name));
    let bytes = crate::vfs::read(&old).map_err(|_| format!("tidak ada versi sebelumnya dari '{}' ({})", name, old))?;
    let (n, v, _) = install_bytes(&bytes)?;
    keep_history(&n, &bytes);
    Ok((cur.version, v))
}

fn install_bytes(bytes: &[u8]) -> Result<(String, String, Option<String>), String> {
    let (a, signer) = verify(bytes).map_err(String::from)?;
    if trust().1 && !matches!(signer, Signer::Trusted(_)) {
        return Err(format!("{} -- ditolak: tanda tangan terpercaya diwajibkan ('bz trust')", signer.describe()));
    }
    *LAST_SIGNER.lock() = signer.describe();
    if crate::pkg::find(&a.name).is_some() {
        return Err(format!("'{}' adalah paket bawaan sistem", a.name));
    }
    with_state(|state| {
        let previous = state.iter().position(|p| p.name == a.name);
        let old_files: Vec<String> = previous.map(|i| state[i].files.clone()).unwrap_or_default();
        // owned by another package, protected, or a system file already there
        for (f, _) in &a.files {
            if protected(f) {
                return Err(format!("{} adalah berkas sistem", f));
            }
            if let Some(owner) = state.iter().find(|p| p.name != a.name && p.files.iter().any(|x| x == f)) {
                return Err(format!("{} milik paket '{}'", f, owner.name));
            }
            let target = format!("/disk/{}", f);
            if !old_files.iter().any(|x| x == f) && crate::vfs::stat(&target).is_ok() {
                return Err(format!("{} sudah ada di /disk (bukan dari paket ini)", f));
            }
        }
        for (f, data) in &a.files {
            make_parents(f)?;
            crate::vfs::write(&format!("/disk/{}", f), data).map_err(|e| format!("menulis {}: {}", f, e))?;
        }
        // files of the old version that the new one no longer has
        let mut gone = Vec::new();
        for f in &old_files {
            if !a.files.iter().any(|(n, _)| n == f) {
                let _ = crate::vfs::remove(&format!("/disk/{}", f));
                gone.push(f.clone());
            }
        }
        prune_folders(&gone);
        let rec = Installed {
            name: a.name.clone(),
            version: a.version.clone(),
            title: a.title.clone(),
            entry: a.entry.clone(),
            files: a.files.iter().map(|(n, _)| n.clone()).collect(),
            category: a.category.clone(),
            // installing grants what the manifest asks for (shown by bz install /
            // bz info; `bz perm` takes it back)
            perms: a.permissions,
            requested: a.permissions,
        };
        let upgraded = match previous {
            Some(i) => Some(core::mem::replace(&mut state[i], rec).version),
            None => {
                state.push(rec);
                None
            }
        };
        save(state).map_err(|e| format!("menyimpan {}: {}", STATE_FILE, e))?;
        Ok((a.name.clone(), a.version.clone(), upgraded))
    })
}

/// Remove a package installed from a file (its files too).
pub fn remove(name: &str) -> Result<usize, String> {
    with_state(|state| {
        let i = state.iter().position(|p| p.name == name).ok_or_else(|| format!("'{}' tidak terpasang dari berkas", name))?;
        let p = state.remove(i);
        let mut n = 0;
        for f in &p.files {
            if crate::vfs::remove(&format!("/disk/{}", f)).is_ok() {
                n += 1;
            }
        }
        prune_folders(&p.files);
        save(state).map_err(|e| format!("menyimpan {}: {}", STATE_FILE, e))?;
        let stem = history_stem(name);
        let _ = crate::vfs::remove(&format!("{}/{}.BZP", HISTORY_DIR, stem));
        let _ = crate::vfs::remove(&format!("{}/{}.OLD", HISTORY_DIR, stem));
        Ok(n)
    })
}

/// Describe a package file without installing it (`bz info FILE`).
pub fn info_file(path: &str) -> Result<Vec<String>, String> {
    let bytes = crate::vfs::read(path).map_err(|e| format!("{}: {}", path, e))?;
    let (a, signer) = verify(&bytes).map_err(String::from)?;
    let mut out = alloc::vec![
        format!("{} {} - {}", a.name, a.version, a.title),
        format!("  jenis {}, kategori {}, entry {}", a.kind, a.category, if a.entry.is_empty() { "-" } else { &a.entry }),
    ];
    if !a.description.is_empty() {
        out.push(format!("  {}", a.description));
    }
    for (f, d) in &a.files {
        out.push(format!("  {:<12} {:>8} B", f, d.len()));
    }
    out.push(String::from("  semua SHA-256 cocok"));
    out.push(format!("  {}", signer.describe()));
    Ok(out)
}

/// Build an archive (the kernel self-test and `bz pack` on the device).
pub fn build(manifest: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(MAGIC);
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&(files.len() as u16).to_le_bytes());
    v.extend_from_slice(&(manifest.len() as u32).to_le_bytes());
    v.extend_from_slice(manifest.as_bytes());
    for (name, data) in files {
        v.push(name.len() as u8);
        v.extend_from_slice(name.as_bytes());
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        let h: [u8; 32] = Sha256::digest(data).into();
        v.extend_from_slice(&h);
        v.extend_from_slice(data);
    }
    let h: [u8; 32] = Sha256::digest(&v).into();
    v.extend_from_slice(&h);
    v
}

/// Boot self-test on `dir` (a writable mount): build an archive, install it,
/// upgrade it, refuse a corrupted copy and a file clash, remove it.
pub fn self_test(dir: &str) -> Result<String, String> {
    let _ = dir;
    let v1 = build("name=bztest\nversion=1.0\ntitle=Uji Paket\nentry=BZTEST.SH\n", &[("BZTEST.SH", b"echo paket v1\n"), ("BZTEST.TXT", b"data")]);
    let v2 = build("name=bztest\nversion=2.0\ntitle=Uji Paket\nentry=BZTEST.SH\n", &[("BZTEST.SH", b"echo paket v2\n")]);
    let file = "/disk/BZTEST.BZP";
    crate::vfs::write(file, &v1).map_err(|e| format!("write: {}", e))?;
    let (_, ver, up) = install_file(file)?;
    if ver != "1.0" || up.is_some() || crate::vfs::read("/disk/BZTEST.TXT").ok().as_deref() != Some(b"data".as_slice()) {
        return Err(String::from("install v1"));
    }
    crate::vfs::write(file, &v2).map_err(|e| format!("write: {}", e))?;
    let (_, ver, up) = install_file(file)?;
    if ver != "2.0" || up.as_deref() != Some("1.0") || crate::vfs::stat("/disk/BZTEST.TXT").is_ok() {
        return Err(String::from("upgrade to v2"));
    }
    if entry_for("bztest").as_deref() != Some("BZTEST.SH") {
        return Err(String::from("entry"));
    }
    let mut bad = v2.clone();
    let k = bad.len() / 2;
    bad[k] ^= 0x55;
    if parse(&bad).is_ok() {
        return Err(String::from("corrupted archive accepted"));
    }
    let clash = build("name=bzclash\nversion=1\n", &[("BZTEST.SH", b"x")]);
    crate::vfs::write(file, &clash).map_err(|e| format!("write: {}", e))?;
    if install_file(file).is_ok() {
        return Err(String::from("file of another package overwritten"));
    }
    let n = remove("bztest")?;
    if n != 1 || crate::vfs::stat("/disk/BZTEST.SH").is_ok() || find("bztest").is_some() {
        return Err(String::from("remove"));
    }
    // permissions: the manifest's become the grant; `bz perm` changes it;
    // an unknown id is refused; a launch target maps to its package
    let pv = build("name=bztperm\nversion=1\npermissions=network,audio\n", &[("BZTPERM.TXT", b"x")]);
    crate::vfs::write(file, &pv).map_err(|e| format!("write: {}", e))?;
    install_file(file)?;
    let granted = find("bztperm").is_some_and(|p| p.perms == crate::perm::NETWORK | crate::perm::AUDIO);
    let _ = perm_command(&["bztperm", "-network", "+devices"]);
    let changed = find("bztperm").is_some_and(|p| p.perms == crate::perm::AUDIO | crate::perm::DEVICES)
        && package_of("/disk/BZTPERM.TXT").is_some_and(|(n, b)| n == "bztperm" && b == crate::perm::AUDIO | crate::perm::DEVICES);
    let unknown = parse(&build("name=bztbad\nversion=1\npermissions=terbang\n", &[("X.TXT", b"x")])).is_err();
    let _ = remove("bztperm");
    if !(granted && changed && unknown) {
        return Err(format!("permissions: granted {} changed {} unknown refused {}", granted, changed, unknown));
    }
    // sub-folders: created on install, an entry inside one runs, the folders
    // a package leaves empty go (on upgrade and on remove)
    let d1 = build(
        "name=bztdir\nversion=1\nentry=BZTDIR/BIN/RUN.SH\n",
        &[("BZTDIR/BIN/RUN.SH", b"echo dari folder\n"), ("BZTDIR/DATA/A.TXT", b"a"), ("BZTDIR/DATA/LAMA/B.TXT", b"b")],
    );
    crate::vfs::write(file, &d1).map_err(|e| format!("write: {}", e))?;
    install_file(file)?;
    let dir_ok = crate::vfs::read("/disk/BZTDIR/DATA/LAMA/B.TXT").ok().as_deref() == Some(b"b".as_slice())
        && crate::vfs::stat("/disk/BZTDIR/BIN").is_ok_and(|e| e.is_dir);
    let d2 = build("name=bztdir\nversion=2\nentry=BZTDIR/BIN/RUN.SH\n", &[("BZTDIR/BIN/RUN.SH", b"echo v2\n"), ("BZTDIR/DATA/A.TXT", b"a2")]);
    crate::vfs::write(file, &d2).map_err(|e| format!("write: {}", e))?;
    install_file(file)?;
    let upgrade_pruned = crate::vfs::stat("/disk/BZTDIR/DATA/LAMA").is_err() && crate::vfs::stat("/disk/BZTDIR/DATA/A.TXT").is_ok();
    let n = remove("bztdir")?;
    let removed = n == 2 && crate::vfs::stat("/disk/BZTDIR").is_err();
    let bad_path = parse(&build("name=bztbad\nversion=1\n", &[("A/B/C/D/E.TXT", b"x")])).is_err()
        && parse(&build("name=bztbad\nversion=1\n", &[("../X.TXT", b"x")])).is_err();
    if !(dir_ok && upgrade_pruned && removed && bad_path) {
        let _ = remove("bztdir");
        return Err(format!("sub-folders: install {} upgrade {} remove {} bad paths {}", dir_ok, upgrade_pruned, removed, bad_path));
    }
    // signatures: a signed archive installs as untrusted, a changed byte or a
    // changed signature is refused, `require=on` refuses an untrusted signer
    // until its key is trusted (the user's TRUST.CFG is restored afterwards)
    let saved_trust = crate::vfs::read(TRUST_FILE).ok();
    let key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
    let signed = sign(&v1, &key);
    let _ = crate::vfs::remove(TRUST_FILE);
    let untrusted_ok = matches!(verify(&signed), Ok((_, Signer::Untrusted(_))));
    let mut tampered = signed.clone();
    tampered[20] ^= 1;
    let mut bad_sig = signed.clone();
    let last = bad_sig.len() - 1;
    bad_sig[last] ^= 1;
    let refused = verify(&tampered).is_err() && verify(&bad_sig).is_err();
    let _ = trust_command(&["require", "on"]);
    crate::vfs::write(file, &signed).map_err(|e| format!("write: {}", e))?;
    let blocked = install_file(file).is_err();
    let k = hex(key.verifying_key().as_bytes());
    let _ = trust_command(&["add", &k, "uji-kunci"]);
    let trusted_ok = install_file(file).is_ok() && last_signer().contains("uji-kunci");
    let _ = remove("bztest");
    match saved_trust {
        Some(t) => {
            let _ = crate::vfs::write(TRUST_FILE, &t);
        }
        None => {
            let _ = crate::vfs::remove(TRUST_FILE);
        }
    }
    let _ = crate::vfs::remove(file);
    if !(untrusted_ok && refused && blocked && trusted_ok) {
        return Err(format!(
            "signatures (untrusted {} refused {} blocked {} trusted {})",
            untrusted_ok, refused, blocked, trusted_ok
        ));
    }
    Ok(String::from("build, verify, install, upgrade 1.0 -> 2.0, corrupted archive refused, file clash refused, remove, Ed25519 signature verified / tampering refused / require=on enforced"))
}
