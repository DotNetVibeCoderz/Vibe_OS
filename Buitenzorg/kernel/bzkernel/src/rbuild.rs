//! Remote C# builds: `bz buildsrv URL`, `bz build DIR [--run]`.
//!
//! C# is not compiled inside the OS. A PC runs `bz build-server` (sdk/bz,
//! `dotnet build` against Buitenzorg.App / Buitenzorg.UI); the device sends a
//! project folder's `.CS` files there and gets back an assembly that RustCLR
//! (CLRHOST.ELF) runs -- the same .dll on x86-64, ARM64 and RISC-V. Request
//! (`POST /build`):
//!
//! ```text
//! BZSRC 1
//! name NAME
//! file PROGRAM.CS 1234      <- then exactly 1234 bytes, then a newline
//! ...
//! ```
//!
//! 200 = the assembly; anything else = the compiler's errors as text, one per
//! line (`error: PROGRAM.CS baris 12: CS1002: ; expected` -- Studio's Problems
//! panel reads the line numbers).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

const CONFIG: &str = "/disk/BUILDSRV.CFG";

fn server() -> Option<String> {
    let t = crate::vfs::read(CONFIG).ok()?;
    String::from_utf8(t).ok()?.lines().find_map(|l| l.strip_prefix("url ").map(|u| u.trim().to_string()))
}

/// The assembly name for a project folder: its last component, 8.3-safe.
pub fn project_name(dir: &str) -> String {
    let last = dir.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let n: String = last.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>().to_ascii_uppercase();
    if n.is_empty() { String::from("PROGRAM") } else { n }
}

/// The request body for the `.CS` files of `dir`.
fn request(dir: &str, name: &str) -> Result<(Vec<u8>, usize), String> {
    let entries = crate::vfs::list_entries(dir).map_err(|e| format!("{}: {}", dir, e))?;
    let mut body = format!("BZSRC 1\nname {}\n", name).into_bytes();
    let mut n = 0;
    for e in entries.iter().filter(|e| !e.is_dir && e.name.to_ascii_uppercase().ends_with(".CS")) {
        let data = crate::vfs::read(&format!("{}/{}", dir, e.name)).map_err(|err| format!("{}: {}", e.name, err))?;
        body.extend_from_slice(format!("file {} {}\n", e.name.to_ascii_uppercase(), data.len()).as_bytes());
        body.extend_from_slice(&data);
        body.push(b'\n');
        n += 1;
    }
    if n == 0 {
        return Err(format!("{}: tidak ada berkas .CS", dir));
    }
    Ok((body, n))
}

/// `bz buildsrv [URL|off]`, `bz build DIR [--run]` (`dir` already resolved).
pub fn command(args: &[&str]) -> Vec<String> {
    let one = |s: String| alloc::vec![s];
    match args {
        ["buildsrv"] => one(match server() {
            Some(u) => format!("buildsrv: {}", u),
            None => String::from("buildsrv: belum diatur ('bz buildsrv http://IP:PORT'; di PC: 'bz build-server'; QEMU user-net PC = 10.0.2.2)"),
        }),
        ["buildsrv", "off"] => {
            let _ = crate::vfs::remove(CONFIG);
            one(String::from("buildsrv: dilepas"))
        }
        ["buildsrv", url] => match crate::repo::parse_url(url) {
            Ok(_) => match crate::vfs::write(CONFIG, format!("url {}\n", url).as_bytes()) {
                Ok(()) => one(format!("buildsrv: {}", url)),
                Err(e) => one(format!("buildsrv: {}: {}", CONFIG, e)),
            },
            Err(e) => one(format!("buildsrv: {}", e)),
        },
        ["build", dir, rest @ ..] => {
            let Some(url) = server() else {
                return one(String::from("bz build: belum ada build server ('bz buildsrv http://IP:PORT'; di PC: 'bz build-server')"));
            };
            let name = project_name(dir);
            let (body, files) = match request(dir, &name) {
                Ok(b) => b,
                Err(e) => return one(format!("bz build: {}", e)),
            };
            let t0 = crate::arch::cpu::nanos();
            match crate::repo::http_request(&url, "POST", "build", &body, 300_000) {
                Ok((200, _, dll)) => {
                    let out = format!("{}/{}.DLL", dir.trim_end_matches('/'), name);
                    if let Err(e) = crate::vfs::write(&out, &dll) {
                        return one(format!("bz build: {}: {}", out, e));
                    }
                    let ms = (crate::arch::cpu::nanos() - t0) / 1_000_000;
                    let mut lines = alloc::vec![format!("bz build: {} ({} berkas .CS -> {} byte, {} ms)", out, files, dll.len(), ms)];
                    if rest.contains(&"--run") {
                        match crate::app::spawn_with_args(&out, "") {
                            Ok(pid) => {
                                crate::terminal::spawned(pid);
                                lines.push(format!("app '{}' berjalan (pid {})", out, pid));
                            }
                            Err(e) => lines.push(format!("bz build: run {}: {}", out, e)),
                        }
                    }
                    lines
                }
                Ok((_, status, text)) => {
                    let mut lines = alloc::vec![format!("bz build: gagal ({})", status)];
                    lines.extend(String::from_utf8_lossy(&text).lines().filter(|l| !l.trim().is_empty()).map(String::from));
                    lines
                }
                Err(e) => one(format!("bz build: {}", e)),
            }
        }
        _ => one(String::from("bz buildsrv [URL|off] | bz build DIR [--run]")),
    }
}
