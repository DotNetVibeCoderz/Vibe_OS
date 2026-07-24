//! Instrumented profiler (v1.0 "Buitenzorg": debugger + profiler).
//!
//! A lightweight, deterministic zone profiler: wrap a scope in
//! [`Guard::new("name")`] and the elapsed CPU cycles (from the timestamp
//! counter) accumulate into a per-name bucket. [`report`] dumps a sorted table
//! — calls, total/avg/min/max cycles, and share of the profiled total.
//!
//! Design notes:
//! * **Inert unless enabled.** [`Guard::new`] does nothing but read one atomic
//!   when profiling is off, so instrumentation left in the tree never perturbs
//!   normal boot timing. Turn it on with [`enable`] around the region of
//!   interest and off with [`disable`].
//! * **Deterministic**, not statistical: it measures the actual inclusive
//!   wall-cycles of each instrumented scope, so a headless run can assert exact
//!   call counts and relative costs (unlike a sampling profiler).
//! * **Single-core / cooperative.** The registry is behind a spin lock taken
//!   with interrupts disabled, so it is safe against the timer IRQ; it is not
//!   meant to be called from interrupt handlers.
//! * TSC is not serialized here — a stray reorder costs a few cycles of noise,
//!   irrelevant next to the scopes this profiles (syscalls, compositor passes,
//!   ELF loads).

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use x86_64::instructions::interrupts::without_interrupts;

/// Maximum number of distinct zone names tracked. Extra names past this are
/// counted in `overflow` and reported once, so the profiler degrades loudly
/// rather than silently dropping data.
const MAX_ZONES: usize = 64;

static ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
struct Zone {
    name: &'static str,
    calls: u64,
    total: u64,
    min: u64,
    max: u64,
}

struct Registry {
    zones: [Option<Zone>; MAX_ZONES],
    len: usize,
    overflow: u64,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    zones: [None; MAX_ZONES],
    len: 0,
    overflow: 0,
});

#[inline(always)]
fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// Start profiling. Cheap; existing accumulations are kept (call [`reset`] for
/// a clean slate).
pub fn enable() {
    ENABLED.store(true, Ordering::SeqCst);
}

/// Stop profiling. Zones already open finish recording (their `Drop` still
/// runs), but no new ones are measured.
pub fn disable() {
    ENABLED.store(false, Ordering::SeqCst);
}

/// True while profiling is active.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// Clear all accumulated data.
pub fn reset() {
    without_interrupts(|| {
        let mut reg = REGISTRY.lock();
        *reg = Registry {
            zones: [None; MAX_ZONES],
            len: 0,
            overflow: 0,
        };
    });
}

fn record(name: &'static str, cycles: u64) {
    without_interrupts(|| {
        let mut reg = REGISTRY.lock();
        // Find an existing zone by name content (same literal at different call
        // sites has distinct pointers, so compare bytes, not addresses).
        for i in 0..reg.len {
            if let Some(z) = reg.zones[i].as_mut() {
                if z.name == name {
                    z.calls += 1;
                    z.total = z.total.wrapping_add(cycles);
                    if cycles < z.min {
                        z.min = cycles;
                    }
                    if cycles > z.max {
                        z.max = cycles;
                    }
                    return;
                }
            }
        }
        if reg.len >= MAX_ZONES {
            reg.overflow += 1;
            return;
        }
        let idx = reg.len;
        reg.zones[idx] = Some(Zone {
            name,
            calls: 1,
            total: cycles,
            min: cycles,
            max: cycles,
        });
        reg.len += 1;
    });
}

/// A scope timer. Times from construction to drop and accumulates the elapsed
/// cycles under `name`. When profiling is disabled it holds no timer and its
/// drop is a no-op.
pub struct Guard {
    name: &'static str,
    start: u64,
    active: bool,
}

impl Guard {
    #[inline]
    pub fn new(name: &'static str) -> Self {
        if !ENABLED.load(Ordering::Relaxed) {
            return Guard { name, start: 0, active: false };
        }
        Guard { name, start: rdtsc(), active: true }
    }
}

impl Drop for Guard {
    #[inline]
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let elapsed = rdtsc().wrapping_sub(self.start);
        record(self.name, elapsed);
    }
}

/// Total accumulated cycles for a named zone (0 if never recorded). For tests.
pub fn zone_total(name: &str) -> u64 {
    without_interrupts(|| {
        let reg = REGISTRY.lock();
        for i in 0..reg.len {
            if let Some(z) = reg.zones[i].as_ref() {
                if z.name == name {
                    return z.total;
                }
            }
        }
        0
    })
}

/// Call count for a named zone (0 if never recorded). For tests.
pub fn zone_calls(name: &str) -> u64 {
    without_interrupts(|| {
        let reg = REGISTRY.lock();
        for i in 0..reg.len {
            if let Some(z) = reg.zones[i].as_ref() {
                if z.name == name {
                    return z.calls;
                }
            }
        }
        0
    })
}

/// Number of distinct zones recorded.
pub fn zone_count() -> usize {
    without_interrupts(|| REGISTRY.lock().len)
}

/// Print a report of all zones, sorted by total cycles descending.
pub fn report() {
    // Snapshot under the lock, then format without holding it.
    let (zones, overflow) = without_interrupts(|| {
        let reg = REGISTRY.lock();
        let mut v: alloc::vec::Vec<Zone> = alloc::vec::Vec::with_capacity(reg.len);
        for i in 0..reg.len {
            if let Some(z) = reg.zones[i] {
                v.push(z);
            }
        }
        (v, reg.overflow)
    });

    if zones.is_empty() {
        crate::println!("[profile] no zones recorded (was it enabled?)");
        return;
    }

    let grand: u64 = zones.iter().map(|z| z.total).sum();
    let mut sorted = zones;
    sorted.sort_by(|a, b| b.total.cmp(&a.total));

    crate::println!("[profile] zone report ({} zones, {} total cycles):", sorted.len(), grand);
    crate::println!(
        "[profile] {:<24} {:>8} {:>14} {:>12} {:>12} {:>6}",
        "zone", "calls", "total", "avg", "max", "share"
    );
    for z in &sorted {
        let avg = if z.calls > 0 { z.total / z.calls } else { 0 };
        // Permille (share x10) so we get one decimal without floating point.
        let permille = if grand > 0 { z.total * 1000 / grand } else { 0 };
        crate::println!(
            "[profile] {:<24} {:>8} {:>14} {:>12} {:>12} {:>4}.{}%",
            z.name,
            z.calls,
            z.total,
            avg,
            z.max,
            permille / 10,
            permille % 10
        );
    }
    if overflow > 0 {
        crate::println!("[profile] WARNING: {} zone(s) dropped (raise MAX_ZONES)", overflow);
    }
}
