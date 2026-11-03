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
}

static STATE: Mutex<ShellState> = Mutex::new(ShellState { cwd: String::new() });

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

/// `deploy [on [code] | off]`: the remote deploy service for the Magic App
/// Generator (BZDP over UDP 7070, `deploy.rs`).
fn deploy_cmd(args: &[&str]) -> Vec<String> {
    match args.first().copied() {
        Some("on") => {
            let code = args.get(1).and_then(|c| c.parse::<u16>().ok()).filter(|c| *c < 10_000).unwrap_or_else(crate::deploy::random_code);
            if !crate::deploy::enable(code) {
                return vec![String::from("deploy: port UDP 7070 tidak bisa dibuka")];
            }
        }
        Some("off") => crate::deploy::disable(),
        None | Some("status") => {}
        Some(_) => return vec![String::from("pakai: deploy [on [kode] | off]")],
    }
    let (on, code, served) = crate::deploy::status();
    let ip = crate::net::local_ip().map_or(String::from("(tanpa jaringan)"), |ip| format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]));
    if on {
        vec![
            format!("deploy: AKTIF di {}:{} (UDP), kode pairing {:04}", ip, crate::deploy::PORT, code),
            format!("         {} permintaan dilayani; Magic App Generator > Perangkat", served),
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
        "echo" => (echo(&args), Effect::None),
        "rm" | "del" => (rm(args.first().copied()), Effect::None),
        "df" => (df(), Effect::None),
        "pwd" => (vec![cwd()], Effect::None),
        "cd" => (cd(args.first().copied()), Effect::None),
        "clear" | "cls" => (Vec::new(), Effect::Clear),
        "ver" | "uname" => (vec![version()], Effect::None),
        "about" | "credits" => (about(), Effect::None),
        "mounts" => (vfs::mounts(), Effect::None),
        "theme" => theme_cmd(args.first().copied()),
        "ws" => ws_cmd(args.first().copied()),
        "run" => run_cmd(args.first().copied(), &args[args.len().min(1)..]),
        "ps" => (ps_cmd(), Effect::None),
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
        "prof" | "profile" => prof_cmd(args.first().copied()),
        "gfx3d" | "3d" => {
            // gfx3d [test]: the 3D API's resources, or its self-test
            let mut out = Vec::new();
            if args.first() == Some(&"test") {
                let ok = crate::gfx3d::self_test();
                out.push(String::from(if ok { "self-test 3D: OK (lihat log serial)" } else { "self-test 3D GAGAL (lihat log serial)" }));
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

fn version() -> String {
    String::from("Buitenzorg OS v0.14 'Babel' -- Gravicode Studios (Kang Fadhil)")
}

fn about() -> Vec<String> {
    ["Buitenzorg OS",
     "  Sistem operasi hibrida & AI-native (kernel Rust, userland C#).",
     "  \"zonder zorg\" - tanpa kekhawatiran. Kebun Raya Bogor edition.",
     "",
     "  Dibuat oleh : Gravicode Studios",
     "  Dipimpin oleh: Kang Fadhil",
     "  Versi       : v0.14 'Babel'"]
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
     "  deploy on|off     terima app dari Magic App Generator (UDP 7070)",
     "  ssh on|off        server SSH (port 22) untuk akses jarak jauh",
     "  ipconfig          alamat IP, MAC, gateway, statistik jaringan",
     "  ping <ip> [n]     uji koneksi (ICMP echo)",
     "  arp, route, netstat, hostname   tabel ARP, rute, koneksi",
     "  ask <prompt>      AI: lengkapi teks (LLM lokal)",
     "  vm <sub>          virtualisasi (list/create/start/snapshot/restore)",
     "  script <lang> [f] polyglot: js|ts|py (jalankan demo/file)",
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

fn run_script(lang: script::Lang, src: &str) -> Vec<String> {
    let out = script::run(lang, src);
    let mut lines = vec![format!("[{}] ({} langkah)", lang.name(), out.steps)];
    for l in &out.lines {
        lines.push(format!("  {}", l));
    }
    if let Some(e) = out.error {
        lines.push(format!("  error: {}", e));
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

fn script_cmd(cmd: &str, args: &[&str]) -> (Vec<String>, Effect) {
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
        Some(sub) => (vec![format!("script {}: list|js|ts|py|run", sub)], Effect::None),
    }
}

fn bz_cmd(args: &[&str]) -> (Vec<String>, Effect) {
    match args.first().copied() {
        Some("theme") => theme_cmd(args.get(1).copied()),
        Some("ws") => ws_cmd(args.get(1).copied()),
        Some("version") | None => (vec![String::from("bz 0.14.0 'Babel' - Buitenzorg CLI")], Effect::None),
        Some("list") | Some("search") => (pkg_list(args.get(1).copied()), Effect::None),
        Some("install") | Some("add") => (pkg_install(args.get(1).copied()), Effect::None),
        Some("remove") | Some("uninstall") => (pkg_remove(args.get(1).copied()), Effect::None),
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
