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
//! per file: u8 name length, 8.3 name (A-Z 0-9 _ - .), u32 size,
//!           32-byte SHA-256 of the data, the data
//! trailer:  32-byte SHA-256 of every byte before it
//! ```
//!
//! On a FAT 8.3 volume the file is named `NAME.BZP` (the extension has three
//! letters); `bz install ./NAME.BZP` (or any path) installs it. Every hash is
//! checked before anything is written; files go to the root of `/disk`; a
//! file that already exists is refused unless it belongs to an older version
//! of the same package (an upgrade). `/disk/PKGS.CFG` lists installed
//! packages, one per line: `pkg<TAB>name<TAB>version<TAB>title<TAB>entry
//! <TAB>FILE,FILE<TAB>category` for packages from files, `reg<TAB>name` for
//! registry packages installed with `bz install NAME`.

use alloc::{format, string::String, vec::Vec};
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
}

static STATE: Mutex<Option<Vec<Installed>>> = Mutex::new(None);
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
        if !valid_file_name(name) {
            return Err("nama berkas bukan 8.3 (A-Z 0-9 _ -)");
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
        text.push_str(&format!("pkg\t{}\t{}\t{}\t{}\t{}\t{}\n", p.name, p.version, p.title, p.entry, p.files.join(","), p.category));
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
pub fn entry_for(name: &str) -> Option<String> {
    find(name).filter(|p| !p.entry.is_empty()).map(|p| p.entry)
}

// ---- install / remove -----------------------------------------------------------

/// Install a package file. Returns (name, version, upgraded-from).
pub fn install_file(path: &str) -> Result<(String, String, Option<String>), String> {
    let bytes = crate::vfs::read(path).map_err(|e| format!("{}: {}", path, e))?;
    let a = parse(&bytes).map_err(String::from)?;
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
            crate::vfs::write(&format!("/disk/{}", f), data).map_err(|e| format!("menulis {}: {}", f, e))?;
        }
        // files of the old version that the new one no longer has
        for f in &old_files {
            if !a.files.iter().any(|(n, _)| n == f) {
                let _ = crate::vfs::remove(&format!("/disk/{}", f));
            }
        }
        let rec = Installed {
            name: a.name.clone(),
            version: a.version.clone(),
            title: a.title.clone(),
            entry: a.entry.clone(),
            files: a.files.iter().map(|(n, _)| n.clone()).collect(),
            category: a.category.clone(),
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
        save(state).map_err(|e| format!("menyimpan {}: {}", STATE_FILE, e))?;
        Ok(n)
    })
}

/// Describe a package file without installing it (`bz info FILE`).
pub fn info_file(path: &str) -> Result<Vec<String>, String> {
    let bytes = crate::vfs::read(path).map_err(|e| format!("{}: {}", path, e))?;
    let a = parse(&bytes).map_err(String::from)?;
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
    let _ = crate::vfs::remove(file);
    if n != 1 || crate::vfs::stat("/disk/BZTEST.SH").is_ok() || find("bztest").is_some() {
        return Err(String::from("remove"));
    }
    Ok(String::from("build, verify, install, upgrade 1.0 -> 2.0, corrupted archive refused, file clash refused, remove"))
}
