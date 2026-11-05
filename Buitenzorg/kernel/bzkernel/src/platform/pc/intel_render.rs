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
//! - 3b: a minimal 3D pipeline drawing one solid triangle, read back (passed).
//! - 3c: Gouraud color, depth buffer, texture (offscreen panels, read back).
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
    /// The GGTT entry for `ggtt` as the GPU sees it.
    fn pte(&self, ggtt: u32) -> u64 {
        unsafe { read_volatile((self.gtt + (ggtt as u64 >> 12) * 8) as *const u64) }
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
    let px = |x: u32, y: u32| unsafe { read_volatile((fb + (y * pitch + x * 4) as u64) as *const u32) } & 0xFF_FFFF;

    // 4. (3b) a solid orange triangle straight into the framebuffer
    let x0 = w.saturating_sub(260);
    let screen = Target { ggtt: fb_ggtt, pitch, w, h };
    let tri = [vtx(x0 + 100, 40, 0, [0; 4]), vtx(x0 + 200, 220, 0, [0; 4]), vtx(x0, 220, 0, [0; 4])];
    match draw(r, &Pipe { rt: screen, depth: None, texture: None, kernel: &PS_SOLID, verts: &tri, vb: None, flags: PLAIN }) {
        Ok(us) => {
            let inside = px(x0 + 100, 150);
            out.push(format!(
                "  4 solid triangle into the framebuffer: {} us, pixel {:#08x} ({})",
                us,
                inside,
                if near_rgb(inside, 0xe6661a) { "orange: ok" } else { "NOT orange 0xe6661a" }
            ));
        }
        Err(e) => {
            out.push(format!("  4 solid triangle: {}", e));
            return out;
        }
    }

    // 4b. diagnosis: the Gouraud kernel into the framebuffer (left of the orange one)
    let gx = x0.saturating_sub(240);
    let tri = [vtx(gx + 100, 40, 0, [256, 0, 0, 256]), vtx(gx + 200, 220, 0, [0, 256, 0, 256]), vtx(gx, 220, 0, [0, 0, 256, 256])];
    match draw(r, &Pipe { rt: screen, depth: None, texture: None, kernel: &PS_GOURAUD, verts: &tri, vb: None, flags: PLAIN }) {
        Ok(us) => out.push(format!(
            "  4b Gouraud kernel into the framebuffer: {} us, near the red corner {:#08x}, center {:#08x}",
            us,
            px(gx + 100, 60),
            px(gx + 100, 160)
        )),
        Err(e) => out.push(format!("  4b Gouraud into the framebuffer: {}", e)),
    }

    // 5-7. (3c) offscreen: three 256x256 panels in one render target
    match offscreen_tests(r, &mut out) {
        Ok(rt_va) => {
            // show the panels below the triangle, copied by the CPU
            let (sx, sy) = (w.saturating_sub(OFF_W + 10), 240u32.min(h.saturating_sub(OFF_H)));
            for y in 0..OFF_H.min(h - sy) {
                for x in 0..OFF_W.min(w - sx) {
                    let v = unsafe { read_volatile((rt_va + (y * OFF_W + x) as usize * 4) as *const u32) };
                    unsafe { write_volatile((fb + ((sy + y) * pitch + (sx + x) * 4) as u64) as *mut u32, v) };
                }
            }
            out.push(String::from("  (triangle + the three panels were on screen for 4 s, right side)"));
        }
        Err(e) => out.push(format!("  offscreen: {}", e)),
    }
    drop(guard);
    let t0 = now();
    while us_since(t0) < 4_000_000 {
        core::hint::spin_loop();
    }
    crate::framebuffer::forget_screen();
    crate::wm::screen_overwritten();
    crate::wm::present_now();
    out
}

/// Same color within +-3 per channel (UNORM rounding).
fn near_rgb(v: u32, want: u32) -> bool {
    (0..3).all(|i| ((v >> (i * 8) & 0xFF) as i32 - (want >> (i * 8) & 0xFF) as i32).abs() <= 3)
}

// ---- the 3D pipeline (steps 3b/3c) -------------------------------------------
//
// After IGT's `rendercopy_gen9.c` (the GPU test suite's minimal Gen9 3D
// pipeline: same packet order, same "null" state for the stages we do not
// use) with packet layouts checked against Mesa's gen9 genxml. Batch and
// state live in contiguous pages mapped at STATE_GGTT: the batch at offset
// 0, state from 0x2000, vertices from 0x2800; surface, dynamic and
// instruction base addresses all point at STATE_GGTT, so state pointers are
// offsets into these pages. No vertex shader: vertices arrive in screen
// pixels (x, y, z) with one vec4 attribute (a color, or u, v for texturing).

const STATE_GGTT: u32 = 0x1000_8000;
const STATE_PAGES_N: usize = 4;
const ST_BT: u32 = 0x2000;
const ST_SURF: u32 = 0x2040;
const ST_KERNEL: u32 = 0x2100;
const ST_CC: u32 = 0x2240;
const ST_BLEND: u32 = 0x2280;
const ST_CC_VP: u32 = 0x2340;
const ST_SF_VP: u32 = 0x2380;
const ST_SCISSOR: u32 = 0x2400;
const ST_SURF_TEX: u32 = 0x2440;
const ST_SAMPLER: u32 = 0x2480;
const ST_VB: u32 = 0x2800;
/// Bytes per vertex: x, y, z, then a vec4 attribute.
/// Bytes per vertex: the position (x, y, z, w), then two vec4 attributes:
/// the color (r, g, b, a) and the texture coordinates (u, v, 0, 1).
const VSTRIDE: u32 = 48;
const MAX_VERTS: usize = (STATE_PAGES_N * 4096 - ST_VB as usize) / VSTRIDE as usize;

/// Header of a 3D command (IGT's GEN4_3D): pipeline, opcode, sub-opcode.
const fn c3d(p: u32, o: u32, s: u32) -> u32 {
    3 << 29 | p << 27 | o << 24 | s << 16
}

/// SIMD16 pixel shader writing one constant color (IGT's "write all"
/// kernel with our immediates): 8 MOVs fill g112..g119 with R, R, G, G,
/// B, B, A, A (two GRFs per channel for 16 pixels), then a render target
/// write (SFID render cache, message length 8, end of thread).
const PS_SOLID: [[u32; 4]; 9] = [
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

/// SIMD16 Gouraud shader: the vertex attribute (R, G, B, A) interpolated
/// with PLN from its setup data (g6.0, g6.4, g7.0, g7.4: the plane of each
/// component) and the perspective barycentrics in g2..g5, into g114..g121,
/// then the render target write from g114. Built from IGT's blit kernel:
/// its two PLNs (u at g6.0, v at g6.4 into g122/g124), with other
/// destination/source registers, and its render target write.
const PS_GOURAUD: [[u32; 4]; 5] = [
    [0x0080_005a, 0x2e40_3ae8, 0x3a00_00c0, 0x008d_0040], // pln(16) g114 g6.0 g2
    [0x0080_005a, 0x2e80_3ae8, 0x3a00_00d0, 0x008d_0040], // pln(16) g116 g6.4 g2
    [0x0080_005a, 0x2ec0_3ae8, 0x3a00_00e0, 0x008d_0040], // pln(16) g118 g7.0 g2
    [0x0080_005a, 0x2f00_3ae8, 0x3a00_00f0, 0x008d_0040], // pln(16) g120 g7.4 g2
    [0x0580_0031, 0x2000_3a40, 0x0e8d_0e40, 0x9003_1000], // RT write from g114, EOT
];

/// Textured and lit: the texel times the interpolated vertex color, as the
/// software rasterizer does. From IGT's Gen9 blit kernel (u, v by PLN, then
/// SAMPLE binding table entry 1 / sampler 0, here into g114..g121), plus
/// four PLNs for the color (attribute 0: g6.0 .. g7.4 -> g102..g109) and
/// four MULs. MUL is opcode 0x41 with PLN's operand layout (dw1 = dst
/// register 0x2000 | reg*32 << 16 | float GRF types 0x3ae8; dw2 = src1
/// GRF/float in bits 25..30 | src0 region <8,8,1> = 0x8d << 16 | src0
/// reg*32; dw3 = region 0x8d << 16 | src1 reg*32). The coordinates are
/// attribute 1 (setup data g8.0 / g8.4).
const PS_TEXTURE: [[u32; 4]; 12] = [
    [0x0080_005a, 0x2f40_3ae8, 0x3a00_0100, 0x008d_0040], // pln(16) g122 g8.0 g2  (u)
    [0x0080_005a, 0x2f80_3ae8, 0x3a00_0110, 0x008d_0040], // pln(16) g124 g8.4 g2  (v)
    [0x0280_0031, 0x2e40_3a48, 0x0e8d_0f40, 0x0884_0001], // sample -> g114..g121
    [0x0080_005a, 0x2cc0_3ae8, 0x3a00_00c0, 0x008d_0040], // pln(16) g102 g6.0 g2  (r)
    [0x0080_005a, 0x2d00_3ae8, 0x3a00_00d0, 0x008d_0040], // pln(16) g104 g6.4 g2  (g)
    [0x0080_005a, 0x2d40_3ae8, 0x3a00_00e0, 0x008d_0040], // pln(16) g106 g7.0 g2  (b)
    [0x0080_005a, 0x2d80_3ae8, 0x3a00_00f0, 0x008d_0040], // pln(16) g108 g7.4 g2  (a)
    [0x0080_0041, 0x2e40_3ae8, 0x3a8d_0e40, 0x008d_0cc0], // mul(16) g114 g114 g102
    [0x0080_0041, 0x2e80_3ae8, 0x3a8d_0e80, 0x008d_0d00], // mul(16) g116 g116 g104
    [0x0080_0041, 0x2ec0_3ae8, 0x3a8d_0ec0, 0x008d_0d40], // mul(16) g118 g118 g106
    [0x0080_0041, 0x2f00_3ae8, 0x3a8d_0f00, 0x008d_0d80], // mul(16) g120 g120 g108
    [0x0580_0031, 0x2000_3a40, 0x0e8d_0e40, 0x9003_1000], // RT write from g114, EOT
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

/// IEEE bits of n / 256 (0 <= n <= 256): colors, z and texture coordinates.
fn unit_bits(n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let b = f32_bits(n);
    b - (8 << 23) // divide by 256 = exponent - 8
}

/// A vertex: screen x, y, depth z/256, color components /256 (no texture).
fn vtx(x: u32, y: u32, z: u32, a: [u32; 4]) -> [u32; 12] {
    let o = unit_bits(256);
    [f32_bits(x), f32_bits(y), unit_bits(z), o, unit_bits(a[0]), unit_bits(a[1]), unit_bits(a[2]), unit_bits(a[3]), 0, 0, 0, o]
}

/// A textured vertex: color `a` (/256) and texture coordinates u, v (/256).
fn vtx_uv(x: u32, y: u32, z: u32, a: [u32; 4], u: u32, v: u32) -> [u32; 12] {
    let mut r = vtx(x, y, z, a);
    r[8] = unit_bits(u);
    r[9] = unit_bits(v);
    r
}

#[derive(Clone, Copy)]
struct Target {
    ggtt: u32,
    pitch: u32,
    w: u32,
    h: u32,
}

struct Pipe<'a> {
    rt: Target,
    /// D32_FLOAT depth buffer of the render target's size (Y-tiled; the
    /// caller clears it, e.g. to 1.0, which needs no tiling knowledge).
    depth: Option<(u32, u32)>, // (GGTT, pitch)
    texture: Option<Target>,
    kernel: &'a [[u32; 4]],
    verts: &'a [[u32; 12]],
    /// GGTT address of vertices the caller already wrote (else they go
    /// into the state pages, at most `MAX_VERTS`).
    vb: Option<u32>,
    flags: Flags,
}

/// Per-draw switches (the self-tests use `PLAIN`).
#[derive(Clone, Copy)]
struct Flags {
    test: bool,
    write: bool,
    blend: bool,
    wire: bool,
    /// Texture coordinates wrap (else clamp).
    wrap: bool,
    /// Vertices in clip space (x, y, z, w): the clipper divides by w and SF
    /// maps to the viewport, so the GPU knows 1/w and interpolates
    /// perspective-correctly. Else screen pixels with w = 1 (the self-tests).
    clip: bool,
    /// Drop pixels whose alpha is 0 (alpha test GREATER 0): transparent texels.
    alpha_test: bool,
}

const PLAIN: Flags = Flags { test: true, write: true, blend: false, wire: false, wrap: false, clip: false, alpha_test: false };

static STATE_PAGES: Mutex<Option<usize>> = Mutex::new(None);

fn map_pages(r: &Rcs, ggtt: u32, phys: u64, pages: usize) {
    for i in 0..pages as u32 {
        r.map(ggtt + i * 4096, phys + i as u64 * 4096);
    }
    r.wr(GFX_FLSH_CNTL, 1);
    let _ = r.rd(GFX_FLSH_CNTL);
}

/// One draw: build the batch + state and run it. Ok(microseconds).
fn draw(r: &mut Rcs, p: &Pipe) -> Result<u64, String> {
    if p.verts.is_empty() || (p.vb.is_none() && p.verts.len() > MAX_VERTS) {
        return Err(format!("{} vertices (1..={})", p.verts.len(), MAX_VERTS));
    }
    let fl = p.flags;
    let va = {
        let mut sp = STATE_PAGES.lock();
        if sp.is_none() {
            let (ph, v) = crate::memory::alloc_dma_contiguous(STATE_PAGES_N).ok_or("no contiguous state pages")?;
            map_pages(r, STATE_GGTT, ph, STATE_PAGES_N);
            *sp = Some(v as usize);
        }
        sp.unwrap()
    };
    let base = va as *mut u32;
    unsafe { core::ptr::write_bytes(base as *mut u8, 0, STATE_PAGES_N * 4096) };
    let put = |off: u32, vals: &[u32]| {
        for (i, v) in vals.iter().enumerate() {
            unsafe { write_volatile(base.add(off as usize / 4 + i), *v) };
        }
    };
    let (rt, tex) = (p.rt, p.texture);

    // --- state ---
    let surface = |t: Target, render: bool| -> [u32; 10] {
        [
            1 << 29 | 0x0C0 << 18 | 1 << 16 | 1 << 14 | (render as u32) << 8, // 2D, B8G8R8A8_UNORM, align 4
            0,                                                                 // qpitch, MOCS 0
            (t.h - 1) << 16 | (t.w - 1),
            t.pitch - 1,
            0,
            1 << 8, // mip tail start LOD 1 (as IGT)
            0,
            4 << 25 | 5 << 22 | 6 << 19 | 7 << 16, // channel selects R G B A
            t.ggtt,
            0,
        ]
    };
    put(ST_BT, &[ST_SURF, ST_SURF_TEX]);
    put(ST_SURF, &surface(rt, true));
    if let Some(t) = tex {
        put(ST_SURF_TEX, &surface(t, false));
        let tcm = if fl.wrap { 0 } else { 2 << 6 | 2 << 3 | 2 }; // WRAP or CLAMP
        put(ST_SAMPLER, &[0, 0, 0, tcm]); // nearest
    }
    for (i, ins) in p.kernel.iter().enumerate() {
        put(ST_KERNEL + i as u32 * 16, ins);
    }
    if p.vb.is_none() {
        for (i, v) in p.verts.iter().enumerate() {
            put(ST_VB + i as u32 * VSTRIDE, v);
        }
    }
    // COLOR_CALC_STATE: alpha test reference 0.0 as FLOAT32
    put(ST_CC, &[fl.alpha_test as u32, 0, 0, 0, 0, 0]);
    let mut blend = [0u32; 33];
    if fl.alpha_test {
        blend[0] = 1 << 27 | 5 << 24; // alpha test enable, GREATER
    }
    for e in 0..16 {
        blend[1 + e * 2] = if fl.blend {
            // color: SRC_ALPHA, INV_SRC_ALPHA, ADD; alpha: ONE, INV_SRC_ALPHA
            1 << 31 | 3 << 26 | 19 << 21 | 1 << 13 | 19 << 8
        } else {
            0x11 << 21 | 1 << 26 // dest ZERO, source ONE, ADD, no blending
        };
        blend[2 + e * 2] = 1 << 1; // pre-blend color clamp
    }
    put(ST_BLEND, &blend);
    put(ST_CC_VP, &[0xf99a_130c, 0x799a_130c]); // depth range -1e35 .. 1e35
    if fl.clip {
        // viewport: x = (ndc + 1) w/2, y = (1 - ndc) h/2, z = (ndc + 1)/2 -- as gfx3d's project()
        let f = crate::virtio::virgl::f32_from_fx;
        let (hw, hh) = (rt.w as i64 * 32768, rt.h as i64 * 32768);
        let half = 0x3f00_0000;
        let g = f(-8 << 16);
        let gp = f(8 << 16);
        put(ST_SF_VP, &[f(hw), f(-hh), half, f(hw), f(hh), half, 0, 0, g, gp, g, gp, 0, f32_bits(rt.w), 0, f32_bits(rt.h)]);
    } else {
        put(ST_SF_VP, &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0x3f80_0000, 0, 0x3f80_0000, 0, 0, 0, 0]);
    }
    put(ST_SCISSOR, &[0, 0]);

    // --- batch ---
    fn zeros(b: &mut Vec<u32>, hdr: u32, n: usize) {
        b.push(hdr | (n as u32 - 2));
        b.extend(core::iter::repeat(0).take(n - 1));
    }
    let mut b: Vec<u32> = Vec::with_capacity(512);
    // Every draw rewrites vertices, state and the kernel at the same
    // addresses: without invalidating the GPU's caches the next draw used
    // the previous draw's vertices (found on the laptop: the second triangle
    // appeared where the first one was, offscreen draws hit nothing). As
    // Mesa: flush render target + depth (CS stall), an empty PIPE_CONTROL
    // (Gen9 needs one before a VF cache invalidate), then invalidate the
    // state, constant, VF, texture and instruction caches.
    b.extend_from_slice(&[PIPE_CONTROL, PC_CS_STALL | 1 << 12 | 1 << 5 | 1 << 0, 0, 0, 0, 0]);
    b.extend_from_slice(&[PIPE_CONTROL, 0, 0, 0, 0, 0]);
    b.extend_from_slice(&[PIPE_CONTROL, 1 << 2 | 1 << 3 | 1 << 4 | 1 << 10 | 1 << 11, 0, 0, 0, 0]);
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
    if fl.clip {
        // CLIP on (normal mode, no clip tests: the CPU clipped already) = perspective divide
        b.extend_from_slice(&[c3d(3, 0, 0x12) | 2, 0, 1 << 31, 0]);
    } else {
        zeros(&mut b, c3d(3, 0, 0x12), 4); // CLIP: pass-through
    }
    // setup: one attribute read from the VUE, no viewport transform (screen coordinates)
    // two attributes (color, texture coordinates), both read in one 256-bit unit
    b.extend_from_slice(&[c3d(3, 0, 0x1f) | 4, 2 << 22 | 1 << 29 | 1 << 28 | 1 << 11 | 1 << 5, 0, 0, 3 | 3 << 2, 0]);
    zeros(&mut b, c3d(3, 0, 0x51), 11); // SBE_SWIZ
    let fill = if fl.wire { 1 << 5 | 1 << 3 } else { 0 }; // front/back fill mode WIREFRAME
    b.extend_from_slice(&[c3d(3, 0, 0x50) | 3, 1 << 21 | 1 << 16 | fill, 0, 0, 0]); // RASTER: CCW, no culling
    if fl.clip {
        b.extend_from_slice(&[c3d(3, 0, 0x13) | 2, 1 << 1, 0, 0]); // SF: viewport transform
    } else {
        zeros(&mut b, c3d(3, 0, 0x13), 4); // SF
    }
    b.extend_from_slice(&[c3d(3, 0, 0x14), 1 << 11]); // WM: perspective pixel barycentrics
    zeros(&mut b, c3d(3, 0, 0x17), 11); // CONSTANT_PS
    let samplers = if tex.is_some() { 1 << 27 | 2 << 18 } else { 1 << 18 };
    b.extend_from_slice(&[
        c3d(3, 0, 0x20) | 10, // PS
        ST_KERNEL,
        0,
        samplers, // binding table entries (+ one sampler for texturing)
        0,
        0,
        62 << 23 | 1 << 1, // max threads, SIMD16 dispatch
        6 << 16,           // payload starts at g6
        0,
        0,
        0,
        0,
    ]);
    // PS_BLEND: writeable RT, + alpha test, + blending mirrored from entry 0
    let mut psb = 1 << 30 | (fl.alpha_test as u32) << 8;
    if fl.blend {
        psb |= 1 << 29 | 3 << 14 | 19 << 9 | 1 << 24 | 19 << 19;
    }
    b.extend_from_slice(&[c3d(3, 0, 0x4d), psb]);
    // PS_EXTRA: valid, attributes (+ kills pixels when alpha-testing)
    b.extend_from_slice(&[c3d(3, 0, 0x4f), 1 << 31 | 1 << 8 | (fl.alpha_test as u32) << 28]);
    let sampler_ptr = if tex.is_some() { ST_SAMPLER } else { 0 };
    b.extend_from_slice(&[c3d(3, 0, 0x2a), ST_BT, c3d(3, 0, 0x2f), sampler_ptr, c3d(3, 0, 0x0f), ST_SCISSOR]);
    match p.depth {
        Some((dz, dpitch)) => {
            // depth test LESS + write; a 2D D32_FLOAT buffer of the target's size
            // test on (function LESS, or ALWAYS when the draw asks for no test), write as asked
            let func = if fl.test { 2 } else { 0 };
            b.extend_from_slice(&[c3d(3, 0, 0x4e) | 2, fl.write as u32 | 1 << 1 | func << 5, 0, 0]);
            b.extend_from_slice(&[
                c3d(3, 0, 0x05) | 6,
                (dpitch - 1) | 1 << 18 | (fl.write as u32) << 28 | 1 << 29,
                dz,
                0,
                (rt.h - 1) << 18 | (rt.w - 1) << 4,
                0,
                0,
                0,
            ]);
        }
        None => {
            zeros(&mut b, c3d(3, 0, 0x4e), 4); // WM_DEPTH_STENCIL: off
            zeros(&mut b, c3d(3, 0, 0x05), 8); // DEPTH_BUFFER: none
        }
    }
    zeros(&mut b, c3d(3, 0, 0x07), 5); // HIER_DEPTH_BUFFER
    zeros(&mut b, c3d(3, 0, 0x06), 5); // STENCIL_BUFFER
    b.extend_from_slice(&[c3d(3, 0, 0x04) | 1, 0x3f80_0000, 1]); // CLEAR_PARAMS: depth 1.0
    b.extend_from_slice(&[c3d(3, 1, 0x00) | 2, 0, (rt.h - 1) << 16 | (rt.w - 1), 0]); // DRAWING_RECTANGLE
    let n = p.verts.len() as u32;
    b.extend_from_slice(&[c3d(3, 0, 0x08) | 3, 1 << 14 | VSTRIDE, p.vb.unwrap_or(STATE_GGTT + ST_VB), 0, n * VSTRIDE]);
    b.extend_from_slice(&[
        c3d(3, 0, 0x09) | 7, // VERTEX_ELEMENTS: pad, position (x, y, z, w), color, texture coordinates
        1 << 25 | 0x000 << 16,
        2 << 28 | 2 << 24 | 2 << 20 | 2 << 16,
        1 << 25 | 0x000 << 16,
        1 << 28 | 1 << 24 | 1 << 20 | 1 << 16,
        1 << 25 | 0x000 << 16 | 16,
        1 << 28 | 1 << 24 | 1 << 20 | 1 << 16,
        1 << 25 | 0x000 << 16 | 32,
        1 << 28 | 1 << 24 | 1 << 20 | 1 << 16,
    ]);
    b.extend_from_slice(&[c3d(3, 0, 0x4b), 0x04]); // VF_TOPOLOGY: triangle list
    b.extend_from_slice(&[c3d(3, 0, 0x0c), 0]); // VF
    b.extend_from_slice(&[c3d(3, 0, 0x49) | 1, 0, 0]); // VF_INSTANCING
    b.extend_from_slice(&[c3d(3, 3, 0x00) | 5, 0, n, 0, 1, 0, 0]); // 3DPRIMITIVE
    // flush the render target and depth caches before the fence
    b.extend_from_slice(&[PIPE_CONTROL, PC_CS_STALL | 1 << 12 | 1 << 0, 0, 0, 0, 0]);
    b.push(0x0A << 23); // MI_BATCH_BUFFER_END
    if b.len() % 2 != 0 {
        b.push(MI_NOOP);
    }
    if b.len() * 4 > ST_BT as usize {
        return Err(format!("batch too long ({} dwords)", b.len()));
    }
    put(0, &b);
    clflush(base as *const u8, STATE_PAGES_N * 4096);
    r.submit(&[0x31 << 23 | 1, STATE_GGTT, 0], Fence::PipeControl)
}

// ---- step 3c: Gouraud, depth, texture, offscreen ------------------------------

const OFF_W: u32 = 768; // three 256x256 panels
const OFF_H: u32 = 256;
const RT_GGTT: u32 = 0x1100_0000;
const DEPTH_GGTT: u32 = 0x1110_0000;
const TEX_GGTT: u32 = 0x1120_0000;
const TEX: u32 = 64;

/// (render target va, depth va, texture va), allocated once.
static OFFSCREEN: Mutex<Option<(usize, usize, usize)>> = Mutex::new(None);

/// Steps 5-7 into an offscreen render target; Ok(its virtual address).
fn offscreen_tests(r: &mut Rcs, out: &mut Vec<String>) -> Result<usize, String> {
    let pages = (OFF_W * OFF_H * 4 / 4096) as usize;
    let tex_pages = (TEX * TEX * 4 / 4096) as usize;
    let (rt_va, z_va, tex_va) = {
        let mut o = OFFSCREEN.lock();
        if o.is_none() {
            let (rp, rv) = crate::memory::alloc_dma_contiguous(pages).ok_or("no pages for the render target")?;
            let (zp, zv) = crate::memory::alloc_dma_contiguous(pages).ok_or("no pages for the depth buffer")?;
            let (tp, tv) = crate::memory::alloc_dma_contiguous(tex_pages).ok_or("no pages for the texture")?;
            map_pages(r, RT_GGTT, rp, pages);
            map_pages(r, DEPTH_GGTT, zp, pages);
            map_pages(r, TEX_GGTT, tp, tex_pages);
            *o = Some((rv as usize, zv as usize, tv as usize));
        }
        o.unwrap()
    };
    let rt32 = rt_va as *mut u32;
    let fill = |p: *mut u32, n: usize, v: u32| {
        for i in 0..n {
            unsafe { write_volatile(p.add(i), v) };
        }
        clflush(p as *const u8, n * 4);
    };
    let n_px = (OFF_W * OFF_H) as usize;
    fill(rt32, n_px, 0x0020_2020);
    fill(z_va as *mut u32, n_px, 0x3f80_0000); // depth 1.0 everywhere (tiling does not matter)
    // texture: 8x8 checker of yellow and blue
    for y in 0..TEX {
        for x in 0..TEX {
            let v = if (x / 8 + y / 8) % 2 == 0 { 0x00F0_D020 } else { 0x0020_40E0 };
            unsafe { write_volatile((tex_va as *mut u32).add((y * TEX + x) as usize), v) };
        }
    }
    clflush(tex_va as *const u8, (TEX * TEX * 4) as usize);
    let rt = Target { ggtt: RT_GGTT, pitch: OFF_W * 4, w: OFF_W, h: OFF_H };
    let read = |x: u32, y: u32| -> u32 {
        let p = unsafe { rt32.add((y * OFF_W + x) as usize) };
        clflush(p as *const u8, 4);
        (unsafe { read_volatile(p) }) & 0xFF_FFFF
    };

    // diagnosis: is the offscreen target reachable? (the GGTT entry, a
    // known-good writer = the blitter, then the known-good solid kernel)
    out.push(format!("  5a render target: GGTT entry {:#x}, first page at {:#x}", r.pte(RT_GGTT), rt_va));
    match super::intel_blt::fill_rect(RT_GGTT, OFF_W * 4, 0, 0, 32, 32, 0xFF00_FF00) {
        Ok(us) => out.push(format!("  5b blitter fill into it: {} us, read back {:#08x} (want 0x00ff00)", us, read(4, 4))),
        Err(e) => out.push(format!("  5b blitter fill: {}", e)),
    }
    let tri = [vtx(40, 4, 0, [0; 4]), vtx(76, 40, 0, [0; 4]), vtx(4, 40, 0, [0; 4])];
    match draw(r, &Pipe { rt, depth: None, texture: None, kernel: &PS_SOLID, verts: &tri, vb: None, flags: PLAIN }) {
        Ok(us) => out.push(format!("  5c solid kernel into it: {} us, read back {:#08x} (want 0xe6661a)", us, read(40, 30))),
        Err(e) => out.push(format!("  5c solid kernel: {}", e)),
    }
    fill(rt32, n_px, 0x0020_2020);

    // 5. Gouraud: red, green, blue corners
    let tri = [vtx(128, 16, 0, [256, 0, 0, 256]), vtx(240, 240, 0, [0, 256, 0, 256]), vtx(16, 240, 0, [0, 0, 256, 256])];
    let us = draw(r, &Pipe { rt, depth: None, texture: None, kernel: &PS_GOURAUD, verts: &tri, vb: None, flags: PLAIN })?;
    let (top, center) = (read(128, 30), read(128, 165));
    let ch = |v: u32, s: u32| v >> s & 0xFF;
    let red_top = ch(top, 16) > 180 && ch(top, 8) < 60 && ch(top, 0) < 60;
    let mixed = [16, 8, 0].iter().all(|&s| (50..=120).contains(&ch(center, s)));
    out.push(format!(
        "  5 Gouraud triangle: {} us, top {:#08x} ({}), center {:#08x} ({})",
        us,
        top,
        if red_top { "red: ok" } else { "NOT red" },
        center,
        if mixed { "mixed: ok" } else { "NOT a mix" }
    ));

    // 6. depth: a near green triangle first, then a far red one over it
    let g = [0, 230, 0, 256];
    let rd = [230, 0, 0, 256];
    let tris = [
        vtx(256 + 20, 40, 64, g),
        vtx(256 + 200, 220, 64, g),
        vtx(256 + 20, 220, 64, g),
        vtx(256 + 236, 40, 192, rd),
        vtx(256 + 236, 220, 192, rd),
        vtx(256 + 56, 220, 192, rd),
    ];
    let z = Some((DEPTH_GGTT, OFF_W * 4));
    let us = draw(r, &Pipe { rt, depth: z, texture: None, kernel: &PS_GOURAUD, verts: &tris, vb: None, flags: PLAIN })?;
    let (overlap, far_only) = (read(256 + 120, 200), read(256 + 228, 200));
    out.push(format!(
        "  6 depth test: {} us, overlap {:#08x} ({}), far only {:#08x} ({})",
        us,
        overlap,
        if near_rgb(overlap, 0x00e600) { "near green wins: ok" } else { "NOT green" },
        far_only,
        if near_rgb(far_only, 0xe60000) { "red: ok" } else { "NOT red" }
    ));

    // 7. texture: the checker on a 128x128 square (two triangles)
    let (x0, y0) = (512 + 64, 64);
    let t = |x: u32, y: u32, u: u32, v: u32| vtx_uv(x, y, 0, [256, 256, 256, 256], u, v);
    let quad = [
        t(x0, y0, 0, 0),
        t(x0 + 128, y0, 256, 0),
        t(x0, y0 + 128, 0, 256),
        t(x0 + 128, y0, 256, 0),
        t(x0 + 128, y0 + 128, 256, 256),
        t(x0, y0 + 128, 0, 256),
    ];
    let texture = Some(Target { ggtt: TEX_GGTT, pitch: TEX * 4, w: TEX, h: TEX });
    let us = draw(r, &Pipe { rt, depth: None, texture, kernel: &PS_TEXTURE, verts: &quad, vb: None, flags: PLAIN })?;
    // 2 pixels per texel: texel (2, 2) is in a yellow cell, texel (10, 2) in a blue one
    let (a, b) = (read(x0 + 5, y0 + 5), read(x0 + 21, y0 + 5));
    out.push(format!(
        "  7 textured square: {} us, {:#08x} ({}), {:#08x} ({})",
        us,
        a,
        if near_rgb(a, 0xf0d020) { "yellow: ok" } else { "NOT yellow" },
        b,
        if near_rgb(b, 0x2040e0) { "blue: ok" } else { "NOT blue" }
    ));

    // 7b. texture x lighting: the same checker with the vertex color at 50 %
    let (x1, y1) = (512 + 8, 200);
    let half = |x: u32, y: u32, u: u32, v: u32| vtx_uv(x, y, 0, [128, 128, 128, 256], u, v);
    let quad = [
        half(x1, y1, 0, 0),
        half(x1 + 48, y1, 256, 0),
        half(x1, y1 + 48, 0, 256),
        half(x1 + 48, y1, 256, 0),
        half(x1 + 48, y1 + 48, 256, 256),
        half(x1, y1 + 48, 0, 256),
    ];
    let us = draw(r, &Pipe { rt, depth: None, texture, kernel: &PS_TEXTURE, verts: &quad, vb: None, flags: PLAIN })?;
    let c = read(x1 + 2, y1 + 2);
    out.push(format!(
        "  7b texture x 50% light: {} us, {:#08x} ({})",
        us,
        c,
        if near_rgb(c, 0x786810) { "half yellow: ok" } else { "NOT 0x786810" }
    ));

    // 7c. alpha test: yellow cells opaque, blue cells alpha 0 -> the background shows
    for y in 0..TEX {
        for x in 0..TEX {
            let v = if (x / 8 + y / 8) % 2 == 0 { 0xFFF0_D020 } else { 0x0020_40E0 };
            unsafe { write_volatile((tex_va as *mut u32).add((y * TEX + x) as usize), v) };
        }
    }
    clflush(tex_va as *const u8, (TEX * TEX * 4) as usize);
    let (x2, y2) = (512 + 64, 200);
    let white = |x: u32, y: u32, u: u32, v: u32| vtx_uv(x, y, 0, [256, 256, 256, 256], u, v);
    let quad = [
        white(x2, y2, 0, 0),
        white(x2 + 48, y2, 256, 0),
        white(x2, y2 + 48, 0, 256),
        white(x2 + 48, y2, 256, 0),
        white(x2 + 48, y2 + 48, 256, 256),
        white(x2, y2 + 48, 0, 256),
    ];
    let flags = Flags { alpha_test: true, ..PLAIN };
    let us = draw(r, &Pipe { rt, depth: None, texture, kernel: &PS_TEXTURE, verts: &quad, vb: None, flags })?;
    // 48 px over 64 texels: offset 2 -> texel 2 (yellow cell), offset 9 -> texel 12 (blue cell)
    let (opaque, clear) = (read(x2 + 2, y2 + 2), read(x2 + 9, y2 + 2));
    out.push(format!(
        "  7c alpha test: {} us, opaque {:#08x} ({}), alpha 0 {:#08x} ({})",
        us,
        opaque,
        if near_rgb(opaque, 0xf0d020) { "yellow: ok" } else { "NOT yellow" },
        clear,
        if near_rgb(clear, 0x202020) { "dropped: ok" } else { "NOT dropped" }
    ));
    Ok(rt_va)
}

// ---- step 3d: the gfx3d backend -------------------------------------------------
//
// `gfx3d` contexts drawn by the render engine: the CPU does the vertex stage,
// clipping and projection (gfx3d::native_draw), these functions keep the GPU
// surfaces. A context = a linear B8G8R8A8 render target + a D32 depth buffer
// (pitch rounded to 128 bytes, height to 32 rows), both cleared by the
// blitter; the picture is read back by the CPU (clflush per row). Surfaces
// are kept on a free list for reuse (their pages stay mapped).

const VB_GGTT: u32 = 0x1200_0000;
const VB_PAGES: usize = 256; // 1 MiB of vertices per submission
const VB_MAX: usize = VB_PAGES * 4096 / VSTRIDE as usize / 3 * 3;
const POOL_GGTT: u32 = 0x2000_0000;
const POOL_END: u32 = 0xF000_0000;

#[derive(Clone, Copy)]
struct Surf {
    ggtt: u32,
    va: usize,
    pages: usize,
}

struct Native3d {
    w: u32,
    h: u32,
    pitch: u32,
    rows: u32,
    rt: Surf,
    z: Surf,
}

struct Pool {
    next_ggtt: u32,
    free: Vec<Surf>,
    targets: Vec<Option<Native3d>>,
    textures: Vec<Option<(Target, Surf)>>,
    vb: Option<usize>,
}

static POOL: Mutex<Option<Pool>> = Mutex::new(None);

/// Run `f` with the render engine up and the pool (lock order: RCS_STATE, POOL).
fn with_gpu<R>(f: impl FnOnce(&mut Rcs, &mut Pool) -> Result<R, String>) -> Result<R, &'static str> {
    let mut guard = RCS_STATE.lock();
    if guard.is_none() {
        let (regs, gtt, ..) = super::intel_blt::gt().ok_or("no Intel GPU")?;
        *guard = Some(Rcs { regs, gtt, up: false, ring: (0, core::ptr::null_mut()), status: (0, core::ptr::null_mut()), tail: 0, seqno: 0 });
    }
    let r = guard.as_mut().unwrap();
    if !r.up {
        let mut lines = Vec::new();
        if !r.bring_up(&mut lines) {
            return Err("render engine did not start");
        }
    }
    let mut pool = POOL.lock();
    let pool = pool.get_or_insert(Pool { next_ggtt: POOL_GGTT, free: Vec::new(), targets: Vec::new(), textures: Vec::new(), vb: None });
    f(r, pool).map_err(|e| {
        crate::println!("[gfx3d] Intel render engine: {}", e);
        "Intel render engine failed (see the log)"
    })
}

fn alloc_surf(r: &Rcs, pool: &mut Pool, pages: usize) -> Result<Surf, String> {
    if let Some(i) = pool.free.iter().position(|s| s.pages == pages) {
        return Ok(pool.free.swap_remove(i));
    }
    let bytes = (pages * 4096) as u32;
    if pool.next_ggtt.saturating_add(bytes) > POOL_END {
        return Err(String::from("GGTT space used up"));
    }
    let (phys, va) = crate::memory::alloc_dma_contiguous(pages).ok_or_else(|| format!("no {} contiguous pages", pages))?;
    let s = Surf { ggtt: pool.next_ggtt, va: va as usize, pages };
    map_pages(r, s.ggtt, phys, pages);
    pool.next_ggtt += (bytes + 0xFFFF) & !0xFFFF;
    Ok(s)
}

fn n_create(w: u32, h: u32) -> Result<usize, &'static str> {
    with_gpu(|r, pool| {
        let pitch = (w * 4 + 127) & !127;
        let rows = (h + 31) & !31;
        let pages = (pitch * rows) as usize / 4096;
        let rt = alloc_surf(r, pool, pages)?;
        let z = alloc_surf(r, pool, pages)?;
        pool.targets.push(Some(Native3d { w, h, pitch, rows, rt, z }));
        Ok(pool.targets.len())
    })
}

fn n_release(h: usize) {
    if let Some(pool) = POOL.lock().as_mut() {
        if let Some(t) = pool.targets.get_mut(h.wrapping_sub(1)).and_then(|t| t.take()) {
            pool.free.push(t.rt);
            pool.free.push(t.z);
        }
    }
}

fn n_clear(h: usize, rgb: u32, color: bool, depth: bool) -> Result<(), &'static str> {
    with_gpu(|_, pool| {
        let t = pool.targets.get(h.wrapping_sub(1)).and_then(|t| t.as_ref()).ok_or("no such target")?;
        if color {
            super::intel_blt::fill_rect(t.rt.ggtt, t.pitch, 0, 0, t.w, t.h, 0xFF00_0000 | rgb)?;
        }
        if depth {
            // depth 1.0 everywhere: a uniform value needs no tiling knowledge
            super::intel_blt::fill_rect(t.z.ggtt, t.pitch, 0, 0, t.pitch / 4, t.rows, 0x3f80_0000)?;
        }
        Ok(())
    })
}

fn n_texture(w: u32, h: u32, px: &[u32]) -> Result<usize, &'static str> {
    with_gpu(|r, pool| {
        let pitch = (w * 4 + 63) & !63;
        let pages = ((pitch * h) as usize).div_ceil(4096);
        let s = alloc_surf(r, pool, pages)?;
        for y in 0..h {
            for x in 0..w {
                let v = px[(y * w + x) as usize];
                unsafe { write_volatile((s.va + (y * pitch + x * 4) as usize) as *mut u32, v) };
            }
        }
        clflush(s.va as *const u8, pages * 4096);
        pool.textures.push(Some((Target { ggtt: s.ggtt, pitch, w, h }, s)));
        Ok(pool.textures.len())
    })
}

fn n_release_texture(h: usize) {
    if let Some(pool) = POOL.lock().as_mut() {
        if let Some((_, s)) = pool.textures.get_mut(h.wrapping_sub(1)).and_then(|t| t.take()) {
            pool.free.push(s);
        }
    }
}

fn n_draw(h: usize, verts: &[[u32; 12]], tex: Option<usize>, d: crate::gfx3d::NativeDraw) -> Result<(), &'static str> {
    with_gpu(|r, pool| {
        if pool.vb.is_none() {
            let (phys, va) = crate::memory::alloc_dma_contiguous(VB_PAGES).ok_or("no pages for vertices")?;
            map_pages(r, VB_GGTT, phys, VB_PAGES);
            pool.vb = Some(va as usize);
        }
        let vb = pool.vb.unwrap() as *mut u32;
        let t = pool.targets.get(h.wrapping_sub(1)).and_then(|t| t.as_ref()).ok_or("no such target")?;
        let texture = match tex {
            Some(i) => Some(pool.textures.get(i.wrapping_sub(1)).and_then(|t| t.as_ref()).ok_or("no such texture")?.0),
            None => None,
        };
        let rt = Target { ggtt: t.rt.ggtt, pitch: t.pitch, w: t.w, h: t.h };
        let flags = Flags { test: d.test, write: d.write, blend: d.blend, wire: d.wire, wrap: true, clip: true, alpha_test: texture.is_some() };
        let kernel: &[[u32; 4]] = if texture.is_some() { &PS_TEXTURE } else { &PS_GOURAUD };
        for chunk in verts.chunks(VB_MAX) {
            for (i, v) in chunk.iter().enumerate() {
                for (k, w) in v.iter().enumerate() {
                    unsafe { write_volatile(vb.add(i * 12 + k), *w) };
                }
            }
            clflush(vb as *const u8, chunk.len() * VSTRIDE as usize);
            let p = Pipe { rt, depth: Some((t.z.ggtt, t.pitch)), texture, kernel, verts: chunk, vb: Some(VB_GGTT), flags };
            draw(r, &p)?;
        }
        Ok(())
    })
}

fn n_read(h: usize, out: &mut [u32]) -> Result<(), &'static str> {
    with_gpu(|_, pool| {
        let t = pool.targets.get(h.wrapping_sub(1)).and_then(|t| t.as_ref()).ok_or("no such target")?;
        let w = t.w as usize;
        for y in 0..t.h as usize {
            let row = t.rt.va + y * t.pitch as usize;
            clflush(row as *const u8, w * 4);
            for x in 0..w.min(out.len().saturating_sub(y * w)) {
                out[y * w + x] = unsafe { read_volatile((row + x * 4) as *const u32) } & 0xFF_FFFF;
            }
        }
        Ok(())
    })
}

/// Step 3d: make the render engine a `gfx3d` backend (runs its self-test).
pub fn install_gfx3d() -> String {
    if let Err(e) = with_gpu(|_, _| Ok(())) {
        return format!("[gfx3d] Intel render engine not used: {}", e);
    }
    crate::gfx3d::install_native(crate::gfx3d::Native {
        name: "Intel render engine",
        create: n_create,
        release: n_release,
        clear: n_clear,
        texture: n_texture,
        release_texture: n_release_texture,
        draw: n_draw,
        read: n_read,
    })
}
