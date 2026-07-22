//! Process registry (v0.9 "Serbuk"): a unified view of kernel tasks plus the
//! running ring-3 user app, backing the PROC_LIST / PROC_KILL / SYS_STAT
//! syscalls and the Task Manager app.
//!
//! The ring-3 model is still single-process: at most one user app runs at a
//! time (tracked here). Kernel tasks come from [`crate::task`].

use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use bz_abi::{proc_state, ProcInfo, SysStat};

/// The currently running user app, if any.
struct RunningApp {
    pid: u64,
    name: [u8; 32],
}

static APP: Mutex<Option<RunningApp>> = Mutex::new(None);
static APP_CPU: AtomicU64 = AtomicU64::new(0);
static NEXT_APP_PID: AtomicU64 = AtomicU64::new(1000);

fn copy_name(name: &str) -> [u8; 32] {
    let mut buf = [0u8; 32];
    for (i, b) in name.bytes().take(31).enumerate() {
        buf[i] = b;
    }
    buf
}

/// Register a launching app as a process. Returns its pid.
pub fn app_start(name: &str) -> u64 {
    let pid = NEXT_APP_PID.fetch_add(1, Ordering::SeqCst);
    APP_CPU.store(0, Ordering::SeqCst);
    *APP.lock() = Some(RunningApp {
        pid,
        name: copy_name(name),
    });
    pid
}

/// Clear the running-app record (on app exit).
pub fn app_exit() {
    *APP.lock() = None;
}

/// Charge a timer tick to the running app (called from the timer IRQ). Since
/// the app runs in ring 3 while the boot task is blocked in `enter_user`, its
/// CPU time is the wall time it is on-CPU.
pub fn account_tick() {
    if APP.lock().is_some() {
        APP_CPU.fetch_add(1, Ordering::Relaxed);
    }
}

/// Fill `out` with process descriptors (kernel tasks + running app). Returns
/// the count written.
pub fn list(out: &mut [ProcInfo]) -> usize {
    let mut n = 0;
    for t in crate::task::list() {
        if n >= out.len() {
            break;
        }
        // Skip finished kernel tasks to keep the listing clean.
        if t.finished {
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
    if let Some(app) = APP.lock().as_ref() {
        if n < out.len() {
            out[n] = ProcInfo {
                pid: app.pid,
                state: proc_state::RUNNING,
                cpu_ticks: APP_CPU.load(Ordering::Relaxed),
                kind: 1, // user app
                name: app.name,
            };
            n += 1;
        }
    }
    n
}

/// Terminate a process by pid. Kernel tasks route to [`crate::task::kill`];
/// the running app cannot be force-killed from here yet (single-process,
/// synchronous), so killing it is refused with a note.
pub fn kill(pid: u64) -> bool {
    if let Some(app) = APP.lock().as_ref() {
        if app.pid == pid {
            // The app runs synchronously inside the launcher; async kill needs
            // the multi-process model (see PLAN.md backlog).
            return false;
        }
    }
    crate::task::kill(pid)
}

/// Current system statistics.
pub fn stat() -> SysStat {
    let (heap_used, heap_total) = crate::allocator::stats();
    let task_count = crate::task::list().iter().filter(|t| !t.finished).count() as u64
        + APP.lock().is_some() as u64;
    SysStat {
        uptime_ticks: crate::interrupts::ticks(),
        tick_hz: 18, // PIT default ~18.2 Hz
        heap_used: heap_used as u64,
        heap_total: heap_total as u64,
        task_count,
        mem_total_mib: crate::memory::total_usable_mib(),
    }
}

