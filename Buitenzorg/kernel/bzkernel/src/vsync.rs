//! Vertical blank (generic): `WIN_CTL VSYNC_WAIT` blocks a program until the
//! next vblank, so a game presents once per display refresh instead of as
//! fast as it can (no tearing between two frames of its own, no wasted CPU).
//!
//! The vblank comes from the display hardware when a driver installed a
//! counter (`install`: Intel Gen9 pipe A's frame counter); otherwise it is a
//! virtual 60 Hz clock on the CPU counter -- what every virtual GPU (QEMU's
//! VGA, virtio-gpu, ramfb, the Pi's mailbox framebuffer) gives us, since none
//! of them reports scan-out timing.

use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

/// A hardware frame counter and the refresh rate it runs at (millihertz).
struct Source {
    counter: fn() -> Option<u32>,
    refresh_mhz: u32,
    name: &'static str,
}

static SOURCE: Mutex<Option<Source>> = Mutex::new(None);
/// Waits served (statistics for `gpu` / tests).
static WAITS: AtomicU64 = AtomicU64::new(0);

const DEFAULT_MHZ: u32 = 60_000;

/// A display driver offers its frame counter (incremented at every vblank).
pub fn install(counter: fn() -> Option<u32>, refresh_mhz: u32, name: &'static str) {
    let mhz = if (20_000..=240_000).contains(&refresh_mhz) { refresh_mhz } else { DEFAULT_MHZ };
    *SOURCE.lock() = Some(Source { counter, refresh_mhz: mhz, name });
    crate::println!("[vsync] vblank from {} at {}.{:03} Hz", name, mhz / 1000, mhz % 1000);
}

/// Refresh rate in millihertz.
pub fn refresh_mhz() -> u32 {
    SOURCE.lock().as_ref().map_or(DEFAULT_MHZ, |s| s.refresh_mhz)
}

/// Where the vblank comes from.
pub fn source_name() -> &'static str {
    SOURCE.lock().as_ref().map_or("virtual 60 Hz clock", |s| s.name)
}

fn period_ns() -> u64 {
    1_000_000_000_000 / refresh_mhz() as u64
}

/// The vblank counter now (vblanks since the CPU counter started).
pub fn counter() -> u64 {
    crate::arch::cpu::nanos() / period_ns()
}

/// Sleep until the CPU counter reaches `deadline` (ns).
fn sleep_until(deadline: u64) {
    let word = 0u32; // nobody wakes it: only the clock ends the wait
    let margin = crate::task::precise_wait_margin(2_000_000);
    loop {
        let now = crate::arch::cpu::nanos();
        if now >= deadline {
            return;
        }
        if deadline - now > margin + 1_000_000 && crate::task::is_started() {
            crate::task::futex_wait_timeout_if(&word as *const u32 as u64, 0, deadline - margin);
        } else {
            crate::task::yield_now();
        }
    }
}

/// Block until the next vblank; returns the vblank counter after it.
pub fn wait() -> u64 {
    WAITS.fetch_add(1, Ordering::Relaxed);
    let period = period_ns();
    let now = crate::arch::cpu::nanos();
    let next = (now / period + 1) * period;
    let hw = SOURCE.lock().as_ref().map(|s| s.counter);
    match hw.and_then(|f| f().map(|c| (f, c))) {
        Some((f, start)) => {
            // sleep to just before the expected edge, then watch the counter
            sleep_until(next.saturating_sub(1_500_000));
            let limit = crate::arch::cpu::nanos() + 2 * period;
            while f().is_some_and(|c| c == start) && crate::arch::cpu::nanos() < limit {
                crate::task::yield_now();
            }
        }
        None => sleep_until(next),
    }
    counter()
}

/// `WIN_CTL VSYNC_INFO`: refresh (mHz) | counter << 32.
pub fn info() -> u64 {
    refresh_mhz() as u64 | (counter() & 0xFFFF_FFFF) << 32
}

/// Waits served so far.
pub fn waits() -> u64 {
    WAITS.load(Ordering::Relaxed)
}

/// Self-test boot: five waits each land on a new vblank (the counter moves
/// by one, or more when the emulator was late) and none returns before the
/// vblank that followed its own start. (Their total proves nothing: when the
/// aligning wait came back late, five waits take less than five periods.)
pub fn self_test() -> bool {
    let period = period_ns();
    // a display's own edge is not in step with the CPU counter's
    let slack = if SOURCE.lock().is_some() { 2_000_000 } else { 0 };
    let mut last = wait();
    let t0 = crate::arch::cpu::nanos();
    let mut steps = 0u64;
    let mut ok = true;
    for _ in 0..5 {
        let edge = (crate::arch::cpu::nanos() / period + 1) * period;
        let c = wait();
        if c <= last || crate::arch::cpu::nanos() + slack < edge {
            ok = false;
        }
        steps += c - last;
        last = c;
    }
    let ms = (crate::arch::cpu::nanos() - t0) / 1_000_000;
    ok &= steps >= 5;
    if ok {
        crate::println!(
            "[kernel] MILESTONE: VSYNC OK ({}, {}.{:03} Hz, 5 waits in {} ms, {} vblank(s))",
            source_name(),
            refresh_mhz() / 1000,
            refresh_mhz() % 1000,
            ms,
            steps
        );
    } else {
        crate::println!("[vsync] self-test failed: 5 waits in {} ms, {} vblank(s)", ms, steps);
    }
    ok
}
