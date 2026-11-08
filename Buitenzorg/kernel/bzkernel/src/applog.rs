//! Per-program logs: what each program printed (DEBUG_WRITE), kept per
//! process -- the last 8 KiB of each of the last 24 programs, also after they
//! ended. Shell `logs [NAME|PID] [--since N]`, from a PC `bz logs APP [-f]`.
//! (The system log `dmesg` / `bz logs` keeps everything mixed together.)

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

const KEEP_BYTES: usize = 8 * 1024;
const KEEP_PROGRAMS: usize = 24;

struct Log {
    pid: u64,
    name: String,
    text: VecDeque<u8>,
    /// Bytes ever written (`--since` offsets count these).
    total: u64,
    exit: Option<u64>,
}

static LOGS: Mutex<Vec<Log>> = Mutex::new(Vec::new());

/// A program printed `text` (the DEBUG_WRITE syscall).
pub fn note(pid: u64, text: &str) {
    if pid == 0 {
        return;
    }
    let mut logs = LOGS.lock();
    let i = match logs.iter().position(|l| l.pid == pid) {
        Some(i) => i,
        None => {
            if logs.len() >= KEEP_PROGRAMS {
                // forget the oldest that ended, else the oldest
                let drop = logs.iter().position(|l| l.exit.is_some()).unwrap_or(0);
                logs.remove(drop);
            }
            let name = crate::process::name_of(pid).unwrap_or_else(|| format!("pid{}", pid));
            logs.push(Log { pid, name, text: VecDeque::new(), total: 0, exit: None });
            logs.len() - 1
        }
    };
    let l = &mut logs[i];
    l.text.extend(text.bytes());
    l.total += text.len() as u64;
    while l.text.len() > KEEP_BYTES {
        l.text.pop_front();
    }
}

/// The program ended (the process registry).
pub fn on_exit(pid: u64, code: u64) {
    if let Some(l) = LOGS.lock().iter_mut().find(|l| l.pid == pid) {
        l.exit = Some(code);
    }
}

/// `logs`, `logs NAME|PID [--since N]`.
pub fn command(args: &[&str]) -> Vec<String> {
    let logs = LOGS.lock();
    let Some(&which) = args.first() else {
        if logs.is_empty() {
            return alloc::vec![String::from("logs: belum ada program yang mencetak sesuatu")];
        }
        let mut out = alloc::vec![String::from("   pid  program           byte  status")];
        for l in logs.iter() {
            let st = match l.exit {
                None => String::from("berjalan"),
                Some(c) => format!("selesai ({:#x})", c),
            };
            out.push(format!("{:>6}  {:<16} {:>6}  {}", l.pid, l.name, l.total, st));
        }
        out.push(String::from("logs NAMA|PID = keluarannya (yang terbaru dengan nama itu)"));
        return out;
    };
    let since = args.iter().position(|a| *a == "--since").and_then(|i| args.get(i + 1)).and_then(|n| n.parse::<u64>().ok());
    let found = match which.parse::<u64>() {
        Ok(pid) => logs.iter().find(|l| l.pid == pid),
        Err(_) => logs.iter().rev().find(|l| l.name.eq_ignore_ascii_case(which) || l.name.to_ascii_lowercase().starts_with(&which.to_ascii_lowercase())),
    };
    let Some(l) = found else {
        return alloc::vec![format!("logs: tidak ada log untuk '{}' ('logs' mendaftar programnya)", which)];
    };
    let start = l.total - l.text.len() as u64;
    let from = since.unwrap_or(start).max(start).min(l.total);
    let bytes: Vec<u8> = l.text.iter().skip((from - start) as usize).copied().collect();
    let mut out = Vec::new();
    if since.is_some() {
        // for `bz logs APP -f`: where the next call continues, and whether it ended
        out.push(format!("@next {} {}", l.total, l.exit.map_or(String::from("running"), |c| format!("exit {:#x}", c))));
    }
    out.extend(String::from_utf8_lossy(&bytes).lines().map(String::from));
    out
}
