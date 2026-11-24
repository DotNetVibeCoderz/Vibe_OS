//! Async I/O, io_uring-style (v0.5 "Dahan"): a submission queue (SQ) and a
//! completion queue (CQ) decouple I/O requests from their results. A submitter
//! pushes SQEs; a kernel worker task drains them, performs the I/O against a
//! block device, and pushes CQEs. This is the "async I/O (io_uring-style)"
//! foundation from requirements.md §8.3, deliberately minimal (single ring).
//!
//! Zero-copy (2026-11-24): like io_uring's registered buffers, a submitter
//! registers physically contiguous buffers once ([`register_buffers`]) and
//! then submits `ReadFixed` / `WriteFixed` with a buffer index + offset. The
//! worker hands the device a slice of that buffer, so the data lands where
//! the consumer reads it: no per-request allocation, no bounce copy. The
//! buffers' physical addresses ([`buffer_phys`]) are what a bus-master driver
//! would program. Kernel-internal for now (no ring-3 ABI yet).

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
    /// Read `sectors` sectors from `lba` into registered buffer `buf` at `buf_off`.
    ReadFixed,
    /// Write `sectors` sectors to `lba` from registered buffer `buf` at `buf_off`.
    WriteFixed,
}

#[derive(Clone, Copy)]
pub struct Sqe {
    pub user_data: u64,
    pub op: OpCode,
    pub lba: u64,
    pub buf: u32,
    pub buf_off: u32,
    pub sectors: u32,
}

impl Sqe {
    pub const fn simple(user_data: u64, op: OpCode, lba: u64) -> Sqe {
        Sqe { user_data, op, lba, buf: 0, buf_off: 0, sectors: 1 }
    }
    pub const fn fixed(user_data: u64, op: OpCode, lba: u64, buf: u32, buf_off: u32, sectors: u32) -> Sqe {
        Sqe { user_data, op, lba, buf, buf_off, sectors }
    }
}

/// A registered buffer: physically contiguous, mapped in the kernel.
#[derive(Clone, Copy)]
struct Fixed {
    phys: u64,
    virt: *mut u8,
    len: usize,
}
// the memory is owned by the ring and only touched by the worker while a
// request on it is in flight (the submitter waits for its completion)
unsafe impl Send for Fixed {}

pub struct Cqe {
    pub user_data: u64,
    pub result: i64, // >= 0 on success (bytes), < 0 on error
}

struct Ring {
    sq: VecDeque<Sqe>,
    cq: VecDeque<Cqe>,
    device: Option<Box<dyn BlockDevice>>,
    buffers: alloc::vec::Vec<Fixed>,
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
        buffers: alloc::vec::Vec::new(),
        stop: false,
    });
    task::spawn_named("aio-worker", worker);
}

/// Register `count` buffers of `pages` contiguous pages each; returns how
/// many were registered (indices continue from those already registered).
pub fn register_buffers(count: usize, pages: usize) -> usize {
    let mut got = alloc::vec::Vec::new();
    for _ in 0..count {
        match crate::memory::alloc_dma_contiguous(pages) {
            Some((phys, virt)) => got.push(Fixed { phys, virt, len: pages * 4096 }),
            None => break,
        }
    }
    let n = got.len();
    if let Some(ring) = RING.lock().as_mut() {
        ring.buffers.extend(got);
    }
    n
}

/// Physical address and length of registered buffer `i`.
pub fn buffer_phys(i: usize) -> Option<(u64, usize)> {
    RING.lock().as_ref().and_then(|r| r.buffers.get(i).map(|b| (b.phys, b.len)))
}

/// Run `f` over registered buffer `i` (kernel view). Only while no request on
/// it is in flight.
pub fn with_buffer<R>(i: usize, f: impl FnOnce(&mut [u8]) -> R) -> Option<R> {
    let b = RING.lock().as_ref().and_then(|r| r.buffers.get(i).copied())?;
    Some(f(unsafe { core::slice::from_raw_parts_mut(b.virt, b.len) }))
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
            let job = ring.sq.pop_front();
            job.map(|sqe| {
                let fixed = ring.buffers.get(sqe.buf as usize).copied();
                (sqe, fixed, ring.device.take())
            })
        };

        match job {
            Some((sqe, fixed, Some(mut device))) => {
                let result = perform(&sqe, fixed, device.as_mut());
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

fn perform(sqe: &Sqe, fixed: Option<Fixed>, device: &mut dyn BlockDevice) -> i64 {
    match sqe.op {
        OpCode::ReadFixed | OpCode::WriteFixed => {
            let Some(b) = fixed else { return -2 };
            let bytes = sqe.sectors as usize * SECTOR_SIZE;
            let off = sqe.buf_off as usize;
            if bytes == 0 || off % SECTOR_SIZE != 0 || off + bytes > b.len || sqe.lba + sqe.sectors as u64 > device.sector_count() {
                return -2;
            }
            // the registered buffer itself: the device reads/writes in place
            let data = unsafe { core::slice::from_raw_parts_mut(b.virt.add(off), bytes) };
            let r = if sqe.op == OpCode::ReadFixed {
                device.read_sectors(sqe.lba, data)
            } else {
                data.chunks_exact(SECTOR_SIZE).enumerate().try_for_each(|(i, c)| {
                    device.write_sector(sqe.lba + i as u64, c.try_into().unwrap())
                })
            };
            match r {
                Ok(()) => bytes as i64,
                Err(_) => -1,
            }
        }
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
    submit(Sqe::simple(u64::MAX, OpCode::Nop, 0));
    for i in 0..count {
        submit(Sqe::simple(i, OpCode::ReadBlock, i % sectors));
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

/// Wait for the completion carrying `user_data` (others stay queued).
fn wait_for(user_data: u64) -> i64 {
    loop {
        {
            let mut g = RING.lock();
            if let Some(r) = g.as_mut() {
                if let Some(i) = r.cq.iter().position(|c| c.user_data == user_data) {
                    return r.cq.remove(i).map(|c| c.result).unwrap_or(-1);
                }
            }
        }
        task::yield_now();
    }
}

/// Zero-copy self-test: registered buffers, a pattern written from one with
/// WriteFixed, read back into another with ReadFixed (whole and at an
/// offset), checked byte for byte, plus bounds refusals; then a throughput
/// comparison of 8-sector reads into a registered buffer vs. the copying
/// path (read into a fresh Vec, copy to the consumer's buffer).
pub fn zero_copy_self_test() -> Result<alloc::string::String, alloc::string::String> {
    use alloc::format;
    if register_buffers(2, 4) != 2 {
        return Err("could not register 2 x 16 KiB buffers".into());
    }
    let base = RING.lock().as_ref().map(|r| r.buffers.len() - 2).unwrap_or(0) as u32;
    let (src, dst) = (base, base + 1);
    with_buffer(src as usize, |b| {
        for (i, x) in b.iter_mut().enumerate() {
            *x = ((i * 7 + 3) as u8) ^ ((i >> 9) as u8);
        }
    });
    with_buffer(dst as usize, |b| b.fill(0));
    // 32 sectors = the whole 16 KiB buffer at LBA 100
    submit(Sqe::fixed(1, OpCode::WriteFixed, 100, src, 0, 32));
    if wait_for(1) != 16384 {
        return Err("WriteFixed failed".into());
    }
    submit(Sqe::fixed(2, OpCode::ReadFixed, 100, dst, 0, 32));
    if wait_for(2) != 16384 {
        return Err("ReadFixed failed".into());
    }
    let a = with_buffer(src as usize, |b| b.to_vec()).unwrap_or_default();
    let b = with_buffer(dst as usize, |b| b.to_vec()).unwrap_or_default();
    if a.is_empty() || a != b {
        return Err("data read back differs".into());
    }
    // 4 sectors from LBA 104 into the destination at offset 8 KiB
    with_buffer(dst as usize, |b| b.fill(0));
    submit(Sqe::fixed(3, OpCode::ReadFixed, 104, dst, 8192, 4));
    if wait_for(3) != 2048 {
        return Err("ReadFixed at an offset failed".into());
    }
    let b = with_buffer(dst as usize, |b| b.to_vec()).unwrap_or_default();
    if b.len() < 10240 || b[8192..10240] != a[2048..4096] || b[..8192].iter().any(|&x| x != 0) || b[10240..].iter().any(|&x| x != 0) {
        return Err("offset read landed in the wrong place".into());
    }
    // refusals: past the buffer, unaligned offset, unknown buffer
    submit(Sqe::fixed(4, OpCode::ReadFixed, 0, dst, 16384 - 512, 2));
    submit(Sqe::fixed(5, OpCode::ReadFixed, 0, dst, 100, 1));
    submit(Sqe::fixed(6, OpCode::ReadFixed, 0, 999, 0, 1));
    if wait_for(4) != -2 || wait_for(5) != -2 || wait_for(6) != -2 {
        return Err("an out-of-bounds request was not refused".into());
    }
    // throughput: registered (zero-copy) vs copying path, 400 x 4 KiB each
    const N: u64 = 400;
    let t0 = crate::arch::cpu::nanos();
    for i in 0..N {
        submit(Sqe::fixed(1000 + i, OpCode::ReadFixed, (i * 8) % 1024, dst, ((i % 4) * 4096) as u32, 8));
    }
    for i in 0..N {
        if wait_for(1000 + i) != 4096 {
            return Err("batched ReadFixed failed".into());
        }
    }
    let fixed_ns = crate::arch::cpu::nanos() - t0;
    let t1 = crate::arch::cpu::nanos();
    {
        let mut g = RING.lock();
        let r = g.as_mut().ok_or("no ring")?;
        let dev = r.device.as_mut().ok_or("device busy")?;
        let mut sink = alloc::vec![0u8; 4096];
        for i in 0..N {
            let mut tmp = alloc::vec![0u8; 4096];
            dev.read_sectors((i * 8) % 1024, &mut tmp).map_err(|e| format!("copy path: {}", e))?;
            sink.copy_from_slice(&tmp);
        }
        core::hint::black_box(&sink);
    }
    let copy_ns = crate::arch::cpu::nanos() - t1;
    Ok(format!(
        "registered buffers at phys {:#x}/{:#x}; 16 KiB write + read back exact, offset read, 3 bad requests refused; {} x 4 KiB: zero-copy {} us vs copying {} us",
        buffer_phys(src as usize).map(|b| b.0).unwrap_or(0),
        buffer_phys(dst as usize).map(|b| b.0).unwrap_or(0),
        N,
        fixed_ns / 1000,
        copy_ns / 1000
    ))
}
