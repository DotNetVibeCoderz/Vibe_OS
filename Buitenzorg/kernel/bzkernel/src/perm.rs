//! Permission broker: what an installed package's programs may do.
//!
//! A package lists the permissions it needs in its manifest
//! (`permissions=network,audio`, the SDK manifest's ids); installing it grants
//! them, `bz perm NAME +x / -x` changes the grant later. Every process carries
//! a grant: the system's own programs (built-in apps, the desktop, the shell)
//! get everything; a package's entry gets the package's grant, and anything a
//! process starts gets at most its parent's grant (no escape through
//! PROC_SPAWN or SHELL_RUN, which itself needs `system`).
//!
//! Enforced at the syscall boundary for ring-3 callers ([`check`]):
//!
//! | permission | syscalls |
//! |---|---|
//! | `network` | NET_SOCKET/BIND/SEND/RECV/CLOSE, TCP_OP, DNS_RESOLVE |
//! | `audio` | AUDIO_SET_VOLUME, AUDIO_TONE, AUDIO_PLAY, AUDIO_WRITE |
//! | `microphone` | AUDIO_READ (recording) |
//! | `devices` | HW_GPIO, HW_I2C, HW_SPI, HW_UART, HW_LED |
//! | `system` | SHELL_RUN, USER_OP, PKG_SET, SETTING (set), PROC_KILL of another process |
//! | `filesystem.write` | FS_WRITE / FS_APPEND / FS_OP outside `/ram` and the package's own `/disk/APPDATA/<NAME>` |
//!
//! A refused call returns `syserr::DENIED` and the first refusal per process
//! and permission is logged with the command that grants it. Reading files,
//! drawing, input, clocks and the rest stay open. `camera`,
//! `gallery`, `filesystem.read` and `ai.*` are recorded and shown, with no
//! syscall to guard yet. Shell scripts (`.SH` entries) run in the kernel's
//! shell with the user's rights, like commands typed in the Terminal.

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;

pub const NETWORK: u32 = 1 << 0;
pub const FILES_WRITE: u32 = 1 << 1;
pub const AUDIO: u32 = 1 << 2;
pub const DEVICES: u32 = 1 << 3;
pub const SYSTEM: u32 = 1 << 4;
pub const MICROPHONE: u32 = 1 << 5;
pub const CAMERA: u32 = 1 << 6;
pub const AI: u32 = 1 << 7;
pub const FILES_READ: u32 = 1 << 8;
pub const GALLERY: u32 = 1 << 9;
/// The system's own programs.
pub const ALL: u32 = u32::MAX;

/// Manifest ids and their bits (the `ai.*` ids share one bit).
pub const NAMES: [(&str, u32); 12] = [
    ("network", NETWORK),
    ("filesystem.write", FILES_WRITE),
    ("filesystem.read", FILES_READ),
    ("audio", AUDIO),
    ("devices", DEVICES),
    ("system", SYSTEM),
    ("microphone", MICROPHONE),
    ("camera", CAMERA),
    ("gallery", GALLERY),
    ("ai.llm", AI),
    ("ai.vision", AI),
    ("ai.genai", AI),
];

/// `network, audio` (or a comma list) -> bits; an unknown id is an error.
pub fn parse(list: &str) -> Result<u32, String> {
    let mut bits = 0;
    for p in list.split(|c: char| c == ',' || c.is_whitespace()).filter(|p| !p.is_empty()) {
        match NAMES.iter().find(|(n, _)| *n == p) {
            Some((_, b)) => bits |= b,
            None => return Err(format!("izin tidak dikenal: '{}'", p)),
        }
    }
    Ok(bits)
}

/// The ids of `bits`, comma separated (`ai` for the ai.* bit).
pub fn names(bits: u32) -> String {
    if bits == ALL {
        return String::from("semua (program sistem)");
    }
    let mut out: Vec<&str> = Vec::new();
    for (n, b) in NAMES {
        if bits & b != 0 && !out.iter().any(|o| NAMES.iter().any(|(m, c)| m == o && *c == b)) {
            out.push(if b == AI { "ai" } else { n });
        }
    }
    if out.is_empty() {
        String::from("(tidak ada)")
    } else {
        out.join(", ")
    }
}

/// A process's grant: permission bits and the package it belongs to ("" =
/// a system program, or a restricted program's child that inherited).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub bits: u32,
    pub pkg: String,
}

impl Grant {
    pub fn system() -> Self {
        Grant { bits: ALL, pkg: String::new() }
    }
}

/// What a program started now from `parent` gets: the package's grant (when
/// it is a package's program) narrowed to the parent's.
pub fn combine(parent: &Grant, pkg: Option<(&str, u32)>) -> Grant {
    match pkg {
        Some((name, bits)) => Grant { bits: parent.bits & bits, pkg: String::from(name) },
        None => parent.clone(),
    }
}

/// The grant of the process running now (the kernel and its tasks: all).
pub fn current() -> Grant {
    match crate::task::current_space().id {
        0 => Grant::system(),
        pid => crate::process::grant(pid),
    }
}

/// [`combine`] with the caller as the parent: used by every launch path.
pub fn for_launch(pkg: Option<(&str, u32)>) -> Grant {
    combine(&current(), pkg)
}

/// The folder a package's programs may always write: `/disk/APPDATA/<NAME>`
/// (letters and digits of the name, upper case, at most 8).
pub fn data_dir(pkg: &str) -> String {
    let stem: String = pkg.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>().to_uppercase();
    format!("/disk/APPDATA/{}", if stem.is_empty() { "APP" } else { stem.as_str() })
}

/// The permission syscall `nr` needs from ring-3 process `me` (0 = none).
/// File writes are decided per path by [`may_write`].
pub fn needed(nr: u64, a0: u64, me: u64) -> u32 {
    use bz_abi::sysno::*;
    match nr {
        NET_SOCKET | NET_BIND | NET_SEND | NET_RECV | NET_CLOSE | TCP_OP | DNS_RESOLVE => NETWORK,
        AUDIO_SET_VOLUME | AUDIO_TONE | AUDIO_PLAY | AUDIO_WRITE => AUDIO,
        AUDIO_READ => MICROPHONE,
        HW_GPIO | HW_I2C | HW_SPI | HW_UART | HW_LED => DEVICES,
        SHELL_RUN | USER_OP | PKG_SET => SYSTEM,
        SETTING if a0 == bz_abi::setting_op::SET => SYSTEM,
        FONT_OP if matches!(a0 & 0xFF, bz_abi::font_op::INSTALL | bz_abi::font_op::REMOVE) => SYSTEM,
        PROC_KILL if a0 != me => SYSTEM,
        _ => 0,
    }
}

/// Whether the syscalls that need a permission are among `nr` (cheap: no
/// lookup for the rest).
fn gated(nr: u64) -> bool {
    use bz_abi::sysno::*;
    matches!(
        nr,
        NET_SOCKET | NET_BIND | NET_SEND | NET_RECV | NET_CLOSE | TCP_OP | DNS_RESOLVE | AUDIO_SET_VOLUME | AUDIO_TONE | AUDIO_PLAY
            | AUDIO_WRITE | AUDIO_READ | HW_GPIO | HW_I2C | HW_SPI | HW_UART | HW_LED | SHELL_RUN | USER_OP | PKG_SET | SETTING | PROC_KILL | FONT_OP
    )
}

/// The ring-3 syscall gate: false = refuse with `syserr::DENIED`.
pub fn check(nr: u64, a0: u64) -> bool {
    if !gated(nr) {
        return true;
    }
    let me = crate::task::current_space().id;
    if me == 0 {
        return true;
    }
    check_for(me, &crate::process::grant(me), nr, a0)
}

fn check_for(me: u64, g: &Grant, nr: u64, a0: u64) -> bool {
    let need = needed(nr, a0, me);
    if need == 0 || g.bits & need == need {
        return true;
    }
    deny(me, g, need);
    false
}

/// Whether the ring-3 caller may change `path` (write, append, mkdir,
/// remove, rename).
pub fn may_write(path: &str) -> bool {
    let me = crate::task::current_space().id;
    if me == 0 {
        return true;
    }
    may_write_for(me, &crate::process::grant(me), path)
}

fn may_write_for(me: u64, g: &Grant, path: &str) -> bool {
    if g.bits & FILES_WRITE != 0 || path.starts_with("/ram/") {
        return true;
    }
    if !g.pkg.is_empty() {
        let dir = data_dir(&g.pkg);
        if path == dir || path.starts_with(&format!("{}/", dir)) || path == "/disk/APPDATA" {
            return true;
        }
    }
    deny(me, g, FILES_WRITE);
    false
}

/// (pid, permission) pairs already reported.
static REPORTED: Mutex<Vec<(u64, u32)>> = Mutex::new(Vec::new());

fn deny(me: u64, g: &Grant, need: u32) {
    let mut r = REPORTED.lock();
    if r.iter().any(|&(p, b)| p == me && b == need) {
        return;
    }
    if r.len() >= 64 {
        r.remove(0);
    }
    r.push((me, need));
    drop(r);
    let who = if g.pkg.is_empty() { String::from("program") } else { format!("paket '{}'", g.pkg) };
    let fix = if g.pkg.is_empty() { String::new() } else { format!(" -- izinkan: bz perm {} +{}", g.pkg, names(need)) };
    crate::println!("[perm] {} (pid {}) ditolak: butuh izin {}{}", who, me, names(need), fix);
}

/// Self-test boot: the policy on a registered test process -- a grant of
/// `audio` only: audio allowed, network / devices / system refused, its own
/// process killable but not another, file writes only in `/ram` and its data
/// folder; children never gain; names round-trip (`PERMISSIONS OK`).
pub fn self_test() -> Result<String, String> {
    use bz_abi::sysno;
    let pid = crate::process::app_start("uji-izin");
    let g = Grant { bits: AUDIO, pkg: String::from("uji-izin") };
    crate::process::set_grant(pid, g.clone());
    let got = crate::process::grant(pid);
    let audio = check_for(pid, &got, sysno::AUDIO_TONE, 440);
    let net = !check_for(pid, &got, sysno::TCP_OP, 0) && !check_for(pid, &got, sysno::DNS_RESOLVE, 0);
    let hw = !check_for(pid, &got, sysno::HW_GPIO, 0) && !check_for(pid, &got, sysno::AUDIO_READ, 0);
    let sys = !check_for(pid, &got, sysno::SHELL_RUN, 0) && !check_for(pid, &got, sysno::SETTING, bz_abi::setting_op::SET);
    let read_ok = check_for(pid, &got, sysno::SETTING, bz_abi::setting_op::GET) && check_for(pid, &got, sysno::FS_READ, 0);
    let kill = check_for(pid, &got, sysno::PROC_KILL, pid) && !check_for(pid, &got, sysno::PROC_KILL, 1);
    let files = may_write_for(pid, &got, "/ram/X.TXT")
        && may_write_for(pid, &got, "/disk/APPDATA/UJIIZIN/SKOR.TXT")
        && !may_write_for(pid, &got, "/disk/PROFILE.SH")
        && !may_write_for(pid, &got, "/disk/APPDATA/LAIN/X.TXT");
    let child = combine(&got, Some(("lain", NETWORK | AUDIO)));
    let narrowed = child.bits == AUDIO && combine(&Grant::system(), Some(("lain", NETWORK))).bits == NETWORK;
    let names_ok = parse("network, audio,ai.llm").ok() == Some(NETWORK | AUDIO | AI)
        && names(NETWORK | AUDIO) == "network, audio"
        && parse("terbang").is_err();
    crate::process::app_exit(pid, 0);
    let unknown_is_system = crate::process::grant(pid) == Grant::system();
    let all = [audio, net, hw, sys, read_ok, kill, files, narrowed, names_ok, unknown_is_system];
    if all.iter().all(|&b| b) {
        Ok(String::from("audio granted; network / devices / system refused; own kill only; writes only in /ram + its APPDATA; children never gain"))
    } else {
        Err(format!("checks {:?}", all))
    }
}
