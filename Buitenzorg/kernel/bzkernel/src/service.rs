//! Service / init manager (v0.5 "Dahan"): dependency-aware, parallel service
//! startup on top of the kernel scheduler. Each service runs as its own task;
//! a service only starts once all of its dependencies report `Running`.
//!
//! This is the "parallel init, dependency-aware" fast-boot policy from
//! requirements.md §8.2 in miniature.

use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicU8, Ordering};
use spin::Mutex;

use crate::task;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum State {
    Pending = 0,
    Running = 1,
    Done = 2,
}

impl State {
    fn from_u8(v: u8) -> State {
        match v {
            1 => State::Running,
            2 => State::Done,
            _ => State::Pending,
        }
    }
}

struct Service {
    name: &'static str,
    deps: &'static [&'static str],
    entry: fn(),
    state: AtomicU8,
    started: bool,
}

static SERVICES: Mutex<Vec<Service>> = Mutex::new(Vec::new());

/// Register a service with its dependency names and a run function. The run
/// function should call [`mark_running`] once it is up, then do its work.
pub fn register(name: &'static str, deps: &'static [&'static str], entry: fn()) {
    SERVICES.lock().push(Service {
        name,
        deps,
        entry,
        state: AtomicU8::new(State::Pending as u8),
        started: false,
    });
}

/// A service calls this from its entry once initialized and ready to serve.
pub fn mark_running(name: &str) {
    let services = SERVICES.lock();
    if let Some(s) = services.iter().find(|s| s.name == name) {
        s.state.store(State::Running as u8, Ordering::SeqCst);
    }
}

/// A service calls this when it finishes (for oneshot services).
pub fn mark_done(name: &str) {
    let services = SERVICES.lock();
    if let Some(s) = services.iter().find(|s| s.name == name) {
        s.state.store(State::Done as u8, Ordering::SeqCst);
    }
}

/// Run the dependency-ordered, parallel startup to completion. Returns the
/// order in which services transitioned to Running (for diagnostics/tests).
pub fn start_all() -> Vec<String> {
    let mut order: Vec<String> = Vec::new();
    let total = SERVICES.lock().len();

    loop {
        // Snapshot (name, deps, started, state) under a single lock, so
        // readiness is decided without re-entering the (non-reentrant) lock.
        let snapshot: Vec<(usize, &'static str, &'static [&'static str], bool, State, fn())> = {
            let services = SERVICES.lock();
            services
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    (
                        i,
                        s.name,
                        s.deps,
                        s.started,
                        State::from_u8(s.state.load(Ordering::SeqCst)),
                        s.entry,
                    )
                })
                .collect()
        };

        let state_of = |name: &str| -> State {
            snapshot
                .iter()
                .find(|(_, n, _, _, _, _)| *n == name)
                .map(|(_, _, _, _, st, _)| *st)
                .unwrap_or(State::Pending)
        };

        // Spawn every not-yet-started service whose deps are all at least Running.
        let mut spawned_any = false;
        for (i, _name, deps, started, _state, entry) in &snapshot {
            if *started {
                continue;
            }
            if deps.iter().all(|d| state_of(d) != State::Pending) {
                SERVICES.lock()[*i].started = true;
                task::spawn(*entry);
                spawned_any = true;
            }
        }

        // Record services that have reached at least Running.
        for (_, name, _, _, state, _) in &snapshot {
            if *state != State::Pending && !order.iter().any(|o| o == name) {
                order.push(String::from(*name));
            }
        }

        if order.len() >= total {
            break;
        }
        if !spawned_any {
            task::yield_now();
        }
        task::yield_now();
    }
    order
}

/// True once every registered service is at least Running.
pub fn all_up() -> bool {
    let services = SERVICES.lock();
    services
        .iter()
        .all(|s| State::from_u8(s.state.load(Ordering::SeqCst)) != State::Pending)
}
