//! Process registry (v0.9 "Serbuk", multi-process since increment 1): a
//! unified view of kernel tasks plus every running ring-3 program, backing the
//! PROC_LIST / PROC_KILL / SYS_STAT syscalls and the Task Manager app.
//!
//! A program's pid is the id of its address space (`aspace.rs`); all of its
//! threads are scheduler tasks tagged with that id, and they are listed once,
//! as the program. Programs are either *attached* (run synchronously by the
//! boot demos via `app::run_named`, on the launching task) or *detached*
//! (spawned as their own process via `app::spawn_named`).

use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use bz_abi::{proc_state, ProcInfo, SysStat};

struct App {
    pid: u64,
    name: [u8; 32],
    cpu_ticks: u64,
    /// Spawned as its own process (killable); false for synchronous launches.
    detached: bool,
    /// Argument string handed to PROC_SPAWN / `run <app> <args>`.
    args: String,
}

static APPS: Mutex<Vec<App>> = Mutex::new(Vec::new());
/// Exit codes of recently finished programs, for `exit_code`.
static EXITED: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
static NEXT_APP_PID: AtomicU64 = AtomicU64::new(1000);

fn copy_name(name: &str) -> [u8; 32] {
    let mut buf = [0u8; 32];
    for (i, b) in name.bytes().take(31).enumerate() {
        buf[i] = b;
    }
    buf
}

/// Register a launching program. Returns its pid.
pub fn app_start(name: &str, detached: bool) -> u64 {
    let pid = NEXT_APP_PID.fetch_add(1, Ordering::SeqCst);
    APPS.lock().push(App {
        pid,
        name: copy_name(name),
        cpu_ticks: 0,
        detached,
        args: String::new(),
    });
    pid
}

/// Futex key a PROC_WAIT on `pid` sleeps on. Far above every user address
/// (user memory ends at 0x8000_0000), so it can never collide with a real futex.
pub fn wait_key(pid: u64) -> u64 {
    0xB2_0000_0000_0000 | pid
}

/// Set the argument string of a just-started program.
pub fn set_args(pid: u64, args: &str) {
    if let Some(a) = APPS.lock().iter_mut().find(|a| a.pid == pid) {
        a.args = String::from(args);
    }
}

/// Argument string of program `pid` (empty if none / unknown).
pub fn args(pid: u64) -> String {
    APPS.lock().iter().find(|a| a.pid == pid).map(|a| a.args.clone()).unwrap_or_default()
}

/// Remove a program from the registry and remember its exit code.
pub fn app_exit(pid: u64, code: u64) {
    APPS.lock().retain(|a| a.pid != pid);
    let mut ex = EXITED.lock();
    if ex.len() >= 32 {
        ex.remove(0);
    }
    ex.push((pid, code));
    drop(ex);
    // Wake every PROC_WAIT sleeping on this program.
    crate::task::futex_wake(wait_key(pid), u64::MAX);
}

/// True while program `pid` is registered (running).
pub fn is_alive(pid: u64) -> bool {
    pid != 0 && APPS.lock().iter().any(|a| a.pid == pid)
}

/// True if `pid` is a running program spawned as its own process.
pub fn is_detached(pid: u64) -> bool {
    APPS.lock().iter().any(|a| a.pid == pid && a.detached)
}

/// Exit code of a recently finished program, if known.
pub fn exit_code(pid: u64) -> Option<u64> {
    EXITED.lock().iter().rev().find(|(p, _)| *p == pid).map(|(_, c)| *c)
}

/// CPU ticks charged to program `pid` so far.
pub fn cpu_ticks(pid: u64) -> u64 {
    APPS.lock().iter().find(|a| a.pid == pid).map(|a| a.cpu_ticks).unwrap_or(0)
}

/// Number of running programs.
pub fn app_count() -> usize {
    APPS.lock().len()
}

/// Charge a timer tick to the program the running task belongs to (called
/// from the timer IRQ, hence the `try_lock`s: a tick is dropped rather than
/// risking a deadlock against kernel code holding a lock). Deliberately not
/// "whichever space is installed": the launcher installs a new program's space
/// while loading it, and that time is not the program's.
pub fn account_tick() {
    let pid = crate::task::current_space_id_irq().unwrap_or(0);
    if pid == 0 {
        return;
    }
    if let Some(mut apps) = APPS.try_lock() {
        if let Some(a) = apps.iter_mut().find(|a| a.pid == pid) {
            a.cpu_ticks += 1;
        }
    }
}

/// Fill `out` with process descriptors (kernel tasks + running programs).
/// Returns the count written.
pub fn list(out: &mut [ProcInfo]) -> usize {
    let mut n = 0;
    let current = crate::aspace::current_id();
    for t in crate::task::list() {
        if n >= out.len() {
            break;
        }
        // Skip finished tasks, and program threads (listed as their program).
        if t.finished || t.space_id != 0 {
            continue;
        }
        out[n] = ProcInfo {
            pid: t.id,
            state: if t.running {
                proc_state::RUNNING
            } else {
                proc_state::RUNNABLE
            },
            cpu_ticks: t.cpu_ticks,
            kind: 0, // kernel task
            name: copy_name(t.name),
        };
        n += 1;
    }
    for app in APPS.lock().iter() {
        if n >= out.len() {
            break;
        }
        out[n] = ProcInfo {
            pid: app.pid,
            state: if app.pid == current {
                proc_state::RUNNING
            } else {
                proc_state::RUNNABLE
            },
            cpu_ticks: app.cpu_ticks,
            kind: 1, // user app
            name: app.name,
        };
        n += 1;
    }
    n
}

/// Terminate a process by pid. Spawned programs are killed through
/// `app::kill`; a program launched synchronously runs on its launcher's task
/// and cannot be killed from outside. Kernel task ids route to `task::kill`.
pub fn kill(pid: u64) -> bool {
    let target = APPS.lock().iter().find(|a| a.pid == pid).map(|a| a.detached);
    match target {
        Some(true) => crate::app::kill(pid),
        Some(false) => false,
        None => crate::task::kill(pid),
    }
}

/// Current system statistics.
pub fn stat() -> SysStat {
    let (heap_used, heap_total) = crate::allocator::stats();
    let task_count = crate::task::list()
        .iter()
        .filter(|t| !t.finished && t.space_id == 0)
        .count() as u64
        + app_count() as u64;
    SysStat {
        uptime_ticks: crate::interrupts::ticks(),
        tick_hz: 18, // PIT default ~18.2 Hz
        heap_used: heap_used as u64,
        heap_total: heap_total as u64,
        task_count,
        mem_total_mib: crate::memory::total_usable_mib(),
    }
}
