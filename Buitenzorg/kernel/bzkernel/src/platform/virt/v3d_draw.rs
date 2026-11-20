//! V3D 4.2 (Raspberry Pi 4) 3D pipeline, steps 2b-2e of the GPU plan.
//!
//! **2b** binning + triangles: a binning control list (CT0) runs the
//! coordinate shader and sorts primitives into per-tile lists in the tile
//! allocation memory; the render control list (CT1) then walks the tiles and
//! branches into each tile's list, where the vertex + fragment shaders run and
//! the tile buffer is stored to memory. Packets follow Mesa's
//! `v3d_packet_v33.xml` (min_ver 41), the job flow Mesa's `v3dx_draw.c` /
//! `v3dx_rcl.c` and Linux `v3d_sched.c` (tile alloc / tile state registers).
//! The QPU shaders are hand-assembled by `scripts/v3d-shaders.py` into
//! `v3d_shaders.rs` (conventions there).
//!
//! **2c** colour per vertex (varyings), depth (32-bit float in the tile
//! buffer, cleared every tile; LESS), textures (TMU, RGBA8 in strictly-UIF
//! layout, nearest + repeat; the texel multiplies the vertex colour), alpha
//! blending and wireframe.
//!
//! **2d** a `gfx3d` native backend: contexts batch their draws (vertices are
//! clip-space x, y, z, w + colour + uv, as for the Intel backend; the GPU does
//! the perspective divide and the viewport transform) and one bin + render
//! job runs when the picture is read. Depth lives only in the tile buffer, so
//! a frame that overflows the vertex buffer is split into two jobs and loses
//! depth between them (`VB_VERTS` = 87381 vertices per job).
//!
//! **2e** the desktop presented through V3D: `framebuffer::Accel` with a
//! cached staging surface; a render job loads the changed 64x64 tiles from it
//! and stores them into the framebuffer (shell `gpu on` / `gpu cpu`).
//!
//! Pixel order: the fragment shader writes the varyings in B, G, R, A order,
//! so the RGBA8 store's first byte is blue and a pixel reads 0xAARRGGBB --
//! the order gfx3d and the framebuffer use. Textures keep 0xAARRGGBB words and
//! are read as RGBA8, so the texel's first channel is blue too.
//!
//! Untested in QEMU (raspi4b has no V3D): every step is verified on a board.

use alloc::{format, string::String, vec, vec::Vec};
use spin::Mutex;

use super::v3d::{self, r, w, Buffer, Cl, Gpu, GPU};
use super::v3d_shaders as sh;

// core registers (Linux v3d_regs.h)
const CLE_CT0CS: u64 = 0x100;
const CLE_CT0CA: u64 = 0x110;
const CLE_BFC: u64 = 0x134;
const CLE_CT0QTS: u64 = 0x15c;
const CLE_CT0QBA: u64 = 0x160;
const CLE_CT0QEA: u64 = 0x168;
const CLE_CT0QMA: u64 = 0x170;
const CLE_CT0QMS: u64 = 0x174;
const PTB_BPOA: u64 = 0x308;
const PTB_BPOS: u64 = 0x30c;
const TILE_STATE_VALID: u32 = 1 << 1;

/// Floats per vertex: clip x, y, z, w, colour r, g, b, a, u, v, 0, 1.
const VFLOATS: usize = 12;
const VSTRIDE: u32 = (VFLOATS * 4) as u32;
/// Vertex buffer: 4 MiB.
const VB_PAGES: usize = 1024;
const VB_VERTS: usize = VB_PAGES * 4096 / VSTRIDE as usize / 3 * 3;
/// Control lists, shader records and uniforms of one job.
const JOB_PAGES: usize = 32;
/// Largest render target / framebuffer edge the tile memory is sized for.
const MAX_EDGE: u32 = 2048;
/// Textures are resampled to a power of two in 32..=MAX_TEX.
const MAX_TEX: u32 = 512;

fn f(v: f32) -> u32 {
    v.to_bits()
}

struct Shared {
    /// Shader code (all four programs) + default attribute values.
    code: Buffer,
    coord: u32,
    vertex: u32,
    frag: u32,
    frag_tex: u32,
    defaults: u32,
    job: Buffer,
    vb: Buffer,
    tile_alloc: Buffer,
    tile_state: Buffer,
    overflow: Buffer,
}

#[derive(Clone, Copy)]
struct DrawFlags {
    test: bool,
    write: bool,
    blend: bool,
    wire: bool,
}

struct Pending {
    first: u32,
    count: u32,
    tex: Option<usize>,
    flags: DrawFlags,
}

struct Target {
    w: u32,
    h: u32,
    image: Buffer,
    /// Clear colour (0xRRGGBB) for the next job, if it clears.
    clear: Option<u32>,
    verts: Vec<u32>,
    draws: Vec<Pending>,
}

struct Tex {
    /// texture shader state at 0, sampler state at 64, texels at 4096
    buf: Buffer,
}

struct Present {
    staging_phys: u64,
    staging: u32,
    fb: u32,
    w: u32,
    h: u32,
    stride: u32,
    frames: u64,
    rects: u64,
    gpu_us: u64,
    error: Option<String>,
}

struct State {
    shared: Shared,
    targets: Vec<Option<Target>>,
    textures: Vec<Option<Tex>>,
    present: Option<Present>,
}

unsafe impl Send for State {}

/// Lock order: `v3d::GPU` -> `STATE`.
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut Gpu, &mut State) -> Result<T, &'static str>) -> Result<T, &'static str> {
    let mut g = GPU.lock();
    let gpu = g.as_mut().ok_or("no V3D")?;
    let mut s = STATE.lock();
    if s.is_none() {
        let mut out = Vec::new();
        if !gpu.bring_up(&mut out) {
            return Err("the V3D MMU did not come up");
        }
        *s = Some(setup(gpu)?);
    }
    f(gpu, s.as_mut().unwrap())
}

fn setup(gpu: &mut Gpu) -> Result<State, &'static str> {
    let oom = "out of GPU-reachable memory";
    let code = gpu.alloc(2).ok_or(oom)?;
    let mut off = 0usize;
    let mut put = |words: &[u64]| -> u32 {
        let at = off;
        for (k, v) in words.iter().enumerate() {
            code.write(at + k * 8, &v.to_le_bytes());
        }
        off = (at + words.len() * 8 + 255) & !255;
        code.va + at as u32
    };
    let coord = put(&sh::COORD);
    let vertex = put(&sh::VERTEX);
    let frag = put(&sh::FRAGMENT);
    let frag_tex = put(&sh::FRAGMENT_TEX);
    // default attribute values: (0, 0, 0, 1) for every attribute slot
    let defaults_off = off;
    for k in 0..16 {
        let v: [u32; 4] = [0, 0, 0, f(1.0)];
        for (i, x) in v.iter().enumerate() {
            code.write(defaults_off + k * 16 + i * 4, &x.to_le_bytes());
        }
    }
    let defaults = code.va + defaults_off as u32;
    code.sync();
    let tiles = (MAX_EDGE / 64) * (MAX_EDGE / 64);
    let tile_alloc_bytes = ((tiles * 64 + 4095) & !4095) + 8192 + 512 * 1024;
    let job = gpu.alloc(JOB_PAGES).ok_or(oom)?;
    let vb = gpu.alloc(VB_PAGES).ok_or(oom)?;
    let tile_alloc = gpu.alloc(tile_alloc_bytes as usize / 4096).ok_or(oom)?;
    let tile_state = gpu.alloc((tiles * 256) as usize / 4096).ok_or(oom)?;
    let overflow = gpu.alloc(256).ok_or(oom)?;
    Ok(State {
        shared: Shared { code, coord, vertex, frag, frag_tex, defaults, job, vb, tile_alloc, tile_state, overflow },
        targets: Vec::new(),
        textures: Vec::new(),
        present: None,
    })
}

// ---- control lists ---------------------------------------------------------

impl Cl {
    fn op32(&mut self, code: u8, v: u32) {
        self.op(code, &v.to_le_bytes());
    }
    fn op2x32(&mut self, code: u8, a: u32, b: u32) {
        let mut x = [0u8; 8];
        x[..4].copy_from_slice(&a.to_le_bytes());
        x[4..].copy_from_slice(&b.to_le_bytes());
        self.op(code, &x);
    }
    fn align(&mut self, n: usize) {
        while self.bytes.len() % n != 0 {
            self.bytes.push(0);
        }
    }
}

/// LOAD_TILE_BUFFER_GENERAL / STORE: RT0, raster, RGBA8, byte stride.
fn raster_rgba8(stride: u32) -> u64 {
    (27 << 12) | ((stride as u64) << 28)
}
fn load(cl: &mut Cl, stride: u32, address: u32) {
    let mut b = [0u8; 12];
    b[..8].copy_from_slice(&raster_rgba8(stride).to_le_bytes());
    b[8..].copy_from_slice(&address.to_le_bytes());
    cl.op(30, &b);
}

/// The render control list for a `w` x `h` RGBA8 raster image at `image`:
/// `clear` = Some(0xRRGGBB) clears the colour, else it is loaded from the
/// image; `binned` = branch into the binner's tile lists (else a plain copy:
/// `src` is loaded instead of `image`). Only the tiles in `tiles` are drawn.
/// Returns (rcl, generic tile list) for the generic list at GPU `generic`.
#[allow(clippy::too_many_arguments)]
fn rcl(w: u32, h: u32, image: u32, stride: u32, src: Option<(u32, u32)>, clear: Option<u32>, binned: bool, tile_list: u32, generic: u32, tiles: &[(u8, u8)]) -> (Vec<u8>, Vec<u8>) {
    let mut g = Cl { bytes: Vec::new() };
    g.op(125, &[]); // TILE_COORDINATES_IMPLICIT
    if clear.is_none() {
        let (a, s) = src.unwrap_or((image, stride));
        load(&mut g, s, a);
    }
    g.op(26, &[]); // END_OF_LOADS
    if binned {
        g.op(56, &[2]); // PRIM_LIST_FORMAT: triangles (the binner's initial mode)
        g.op(21, &[0]); // BRANCH_TO_IMPLICIT_TILE_LIST set 0
    }
    g.store(raster_rgba8(stride), image);
    g.op(25, &[0b11]); // CLEAR_TILE_BUFFERS: Z/S + colour, ready for the next tile
    g.op(27, &[]); // END_OF_TILE_MARKER
    g.op(18, &[]); // RETURN_FROM_SUB_LIST
    let gen_end = generic + g.bytes.len() as u32;

    let tiles_x = w.div_ceil(64) as u64;
    let tiles_y = h.div_ceil(64) as u64;
    let mut c = Cl { bytes: Vec::new() };
    // common: 1 RT, size, 32 bpp, early Z off, internal depth 32F
    c.op64(121, ((w as u64) << 8) | ((h as u64) << 24) | (1 << 46));
    c.op64(121, 3 | ((0xFF00_0000u64 | clear.unwrap_or(0) as u64) << 8)); // clear colour RT0
    c.op64(121, 1 | (2 << 6)); // colour cfg: RT0 32 bpp, internal type 8
    c.op64(121, 2 | (0x3F80_0000u64 << 16)); // ZS clear: Z 1.0
    c.op(126, &[1 << 2]); // TILE_LIST_INITIAL_BLOCK_SIZE: auto-chained, 64 B
    c.op32(123, tile_list); // MULTICORE_RENDERING_TILE_LIST_SET_BASE (set 0)
    c.op64(122, (tiles_x << 16) | (tiles_y << 24) | (tiles_x << 32) | (tiles_y << 44)); // 1x1-tile supertiles
    for i in 0..2 {
        // GFXH-1742: two dummy tiles; the first clears the tile buffer
        c.op(124, &[0, 0, 0]);
        c.op(26, &[]);
        c.store(8, 0);
        if i == 0 {
            c.op(25, &[0b11]);
        }
        c.op(27, &[]);
    }
    c.op(19, &[]); // FLUSH_VCD_CACHE
    c.op2x32(20, generic, gen_end); // START_ADDRESS_OF_GENERIC_TILE_LIST
    for &(x, y) in tiles {
        c.op(23, &[x, y]); // SUPERTILE_COORDINATES
    }
    c.op(13, &[]); // END_OF_RENDERING
    (c.bytes, g.bytes)
}

fn all_tiles(w: u32, h: u32) -> Vec<(u8, u8)> {
    let mut t = Vec::new();
    for y in 0..h.div_ceil(64) {
        for x in 0..w.div_ceil(64) {
            t.push((x as u8, y as u8));
        }
    }
    t
}

/// The 36-byte GL shader state record + three 16-byte attribute records.
fn shader_record(sh: &Shared, frag: u32, fs_uni: u32, vs_uni: u32, cs_uni: u32, vb: u32, nverts: u32) -> [u8; 36 + 48] {
    let mut b = [0u8; 84];
    // byte 0: enable clipping; byte 1: FS uses the pixel centre W
    b[0] = 1 << 1;
    b[1] = 1 << 4;
    b[3] = 6; // varyings: B, G, R, A, u, v
    b[4] = 1; // CS output segment 1 sector, Ve 0
    b[5] = 1; // CS input segment 1 (shared segments), As - 1 = 0
    b[6] = 2; // VS output segment 2 sectors (10 outputs)
    b[7] = 1; // VS input segment 1, As - 1 = 0
    b[8..12].copy_from_slice(&sh.defaults.to_le_bytes());
    let code = |a: u32, start_final: bool| a | 1 | if start_final { 2 } else { 0 }; // 4-way
    b[12..16].copy_from_slice(&code(frag, false).to_le_bytes());
    b[16..20].copy_from_slice(&fs_uni.to_le_bytes());
    b[20..24].copy_from_slice(&code(sh.vertex, true).to_le_bytes());
    b[24..28].copy_from_slice(&vs_uni.to_le_bytes());
    b[28..32].copy_from_slice(&code(sh.coord, true).to_le_bytes());
    b[32..36].copy_from_slice(&cs_uni.to_le_bytes());
    // attributes: position (CS 4, VS 4), colour (VS 4), uv (vec2, VS 2)
    let attrs: [(u32, u8, u8, u8); 3] = [(0, 0, 4, 4), (16, 0, 0, 4), (32, 2, 0, 2)];
    for (k, &(off, vec, cs, vs)) in attrs.iter().enumerate() {
        let a = &mut b[36 + k * 16..52 + k * 16];
        a[0..4].copy_from_slice(&(vb + off).to_le_bytes());
        a[4] = (vec & 3) | (2 << 2); // vec size (0 = 4), type float
        a[5] = cs | (vs << 4);
        a[8..12].copy_from_slice(&VSTRIDE.to_le_bytes());
        a[12..16].copy_from_slice(&(nverts.max(1) - 1).to_le_bytes());
    }
    b
}

/// Texture shader state (strictly UIF, no XOR, extended) + sampler state.
fn texture_states(base: u32, size: u32) -> ([u8; 32], [u8; 24]) {
    let mut t: u128 = base as u128;
    t |= (size as u128) << 58; // width
    t |= (size as u128) << 72; // height
    t |= 1u128 << 86; // depth
    t |= 4u128 << 100; // RGBA8
    t |= 1u128 << 107; // extended
    t |= (2u128 << 108) | (3u128 << 111) | (4u128 << 114) | (5u128 << 117); // swizzle R G B A
    // max level 0, base level 0
    let mut ts = [0u8; 32];
    ts[..16].copy_from_slice(&t.to_le_bytes());
    ts[16] = 1 << 6; // bit 134: level 0 strictly UIF (UB_PAD 0, XOR off)
    // sampler: mag/min/mip nearest, LOD 0..0, wrap repeat (0)
    let s: u64 = 0b111;
    let mut ss = [0u8; 24];
    ss[..8].copy_from_slice(&s.to_le_bytes());
    (ts, ss)
}

/// Byte offset of pixel (x, y) in a strictly-UIF RGBA8 image of height `h`
/// (Mesa `v3d_get_uif_pixel_offset`, no XOR): 4x4 utiles, 8x8 UIF blocks.
fn uif_offset(x: u32, y: u32, h: u32) -> u32 {
    let (mb_x, mb_y) = (x / 8, y / 8);
    let mb_h = h / 8;
    let mb_id = (mb_x / 4) * ((mb_h - 1) * 4) + mb_x + mb_y * 4;
    let (px, py) = (x % 8, y % 8);
    let tile = if py < 4 { 0 } else { 128 } + if px < 4 { 0 } else { 64 };
    mb_id * 256 + tile + ((py % 4) * 4 + px % 4) * 4
}

// ---- jobs ------------------------------------------------------------------

/// Run the binning list `[start, end)` on CT0 and wait until it flushed.
fn run_bin(gpu: &mut Gpu, sh: &Shared, start: u32, end: u32) -> Result<u64, String> {
    let core = gpu.core;
    w(core, v3d::CTL_L2TCACTL, 1);
    w(core, v3d::CTL_SLCACTL, 0x0F0F_0F0F);
    w(core, v3d::CTL_INT_CLR, 0xFF);
    let bfc0 = r(core, CLE_BFC) & 0xFF;
    w(core, CLE_CT0QMA, sh.tile_alloc.va);
    w(core, CLE_CT0QMS, (sh.tile_alloc.frames.len() * 4096) as u32);
    w(core, CLE_CT0QTS, sh.tile_state.va | TILE_STATE_VALID);
    let t0 = crate::arch::cpu::timestamp();
    w(core, CLE_CT0QBA, start);
    w(core, CLE_CT0QEA, end);
    let mut gave_overflow = false;
    loop {
        let sts = r(core, v3d::CTL_INT_STS);
        if sts & v3d::INT_OUTOMEM != 0 && !gave_overflow {
            // the binner ran out of tile memory: hand it the overflow block
            w(core, PTB_BPOA, sh.overflow.va);
            w(core, PTB_BPOS, (sh.overflow.frames.len() * 4096) as u32);
            w(core, v3d::CTL_INT_CLR, v3d::INT_OUTOMEM);
            gave_overflow = true;
            continue;
        }
        if sts & v3d::INT_GMPV != 0 || (sts & v3d::INT_OUTOMEM != 0 && gave_overflow) {
            let e = format!(
                "binning FAULTED: INT_STS {:#x} CT0CS {:#x} CT0CA {:#x} MMU VIO {:#x}; {}",
                sts,
                r(core, CLE_CT0CS),
                r(core, CLE_CT0CA),
                r(gpu.hub, v3d::MMU_VIO_ADDR),
                gpu.debug_regs()
            );
            gpu.reset();
            return Err(e);
        }
        if sts & v3d::INT_FLDONE != 0 || r(core, CLE_BFC) & 0xFF != bfc0 {
            break;
        }
        if v3d::elapsed_us(t0) > 500_000 {
            let e = format!(
                "binning TIMED OUT (500 ms): INT_STS {:#x} CT0CS {:#x} CT0CA {:#x} (list {:#x}..{:#x}) BFC {}; {}",
                sts,
                r(core, CLE_CT0CS),
                r(core, CLE_CT0CA),
                start,
                end,
                r(core, CLE_BFC),
                gpu.debug_regs()
            );
            gpu.reset();
            return Err(e);
        }
        core::hint::spin_loop();
    }
    let us = v3d::elapsed_us(t0);
    w(core, v3d::CTL_INT_CLR, 0xFF);
    Ok(us)
}

fn render(gpu: &mut Gpu, start: u32, end: u32) -> Result<u64, String> {
    let mut out = Vec::new();
    gpu.run_render(start, end, &mut out).ok_or_else(|| out.join("; "))
}

/// Bin + render everything pending on target `t`. Returns the GPU time (us).
fn flush(gpu: &mut Gpu, st: &mut State, t: usize) -> Result<u64, String> {
    let State { shared: sh, targets, textures, .. } = st;
    let tg = targets.get_mut(t).and_then(|x| x.as_mut()).ok_or("no such target")?;
    if tg.draws.is_empty() && tg.clear.is_none() {
        return Ok(0);
    }
    // vertices
    let nverts = (tg.verts.len() / VFLOATS) as u32;
    sh.vb.write_words(0, &tg.verts);
    sh.vb.sync_range(0, tg.verts.len() * 4);

    let job = &sh.job;
    let (w_, h_) = (tg.w as f32, tg.h as f32);
    // uniforms: CS [x scale, y scale], VS [x scale, y scale, z scale, z offset]
    let mut data = Cl { bytes: Vec::new() };
    let uni_base = job.va + 0x6000;
    let cs_uni = uni_base;
    for v in [f(w_ * 128.0), f(-h_ * 128.0)] {
        data.bytes.extend_from_slice(&v.to_le_bytes());
    }
    let vs_uni = uni_base + data.bytes.len() as u32;
    for v in [f(w_ * 128.0), f(-h_ * 128.0), f(0.5), f(0.5)] {
        data.bytes.extend_from_slice(&v.to_le_bytes());
    }
    let fs_none = uni_base + data.bytes.len() as u32;
    data.bytes.extend_from_slice(&[0; 8]);
    // per-draw shader records (32-byte aligned)
    let mut recs: Vec<u32> = Vec::with_capacity(tg.draws.len());
    for d in &tg.draws {
        let (frag, fs_uni) = match d.tex.and_then(|i| textures.get(i).and_then(|x| x.as_ref())) {
            Some(tx) => {
                data.align(8);
                let at = uni_base + data.bytes.len() as u32;
                data.bytes.extend_from_slice(&(tx.buf.va | 0xF).to_le_bytes()); // P0: state, 4 words
                data.bytes.extend_from_slice(&((tx.buf.va + 64) | 1).to_le_bytes()); // P1: sampler, 32-bit
                (sh.frag_tex, at)
            }
            None => (sh.frag, fs_none),
        };
        data.align(32);
        let at = uni_base + data.bytes.len() as u32;
        let rec = shader_record(sh, frag, fs_uni, vs_uni, cs_uni, sh.vb.va + d.first * VSTRIDE, d.count);
        data.bytes.extend_from_slice(&rec);
        recs.push(at);
    }
    if data.bytes.len() > (JOB_PAGES * 4096 - 0x6000) {
        return Err(String::from("too many draws in one job"));
    }

    // binning list
    let (wi, hi) = (tg.w, tg.h);
    let mut b = Cl { bytes: Vec::new() };
    b.op(119, &[0]); // NUMBER_OF_LAYERS 1
    b.op64(120, (((wi - 1) as u64) << 32) | (((hi - 1) as u64) << 48)); // TILE_BINNING_MODE_CFG
    b.op(19, &[]); // FLUSH_VCD_CACHE
    b.op32(92, 0); // OCCLUSION_QUERY_COUNTER off
    b.op(6, &[]); // START_TILE_BINNING
    b.op64(107, ((wi as u64) << 32) | ((hi as u64) << 48)); // CLIP_WINDOW 0, 0, w, h
    b.op2x32(110, f(w_ * 128.0), f(-h_ * 128.0)); // CLIPPER_XY_SCALING
    b.op2x32(111, f(0.5), f(0.5)); // CLIPPER_Z_SCALE_OFFSET
    b.op2x32(109, f(0.0), f(1.0)); // CLIPPER_Z_MIN_MAX
    let cx = ((wi * 128) & 0x3F_FFFF) as u64;
    let cy = ((hi * 128) & 0x3F_FFFF) as u64;
    b.op64(108, cx | (cy << 32)); // VIEWPORT_OFFSET (s14.8 centre)
    b.op32(91, 0x3F80_0000 | 0xF); // SAMPLE_STATE coverage 1.0, mask 0xF
    b.op32(105, f(1.0)); // LINE_WIDTH
    b.op(97, &[]); // ZERO_ALL_FLAT_SHADE_FLAGS
    b.op(99, &[]); // ZERO_ALL_NON_PERSPECTIVE_FLAGS
    b.op(88, &[]); // ZERO_ALL_CENTROID_FLAGS
    b.op32(87, 0); // COLOR_WRITE_MASKS: write everything
    b.op(71, &[0x22]); // VCM_CACHE_SIZE: 2 batches for binning and rendering
    for (d, &rec) in tg.draws.iter().zip(&recs) {
        let func: u32 = if d.flags.test { 1 } else { 7 }; // LESS / ALWAYS
        let mut cfg: u32 = 0b11 | (func << 12); // both faces (gfx3d culls on the CPU)
        if d.flags.write {
            cfg |= 1 << 15;
        }
        if d.flags.wire {
            cfg |= 1 << 11;
        }
        if d.flags.blend {
            cfg |= 1 << 19;
        }
        b.op(96, &cfg.to_le_bytes()[..3]); // CFG_BITS
        if d.flags.blend {
            b.op(83, &[1]); // BLEND_ENABLES RT0
            // RT mask, colour/alpha: ADD of SRC_ALPHA, INV_SRC_ALPHA
            b.op32(84, (0xF << 24) | (7 << 20) | (6 << 16) | (7 << 8) | (6 << 4));
        } else {
            b.op(83, &[0]);
        }
        b.op32(64, rec | 3); // GL_SHADER_STATE: record, 3 attribute arrays
        let mut p = [0u8; 9];
        p[0] = 4; // TRIANGLES
        p[1..5].copy_from_slice(&d.count.to_le_bytes());
        p[5..9].copy_from_slice(&0u32.to_le_bytes());
        b.op(36, &p); // VERTEX_ARRAY_PRIMS
    }
    b.op(4, &[]); // FLUSH
    let _ = nverts;

    // render list (tile lists from the binner)
    let generic = job.va + 0x4000;
    let (rc, gl) = rcl(wi, hi, tg.image.va, wi * 4, None, tg.clear, true, sh.tile_alloc.va, generic, &all_tiles(wi, hi));
    if b.bytes.len() > 0x2000 || rc.len() > 0x2000 || gl.len() > 0x1000 {
        return Err(String::from("control list too long"));
    }
    job.write(0, &b.bytes);
    job.write(0x2000, &rc);
    job.write(0x4000, &gl);
    job.write(0x6000, &data.bytes);
    job.sync();
    tg.clear = None;
    tg.verts.clear();
    tg.draws.clear();
    // the CPU never writes the image, so it holds no dirty lines: no clean
    // needed before the job, only an invalidate after it
    let bin_us = run_bin(gpu, sh, job.va, job.va + b.bytes.len() as u32)?;
    let render_us = render(gpu, job.va + 0x2000, job.va + 0x2000 + rc.len() as u32)?;
    tg.image.sync();
    Ok(bin_us + render_us)
}

// ---- the gfx3d backend -----------------------------------------------------

fn n_create(w_: u32, h_: u32) -> Result<usize, &'static str> {
    if w_ == 0 || h_ == 0 || w_ > MAX_EDGE || h_ > MAX_EDGE {
        return Err("bad target size");
    }
    with(|gpu, st| {
        let image = gpu.alloc(((w_ * 4 * h_) as usize).div_ceil(4096)).ok_or("out of GPU memory")?;
        st.targets.push(Some(Target { w: w_, h: h_, image, clear: Some(0), verts: Vec::new(), draws: Vec::new() }));
        Ok(st.targets.len())
    })
}

fn n_release(h: usize) {
    let _ = with(|gpu, st| {
        if let Some(t) = st.targets.get_mut(h.wrapping_sub(1)).and_then(|t| t.take()) {
            gpu.free(t.image);
        }
        Ok(())
    });
}

fn n_clear(h: usize, rgb: u32, color: bool, _depth: bool) -> Result<(), &'static str> {
    with(|gpu, st| {
        let t = h.wrapping_sub(1);
        // draws before a clear must land first unless the clear covers them
        let pending = st.targets.get(t).and_then(|x| x.as_ref()).ok_or("no such target")?.draws.is_empty();
        if !pending && !color {
            flush(gpu, st, t).map_err(|_| "V3D job failed")?;
        }
        let tg = st.targets[t].as_mut().unwrap();
        if color {
            tg.verts.clear();
            tg.draws.clear();
            tg.clear = Some(rgb & 0xFF_FFFF);
        }
        // depth is cleared in every tile of every job (it lives in the tile buffer)
        Ok(())
    })
}

fn n_texture(w_: u32, h_: u32, px: &[u32]) -> Result<usize, &'static str> {
    let size = w_.max(h_).next_power_of_two().clamp(32, MAX_TEX);
    with(|gpu, st| {
        let bytes = (size * size * 4) as usize;
        let buf = gpu.alloc(1 + bytes / 4096).ok_or("out of GPU memory")?;
        let base = buf.va + 4096;
        let (ts, ss) = texture_states(base, size);
        buf.write(0, &ts);
        buf.write(64, &ss);
        let mut texels = vec![0u32; (size * size) as usize];
        for y in 0..size {
            let sy = (y * h_ / size) as usize;
            for x in 0..size {
                let sx = (x * w_ / size) as usize;
                texels[(uif_offset(x, y, size) / 4) as usize] = px[sy * w_ as usize + sx];
            }
        }
        buf.write_words(4096, &texels);
        buf.sync();
        st.textures.push(Some(Tex { buf }));
        Ok(st.textures.len())
    })
}

fn n_release_texture(h: usize) {
    let _ = with(|gpu, st| {
        if let Some(t) = st.textures.get_mut(h.wrapping_sub(1)).and_then(|t| t.take()) {
            gpu.free(t.buf);
        }
        Ok(())
    });
}

fn n_draw(h: usize, verts: &[[u32; 12]], tex: Option<usize>, d: crate::gfx3d::NativeDraw) -> Result<(), &'static str> {
    let flags = DrawFlags { test: d.test, write: d.write, blend: d.blend, wire: d.wire };
    with(|gpu, st| {
        let t = h.wrapping_sub(1);
        st.targets.get(t).and_then(|x| x.as_ref()).ok_or("no such target")?;
        let tex = tex.map(|i| i - 1);
        for chunk in verts.chunks(VB_VERTS) {
            let full = {
                let tg = st.targets[t].as_ref().unwrap();
                tg.verts.len() / VFLOATS + chunk.len() > VB_VERTS || tg.draws.len() >= 64
            };
            if full {
                flush(gpu, st, t).map_err(|_| "V3D job failed")?;
            }
            let tg = st.targets[t].as_mut().unwrap();
            let first = (tg.verts.len() / VFLOATS) as u32;
            for v in chunk {
                tg.verts.extend_from_slice(v);
            }
            tg.draws.push(Pending { first, count: chunk.len() as u32, tex, flags });
        }
        Ok(())
    })
}

fn n_read(h: usize, out: &mut [u32]) -> Result<(), &'static str> {
    with(|gpu, st| {
        let t = h.wrapping_sub(1);
        if let Err(e) = flush(gpu, st, t) {
            let mut first = FIRST_ERROR.lock();
            if first.is_empty() {
                *first = e.clone();
            }
            *LAST_ERROR.lock() = e;
            return Err("V3D job failed (gpu v3d shows why)");
        }
        let tg = st.targets[t].as_ref().unwrap();
        let n = ((tg.w * tg.h) as usize).min(out.len());
        tg.image.read_words(0, &mut out[..n]);
        for p in &mut out[..n] {
            *p &= 0xFF_FFFF;
        }
        Ok(())
    })
}

static LAST_ERROR: Mutex<String> = Mutex::new(String::new());
/// The first job error since boot (what broke first).
static FIRST_ERROR: Mutex<String> = Mutex::new(String::new());

/// Step 2d: make V3D a `gfx3d` backend (runs gfx3d's self-test on it; on a
/// failure contexts keep drawing in software).
pub fn install_gfx3d() -> String {
    if let Err(e) = with(|_, _| Ok(())) {
        return format!("[gfx3d] V3D not used: {}", e);
    }
    let r = crate::gfx3d::install_native(crate::gfx3d::Native {
        name: "V3D 4.2",
        create: n_create,
        release: n_release,
        clear: n_clear,
        texture: n_texture,
        release_texture: n_release_texture,
        draw: n_draw,
        read: n_read,
    });
    let e = LAST_ERROR.lock().clone();
    if e.is_empty() { r } else { format!("{} [{}]", r, e) }
}

// ---- 2b / 2c self-test -----------------------------------------------------

fn vtx(x: f32, y: f32, z: f32, c: u32, u: f32, v: f32) -> [u32; 12] {
    // x, y in pixels of a 256x128 target -> clip space (w = 1)
    let cx = x / 128.0 - 1.0;
    let cy = 1.0 - y / 64.0;
    let ch = |s: u32| f(((c >> s) & 0xFF) as f32 / 255.0);
    [f(cx), f(cy), f(z), f(1.0), ch(16), ch(8), ch(0), f(1.0), f(u), f(v), 0, f(1.0)]
}

/// Steps 3-5 of `gpu v3d`: a solid triangle, a Gouraud triangle + depth
/// test, and a textured quad, each read back. Appends to `out`; returns
/// (passed, total).
pub fn self_test(out: &mut Vec<String>) -> (u32, u32) {
    const W: u32 = 256;
    const H: u32 = 128;
    let mut pass = 0;
    let mut total = 0;
    let target = match n_create(W, H) {
        Ok(t) => t,
        Err(e) => {
            out.push(format!("  3D: no render target ({})", e));
            return (0, 3);
        }
    };
    let mut px = vec![0u32; (W * H) as usize];
    let solid = crate::gfx3d::NativeDraw { test: false, write: false, blend: false, wire: false };
    let depth = crate::gfx3d::NativeDraw { test: true, write: true, blend: false, wire: false };
    let at = |px: &[u32], x: u32, y: u32| px[(y * W + x) as usize];
    fn run(target: usize, name: &str, out: &mut Vec<String>, px: &mut [u32], f: &dyn Fn(&[u32]) -> Result<String, String>) -> bool {
        let t0 = crate::arch::cpu::timestamp();
        match n_read(target, px) {
            Err(e) => out.push(format!("  {}: {} -- {}", name, e, LAST_ERROR.lock())),
            Ok(()) => match f(px) {
                Ok(m) => {
                    out.push(format!("  {}: ok ({}; {} us)", name, m, v3d::elapsed_us(t0)));
                    return true;
                }
                Err(m) => out.push(format!("  {}: WRONG -- {}", name, m)),
            },
        }
        false
    }

    // 3. one orange triangle on a dark background (2b)
    let _ = n_clear(target, 0x202020, true, true);
    let tri = [vtx(40.0, 100.0, 0.5, 0xE6661A, 0.0, 0.0), vtx(128.0, 10.0, 0.5, 0xE6661A, 0.0, 0.0), vtx(216.0, 100.0, 0.5, 0xE6661A, 0.0, 0.0)];
    let _ = n_draw(target, &tri, None, solid);
    total += 1;
    pass += run(target, "3 triangle", out, &mut px, &|p| {
        let (c, bg) = (at(p, 128, 70), at(p, 5, 5));
        if close(c, 0xE6661A) && bg == 0x202020 { Ok(format!("centre {:06x}, corner {:06x}", c, bg)) } else { Err(format!("centre {:06x} (want e6661a), corner {:06x} (want 202020)", c, bg)) }
    }) as u32;

    // 4. Gouraud corners + depth: a near green triangle drawn before a far red one (2c)
    let _ = n_clear(target, 0x000000, true, true);
    let g = [vtx(10.0, 120.0, 0.5, 0xFF0000, 0.0, 0.0), vtx(60.0, 10.0, 0.5, 0x00FF00, 0.0, 0.0), vtx(110.0, 120.0, 0.5, 0x0000FF, 0.0, 0.0)];
    let near = [vtx(140.0, 110.0, 0.2, 0x00C000, 0.0, 0.0), vtx(190.0, 20.0, 0.2, 0x00C000, 0.0, 0.0), vtx(240.0, 110.0, 0.2, 0x00C000, 0.0, 0.0)];
    let far = [vtx(130.0, 100.0, 0.8, 0xC00000, 0.0, 0.0), vtx(190.0, 30.0, 0.8, 0xC00000, 0.0, 0.0), vtx(250.0, 100.0, 0.8, 0xC00000, 0.0, 0.0)];
    let _ = n_draw(target, &g, None, depth);
    let _ = n_draw(target, &near, None, depth);
    let _ = n_draw(target, &far, None, depth);
    total += 1;
    pass += run(target, "4 gouraud + depth", out, &mut px, &|p| {
        let red_corner = at(p, 16, 116);
        let mix = at(p, 60, 85);
        let overlap = at(p, 190, 80);
        let far_only = at(p, 135, 99);
        let ok = (red_corner >> 16) > 0xB0 && (red_corner & 0xFFFF) < 0x4040 && mix & 0xFF > 0x20 && (mix >> 16) & 0xFF > 0x20 && close(overlap, 0x00C000) && close(far_only, 0xC00000);
        let m = format!("red corner {:06x}, mix {:06x}, overlap {:06x} (near green wins), far only {:06x}", red_corner, mix, overlap, far_only);
        if ok { Ok(m) } else { Err(m) }
    }) as u32;

    // 5. a textured quad: 2x2 yellow/blue checker at half light (texel x colour)
    let checker = [0xFFFFFF00u32, 0xFF0000FF, 0xFF0000FF, 0xFFFFFF00];
    let tex = n_texture(2, 2, &checker);
    let _ = n_clear(target, 0x000000, true, true);
    match tex {
        Ok(t) => {
            let c = 0x808080;
            let q = [
                vtx(64.0, 0.0, 0.5, c, 0.0, 0.0),
                vtx(64.0, 128.0, 0.5, c, 0.0, 1.0),
                vtx(192.0, 128.0, 0.5, c, 1.0, 1.0),
                vtx(64.0, 0.0, 0.5, c, 0.0, 0.0),
                vtx(192.0, 128.0, 0.5, c, 1.0, 1.0),
                vtx(192.0, 0.0, 0.5, c, 1.0, 0.0),
            ];
            let _ = n_draw(target, &q, Some(t), solid);
            total += 1;
            pass += run(target, "5 texture", out, &mut px, &|p| {
                let (a, b2, c2, d) = (at(p, 80, 16), at(p, 176, 16), at(p, 80, 112), at(p, 176, 112));
                let ok = close(a, 0x808000) && close(d, 0x808000) && close(b2, 0x000080) && close(c2, 0x000080);
                let m = format!("quadrants {:06x} {:06x} / {:06x} {:06x} (want 808000 000080 / 000080 808000)", a, b2, c2, d);
                if ok { Ok(m) } else { Err(m) }
            }) as u32;
            n_release_texture(t);
        }
        Err(e) => {
            total += 1;
            out.push(format!("  5 texture: {}", e));
        }
    }
    show(target, out);
    n_release(target);
    (pass, total)
}

fn close(a: u32, b: u32) -> bool {
    (0..3).all(|k| {
        let (x, y) = ((a >> (k * 8)) & 0xFF, (b >> (k * 8)) & 0xFF);
        x.abs_diff(y) <= 6
    })
}

/// Show the last test image at the top right of the screen for 4 s.
fn show(target: usize, out: &mut Vec<String>) {
    let Some(fb) = crate::boot::info().framebuffer else { return };
    let mut px = vec![0u32; 256 * 128];
    if n_read(target, &mut px).is_err() || fb.bytes_per_pixel != 4 || fb.width < 272 || fb.height < 144 {
        return;
    }
    let x0 = fb.width - 256 - 16;
    for y in 0..128 {
        for x in 0..256 {
            let p = px[y * 256 + x];
            let v = match fb.format {
                crate::boot::PixelFormat::Rgb => ((p & 0xFF) << 16) | (p & 0xFF00) | ((p >> 16) & 0xFF),
                _ => p,
            };
            unsafe { ((fb.addr + (((16 + y) * fb.stride + x0 + x) * 4) as u64) as *mut u32).write_volatile(v) };
        }
    }
    out.push(String::from("  the last image is shown at the top right for 4 s"));
    v3d::wait_us(4_000_000, || false);
    crate::framebuffer::forget_screen();
    crate::wm::screen_overwritten();
}

// ---- 2e: presenting the desktop through V3D --------------------------------

/// Install the V3D copy as the console's screen accelerator.
pub fn enable_present() -> String {
    let Some(fb) = crate::boot::info().framebuffer else { return String::from("no framebuffer") };
    let phys = crate::arch::aarch64::rpi::framebuffer_phys();
    if phys == 0 || fb.bytes_per_pixel != 4 || !matches!(fb.format, crate::boot::PixelFormat::Bgr) {
        return String::from("the framebuffer is not a 32-bit BGR mailbox framebuffer");
    }
    if fb.width as u32 > MAX_EDGE || fb.height as u32 > MAX_EDGE {
        return String::from("screen larger than the tile memory");
    }
    let stride = (fb.stride * 4) as u32;
    let pages = (stride as usize * fb.height).div_ceil(4096);
    let r = with(|gpu, st| {
        if st.present.is_none() {
            let (sphys, _sva) = crate::memory::alloc_dma_contiguous(pages).ok_or("no contiguous memory for the staging surface")?;
            let staging = gpu.map_contig(sphys, pages).ok_or("cannot map the staging surface")?;
            let fbva = gpu.map_contig(phys & !0xFFF, pages + 1).ok_or("cannot map the framebuffer")? + (phys & 0xFFF) as u32;
            st.present = Some(Present { staging_phys: sphys, staging, fb: fbva, w: fb.width as u32, h: fb.height as u32, stride, frames: 0, rects: 0, gpu_us: 0, error: None });
        }
        Ok(st.present.as_ref().unwrap().staging_phys)
    });
    match r {
        Ok(sphys) => {
            crate::framebuffer::set_accel(Some(crate::framebuffer::Accel {
                staging: crate::memory::phys_to_virt(sphys) as usize,
                stride: stride as usize,
                flush: |va, len| crate::arch::cpu::dma_sync(va as u64, len),
                blit: present_blit,
            }));
            format!("the desktop is presented by V3D ({}x{}, staging {} KiB)", fb.width, fb.height, pages * 4)
        }
        Err(e) => format!("V3D present not enabled: {}", e),
    }
}

pub fn disable_present() {
    crate::framebuffer::set_accel(None);
}

/// Copy the tiles covering `rects` from the staging surface to the screen.
fn present_blit(rects: &[(u32, u32, u32, u32)]) -> bool {
    let mut g = GPU.lock();
    let Some(gpu) = g.as_mut() else { return false };
    let mut s = STATE.lock();
    let Some(st) = s.as_mut() else { return false };
    let Some(p) = st.present.as_mut() else { return false };
    let (tx, ty) = (p.w.div_ceil(64), p.h.div_ceil(64));
    let mut hit = vec![false; (tx * ty) as usize];
    for &(x, y, rw, rh) in rects {
        if rw == 0 || rh == 0 {
            continue;
        }
        for j in y / 64..((y + rh).min(p.h)).div_ceil(64) {
            for i in x / 64..((x + rw).min(p.w)).div_ceil(64) {
                hit[(j * tx + i) as usize] = true;
            }
        }
    }
    let tiles: Vec<(u8, u8)> = (0..tx * ty).filter(|&k| hit[k as usize]).map(|k| ((k % tx) as u8, (k / tx) as u8)).collect();
    if tiles.is_empty() {
        return true;
    }
    let job = &st.shared.job;
    let generic = job.va + 0x7800;
    let (rc, gl) = rcl(p.w, p.h, p.fb, p.stride, Some((p.staging, p.stride)), None, false, st.shared.tile_alloc.va, generic, &tiles);
    if rc.len() > 0x1800 {
        p.error = Some(String::from("present list too long"));
        return false;
    }
    job.write(0x6000, &rc);
    job.write(0x7800, &gl);
    job.sync();
    let mut out = Vec::new();
    match gpu.run_render(job.va + 0x6000, job.va + 0x6000 + rc.len() as u32, &mut out) {
        Some(us) => {
            p.frames += 1;
            p.rects += rects.len() as u64;
            p.gpu_us += us;
            true
        }
        None => {
            p.error = Some(out.join("; "));
            false
        }
    }
}

/// `gpu` status lines for the V3D paths.
pub fn report() -> Vec<String> {
    let mut v = Vec::new();
    if let Some(st) = STATE.try_lock().as_ref().and_then(|s| s.as_ref()) {
        let n = st.targets.iter().filter(|t| t.is_some()).count();
        v.push(format!("  V3D 3D: {} render target(s), {} texture(s)", n, st.textures.iter().filter(|t| t.is_some()).count()));
        if let Some(p) = &st.present {
            v.push(format!(
                "  V3D present: {} frame(s), {} rectangle(s), {} us GPU per frame{}",
                p.frames,
                p.rects,
                if p.frames > 0 { p.gpu_us / p.frames } else { 0 },
                p.error.as_ref().map(|e| format!("; stopped: {}", e)).unwrap_or_default()
            ));
        }
    }
    let e = FIRST_ERROR.lock();
    if !e.is_empty() {
        v.push(format!("  first V3D job error: {}", e));
    }
    let e = LAST_ERROR.lock();
    if !e.is_empty() {
        v.push(format!("  last V3D job error: {}", e));
    }
    v.push(format!("  V3D resets: {}", v3d::RESETS.load(core::sync::atomic::Ordering::Relaxed)));
    v
}
