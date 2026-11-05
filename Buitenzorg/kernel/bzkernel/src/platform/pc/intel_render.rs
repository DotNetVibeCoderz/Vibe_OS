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
    out.push(String::from("  render engine runs"));
    let Some((_, _, fb_ggtt, pitch, w, h)) = super::intel_blt::gt() else { return out };
    let Some(fb) = crate::framebuffer::virtual_base() else { return out };
    match triangle(r, fb_ggtt, pitch, w, h) {
        Ok((us, x0)) => {
            // read back: inside the triangle (near its centroid) and outside it
            let px = |x: u32, y: u32| unsafe { read_volatile((fb + (y * pitch + x * 4) as u64) as *const u32) } & 0xFF_FFFF;
            let inside = px(x0 + 100, 150);
            let near = |v: u32, want: u32| (v as i32 - want as i32).abs() <= 2;
            let ok = near(inside >> 16 & 0xFF, 230) && near(inside >> 8 & 0xFF, 102) && near(inside & 0xFF, 26);
            out.push(format!(
                "  4 triangle through the 3D pipeline: {} us, pixel inside {:#08x} ({}), outside {:#08x}",
                us,
                inside,
                if ok { "orange: ok" } else { "NOT the expected orange 0xe6661a" },
                px(x0 + 4, 44)
            ));
            out.push(String::from("  (the orange triangle was on screen for 4 s, top right)"));
            drop(guard);
            let t0 = now();
            while us_since(t0) < 4_000_000 {
                core::hint::spin_loop();
            }
            crate::framebuffer::forget_screen();
            crate::wm::screen_overwritten();
            crate::wm::present_now();
        }
        Err(e) => out.push(format!("  4 triangle: {}", e)),
    }
    out
}

// ---- step 3b: one solid triangle through the 3D pipeline -------------------
//
// After IGT's `rendercopy_gen9.c` (the GPU test suite's minimal Gen9 3D
// pipeline: same packet order, same "null" state for the stages we do not
// use) with packet layouts checked against Mesa's gen9 genxml. Everything
// lives in 4 contiguous pages mapped at STATE_GGTT: the batch at offset 0,
// state from 0x2000; surface, dynamic and instruction base addresses all
// point at STATE_GGTT, so state pointers are offsets into these pages.

const STATE_GGTT: u32 = 0x1000_8000;
const ST_BT: u32 = 0x2000;
const ST_SURF: u32 = 0x2040;
const ST_KERNEL: u32 = 0x2100;
const ST_VB: u32 = 0x2200;
const ST_CC: u32 = 0x2240;
const ST_BLEND: u32 = 0x2280;
const ST_CC_VP: u32 = 0x2340;
const ST_SF_VP: u32 = 0x2380;
const ST_SCISSOR: u32 = 0x2400;

/// Header of a 3D command (IGT's GEN4_3D): pipeline, opcode, sub-opcode.
const fn c3d(p: u32, o: u32, s: u32) -> u32 {
    3 << 29 | p << 27 | o << 24 | s << 16
}

/// SIMD16 pixel shader writing one constant color (IGT's "write all"
/// kernel with other immediates): 8 MOVs fill g112..g119 with R, R, G, G,
/// B, B, A, A (two GRFs per channel for 16 pixels), then a render target
/// write (SFID render cache, message length 8, end of thread).
const PS_KERNEL: [[u32; 4]; 9] = [
    [0x0060_0001, 0x2e00_0608, 0, 0x3f66_6666], // R = 0.9
    [0x0060_0001, 0x2e20_0608, 0, 0x3f66_6666],
    [0x0060_0001, 0x2e40_0608, 0, 0x3ecc_cccd], // G = 0.4
    [0x0060_0001, 0x2e60_0608, 0, 0x3ecc_cccd],
    [0x0060_0001, 0x2e80_0608, 0, 0x3dcc_cccd], // B = 0.1
    [0x0060_0001, 0x2ea0_0608, 0, 0x3dcc_cccd],
    [0x0060_0001, 0x2ec0_0608, 0, 0x3f80_0000], // A = 1.0
    [0x0060_0001, 0x2ee0_0608, 0, 0x3f80_0000],
    [0x0580_0031, 0x2000_22e0, 0x0e00_0e00, 0x9003_1000],
];

/// IEEE single bits of a small non-negative integer (the kernel is soft-float).
fn f32_bits(v: u32) -> u32 {
    if v == 0 {
        return 0;
    }
    let e = 31 - v.leading_zeros();
    let m = if e > 23 { v >> (e - 23) } else { v << (23 - e) };
    (e + 127) << 23 | (m & 0x7F_FFFF)
}

static STATE_PAGES: Mutex<Option<(u64, usize)>> = Mutex::new(None);

/// Draw an orange triangle into the framebuffer (top right). Ok((us, x0)).
fn triangle(r: &mut Rcs, fb_ggtt: u32, pitch: u32, w: u32, h: u32) -> Result<(u64, u32), String> {
    let (phys, va) = {
        let mut sp = STATE_PAGES.lock();
        if sp.is_none() {
            let (p, v) = crate::memory::alloc_dma_contiguous(4).ok_or("no 4 contiguous pages")?;
            for i in 0..4u32 {
                r.map(STATE_GGTT + i * 4096, p + i as u64 * 4096);
            }
            r.wr(GFX_FLSH_CNTL, 1);
            let _ = r.rd(GFX_FLSH_CNTL);
            *sp = Some((p, v as usize));
        }
        sp.unwrap()
    };
    let _ = phys;
    let base = va as *mut u32;
    unsafe { core::ptr::write_bytes(base as *mut u8, 0, 4 * 4096) };
    let put = |off: u32, vals: &[u32]| {
        for (i, v) in vals.iter().enumerate() {
            unsafe { write_volatile(base.add(off as usize / 4 + i), *v) };
        }
    };

    // --- state ---
    put(ST_BT, &[ST_SURF]);
    put(
        ST_SURF,
        &[
            1 << 29 | 0x0C0 << 18 | 1 << 16 | 1 << 14 | 1 << 8, // 2D, B8G8R8A8_UNORM, valign/halign 4, render cache r/w
            0,                                                  // qpitch, MOCS 0
            (h - 1) << 16 | (w - 1),
            pitch - 1,
            0,
            1 << 8, // mip tail start LOD 1 (as IGT)
            0,
            4 << 25 | 5 << 22 | 6 << 19 | 7 << 16, // channel selects R G B A
            fb_ggtt,
            0,
        ],
    );
    for (i, ins) in PS_KERNEL.iter().enumerate() {
        put(ST_KERNEL + i as u32 * 16, ins);
    }
    let x0 = w.saturating_sub(260);
    let verts = [(x0 + 100, 40u32), (x0 + 200, 220), (x0, 220)];
    for (i, (x, y)) in verts.iter().enumerate() {
        put(ST_VB + i as u32 * 16, &[f32_bits(*x), f32_bits(*y), 0, 0]);
    }
    put(ST_CC, &[0, 0, 0, 0, 0, 0]);
    let mut blend = [0u32; 33];
    for e in 0..16 {
        blend[1 + e * 2] = 0x11 << 21 | 1 << 26; // dest ZERO, source ONE, ADD, no blending
        blend[2 + e * 2] = 1 << 1; // pre-blend color clamp
    }
    put(ST_BLEND, &blend);
    put(ST_CC_VP, &[0xf99a_130c, 0x799a_130c]); // depth range -1e35 .. 1e35
    put(ST_SF_VP, &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0x3f80_0000, 0, 0x3f80_0000, 0, 0, 0, 0]);
    put(ST_SCISSOR, &[0, 0]);

    // --- batch ---
    let mut b: Vec<u32> = Vec::with_capacity(512);
    // a packet of `n` dwords whose body is all zero ("null" state)
    fn zeros(b: &mut Vec<u32>, hdr: u32, n: usize) {
        b.push(hdr | (n as u32 - 2));
        b.extend(core::iter::repeat(0).take(n - 1));
    }
    b.push(PIPELINE_SELECT_3D);
    zeros(&mut b, c3d(0, 1, 2), 3); // STATE_SIP
    for s in [0x12, 0x13, 0x14, 0x15, 0x16] {
        b.extend_from_slice(&[c3d(3, 1, s), 0]); // PUSH_CONSTANT_ALLOC_VS/HS/DS/GS/PS
    }
    let sb = STATE_GGTT | 1;
    b.extend_from_slice(&[
        c3d(0, 1, 1) | 17, // STATE_BASE_ADDRESS, 19 dwords
        1, 0, // general: 0, modify
        0, // stateless MOCS
        sb, 0, // surface
        sb, 0, // dynamic
        0, 0, // indirect (not modified)
        sb, 0, // instruction
        0xFFFF_F000 | 1, 0xFFFF_F000 | 1, 0xFFFF_F000 | 1, 0xFFFF_F000 | 1, // buffer sizes: all
        0, 0, 0, // bindless (not modified)
    ]);
    b.extend_from_slice(&[c3d(3, 0, 0x23), ST_CC_VP, c3d(3, 0, 0x21), ST_SF_VP]);
    b.extend_from_slice(&[c3d(3, 0, 0x30), 64 | 1 << 16 | 4 << 25]); // URB_VS
    for s in [0x33, 0x31, 0x32] {
        b.extend_from_slice(&[c3d(3, 0, s), 4 << 25]); // URB_GS/HS/DS
    }
    b.extend_from_slice(&[c3d(3, 0, 0x24), ST_BLEND | 1, c3d(3, 0, 0x0E), ST_CC | 1]);
    b.extend_from_slice(&[c3d(3, 0, 0x0d), 0, c3d(3, 0, 0x18), 1]); // MULTISAMPLE, SAMPLE_MASK
    // null state for the stages we do not use
    zeros(&mut b, c3d(3, 0, 0x52), 5); // WM_HZ_OP
    zeros(&mut b, c3d(3, 0, 0x19), 11); // CONSTANT_HS
    zeros(&mut b, c3d(3, 0, 0x1b), 9); // HS
    b.extend_from_slice(&[c3d(3, 0, 0x27), 0, c3d(3, 0, 0x2c), 0]);
    zeros(&mut b, c3d(3, 0, 0x1c), 4); // TE
    zeros(&mut b, c3d(3, 0, 0x16), 11); // CONSTANT_GS
    zeros(&mut b, c3d(3, 0, 0x11), 10); // GS
    b.extend_from_slice(&[c3d(3, 0, 0x29), 0, c3d(3, 0, 0x2e), 0]);
    zeros(&mut b, c3d(3, 0, 0x1a), 11); // CONSTANT_DS
    zeros(&mut b, c3d(3, 0, 0x1d), 11); // DS
    b.extend_from_slice(&[c3d(3, 0, 0x28), 0, c3d(3, 0, 0x2d), 0]);
    zeros(&mut b, c3d(3, 0, 0x15), 11); // CONSTANT_VS
    b.extend_from_slice(&[c3d(3, 0, 0x26), 0, c3d(3, 0, 0x2b), 0]);
    zeros(&mut b, c3d(3, 0, 0x10), 9); // VS: disabled (vertices pass through)
    zeros(&mut b, c3d(3, 0, 0x1e), 5); // STREAMOUT
    zeros(&mut b, c3d(3, 0, 0x12), 4); // CLIP: pass-through
    // setup: one attribute read from the VUE, no viewport transform (screen coordinates)
    b.extend_from_slice(&[c3d(3, 0, 0x1f) | 4, 1 << 22 | 1 << 29 | 1 << 28 | 1 << 11 | 1 << 5, 0, 0, 3, 0]);
    zeros(&mut b, c3d(3, 0, 0x51), 11); // SBE_SWIZ
    b.extend_from_slice(&[c3d(3, 0, 0x50) | 3, 1 << 21 | 1 << 16, 0, 0, 0]); // RASTER: CCW, no culling
    zeros(&mut b, c3d(3, 0, 0x13), 4); // SF
    b.extend_from_slice(&[c3d(3, 0, 0x14), 1 << 11]); // WM: perspective pixel barycentrics
    zeros(&mut b, c3d(3, 0, 0x17), 11); // CONSTANT_PS
    b.extend_from_slice(&[
        c3d(3, 0, 0x20) | 10, // PS
        ST_KERNEL,
        0,
        1 << 18, // one binding table entry, no samplers
        0,
        0,
        62 << 23 | 1 << 1, // max threads, SIMD16 dispatch
        6 << 16,           // payload starts at g6
        0,
        0,
        0,
        0,
    ]);
    b.extend_from_slice(&[c3d(3, 0, 0x4d), 1 << 30]); // PS_BLEND: writeable RT
    b.extend_from_slice(&[c3d(3, 0, 0x4f), 1 << 31 | 1 << 8]); // PS_EXTRA: valid, attributes
    b.extend_from_slice(&[c3d(3, 0, 0x2a), ST_BT, c3d(3, 0, 0x2f), 0, c3d(3, 0, 0x0f), ST_SCISSOR]);
    zeros(&mut b, c3d(3, 0, 0x4e), 4); // WM_DEPTH_STENCIL
    zeros(&mut b, c3d(3, 0, 0x05), 8); // DEPTH_BUFFER (none)
    zeros(&mut b, c3d(3, 0, 0x07), 5); // HIER_DEPTH_BUFFER
    zeros(&mut b, c3d(3, 0, 0x06), 5); // STENCIL_BUFFER
    b.extend_from_slice(&[c3d(3, 0, 0x04) | 1, 0, 1]); // CLEAR_PARAMS
    b.extend_from_slice(&[c3d(3, 1, 0x00) | 2, 0, (h - 1) << 16 | (w - 1), 0]); // DRAWING_RECTANGLE
    b.extend_from_slice(&[c3d(3, 0, 0x08) | 3, 1 << 14 | 16, STATE_GGTT + ST_VB, 0, 3 * 16]); // VERTEX_BUFFERS
    b.extend_from_slice(&[
        c3d(3, 0, 0x09) | 5, // VERTEX_ELEMENTS: pad, position (x, y, 0, 1), uv (unused)
        1 << 25 | 0x000 << 16,
        2 << 28 | 2 << 24 | 2 << 20 | 2 << 16,
        1 << 25 | 0x085 << 16,
        1 << 28 | 1 << 24 | 2 << 20 | 3 << 16,
        1 << 25 | 0x085 << 16 | 8,
        1 << 28 | 1 << 24 | 2 << 20 | 3 << 16,
    ]);
    b.extend_from_slice(&[c3d(3, 0, 0x4b), 0x04]); // VF_TOPOLOGY: triangle list
    b.extend_from_slice(&[c3d(3, 0, 0x0c), 0]); // VF
    b.extend_from_slice(&[c3d(3, 0, 0x49) | 1, 0, 0]); // VF_INSTANCING
    b.extend_from_slice(&[c3d(3, 3, 0x00) | 5, 0, 3, 0, 1, 0, 0]); // 3DPRIMITIVE: 3 vertices
    // flush the render target cache before the fence
    b.extend_from_slice(&[PIPE_CONTROL, PC_CS_STALL | 1 << 12 | 1 << 0, 0, 0, 0, 0]);
    b.push(0x0A << 23); // MI_BATCH_BUFFER_END
    if b.len() % 2 != 0 {
        b.push(MI_NOOP);
    }
    if b.len() * 4 > ST_BT as usize {
        return Err(format!("batch too long ({} dwords)", b.len()));
    }
    put(0, &b);
    clflush(base as *const u8, 4 * 4096);
    let us = r.submit(&[0x31 << 23 | 1, STATE_GGTT, 0], Fence::PipeControl)?;
    Ok((us, x0))
}
