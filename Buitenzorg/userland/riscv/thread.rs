//! Threads in one process: the main thread and a worker share the address
//! space; each adds 1 to a shared counter 1000 times (atomic `amoadd`), the
//! main thread joins the worker, the total must be 2000.
#![no_std]
#![no_main]
mod rt;
use core::sync::atomic::{AtomicU64, Ordering};
use rt::*;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn bump() {
    for i in 0..1000 {
        COUNTER.fetch_add(1, Ordering::Relaxed);
        if i % 100 == 99 {
            yield_now();
        }
    }
}

extern "C" fn worker(_arg: u64) -> ! {
    bump();
    thread_exit()
}

fn main(_arg: u64) -> u64 {
    let id = spawn(worker, 0);
    bump();
    join(id);
    let n = COUNTER.load(Ordering::Relaxed);
    print("thread: main + worker counted ");
    print_u64(n);
    print("\n");
    if n == 2000 { 0 } else { 1 }
}
