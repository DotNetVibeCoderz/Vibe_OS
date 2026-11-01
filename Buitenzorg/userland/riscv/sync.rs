//! A futex-backed mutex under contention: two workers enter a critical
//! section 200 times each; inside, each stamps its id, yields to the other,
//! and checks the stamp is unchanged (real mutual exclusion), then bumps a
//! plain (non-atomic) counter. Waiters block in FUTEX_WAIT.
#![no_std]
#![no_main]
mod rt;
use core::sync::atomic::{AtomicU64, Ordering};
use rt::*;

static LOCK: Mutex = Mutex::new();
static OWNER: AtomicU64 = AtomicU64::new(0);
static VIOLATIONS: AtomicU64 = AtomicU64::new(0);
static mut COUNT: u64 = 0;

extern "C" fn worker(id: u64) -> ! {
    for _ in 0..200 {
        LOCK.lock();
        OWNER.store(id, Ordering::Relaxed);
        yield_now();
        if OWNER.load(Ordering::Relaxed) != id {
            VIOLATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe {
            let c = core::ptr::addr_of_mut!(COUNT);
            c.write_volatile(c.read_volatile() + 1);
        }
        LOCK.unlock();
    }
    thread_exit()
}

fn main(_arg: u64) -> u64 {
    let a = spawn(worker, 1);
    let b = spawn(worker, 2);
    join(a);
    join(b);
    let count = unsafe { core::ptr::addr_of!(COUNT).read_volatile() };
    let bad = VIOLATIONS.load(Ordering::Relaxed);
    print("sync: 2 workers x 200 critical sections: count ");
    print_u64(count);
    print(", violations ");
    print_u64(bad);
    print("\n");
    if count == 400 && bad == 0 { 0 } else { 1 }
}
