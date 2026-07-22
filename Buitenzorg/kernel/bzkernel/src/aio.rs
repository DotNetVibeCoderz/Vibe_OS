//! Async I/O, io_uring-style (v0.5 "Dahan"): a submission queue (SQ) and a
//! completion queue (CQ) decouple I/O requests from their results. A submitter
//! pushes SQEs; a kernel worker task drains them, performs the I/O against a
//! block device, and pushes CQEs. This is the "async I/O (io_uring-style)"
//! foundation from requirements.md §8.3, deliberately minimal (single ring).

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::driver::{BlockDevice, SECTOR_SIZE};
use crate::task;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OpCode {
    Nop,
    ReadBlock,
}

pub struct Sqe {
    pub user_data: u64,
    pub op: OpCode,
    pub lba: u64,
}

pub struct Cqe {
    pub user_data: u64,
    pub result: i64, // >= 0 on success (bytes), < 0 on error
}

struct Ring {
    sq: VecDeque<Sqe>,
    cq: VecDeque<Cqe>,
    device: Option<Box<dyn BlockDevice>>,
    stop: bool,
}

static RING: Mutex<Option<Ring>> = Mutex::new(None);
static COMPLETED: AtomicU64 = AtomicU64::new(0);

/// Install a block device the worker reads from and start the worker task.
pub fn init(device: Box<dyn BlockDevice>) {
    *RING.lock() = Some(Ring {
        sq: VecDeque::new(),
        cq: VecDeque::new(),
        device: Some(device),
        stop: false,
    });
    task::spawn_named("aio-worker", worker);
}

/// Submit an I/O request.
pub fn submit(sqe: Sqe) {
    if let Some(ring) = RING.lock().as_mut() {
        ring.sq.push_back(sqe);
    }
}

/// Reap one completion, if available.
pub fn reap() -> Option<Cqe> {
    RING.lock().as_mut().and_then(|r| r.cq.pop_front())
}

/// Total completions processed since boot.
pub fn completed() -> u64 {
    COMPLETED.load(Ordering::Relaxed)
}

/// Ask the worker to stop (after draining current submissions).
pub fn shutdown() {
    if let Some(ring) = RING.lock().as_mut() {
        ring.stop = true;
    }
}

/// Worker task: drain the SQ, perform I/O, push the CQ.
fn worker() {
    loop {
        // Pop one SQE (and take the device out) under the lock, then release
        // it while doing the actual I/O so submitters are not blocked.
        let job = {
            let mut guard = RING.lock();
            let Some(ring) = guard.as_mut() else { return };
            if ring.stop && ring.sq.is_empty() {
                return;
            }
            ring.sq.pop_front().map(|sqe| (sqe, ring.device.take()))
        };

        match job {
            Some((sqe, Some(mut device))) => {
                let result = perform(&sqe, device.as_mut());
                let mut guard = RING.lock();
                if let Some(ring) = guard.as_mut() {
                    ring.device = Some(device);
                    ring.cq.push_back(Cqe {
                        user_data: sqe.user_data,
                        result,
                    });
                }
                COMPLETED.fetch_add(1, Ordering::Relaxed);
            }
            _ => task::yield_now(),
        }
    }
}

fn perform(sqe: &Sqe, device: &mut dyn BlockDevice) -> i64 {
    match sqe.op {
        OpCode::Nop => 0,
        OpCode::ReadBlock => {
            let mut buf = [0u8; SECTOR_SIZE];
            match device.read_sector(sqe.lba, &mut buf) {
                Ok(()) => SECTOR_SIZE as i64,
                Err(_) => -1,
            }
        }
    }
}

/// Benchmark: submit `count` ReadBlock ops across the device and drain all
/// completions, returning (ops, ticks_elapsed). Cooperates with the worker
/// task by yielding. "benchmark-able" per the v0.5 milestone.
pub fn benchmark(count: u64) -> (u64, u64) {
    let sectors = RING
        .lock()
        .as_ref()
        .and_then(|r| r.device.as_ref().map(|d| d.sector_count()))
        .unwrap_or(1)
        .max(1);

    let start_tick = crate::interrupts::ticks();
    let start_done = completed();

    // One NOP first (exercises the opcode), then `count` block reads. Each
    // completion carries back its submission's user_data so the submitter can
    // match results to requests.
    submit(Sqe { user_data: u64::MAX, op: OpCode::Nop, lba: 0 });
    for i in 0..count {
        submit(Sqe {
            user_data: i,
            op: OpCode::ReadBlock,
            lba: i % sectors,
        });
    }

    let mut reaped: u64 = 0;
    let mut user_data_xor: u64 = 0;
    while reaped <= count {
        if let Some(cqe) = reap() {
            debug_assert!(cqe.result >= 0);
            if cqe.user_data != u64::MAX {
                user_data_xor ^= cqe.user_data;
            }
            reaped += 1;
        } else {
            task::yield_now();
        }
    }
    // Every submitted user_data (0..count) must have come back exactly once.
    let expected = (0..count).fold(0u64, |a, i| a ^ i);
    debug_assert_eq!(user_data_xor, expected);

    let elapsed = crate::interrupts::ticks() - start_tick;
    let processed = completed() - start_done;
    (processed, elapsed)
}
