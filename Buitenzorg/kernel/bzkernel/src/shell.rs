//! Built-in shell (v0.7 "Kanopi"): popular cross-ecosystem commands (Unix +
//! Windows aliases) over the VFS, the theme, and the window manager. Pure over
//! its input line: `run` returns output lines plus an [`Effect`].
//!
//! Supported: help, ls/dir, cat/type, echo, pwd, cd, clear/cls, ver, uname,
//! theme, ws, mounts, bz.

use alloc::{format, string::String, string::ToString, vec, vec::Vec};
use spin::Mutex;

use crate::{ai, app, model, pkg, power, screensaver, theme, vfs, wallpaper, wm};

pub struct ShellState {
    pub cwd: String,
}

static STATE: Mutex<ShellState> = Mutex::new(ShellState { cwd: String::new() });

/// Side effect a command asks the terminal/desktop to perform.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    Clear,
    Redraw, // theme/workspace changed; recompose the desktop
}

pub fn cwd() -> String {
    let s = STATE.lock();
    if s.cwd.is_empty() { String::from("/disk") } else { s.cwd.clone() }
}

pub fn prompt() -> String {
    format!("buitenzorg:{}$ ", cwd())
}

/// Run one command line; returns (output lines, effect).
pub fn run(line: &str) -> (Vec<String>, Effect) {
    let line = line.trim();
    if line.is_empty() {
        return (Vec::new(), Effect::None);
    }
    let mut parts = line.split_whitespace();
    let cmd = parts.next().unwrap();
    let args: Vec<&str> = parts.collect();

    match cmd {
        "help" => (help(), Effect::None),
        "ls" | "dir" => (list_dir(args.first().copied()), Effect::None),
        "cat" | "type" => (cat(args.first().copied()), Effect::None),
        "echo" => (vec![args.join(" ")], Effect::None),
        "pwd" => (vec![cwd()], Effect::None),
        "cd" => (cd(args.first().copied()), Effect::None),
        "clear" | "cls" => (Vec::new(), Effect::Clear),
        "ver" | "uname" => (vec![version()], Effect::None),
        "about" | "credits" => (about(), Effect::None),
        "mounts" => (vfs::mounts(), Effect::None),
        "theme" => theme_cmd(args.first().copied()),
        "ws" => ws_cmd(args.first().copied()),
        "run" => run_cmd(args.first().copied()),
        "bg" | "wallpaper" => bg_cmd(args.first().copied()),
        "saver" | "screensaver" => saver_cmd(args.first().copied()),
        "cursor" => cursor_cmd(args.first().copied()),
        "anim" => anim_cmd(args.first().copied()),
        "settings" | "personalize" => (settings_show(), Effect::None),
        "ask" | "ai" => (ai_ask(&args), Effect::Redraw),
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

fn version() -> String {
    String::from("Buitenzorg OS v0.12 'Nalar' -- Gravicode Studios (Kang Fadhil)")
}

fn about() -> Vec<String> {
    ["Buitenzorg OS",
     "  Sistem operasi hibrida & AI-native (kernel Rust, userland C#).",
     "  \"zonder zorg\" - tanpa kekhawatiran. Kebun Raya Bogor edition.",
     "",
     "  Dibuat oleh : Gravicode Studios",
     "  Dipimpin oleh: Kang Fadhil",
     "  Versi       : v0.12 'Nalar'"]
        .iter()
        .map(|s| String::from(*s))
        .collect()
}

fn help() -> Vec<String> {
    ["Perintah:",
     "  ls|dir [path]     daftar isi direktori (VFS)",
     "  cat|type <file>   tampilkan isi file",
     "  cd <path>         pindah direktori",
     "  pwd               direktori sekarang",
     "  echo <teks>       cetak teks",
     "  mounts            daftar mount VFS",
     "  theme [nama|cycle|list]     ganti tema (8 style + dark/light)",
     "  bg [nama|list|/path.bmp]    ganti wallpaper (bawaan / gambar user)",
     "  saver [nama|list|off]       screensaver (starfield, mystify, pipes, ...)",
     "  cursor [normal|besar]       ukuran kursor",
     "  anim [on|off]               micro-interaction on/off",
     "  settings                    tampilkan pengaturan (personalization)",
     "  ws [1-4]          pindah virtual desktop",
     "  run <app>         jalankan app (xox, paint, taskmgr)",
     "  ask <prompt>      AI: lengkapi teks (LLM lokal)",
     "  shutdown|restart|sleep      power management",
     "  bz <sub>          CLI (install/remove/list, model, power)",
     "  clear|cls         bersihkan layar",
     "  ver|uname         versi OS",
     "  about|credits     tentang & pembuat"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Resolve `arg` (optional) against the cwd into an absolute VFS path.
fn resolve(arg: Option<&str>) -> String {
    match arg {
        Some(p) if p.starts_with('/') => p.to_string(),
        Some(p) => {
            let base = cwd();
            format!("{}/{}", base.trim_end_matches('/'), p)
        }
        None => cwd(),
    }
}

fn list_dir(arg: Option<&str>) -> Vec<String> {
    let path = resolve(arg);
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

fn run_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    let Some(name) = arg else {
        return (vec![String::from("run: butuh nama app (mis. 'run xox')")], Effect::None);
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
    match app::run_named(name) {
        Ok(code) => (vec![format!("app '{}' selesai (exit {})", name, code)], Effect::Redraw),
        Err(e) => (vec![format!("run: {}: {}", name, e)], Effect::None),
    }
}

fn bg_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    match arg {
        None => (vec![format!("wallpaper: {}", wallpaper::label())], Effect::None),
        Some("list") => {
            let mut out = vec![String::from("wallpaper bawaan:")];
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

fn bz_cmd(args: &[&str]) -> (Vec<String>, Effect) {
    match args.first().copied() {
        Some("theme") => theme_cmd(args.get(1).copied()),
        Some("ws") => ws_cmd(args.get(1).copied()),
        Some("version") | None => (vec![String::from("bz 0.12.0 'Nalar' - Buitenzorg CLI")], Effect::None),
        Some("list") | Some("search") => (pkg_list(args.get(1).copied()), Effect::None),
        Some("install") | Some("add") => (pkg_install(args.get(1).copied()), Effect::None),
        Some("remove") | Some("uninstall") => (pkg_remove(args.get(1).copied()), Effect::None),
        Some("run") => run_cmd(args.get(1).copied()),
        Some("model") => model_cmd(&args[1..]),
        Some("power") => power_cmd(&args[1..]),
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
    out
}

fn pkg_install(name: Option<&str>) -> Vec<String> {
    let Some(name) = name else {
        return vec![String::from("bz install: butuh nama paket (lihat 'bz list')")];
    };
    match pkg::install(name) {
        Ok(ver) => vec![format!("terpasang: {} v{}", name, ver)],
        Err(e) => vec![format!("bz install: {}: {}", name, e)],
    }
}

fn pkg_remove(name: Option<&str>) -> Vec<String> {
    let Some(name) = name else {
        return vec![String::from("bz remove: butuh nama paket")];
    };
    match pkg::remove(name) {
        Ok(()) => vec![format!("dihapus: {}", name)],
        Err(e) => vec![format!("bz remove: {}: {}", name, e)],
    }
}
