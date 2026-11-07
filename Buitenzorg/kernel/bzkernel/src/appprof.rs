//! Per-app sampling profiler (`prof app NAME`, `bz prof` from a PC).
//!
//! While one process is profiled, every timer interrupt on every CPU that
//! lands in that process records where it was: the user pc when the timer
//! came from ring 3, a "kernel" tick when it came during one of the
//! program's syscalls. The report groups the pcs by the function symbols the
//! kernel keeps for crash reports (`crash.rs`, from the ELF's `.symtab`) and
//! lists the hottest functions with their share of the samples.
//!
//! Sampling runs inside the timer interrupt, so it takes no lock that could
//! block and allocates nothing: the pcs go into a fixed array of atomics.
//! The rate is the timer's (100 Hz on ARM64 / RISC-V and on x86 secondary
//! CPUs, 18.2 Hz on x86 CPU 0): a profile wants a program that runs for a
//! second or more. The report is built at `prof stop`, or when the program
//! ends (`crash::on_exit` calls [`on_exit`] while its symbols still exist).

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

const MAX_SAMPLES: usize = 16384;

/// The profiled process (0 = off).
static TARGET: AtomicU64 = AtomicU64::new(0);
static PCS: [AtomicU64; MAX_SAMPLES] = [const { AtomicU64::new(0) }; MAX_SAMPLES];
static COUNT: AtomicUsize = AtomicUsize::new(0);
static KERNEL: AtomicU64 = AtomicU64::new(0);
static STARTED_NS: AtomicU64 = AtomicU64::new(0);
/// The last finished report.
static LAST: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Timer interrupt (interrupts off, any CPU): one sample if the profiled
/// program is the one running here.
pub fn sample(pc: u64, from_user: bool) {
    let t = TARGET.load(Ordering::Relaxed);
    if t == 0 || crate::task::current_space_id_irq() != Some(t) {
        return;
    }
    if !from_user {
        KERNEL.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let i = COUNT.fetch_add(1, Ordering::Relaxed);
    if i < MAX_SAMPLES {
        PCS[i].store(pc, Ordering::Relaxed);
    }
}

/// Start profiling `pid` (drops samples of an earlier profile).
pub fn start(pid: u64) {
    TARGET.store(0, Ordering::SeqCst);
    COUNT.store(0, Ordering::SeqCst);
    KERNEL.store(0, Ordering::SeqCst);
    STARTED_NS.store(crate::arch::cpu::nanos(), Ordering::SeqCst);
    TARGET.store(pid, Ordering::SeqCst);
}

pub fn target() -> u64 {
    TARGET.load(Ordering::Relaxed)
}

/// The report so far (the program's symbols must still exist).
pub fn report(pid: u64) -> Vec<String> {
    let n = COUNT.load(Ordering::SeqCst).min(MAX_SAMPLES);
    let kernel = KERNEL.load(Ordering::SeqCst);
    let ms = crate::arch::cpu::nanos().saturating_sub(STARTED_NS.load(Ordering::SeqCst)) / 1_000_000;
    let mut by_fn: BTreeMap<String, u64> = BTreeMap::new();
    for pc in PCS.iter().take(n) {
        let pc = pc.load(Ordering::Relaxed);
        let name = match crate::crash::symbol_of(pid, pc) {
            Some(s) => match s.rfind('+') {
                Some(p) => String::from(&s[..p]),
                None => s,
            },
            None => String::from("(tanpa simbol)"),
        };
        *by_fn.entry(name).or_insert(0) += 1;
    }
    let total = n as u64 + kernel;
    let name = crate::process::name_of(pid).unwrap_or_default();
    let mut out = alloc::vec![format!(
        "profil pid {} '{}': {} sampel dalam {} ms ({} di program, {} di kernel/syscall)",
        pid, name, total, ms, n, kernel
    )];
    if total == 0 {
        out.push(String::from("  (belum ada sampel: program belum mendapat CPU, atau terlalu singkat)"));
        return out;
    }
    let mut rows: Vec<(String, u64)> = by_fn.into_iter().collect();
    if kernel > 0 {
        rows.push((String::from("[kernel: syscall]"), kernel));
    }
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out.push(String::from("   sampel     %  fungsi"));
    for (f, c) in rows.iter().take(20) {
        let pm = c * 1000 / total;
        out.push(format!("  {:>7} {:>3}.{}  {}", c, pm / 10, pm % 10, f));
    }
    if rows.len() > 20 {
        out.push(format!("  ... {} fungsi lain", rows.len() - 20));
    }
    out
}

/// Stop profiling; the report.
pub fn stop() -> Vec<String> {
    let pid = TARGET.swap(0, Ordering::SeqCst);
    if pid == 0 {
        return LAST.lock().clone().unwrap_or_else(|| alloc::vec![String::from("prof: tidak ada profil (prof app NAMA | prof pid N)")]);
    }
    let r = report(pid);
    *LAST.lock() = Some(r.clone());
    r
}

/// The profiled program ended: keep its report (its symbols go next).
pub fn on_exit(pid: u64) {
    if pid == 0 || TARGET.load(Ordering::SeqCst) != pid {
        return;
    }
    let r = report(pid);
    TARGET.store(0, Ordering::SeqCst);
    for l in &r {
        crate::println!("[prof] {}", l);
    }
    *LAST.lock() = Some(r);
}

/// The last finished report.
pub fn last() -> Option<Vec<String>> {
    LAST.lock().clone()
}

/// Shell: `prof app NAME [args]`, `prof pid N`, `prof show`, `prof stop`, `prof last`.
pub fn command(args: &[&str]) -> Vec<String> {
    let one = |s: String| alloc::vec![s];
    match args {
        ["app", name, rest @ ..] => match crate::app::spawn_with_args(name, &rest.join(" ")) {
            Ok(pid) => {
                start(pid);
                one(format!("prof: memprofil pid {} '{}' ('prof stop' atau tunggu ia selesai, lalu 'prof last')", pid, name))
            }
            Err(e) => one(format!("prof: {}", e)),
        },
        ["pid", p] => match p.parse::<u64>() {
            Ok(pid) if crate::process::is_alive(pid) => {
                start(pid);
                one(format!("prof: memprofil pid {}", pid))
            }
            _ => one(format!("prof: tidak ada proses {}", p)),
        },
        ["show"] => match target() {
            0 => one(String::from("prof: tidak sedang memprofil")),
            pid => report(pid),
        },
        ["stop"] => stop(),
        ["last"] => last().unwrap_or_else(|| one(String::from("prof: belum ada profil yang selesai"))),
        _ => alloc::vec![
            String::from("prof app NAMA [arg]   jalankan program dan profil (sampel timer per fungsi)"),
            String::from("prof pid N            profil proses yang sedang berjalan"),
            String::from("prof show | stop      laporan sejauh ini / berhenti + laporan"),
            String::from("prof last             laporan terakhir (juga dicetak saat program selesai)"),
        ],
    }
}

/// Boot self-test: profile `app spin` (a program that spends most of its time
/// in `hot` and a little in `cold`) and require `hot` to dominate.
pub fn self_test(app: &str, hot: &str, cold: &str) -> bool {
    let started = command(&["app", app, "spin"]).join(" ");
    let Some(pid) = started.split_whitespace().nth(3).and_then(|p| p.parse::<u64>().ok()) else {
        crate::println!("[prof] self-test: {}", started);
        return false;
    };
    let code = crate::app::wait(pid);
    let r = last().unwrap_or_default();
    let count = |name: &str| -> u64 {
        r.iter()
            .filter(|l| l.ends_with(name) || l.contains(&format!("_{}", name)) || l.contains(&format!("::{}", name)))
            .filter_map(|l| l.split_whitespace().next()?.parse::<u64>().ok())
            .sum()
    };
    let total: u64 = r.first().and_then(|l| l.split(": ").nth(1)?.split_whitespace().next()?.parse().ok()).unwrap_or(0);
    let (h, c) = (count(hot), count(cold));
    let ok = code == 0 && total >= 10 && h * 2 >= total && h > c && c > 0;
    crate::println!("[prof] self-test: exit {}, {} samples, {} in {}, {} in {}", code, total, h, hot, c, cold);
    if ok {
        crate::println!(
            "[kernel] MILESTONE: APP PROFILER OK (pid {}: {} samples, {} {}%, {} {}%)",
            pid,
            total,
            hot,
            h * 100 / total.max(1),
            cold,
            c * 100 / total.max(1)
        );
    }
    ok
}
