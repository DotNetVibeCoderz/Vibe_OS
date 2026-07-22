//! Minimal kernel IPC: bounded message-passing channel between tasks
//! (v0.2 "Akar": IPC message passing, first cut — kernel-space only).

use alloc::collections::VecDeque;
use spin::Mutex;
use x86_64::instructions::interrupts;

use crate::task;

const CAPACITY: usize = 64;

/// A FIFO of `u64` messages. Blocking ops cooperate with the scheduler by
/// yielding while full/empty.
pub struct Channel {
    queue: Mutex<VecDeque<u64>>,
}

impl Channel {
    pub const fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
        }
    }

    pub fn try_send(&self, msg: u64) -> bool {
        interrupts::without_interrupts(|| {
            let mut q = self.queue.lock();
            if q.len() >= CAPACITY {
                return false;
            }
            q.push_back(msg);
            true
        })
    }

    pub fn try_recv(&self) -> Option<u64> {
        interrupts::without_interrupts(|| self.queue.lock().pop_front())
    }

    /// Send, yielding to other tasks while the channel is full.
    pub fn send(&self, msg: u64) {
        while !self.try_send(msg) {
            task::yield_now();
        }
    }

    /// Receive, yielding to other tasks while the channel is empty.
    pub fn recv(&self) -> u64 {
        loop {
            if let Some(msg) = self.try_recv() {
                return msg;
            }
            task::yield_now();
        }
    }
}
