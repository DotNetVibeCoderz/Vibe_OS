//! Intel graphics, step 3 of GPU support: the render engine (RCS) of a Gen9
//! GPU, for 3D. The plan mirrors the virgl backend of `gfx3d`: the CPU keeps
//! the vertex stage (transform + lighting), the GPU rasterizes (depth test,
//! interpolation, texture) with one small hand-assembled pixel shader and no
//! vertex shader. It is built in steps, each checked on real hardware by a
//! shell self-test, because QEMU has no Intel GPU:
//!
//! - **3a (this file so far): the engine runs.** Shell `gpu render`: wake the
//!   GT, start RCS as a legacy ring buffer (like the blitter), then three
//!   submissions -- MI_STORE_DATA_IMM (the command streamer runs),
//!   PIPE_CONTROL with a post-sync write (the 3D pipe's own flush/sync), and
//!   PIPELINE_SELECT 3D + PIPE_CONTROL (the 3D pipeline can be selected).
//! - 3b: a minimal 3D pipeline drawing one solid triangle, read back.
//! - 3c: Gouraud color, depth buffer, texture.
//! - 3d: a `gfx3d` backend (software -> virgl -> Intel), CPU fallback.

use alloc::{format, string::String, vec::Vec};
use core::ptr::{read_volatile, write_volatile};
use spin::Mutex;

const RCS: u64 = 0x2000;
const RING_TAIL: u64 = 0x30;
const RING_HEAD: u64 = 0x34;
const RING_START: u64 = 0x38;
const RING_CTL: u64 = 0x3C;
const RING_IPEHR: u64 = 0x68;
const RING_INSTDONE: u64 = 0x6C;
const RING_ACTHD: u64 = 0x74;
const RING_MI_MODE: u64 = 0x9C;
const RING_GFX_MODE: u64 = 0x29C;
const GFX_FLSH_CNTL: u64 = 0x101008;
const FORCEWAKE: [(u64, u64); 3] = [(0xA278, 0x0D84), (0xA270, 0x0D88), (0xA188, 0x130044)];

/// Our GGTT pages (the blitter uses BASE + 0, + 0x1000, + 0x10000 and up).
const RING_GGTT: u32 = 0x1000_2000;
const STATUS_GGTT: u32 = 0x1000_3000;
const RING_SIZE: u32 = 4096;

const MI_NOOP: u32 = 0;
/// MI_STORE_DATA_IMM, GGTT address, 64-bit address + one dword (Gen8+).
const MI_STORE_DATA_IMM: u32 = 0x20 << 23 | 1 << 22 | 2;
/// PIPE_CONTROL, 6 dwords (Gen8+).
const PIPE_CONTROL: u32 = 3 << 29 | 3 << 27 | 2 << 24 | 4;
const PC_GLOBAL_GTT: u32 = 1 << 24;
const PC_CS_STALL: u32 = 1 << 20;
const PC_WRITE_IMM: u32 = 1 << 14;
/// PIPELINE_SELECT (Gen9: mask bits 9:8), pipeline 0 = 3D.
const PIPELINE_SELECT_3D: u32 = 3 << 29 | 1 << 27 | 1 << 24 | 4 << 16 | 3 << 8;

struct Rcs {
    regs: u64,
    gtt: u64,
    up: bool,
    ring: (u64, *mut u8),
    status: (u64, *mut u8),
    tail: u32,
    seqno: u32,
}

// MMIO and DMA pages owned by this driver; used under RCS_STATE's lock only.
unsafe impl Send for Rcs {}

static RCS_STATE: Mutex<Option<Rcs>> = Mutex::new(None);

fn clflush(va: *const u8, len: usize) {
    let mut p = va as usize & !63;
    let end = va as usize + len;
    while p < end {
        unsafe { core::arch::asm!("clflush [{}]", in(reg) p, options(nostack, preserves_flags)) };
        p += 64;
    }
    unsafe { core::arch::asm!("mfence", options(nostack, preserves_flags)) };
}

fn now() -> u64 {
    crate::arch::cpu::timestamp()
}

fn us_since(t0: u64) -> u64 {
    now().wrapping_sub(t0) * 1_000_000 / crate::arch::cpu::counter_hz().max(1)
}

/// How a submission reports that it is done.
#[derive(Clone, Copy)]
enum Fence {
    StoreDataImm,
    PipeControl,
}

impl Rcs {
    fn rd(&self, o: u64) -> u32 {
        unsafe { read_volatile((self.regs + o) as *const u32) }
    }
    fn wr(&self, o: u64, v: u32) {
        unsafe { write_volatile((self.regs + o) as *mut u32, v) }
    }
    fn map(&self, ggtt: u32, phys: u64) {
        unsafe { write_volatile((self.gtt + (ggtt as u64 >> 12) * 8) as *mut u64, phys | 1) };
    }

    fn bring_up(&mut self, out: &mut Vec<String>) -> bool {
        let mut acks = 0;
        for (req, ack) in FORCEWAKE {
            self.wr(req, 0x0001_0001);
            let t0 = now();
            while self.rd(ack) & 1 == 0 && us_since(t0) < 50_000 {}
            acks += (self.rd(ack) & 1) as u32;
        }
        let (Some(ring), Some(status)) = (crate::memory::alloc_dma_frame(), crate::memory::alloc_dma_frame()) else {
            out.push(String::from("  out of memory"));
            return false;
        };
        unsafe {
            core::ptr::write_bytes(ring.1, 0, 4096);
            core::ptr::write_bytes(status.1, 0, 4096);
        }
        clflush(ring.1, 4096);
        clflush(status.1, 4096);
        self.map(RING_GGTT, ring.0);
        self.map(STATUS_GGTT, status.0);
        self.wr(GFX_FLSH_CNTL, 1);
        let _ = self.rd(GFX_FLSH_CNTL);
        self.ring = ring;
        self.status = status;

        let e = |r: u64| RCS + r;
        let before = (self.rd(e(RING_CTL)), self.rd(e(RING_HEAD)), self.rd(e(RING_GFX_MODE)));
        self.wr(e(RING_CTL), 0);
        self.wr(e(RING_HEAD), 0);
        self.wr(e(RING_TAIL), 0);
        self.wr(e(RING_GFX_MODE), (1 << 15) << 16); // execlists off (masked write)
        self.wr(e(RING_MI_MODE), (1 << 8) << 16); // STOP_RING off (masked write)
        self.wr(e(RING_START), RING_GGTT);
        self.wr(e(RING_CTL), (RING_SIZE - 4096) | 1);
        let t0 = now();
        while self.rd(e(RING_CTL)) & 1 == 0 && us_since(t0) < 10_000 {}
        out.push(format!(
            "  forcewake {}/3; RCS was CTL {:#x} HEAD {:#x} GFX_MODE {:#x}; now CTL {:#x} START {:#x} HEAD {:#x} MI_MODE {:#x}",
            acks,
            before.0,
            before.1,
            before.2,
            self.rd(e(RING_CTL)),
            self.rd(e(RING_START)),
            self.rd(e(RING_HEAD)),
            self.rd(e(RING_MI_MODE))
        ));
        self.tail = 0;
        self.up = self.rd(e(RING_START)) == RING_GGTT && self.rd(e(RING_CTL)) & 1 != 0;
        self.up
    }

    fn emit(&mut self, dw: u32) {
        unsafe { write_volatile(self.ring.1.add(self.tail as usize) as *mut u32, dw) };
        self.tail = (self.tail + 4) % RING_SIZE;
    }

    /// Queue `cmds`, then a fence writing the next seqno; wait for it.
    fn submit(&mut self, cmds: &[u32], fence: Fence) -> Result<u64, String> {
        let need = (cmds.len() as u32 + 8) * 4;
        if self.tail + need > RING_SIZE {
            let start = self.tail;
            while self.tail != 0 {
                self.emit(MI_NOOP);
            }
            clflush(unsafe { self.ring.1.add(start as usize) }, (RING_SIZE - start) as usize);
        }
        let start = self.tail;
        for &c in cmds {
            self.emit(c);
        }
        self.seqno = self.seqno.wrapping_add(1);
        match fence {
            Fence::StoreDataImm => {
                self.emit(MI_STORE_DATA_IMM);
                self.emit(STATUS_GGTT);
                self.emit(0);
                self.emit(self.seqno);
            }
            Fence::PipeControl => {
                self.emit(PIPE_CONTROL);
                self.emit(PC_GLOBAL_GTT | PC_CS_STALL | PC_WRITE_IMM);
                self.emit(STATUS_GGTT);
                self.emit(0);
                self.emit(self.seqno);
                self.emit(0);
            }
        }
        if self.tail % 8 != 0 {
            self.emit(MI_NOOP);
        }
        let end = if self.tail == 0 { RING_SIZE } else { self.tail };
        clflush(unsafe { self.ring.1.add(start as usize) }, (end - start) as usize);
        let t0 = now();
        self.wr(RCS + RING_TAIL, self.tail);
        loop {
            clflush(self.status.1, 64);
            if unsafe { read_volatile(self.status.1 as *const u32) } == self.seqno {
                return Ok(us_since(t0));
            }
            if us_since(t0) > 200_000 {
                return Err(format!(
                    "not finished: HEAD {:#x} TAIL {:#x} ACTHD {:#x} CTL {:#x} IPEHR {:#x} INSTDONE {:#x} status {:#x}",
                    self.rd(RCS + RING_HEAD),
                    self.rd(RCS + RING_TAIL),
                    self.rd(RCS + RING_ACTHD),
                    self.rd(RCS + RING_CTL),
                    self.rd(RCS + RING_IPEHR),
                    self.rd(RCS + RING_INSTDONE),
                    unsafe { read_volatile(self.status.1 as *const u32) }
                ));
            }
        }
    }
}

/// Shell `gpu render`: step 3a.
pub fn self_test() -> Vec<String> {
    let Some((regs, gtt, ..)) = super::intel_blt::gt() else {
        return alloc::vec![String::from("gpu render: needs a Gen9 Intel GPU (see 'gpu')")];
    };
    let mut guard = RCS_STATE.lock();
    let r = guard.get_or_insert(Rcs {
        regs,
        gtt,
        up: false,
        ring: (0, core::ptr::null_mut()),
        status: (0, core::ptr::null_mut()),
        tail: 0,
        seqno: 0,
    });
    let mut out = alloc::vec![String::from("render engine (RCS) self-test, GPU step 3a")];
    if !r.up && !r.bring_up(&mut out) {
        out.push(String::from("  RCS ring did not start"));
        return out;
    }
    let steps: [(&str, &[u32], Fence); 3] = [
        ("1 MI_STORE_DATA_IMM (command streamer runs)", &[MI_NOOP, MI_NOOP], Fence::StoreDataImm),
        ("2 PIPE_CONTROL post-sync write (3D pipe flush)", &[MI_NOOP, MI_NOOP], Fence::PipeControl),
        ("3 PIPELINE_SELECT 3D + PIPE_CONTROL", &[PIPELINE_SELECT_3D, MI_NOOP], Fence::PipeControl),
    ];
    for (name, cmds, fence) in steps {
        match r.submit(cmds, fence) {
            Ok(us) => out.push(format!("  {}: ok, {} us", name, us)),
            Err(e) => {
                out.push(format!("  {}: {}", name, e));
                out.push(String::from("  (stopped here; the photo of these lines is what step 3 needs next)"));
                return out;
            }
        }
    }
    out.push(String::from("  render engine runs: step 3b (a triangle) can build on it"));
    out
}
