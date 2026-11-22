//! Global search (generic): what the Start menu's search box and the shell's
//! `cari` command look through -- every app of the Start menu and the
//! desktop, the settings pages, and the files and folders on the mounted
//! volumes (the root and one folder level of each mount, indexed when a
//! search starts and kept for ~10 s).
//!
//! A hit carries the action the desktop performs: an app's launch name, a
//! system action (`@theme`), or `open:PATH`, which opens Berkas at that file.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    App,
    Setting,
    Folder,
    File,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::App => "Aplikasi",
            Kind::Setting => "Pengaturan",
            Kind::Folder => "Folder",
            Kind::File => "Berkas",
        }
    }
}

#[derive(Clone)]
pub struct Hit {
    pub label: String,
    /// Second line: the path of a file, the kind of anything else.
    pub detail: String,
    pub kind: Kind,
    /// What the desktop runs: a launch name, `@action` or `open:PATH`.
    pub action: String,
    /// Launch name whose icon the result shows ("" = none).
    pub icon: String,
}

/// Settings pages and actions: (label, extra words, action).
const SETTINGS: [(&str, &str, &str); 9] = [
    ("Tema", "theme warna gelap terang dark light", "@theme"),
    ("Wallpaper berikutnya", "latar background gambar desktop", "@wallpaper"),
    ("Rapikan jendela", "tile susun window", "@tile"),
    ("Pengaturan tampilan", "display layar resolusi skala scaling ukuran", "msetting"),
    ("Pengaturan suara", "audio volume suara speaker", "msetting"),
    ("Screensaver", "layar tidur saver", "msetting"),
    ("Pengguna", "user akun password kata sandi", "msetting"),
    ("Tanggal & Waktu", "jam zona waktu date time clock bahasa region", "mwaktu"),
    ("Notifikasi", "pusat notifikasi pemberitahuan action center", "@notifications"),
];

/// (tick built, paths): the file index.
static INDEX: Mutex<Option<(u64, Vec<(String, bool)>)>> = Mutex::new(None);
/// Rebuild the index when it is older than this (ticks): ~10 s.
const INDEX_TTL: u64 = 182;

/// Paths of the files and folders on every mount (root + one level), with
/// a folder flag. At most 4000.
fn build_index() -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for mount in crate::vfs::mounts() {
        let Ok(top) = crate::vfs::list_entries(&mount) else { continue };
        for e in top {
            if out.len() >= 4000 {
                return out;
            }
            let path = format!("{}/{}", mount, e.name);
            if e.is_dir {
                if let Ok(sub) = crate::vfs::list_entries(&path) {
                    for s in sub.into_iter().filter(|s| s.name != "." && s.name != "..") {
                        if out.len() < 4000 {
                            out.push((format!("{}/{}", path, s.name), s.is_dir));
                        }
                    }
                }
            }
            out.push((path, e.is_dir));
        }
    }
    out
}

/// Drop the file index (a file was saved, a disk plugged in).
pub fn invalidate() {
    *INDEX.lock() = None;
}

fn files() -> Vec<(String, bool)> {
    let now = crate::interrupts::ticks();
    if let Some((t, v)) = INDEX.lock().as_ref() {
        if now.saturating_sub(*t) < INDEX_TTL {
            return v.clone();
        }
    }
    let v = build_index();
    *INDEX.lock() = Some((now, v.clone()));
    v
}

fn lower(s: &str) -> String {
    s.chars().flat_map(|c| c.to_lowercase()).collect()
}

/// 0 = a word of the label starts with the query, 1 = the label contains
/// it, 2 = the extra words do; None = no match.
fn score(label: &str, extra: &str, q: &str) -> Option<u8> {
    let l = lower(label);
    if l.starts_with(q) || l.split(|c: char| c == ' ' || c == '-' || c == '_' || c == '.').any(|w| w.starts_with(q)) {
        Some(0)
    } else if l.contains(q) {
        Some(1)
    } else if lower(extra).contains(q) {
        Some(2)
    } else {
        None
    }
}

/// Results for `query` (case-insensitive), best first: apps, settings, then
/// folders and files. At most `max`.
pub fn query(query: &str, max: usize) -> Vec<Hit> {
    let q = lower(query.trim());
    if q.is_empty() {
        return Vec::new();
    }
    let mut ranked: Vec<(u8, u8, Hit)> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for (name, label) in crate::wm::app_catalog() {
        if seen.contains(&name) {
            continue;
        }
        seen.push(name);
        if let Some(s) = score(label, name, &q) {
            let kind = if name.starts_with('@') { Kind::Setting } else { Kind::App };
            ranked.push((0, s, Hit { label: String::from(label), detail: String::from(kind.label()), kind, action: String::from(name), icon: String::from(if name.starts_with('@') { "" } else { name }) }));
        }
    }
    for (label, extra, action) in SETTINGS {
        if let Some(s) = score(label, extra, &q) {
            if ranked.iter().any(|(_, _, h)| h.label == label) {
                continue;
            }
            ranked.push((1, s, Hit { label: String::from(label), detail: String::from("Pengaturan"), kind: Kind::Setting, action: String::from(action), icon: String::from(if action.starts_with('@') { "" } else { action }) }));
        }
    }
    if q.chars().count() >= 2 {
        for (path, dir) in files() {
            let name = path.rsplit('/').next().unwrap_or(&path);
            if let Some(s) = score(name, "", &q) {
                let kind = if dir { Kind::Folder } else { Kind::File };
                ranked.push((2 + !dir as u8, s, Hit { label: String::from(name), detail: path.clone(), kind, action: format!("open:{}", path), icon: String::from("mfiles") }));
            }
        }
    }
    ranked.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    ranked.into_iter().take(max).map(|(_, _, h)| h).collect()
}

/// Shell `cari QUERY`.
pub fn command(args: &[&str]) -> Vec<String> {
    if args.is_empty() {
        return alloc::vec![String::from("pakai: cari KATA  (aplikasi, pengaturan, berkas)")];
    }
    let hits = query(&args.join(" "), 30);
    if hits.is_empty() {
        return alloc::vec![format!("cari: tidak ada yang cocok dengan '{}'", args.join(" "))];
    }
    hits.iter().map(|h| format!("{:<10} {:<28} {}", h.kind.label(), h.label, if h.kind == Kind::File || h.kind == Kind::Folder { h.detail.as_str() } else { h.action.as_str() })).collect()
}

/// Self-test boot: an app by a word of its label, a setting by an extra
/// word, a file on /disk by part of its name, and no hit for nonsense.
pub fn self_test() -> bool {
    invalidate();
    let app = query("kalku", 5).first().is_some_and(|h| h.kind == Kind::App && h.action == "mcalc");
    let setting = query("resolusi", 5).iter().any(|h| h.kind == Kind::Setting && h.action == "msetting");
    let file = query("batang", 10).iter().any(|h| h.kind == Kind::File && h.action == "open:/disk/BATANG.TXT");
    let has_disk = crate::vfs::mounts().iter().any(|m| m == "/disk");
    let none = query("zzqxw", 5).is_empty();
    let cmd = command(&["kalku"]).first().is_some_and(|l| l.contains("Kalkulator"));
    let ok = app && setting && (file || !has_disk) && none && cmd;
    if ok {
        crate::println!(
            "[kernel] MILESTONE: SEARCH OK (apps, settings{}, {} indexed path(s))",
            if has_disk { ", files on /disk" } else { "" },
            files().len()
        );
    } else {
        crate::println!("[search] self-test: app {} setting {} file {} (disk {}) none {} cmd {}", app, setting, file, has_disk, none, cmd);
    }
    ok
}
