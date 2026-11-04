//! virgl: the G3 GPU backend over virtio-gpu 3D (QEMU `virtio-gpu-gl`, which
//! hands the commands to virglrenderer and the host's OpenGL).
//!
//! The division of labour keeps one pipeline for both backends: `gfx3d.rs`
//! still runs the vertex stage (transform, per-vertex lighting, fog -- fixed
//! point, identical results), and the host GPU does what costs per pixel:
//! clipping, rasterization, depth test, texturing, blending, wireframe. The
//! vertices go up as `R32G32B32A32_FLOAT` clip position + `R8G8B8A8_UNORM`
//! color + `R32G32_FLOAT` uv through RESOURCE_INLINE_WRITE into one vertex
//! buffer, drawn as triangle lists by pass-through TGSI shaders (the fragment
//! shader multiplies color and texel and discards texels with alpha 0, like
//! the software rasterizer). A picture is read back (TRANSFER_FROM_HOST_3D
//! into guest backing) only when it is presented or read, and flipped: GL's
//! rows run bottom-up.
//!
//! Protocol: virglrenderer's `virgl_protocol.h` / `vrend_decode.c` (command
//! header = cmd | object type << 8 | payload dwords << 16) and the virtio-gpu
//! 3D commands (CTX_CREATE 0x200, CTX_ATTACH_RESOURCE 0x202,
//! RESOURCE_CREATE_3D 0x204, TRANSFER_FROM_HOST_3D 0x206, SUBMIT_3D 0x207).
//! Errors inside a command stream are not reported back (the host marks the
//! context broken), which is why `gfx3d`'s self-test checks pixels.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::gpu;

const CTX: u32 = 1;
const CTX_CREATE: u32 = 0x0200;
const CTX_ATTACH_RESOURCE: u32 = 0x0202;
const CTX_DETACH_RESOURCE: u32 = 0x0203;
const RESOURCE_CREATE_3D: u32 = 0x0204;
const TRANSFER_FROM_HOST_3D: u32 = 0x0206;
const RESOURCE_UNREF: u32 = 0x0102;
const RESOURCE_ATTACH_BACKING: u32 = 0x0106;
const GET_CAPSET_INFO: u32 = 0x0108;
const GET_CAPSET: u32 = 0x0109;
const FMT_Z16: u32 = 16;
const FMT_Z32F: u32 = 18;
const FMT_Z24X8: u32 = 21;

// virgl_context_cmd
const CREATE_OBJECT: u32 = 1;
const BIND_OBJECT: u32 = 2;
const DESTROY_OBJECT: u32 = 3;
const SET_VIEWPORT_STATE: u32 = 4;
const SET_FRAMEBUFFER_STATE: u32 = 5;
const SET_VERTEX_BUFFERS: u32 = 6;
const CLEAR: u32 = 7;
const DRAW_VBO: u32 = 8;
const RESOURCE_INLINE_WRITE: u32 = 9;
const SET_SAMPLER_VIEWS: u32 = 10;
const BIND_SAMPLER_STATES: u32 = 18;
const BIND_SHADER: u32 = 31;

// virgl_object_type
const OBJ_BLEND: u32 = 1;
const OBJ_RASTERIZER: u32 = 2;
const OBJ_DSA: u32 = 3;
const OBJ_SHADER: u32 = 4;
const OBJ_VERTEX_ELEMENTS: u32 = 5;
const OBJ_SAMPLER_VIEW: u32 = 6;
const OBJ_SAMPLER_STATE: u32 = 7;
const OBJ_SURFACE: u32 = 8;

// virgl formats
const FMT_B8G8R8A8: u32 = 1;
const FMT_RG32F: u32 = 29;
const FMT_RGBA32F: u32 = 31;
const FMT_R8: u32 = 64;
const FMT_RGBA8: u32 = 67;

// bind flags
const BIND_DEPTH_STENCIL: u32 = 1 << 0;
const BIND_RENDER_TARGET: u32 = 1 << 1;
const BIND_SAMPLER_VIEW: u32 = 1 << 3;
const BIND_VERTEX_BUFFER: u32 = 1 << 4;

// pipe enums
const TARGET_BUFFER: u32 = 0;
const TARGET_2D: u32 = 2;
const SHADER_VERTEX: u32 = 0;
const SHADER_FRAGMENT: u32 = 1;
const PRIM_TRIANGLES: u32 = 4;
const CLEAR_DEPTH: u32 = 1 << 0;
const CLEAR_STENCIL: u32 = 1 << 1;
const CLEAR_COLOR0: u32 = 1 << 2;

// object handles (one virgl context, created once)
const H_VS: u32 = 1;
const H_FS_TEX: u32 = 2;
const H_FS_COLOR: u32 = 3;
const H_VE: u32 = 4;
const H_RAST: u32 = 10; // + cull_none | wireframe << 1
const H_DSA: u32 = 20; // + test | write << 1
const H_BLEND: u32 = 30; // + blend
const H_SAMPLER: u32 = 40;
const VB_RES: u32 = 100;
/// Bytes per vertex: position 4 x f32, color 4 x u8, uv 2 x f32.
const STRIDE: usize = 28;
/// Vertices per inline write (keeps one submit under the command buffer).
const CHUNK: usize = 3600;
const VB_BYTES: u32 = (CHUNK * STRIDE) as u32;

static READY: AtomicBool = AtomicBool::new(false);
static TRIED: AtomicBool = AtomicBool::new(false);
/// Next resource id (the 2D driver uses 1 and 2) / object handle.
static NEXT_RES: AtomicU32 = AtomicU32::new(VB_RES + 1);
static NEXT_HANDLE: AtomicU32 = AtomicU32::new(1000);
/// Formats the host supports, chosen from the capset at init: depth, and
/// whether colors go as B8G8R8A8 (else R8G8B8A8, with red and blue swapped
/// on the way in and out).
static DEPTH_FMT: AtomicU32 = AtomicU32::new(FMT_Z24X8);
static COLOR_BGRA: AtomicBool = AtomicBool::new(true);
static TEX_BGRA: AtomicBool = AtomicBool::new(true);

fn color_fmt() -> u32 {
    if COLOR_BGRA.load(Ordering::Relaxed) { FMT_B8G8R8A8 } else { FMT_RGBA8 }
}

fn tex_fmt() -> u32 {
    if TEX_BGRA.load(Ordering::Relaxed) { FMT_B8G8R8A8 } else { FMT_RGBA8 }
}

/// 0xAARRGGBB <-> 0xAABBGGRR.
fn swap_rb(p: u32) -> u32 {
    (p & 0xFF00_FF00) | (p >> 16 & 0xFF) | (p & 0xFF) << 16
}

/// Off: everything renders in software (shell `gfx3d sw`).
static ENABLED: AtomicBool = AtomicBool::new(true);

const VS: &str = "VERT\nDCL IN[0]\nDCL IN[1]\nDCL IN[2]\nDCL OUT[0], POSITION\nDCL OUT[1], GENERIC[0]\nDCL OUT[2], GENERIC[1]\n  0: MOV OUT[0], IN[0]\n  1: MOV OUT[1], IN[1]\n  2: MOV OUT[2], IN[2]\n  3: END\n";
const FS_COLOR: &str = "FRAG\nPROPERTY FS_COLOR0_WRITES_ALL_CBUFS 1\nDCL IN[0], GENERIC[0], PERSPECTIVE\nDCL OUT[0], COLOR\n  0: MOV OUT[0], IN[0]\n  1: END\n";
const FS_TEX: &str = "FRAG\nPROPERTY FS_COLOR0_WRITES_ALL_CBUFS 1\nDCL IN[0], GENERIC[0], PERSPECTIVE\nDCL IN[1], GENERIC[1], PERSPECTIVE\nDCL OUT[0], COLOR\nDCL SAMP[0]\nDCL SVIEW[0], 2D, FLOAT\nDCL TEMP[0..1]\nIMM[0] FLT32 { -0.0020, 0.0000, 0.0000, 0.0000}\n  0: TEX TEMP[0], IN[1], SAMP[0], 2D\n  1: ADD TEMP[1], TEMP[0].wwww, IMM[0].xxxx\n  2: KILL_IF TEMP[1].xxxx\n  3: MUL OUT[0], TEMP[0], IN[0]\n  4: END\n";

/// An f32 (as bits) from 16.16 fixed point, without floating point.
pub fn f32_from_fx(v: i64) -> u32 {
    if v == 0 {
        return 0;
    }
    let sign = if v < 0 { 1u32 << 31 } else { 0 };
    let m = v.unsigned_abs();
    let msb = 63 - m.leading_zeros() as i32; // value = m * 2^-16
    let exp = msb - 16 + 127;
    if exp <= 0 {
        return sign;
    }
    let mant = if msb >= 23 { (m >> (msb - 23)) as u32 } else { (m << (23 - msb)) as u32 } & 0x7F_FFFF;
    sign | ((exp as u32) << 23) | mant
}

const ONE_F: u32 = 0x3F80_0000;
const HALF_F: u32 = 0x3F00_0000;

fn hdr(cmd: u32, obj: u32, len: usize) -> u32 {
    cmd | obj << 8 | (len as u32) << 16
}

fn push(cs: &mut Vec<u32>, cmd: u32, obj: u32, payload: &[u32]) {
    cs.push(hdr(cmd, obj, payload.len()));
    cs.extend_from_slice(payload);
}

fn shader(cs: &mut Vec<u32>, handle: u32, kind: u32, text: &str) {
    let bytes = text.as_bytes();
    let len = bytes.len() + 1; // with the NUL
    let words = len.div_ceil(4);
    let mut payload = alloc::vec![handle, kind, len as u32, 300, 0];
    let mut w = alloc::vec![0u32; words];
    for (i, b) in bytes.iter().enumerate() {
        w[i / 4] |= (*b as u32) << ((i % 4) * 8);
    }
    payload.extend_from_slice(&w);
    push(cs, CREATE_OBJECT, OBJ_SHADER, &payload);
}

fn submit(cs: &[u32]) -> Result<(), &'static str> {
    gpu::with(|g| g.submit3d(CTX, cs)).unwrap_or(Err("virtio-gpu: none"))
}

/// The 3D context and the fixed state objects, made on first use.
pub fn available() -> bool {
    if !ENABLED.load(Ordering::Relaxed) {
        return false;
    }
    if READY.load(Ordering::Acquire) {
        return true;
    }
    if TRIED.swap(true, Ordering::AcqRel) {
        return READY.load(Ordering::Acquire);
    }
    match init() {
        Ok(v) => {
            crate::println!("[virgl] 3D context up ({}): the GPU draws for Buitenzorg.Graphics3D", v);
            READY.store(true, Ordering::Release);
            true
        }
        Err(e) => {
            crate::println!("[virgl] not used: {}", e);
            false
        }
    }
}

/// Use the GPU (true) or software only (false) for contexts made from now on.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

fn init() -> Result<alloc::string::String, &'static str> {
    let has = gpu::with(|g| g.virgl).ok_or("no virtio GPU")?;
    if !has {
        return Err("the virtio GPU has no 3D (start QEMU with virtio-gpu-gl and gl=on)");
    }
    // capset 0: VIRGL or VIRGL2, version, size (informational)
    let info = gpu::with(|g| g.query(GET_CAPSET_INFO, &[0, 0], 3)).ok_or("gpu")??;
    let (id, ver, size) = (info.1[0], info.1[1], info.1[2] as usize);
    // virgl_caps_v1: max_version, then 16-word format masks: sampler, render, depth/stencil
    let caps = gpu::with(|g| g.query(GET_CAPSET, &[id, ver], (size / 4).min(500))).ok_or("gpu")??.1;
    let has = |mask: usize, f: u32| caps.get(1 + mask * 16 + (f / 32) as usize).is_some_and(|w| w & (1 << (f % 32)) != 0);
    // a v1 capset may leave the depth mask empty: vrend's base table always
    // has Z24X8 (but not Z24_UNORM_S8_UINT), so that is the default
    let depth = [FMT_Z24X8, FMT_Z32F, FMT_Z16].into_iter().find(|&f| has(2, f)).unwrap_or(FMT_Z24X8);
    DEPTH_FMT.store(depth, Ordering::Relaxed);
    if !has(1, FMT_B8G8R8A8) && !has(1, FMT_RGBA8) {
        return Err("no 8-bit color render format");
    }
    COLOR_BGRA.store(has(1, FMT_B8G8R8A8), Ordering::Relaxed);
    TEX_BGRA.store(has(0, FMT_B8G8R8A8), Ordering::Relaxed);
    let name = b"buitenzorg-g3";
    let mut args = alloc::vec![name.len() as u32, 0];
    let mut nm = [0u32; 16];
    for (i, b) in name.iter().enumerate() {
        nm[i / 4] |= (*b as u32) << ((i % 4) * 8);
    }
    args.extend_from_slice(&nm);
    gpu::with(|g| g.ok_ctx(CTX_CREATE, CTX, &args)).ok_or("gpu")??;
    // the vertex buffer
    create_resource(VB_RES, TARGET_BUFFER, FMT_R8, BIND_VERTEX_BUFFER, VB_BYTES, 1)?;

    let mut cs = Vec::new();
    shader(&mut cs, H_VS, SHADER_VERTEX, VS);
    shader(&mut cs, H_FS_COLOR, SHADER_FRAGMENT, FS_COLOR);
    shader(&mut cs, H_FS_TEX, SHADER_FRAGMENT, FS_TEX);
    // position, color, uv from buffer 0
    push(&mut cs, CREATE_OBJECT, OBJ_VERTEX_ELEMENTS, &[H_VE, 0, 0, 0, FMT_RGBA32F, 16, 0, 0, FMT_RGBA8, 20, 0, 0, FMT_RG32F]);
    for k in 0..4u32 {
        let cull = if k & 1 != 0 { 0 } else { 2 }; // none / back
        let fill = if k & 2 != 0 { 1 } else { 0 }; // line / fill
        // depth clip, cull, fill front+back, front = counter-clockwise, half-pixel centers
        let s0 = 1 << 1 | cull << 8 | fill << 10 | fill << 12 | 1 << 15 | 1 << 29;
        push(&mut cs, CREATE_OBJECT, OBJ_RASTERIZER, &[H_RAST + k, s0, ONE_F, 0, 0, ONE_F, 0, 0, 0]);
    }
    for k in 0..4u32 {
        let test = k & 1 != 0;
        let write = k & 2 != 0;
        // depth enable, write mask, func LESS (1) / ALWAYS (7)
        let s0 = 1 | (write as u32) << 1 | (if test { 1 } else { 7 }) << 2;
        push(&mut cs, CREATE_OBJECT, OBJ_DSA, &[H_DSA + k, s0, 0, 0, 0]);
    }
    for k in 0..2u32 {
        let rt = if k == 1 {
            // enable, ADD, SRC_ALPHA (3), INV_SRC_ALPHA (0x13), same for alpha, mask RGBA
            1 | 3 << 4 | 0x13 << 9 | 3 << 17 | 0x13 << 22 | 0xF << 27
        } else {
            0xF << 27
        };
        let mut p = alloc::vec![H_BLEND + k, 0, 0];
        p.extend_from_slice(&[rt, 0, 0, 0, 0, 0, 0, 0]);
        push(&mut cs, CREATE_OBJECT, OBJ_BLEND, &p);
    }
    // repeat wrap (0), nearest filters, no mipmaps (MIN_MIP NONE = 2)
    push(&mut cs, CREATE_OBJECT, OBJ_SAMPLER_STATE, &[H_SAMPLER, 2 << 11, 0, 0, 0, 0, 0, 0, 0]);
    push(&mut cs, BIND_SHADER, 0, &[H_VS, SHADER_VERTEX]);
    push(&mut cs, BIND_OBJECT, OBJ_VERTEX_ELEMENTS, &[H_VE]);
    submit(&cs)?;
    Ok(alloc::format!(
        "capset {} v{}, depth format {}, color {}, textures {}",
        id,
        ver,
        depth,
        if COLOR_BGRA.load(Ordering::Relaxed) { "BGRA8" } else { "RGBA8" },
        if TEX_BGRA.load(Ordering::Relaxed) { "BGRA8" } else { "RGBA8" }
    ))
}

fn create_resource(res: u32, target: u32, format: u32, bind: u32, w: u32, h: u32) -> Result<(), &'static str> {
    gpu::with(|g| {
        g.ok_ctx(RESOURCE_CREATE_3D, CTX, &[res, target, format, bind, w, h, 1, 1, 0, 0, 0, 0])?;
        g.ok_ctx(CTX_ATTACH_RESOURCE, CTX, &[res, 0])
    })
    .ok_or("gpu")?
}

fn unref(res: u32) {
    let _ = gpu::with(|g| {
        let _ = g.ok_ctx(CTX_DETACH_RESOURCE, CTX, &[res, 0]);
        g.ok_ctx(RESOURCE_UNREF, CTX, &[res, 0])
    });
}

/// A render target: color (read back into `backing`) + depth.
pub struct Target {
    pub w: u32,
    pub h: u32,
    color: u32,
    depth: u32,
    csurf: u32,
    zsurf: u32,
    backing: *mut u32,
}

unsafe impl Send for Target {}
unsafe impl Sync for Target {}

pub fn create_target(w: u32, h: u32) -> Result<Target, &'static str> {
    let bytes = (w * h * 4) as usize;
    let (phys, virt) = crate::memory::alloc_dma_contiguous(bytes.div_ceil(4096)).ok_or("virgl: no contiguous memory for the read-back")?;
    let color = NEXT_RES.fetch_add(1, Ordering::Relaxed);
    let depth = NEXT_RES.fetch_add(1, Ordering::Relaxed);
    create_resource(color, TARGET_2D, color_fmt(), BIND_RENDER_TARGET | BIND_SAMPLER_VIEW, w, h)?;
    create_resource(depth, TARGET_2D, DEPTH_FMT.load(Ordering::Relaxed), BIND_DEPTH_STENCIL, w, h)?;
    gpu::with(|g| g.ok_ctx(RESOURCE_ATTACH_BACKING, CTX, &[color, 1, phys as u32, (phys >> 32) as u32, bytes as u32, 0])).ok_or("gpu")??;
    let csurf = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    let zsurf = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    let mut cs = Vec::new();
    push(&mut cs, CREATE_OBJECT, OBJ_SURFACE, &[csurf, color, color_fmt(), 0, 0]);
    push(&mut cs, CREATE_OBJECT, OBJ_SURFACE, &[zsurf, depth, DEPTH_FMT.load(Ordering::Relaxed), 0, 0]);
    submit(&cs)?;
    Ok(Target { w, h, color, depth, csurf, zsurf, backing: virt as *mut u32 })
}

impl Drop for Target {
    fn drop(&mut self) {
        let mut cs = Vec::new();
        push(&mut cs, DESTROY_OBJECT, OBJ_SURFACE, &[self.csurf]);
        push(&mut cs, DESTROY_OBJECT, OBJ_SURFACE, &[self.zsurf]);
        let _ = submit(&cs);
        unref(self.color);
        unref(self.depth);
        // the read-back pages stay with the device's backing until the
        // resource is gone; they are leaked rather than reused under it
    }
}

fn bind_target(cs: &mut Vec<u32>, t: &Target) {
    push(cs, SET_FRAMEBUFFER_STATE, 0, &[1, t.zsurf, t.csurf]);
    let (hw, hh) = (f32_from_fx((t.w as i64) << 15), f32_from_fx((t.h as i64) << 15));
    push(cs, SET_VIEWPORT_STATE, 0, &[0, hw, hh, HALF_F, hw, hh, HALF_F]);
}

/// Fill color (`0xRRGGBB`) and reset depth.
pub fn clear(t: &Target, rgb: u32, color: bool, depth: bool) -> Result<(), &'static str> {
    let ch = |s: u32| f32_from_fx((((rgb >> s) & 0xFF) as i64 * 65536) / 255);
    let mut cs = Vec::new();
    bind_target(&mut cs, t);
    let bits = if color { CLEAR_COLOR0 } else { 0 } | if depth { CLEAR_DEPTH | CLEAR_STENCIL } else { 0 };
    // color r, g, b, a, then depth 1.0 as a double (low word first), stencil
    push(&mut cs, CLEAR, 0, &[bits, ch(16), ch(8), ch(0), ONE_F, 0, 0x3FF0_0000, 0]);
    submit(&cs)
}

/// A texture on the GPU: (resource, sampler view handle).
pub struct GpuTexture {
    res: u32,
    view: u32,
}

pub fn texture(w: u32, h: u32, px: &[u32]) -> Result<GpuTexture, &'static str> {
    let res = NEXT_RES.fetch_add(1, Ordering::Relaxed);
    let bgra = TEX_BGRA.load(Ordering::Relaxed);
    create_resource(res, TARGET_2D, tex_fmt(), BIND_SAMPLER_VIEW, w, h)?;
    // rows in pieces that fit a submit
    let rows_per = ((gpu::CBUF_BYTES - 256) / (w as usize * 4)).max(1);
    let mut y = 0;
    while y < h as usize {
        let n = rows_per.min(h as usize - y);
        let mut cs = Vec::with_capacity(13 + n * w as usize);
        let mut p = alloc::vec![res, 0, 0, w * 4, 0, 0, y as u32, 0, w, n as u32, 1];
        let rows = &px[y * w as usize..(y + n) * w as usize];
        if bgra {
            p.extend_from_slice(rows);
        } else {
            p.extend(rows.iter().map(|&c| swap_rb(c)));
        }
        push(&mut cs, RESOURCE_INLINE_WRITE, 0, &p);
        submit(&cs)?;
        y += n;
    }
    let view = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    let mut cs = Vec::new();
    // format | target << 24, layers, levels, swizzle identity (x y z w)
    push(&mut cs, CREATE_OBJECT, OBJ_SAMPLER_VIEW, &[view, res, tex_fmt() | TARGET_2D << 24, 0, 0, 1 << 3 | 2 << 6 | 3 << 9]);
    submit(&cs)?;
    Ok(GpuTexture { res, view })
}

impl Drop for GpuTexture {
    fn drop(&mut self) {
        let mut cs = Vec::new();
        push(&mut cs, DESTROY_OBJECT, OBJ_SAMPLER_VIEW, &[self.view]);
        let _ = submit(&cs);
        unref(self.res);
    }
}

/// One vertex for the GPU: clip position (f32 bits), color (R, G, B, A
/// bytes), uv (f32 bits).
#[derive(Clone, Copy)]
pub struct GVertex(pub [u32; 7]);

/// Draw triangle-list vertices into `t`.
pub fn draw(t: &Target, verts: &[GVertex], tex: Option<&GpuTexture>, cull: bool, wire: bool, test: bool, write: bool, blend: bool) -> Result<(), &'static str> {
    let mut cs = Vec::with_capacity(64);
    bind_target(&mut cs, t);
    // culling is the CPU's job (gfx3d drops back faces in front of the eye):
    // vrend renders into an FBO upside down, which flips the winding the GPU sees
    let _ = cull;
    push(&mut cs, BIND_OBJECT, OBJ_RASTERIZER, &[H_RAST + 1 + ((wire as u32) << 1)]);
    push(&mut cs, BIND_OBJECT, OBJ_DSA, &[H_DSA + test as u32 + ((write as u32) << 1)]);
    push(&mut cs, BIND_OBJECT, OBJ_BLEND, &[H_BLEND + blend as u32]);
    match tex {
        Some(tx) => {
            push(&mut cs, BIND_SHADER, 0, &[H_FS_TEX, SHADER_FRAGMENT]);
            push(&mut cs, SET_SAMPLER_VIEWS, 0, &[SHADER_FRAGMENT, 0, tx.view]);
            push(&mut cs, BIND_SAMPLER_STATES, 0, &[SHADER_FRAGMENT, 0, H_SAMPLER]);
        }
        None => push(&mut cs, BIND_SHADER, 0, &[H_FS_COLOR, SHADER_FRAGMENT]),
    }
    push(&mut cs, SET_VERTEX_BUFFERS, 0, &[STRIDE as u32, 0, VB_RES]);
    for chunk in verts.chunks(CHUNK - CHUNK % 3) {
        let bytes = (chunk.len() * STRIDE) as u32;
        let mut p = Vec::with_capacity(11 + chunk.len() * 7);
        p.extend_from_slice(&[VB_RES, 0, 0, 0, 0, 0, 0, 0, bytes, 1, 1]);
        for v in chunk {
            p.extend_from_slice(&v.0);
        }
        push(&mut cs, RESOURCE_INLINE_WRITE, 0, &p);
        let n = chunk.len() as u32;
        push(&mut cs, DRAW_VBO, 0, &[0, n, PRIM_TRIANGLES, 0, 1, 0, 0, 0, 0, 0, n.saturating_sub(1), 0]);
        submit(&cs)?;
        cs.clear();
    }
    if !cs.is_empty() {
        submit(&cs)?;
    }
    Ok(())
}

/// Read the color buffer into `out` (`w * h`, top row first, 0x00RRGGBB).
pub fn read(t: &Target, out: &mut [u32]) -> Result<(), &'static str> {
    let (w, h) = (t.w, t.h);
    // box x, y, z, w, h, d; offset (u64); resource; level; stride; layer stride
    gpu::with(|g| g.ok_ctx(TRANSFER_FROM_HOST_3D, CTX, &[0, 0, 0, w, h, 1, 0, 0, t.color, 0, w * 4, 0])).ok_or("gpu")??;
    crate::arch::cpu::dma_sync(t.backing as u64, (w * h * 4) as usize);
    let bgra = COLOR_BGRA.load(Ordering::Relaxed);
    for y in 0..h as usize {
        let src = unsafe { t.backing.add((h as usize - 1 - y) * w as usize) };
        for x in 0..w as usize {
            let p = unsafe { src.add(x).read_volatile() };
            out[y * w as usize + x] = if bgra { p } else { swap_rb(p) } & 0x00FF_FFFF;
        }
    }
    Ok(())
}
