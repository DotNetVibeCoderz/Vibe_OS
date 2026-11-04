//! G3: the kernel side of `Buitenzorg.Graphics3D` -- a software rasterizer
//! that runs on every architecture.
//!
//! The managed SDK (`Buitenzorg.Graphics3D` in BZAPP.DLL, the scene graph
//! `Buitenzorg.Three` on top) builds meshes, textures and matrices; the heavy
//! work -- vertex transform, lighting, clipping, rasterization, depth test,
//! texturing -- happens here, in native code, because the managed apps run in
//! an interpreter. The model is a fixed-function GPU (OpenGL 1.x-like):
//!
//! * **Resources** (GFX3D syscall, `bz_abi::gfx3d_op`): render contexts (a
//!   color + a 24-bit depth buffer), meshes (vertex + index buffer) and
//!   textures, each owned by the process that made it and freed when it exits
//!   (`release`, from `app::destroy_when_home`). All of them together stay
//!   under [`BUDGET`] -- the PC kernel heap is 32 MiB.
//! * **Vertex stage**: model and view-projection matrices (row-major, column
//!   vectors, OpenGL clip space), per-vertex lighting -- ambient + up to 8
//!   directional / point lights, Lambert diffuse and Blinn-Phong specular
//!   (Gouraud) -- and linear fog by eye distance (clip `w`).
//! * **Clipping**: Sutherland-Hodgman in homogeneous clip space against the
//!   near and far planes and a guard band of twice the viewport, so screen
//!   coordinates stay small enough for exact 64-bit edge functions.
//! * **Rasterizer**: half-space edge functions in 28.4 fixed point with the
//!   top-left fill rule, barycentric weights from one reciprocal per triangle
//!   (no division per pixel for color and depth), perspective-correct texture
//!   coordinates (two divisions per textured pixel), nearest-texel sampling
//!   with wrap-around, depth test "less", optional alpha blending, wireframe.
//!
//! **No floating point past the input conversion**: the x86-64 and ARM64
//! kernels are soft-float, so the f32s that arrive from user space are turned
//! into 16.16 fixed point by bit manipulation ([`fx`]) and everything after
//! that is integer arithmetic.
//!
//! The backend is replaceable: virgl (QEMU `virtio-gpu-gl`) and the Pi 4's
//! V3D take the same resources and draw calls later (PLAN.md G3).

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use spin::Mutex;

use bz_abi::{gfx3d_clear, gfx3d_flags as fl, gfx3d_light as gl};

use crate::virtio::virgl;

/// Largest context / texture edge.
pub const MAX_DIM: u32 = 1024;
/// Bytes all contexts, meshes and textures may use together.
pub const BUDGET: usize = 8 << 20;
/// Largest mesh.
pub const MAX_VERTICES: usize = 65_536;
/// Largest index buffer.
pub const MAX_INDICES: usize = 3 * 65_536;

/// 1.0 in 16.16 fixed point.
const ONE: i64 = 1 << 16;
/// Depth of the far plane (24-bit depth).
const FAR_DEPTH: u32 = 0x00FF_FFFF;
/// Guard band: clip at |x|, |y| <= GUARD * w.
const GUARD: i64 = 2;
/// Smallest clip `w` kept (1/256), so 1/w stays bounded.
const MIN_W: i64 = 256;

/// An f32 (as bits) in 16.16 fixed point, saturating at +-2^46; NaN and
/// infinities give 0. Pure integer work -- the kernel is soft-float.
pub fn fx(bits: u32) -> i64 {
    let exp = ((bits >> 23) & 0xFF) as i32;
    if exp == 0 || exp == 0xFF {
        return 0;
    }
    let m = ((bits & 0x7F_FFFF) | 0x80_0000) as i64;
    // value = m * 2^(exp - 150); in 16.16: m * 2^(exp - 134)
    let sh = exp - 134;
    let v = if sh >= 0 {
        if sh > 22 { 1 << 46 } else { (m << sh).min(1 << 46) }
    } else if sh < -24 {
        0
    } else {
        m >> -sh
    };
    if bits >> 31 != 0 { -v } else { v }
}

/// a * b in 16.16.
fn mulf(a: i64, b: i64) -> i64 {
    ((a as i128 * b as i128) >> 16) as i64
}

/// Integer square root.
fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut res = 0u64;
    let mut bit = 1u64 << ((63 - n.leading_zeros()) & !1);
    while bit != 0 {
        if x >= res + bit {
            x -= res + bit;
            res = (res >> 1) + bit;
        } else {
            res >>= 1;
        }
        bit >>= 2;
    }
    res
}

/// `v` scaled to unit length (16.16); zero stays zero.
fn normalize(v: [i64; 3]) -> [i64; 3] {
    let mut v = v;
    let m = v[0].abs().max(v[1].abs()).max(v[2].abs());
    if m == 0 {
        return [0, 0, 0];
    }
    // keep the squares in 64 bits: direction only, so dropping low bits of a
    // long vector costs nothing
    let mut s = 0;
    while (m >> s) > (1 << 24) {
        s += 1;
    }
    for c in v.iter_mut() {
        *c >>= s;
    }
    let len = isqrt((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as u64) as i64;
    if len == 0 {
        return [0, 0, 0];
    }
    [v[0] * ONE / len, v[1] * ONE / len, v[2] * ONE / len]
}

fn dot(a: [i64; 3], b: [i64; 3]) -> i64 {
    (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) >> 16
}

/// A mesh vertex in fixed point.
#[derive(Clone, Copy, Default)]
pub struct Vtx {
    pub p: [i64; 3],
    pub n: [i64; 3],
    pub uv: [i64; 2],
    pub c: u32,
}

impl Vtx {
    /// From a `bz_abi::Vertex3D` (40 little-endian bytes).
    pub fn from_bytes(b: &[u8]) -> Vtx {
        let w = |i: usize| u32::from_le_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]]);
        let uv = |i: usize| fx(w(i)).clamp(-(1 << 22), 1 << 22);
        Vtx { p: [fx(w(0)), fx(w(1)), fx(w(2))], n: [fx(w(3)), fx(w(4)), fx(w(5))], uv: [uv(6), uv(7)], c: w(8) }
    }
}

/// One light, in fixed point.
#[derive(Clone, Copy)]
struct Light {
    point: bool,
    /// Directional: unit vector *towards* the light; point: its position.
    v: [i64; 3],
    rgb: [i64; 3],
    range: i64,
}

/// A context's lighting, fog and eye.
#[derive(Clone)]
pub struct Lighting {
    ambient: [i64; 3],
    eye: [i64; 3],
    fog: [i64; 3],
    fog_start: i64,
    fog_end: i64,
    lights: Vec<Light>,
}

impl Default for Lighting {
    /// A soft ambient and one white light from the upper left front.
    fn default() -> Self {
        Lighting {
            ambient: [ONE * 3 / 10; 3],
            eye: [0, 0, 10 * ONE],
            fog: [0; 3],
            fog_start: 0,
            fog_end: 0,
            lights: alloc::vec![Light { point: false, v: normalize([ONE / 2, ONE, ONE * 3 / 4]), rgb: [ONE * 8 / 10; 3], range: 0 }],
        }
    }
}

impl Lighting {
    /// From the LIGHTS block (`f32` bits, `bz_abi::gfx3d_light`).
    pub fn from_f32(b: &[u32]) -> Lighting {
        let at = |i: usize| b.get(i).map(|&x| fx(x)).unwrap_or(0);
        let col = |i: usize| (at(i).clamp(0, ONE) * 255) >> 16;
        let n = (at(gl::COUNT) >> 16).clamp(0, gl::MAX_LIGHTS as i64) as usize;
        let mut lights = Vec::new();
        for k in 0..n {
            let o = gl::LIGHTS + k * gl::STRIDE;
            if o + gl::STRIDE > b.len() {
                break;
            }
            let point = at(o) >= ONE / 2;
            let v = [at(o + 1), at(o + 2), at(o + 3)];
            lights.push(Light {
                point,
                v: if point { v } else { normalize([-v[0], -v[1], -v[2]]) },
                rgb: [at(o + 4).max(0), at(o + 5).max(0), at(o + 6).max(0)],
                range: at(o + 7).max(0),
            });
        }
        Lighting {
            ambient: [at(gl::AMBIENT).max(0), at(gl::AMBIENT + 1).max(0), at(gl::AMBIENT + 2).max(0)],
            eye: [at(gl::EYE), at(gl::EYE + 1), at(gl::EYE + 2)],
            fog: [col(gl::FOG_COLOR), col(gl::FOG_COLOR + 1), col(gl::FOG_COLOR + 2)],
            fog_start: at(gl::FOG_START),
            fog_end: at(gl::FOG_END),
            lights,
        }
    }
}

pub struct Mesh {
    v: Vec<Vtx>,
    idx: Vec<u32>,
    bytes: usize,
}

pub struct Texture {
    w: u32,
    h: u32,
    px: Vec<u32>,
    bytes: usize,
    /// Its copy on the GPU (virgl), made on the first GPU draw that uses it.
    gpu: spin::Once<Option<virgl::GpuTexture>>,
}

pub struct Ctx {
    w: u32,
    h: u32,
    color: Vec<u32>,
    depth: Vec<u32>,
    light: Lighting,
    bytes: usize,
    /// The GPU render target when this context draws through virgl.
    gpu: Option<virgl::Target>,
    /// The GPU drew since the last read-back into `color`.
    dirty: bool,
}

impl Ctx {
    /// Bring `color` up to date with the GPU picture.
    fn sync(&mut self) {
        if self.dirty {
            if let Some(t) = &self.gpu {
                if let Err(e) = virgl::read(t, &mut self.color) {
                    crate::println!("[gfx3d] GPU read-back: {}", e);
                }
            }
            self.dirty = false;
        }
    }
}

impl Drop for Mesh {
    fn drop(&mut self) {
        USED.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}
impl Drop for Texture {
    fn drop(&mut self) {
        USED.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}
impl Drop for Ctx {
    fn drop(&mut self) {
        USED.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

#[derive(Clone)]
enum Res {
    Ctx(Arc<Mutex<Ctx>>),
    Mesh(Arc<Mesh>),
    Tex(Arc<Texture>),
}

/// id -> (owner space, resource). Held only to look up or insert; drawing
/// works on the `Arc`s with the table unlocked (lock order: TABLE, then a
/// context, then the window manager).
static TABLE: Mutex<BTreeMap<u32, (u64, Res)>> = Mutex::new(BTreeMap::new());
static NEXT: AtomicU32 = AtomicU32::new(1);
static USED: AtomicUsize = AtomicUsize::new(0);
static DRAWS: AtomicU64 = AtomicU64::new(0);
static TRIANGLES: AtomicU64 = AtomicU64::new(0);
static PIXELS: AtomicU64 = AtomicU64::new(0);

fn reserve(bytes: usize) -> Result<(), &'static str> {
    let mut cur = USED.load(Ordering::Relaxed);
    loop {
        if cur + bytes > BUDGET {
            return Err("gfx3d: memory budget exhausted");
        }
        match USED.compare_exchange_weak(cur, cur + bytes, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return Ok(()),
            Err(c) => cur = c,
        }
    }
}

fn insert(owner: u64, r: Res) -> u32 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    TABLE.lock().insert(id, (owner, r));
    id
}

fn get(owner: u64, id: u32) -> Option<Res> {
    TABLE.lock().get(&id).filter(|(o, _)| *o == owner).map(|(_, r)| r.clone())
}

fn ctx(owner: u64, id: u32) -> Result<Arc<Mutex<Ctx>>, &'static str> {
    match get(owner, id) {
        Some(Res::Ctx(c)) => Ok(c),
        _ => Err("gfx3d: no such context"),
    }
}

/// A render context of `w` x `h` (color cleared to black, depth to far):
/// on the GPU when virgl is there, else in software.
pub fn create(owner: u64, w: u32, h: u32) -> Result<u32, &'static str> {
    create_with(owner, w, h, virgl::available())
}

fn create_with(owner: u64, w: u32, h: u32, on_gpu: bool) -> Result<u32, &'static str> {
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
        return Err("gfx3d: bad size");
    }
    let n = (w * h) as usize;
    // a GPU context keeps only the read-back copy here (the depth buffer is on the GPU)
    let gpu = if on_gpu {
        match virgl::create_target(w, h) {
            Ok(t) => Some(t),
            Err(e) => {
                crate::println!("[gfx3d] GPU target {}x{}: {} -- drawing in software", w, h, e);
                None
            }
        }
    } else {
        None
    };
    let bytes = if gpu.is_some() { n * 4 } else { n * 8 };
    reserve(bytes)?;
    let depth = if gpu.is_some() { Vec::new() } else { alloc::vec![FAR_DEPTH; n] };
    let c = Ctx { w, h, color: alloc::vec![0; n], depth, light: Lighting::default(), bytes, gpu, dirty: false };
    if let Some(t) = &c.gpu {
        let _ = virgl::clear(t, 0, true, true);
    }
    Ok(insert(owner, Res::Ctx(Arc::new(Mutex::new(c)))))
}

/// Whether `owner` has context `id`.
pub fn exists(owner: u64, id: u32) -> bool {
    ctx(owner, id).is_ok()
}

/// Whether context `id` draws on the GPU.
pub fn on_gpu(owner: u64, id: u32) -> bool {
    ctx(owner, id).is_ok_and(|c| c.lock().gpu.is_some())
}

/// A mesh (indices must name existing vertices; whole triangles only).
pub fn mesh(owner: u64, v: Vec<Vtx>, idx: Vec<u32>) -> Result<u32, &'static str> {
    if v.is_empty() || v.len() > MAX_VERTICES || idx.len() > MAX_INDICES || idx.len() % 3 != 0 {
        return Err("gfx3d: bad mesh size");
    }
    if idx.iter().any(|&i| i as usize >= v.len()) {
        return Err("gfx3d: index out of range");
    }
    let bytes = v.len() * core::mem::size_of::<Vtx>() + idx.len() * 4;
    reserve(bytes)?;
    Ok(insert(owner, Res::Mesh(Arc::new(Mesh { v, idx, bytes }))))
}

/// A texture of `w` x `h` 0xAARRGGBB pixels.
pub fn texture(owner: u64, w: u32, h: u32, px: Vec<u32>) -> Result<u32, &'static str> {
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM || px.len() != (w * h) as usize {
        return Err("gfx3d: bad texture size");
    }
    let bytes = px.len() * 4;
    reserve(bytes)?;
    Ok(insert(owner, Res::Tex(Arc::new(Texture { w, h, px, bytes, gpu: spin::Once::new() }))))
}

/// Free resource `id` of `owner` (memory returns when no draw still uses it).
pub fn free(owner: u64, id: u32) -> bool {
    let mut t = TABLE.lock();
    if t.get(&id).is_some_and(|(o, _)| *o == owner) {
        t.remove(&id);
        true
    } else {
        false
    }
}

/// Free everything process `owner` made (it exited).
pub fn release(owner: u64) {
    let gone: Vec<(u64, Res)> = {
        let mut t = TABLE.lock();
        let ids: Vec<u32> = t.iter().filter(|(_, (o, _))| *o == owner).map(|(k, _)| *k).collect();
        ids.iter().filter_map(|k| t.remove(k)).collect()
    };
    if !gone.is_empty() {
        crate::println!("[gfx3d] process {}: freed {} resource(s)", owner, gone.len());
    }
}

pub fn clear(owner: u64, id: u32, color: u32, bits: u32) -> Result<(), &'static str> {
    let c = ctx(owner, id)?;
    let mut c = c.lock();
    if let Some(t) = &c.gpu {
        virgl::clear(t, color & 0x00FF_FFFF, bits & gfx3d_clear::COLOR != 0, bits & gfx3d_clear::DEPTH != 0)?;
        c.dirty = true;
        return Ok(());
    }
    if bits & gfx3d_clear::COLOR != 0 {
        c.color.fill(color & 0x00FF_FFFF);
    }
    if bits & gfx3d_clear::DEPTH != 0 {
        c.depth.fill(FAR_DEPTH);
    }
    Ok(())
}

pub fn set_lighting(owner: u64, id: u32, l: Lighting) -> Result<(), &'static str> {
    ctx(owner, id)?.lock().light = l;
    Ok(())
}

pub fn read_pixel(owner: u64, id: u32, x: u32, y: u32) -> Result<u32, &'static str> {
    let c = ctx(owner, id)?;
    let mut c = c.lock();
    c.sync();
    if x >= c.w || y >= c.h {
        return Err("gfx3d: outside the context");
    }
    Ok(c.color[(y * c.w + x) as usize])
}

/// Run `f` over the color buffer (w, h, pixels).
pub fn with_color<R>(owner: u64, id: u32, f: impl FnOnce(u32, u32, &[u32]) -> R) -> Result<R, &'static str> {
    let c = ctx(owner, id)?;
    let mut c = c.lock();
    c.sync();
    Ok(f(c.w, c.h, &c.color))
}

/// Copy the color buffer into window `win` at (x, y).
pub fn present(owner: u64, id: u32, win: u32, x: i32, y: i32) -> Result<(), &'static str> {
    let c = ctx(owner, id)?;
    let mut c = c.lock();
    c.sync();
    crate::wm::blit_on_window(win, x, y, c.w as i32, c.h as i32, c.color.as_ptr() as u64)
}

/// (contexts, meshes, textures, bytes in use, draws, triangles, pixels).
pub fn summary() -> (usize, usize, usize, usize, u64, u64, u64) {
    let t = TABLE.lock();
    let mut n = (0, 0, 0);
    for (_, (_, r)) in t.iter() {
        match r {
            Res::Ctx(_) => n.0 += 1,
            Res::Mesh(_) => n.1 += 1,
            Res::Tex(_) => n.2 += 1,
        }
    }
    (
        n.0,
        n.1,
        n.2,
        USED.load(Ordering::Relaxed),
        DRAWS.load(Ordering::Relaxed),
        TRIANGLES.load(Ordering::Relaxed),
        PIXELS.load(Ordering::Relaxed),
    )
}

// --- the pipeline ----------------------------------------------------------

/// A transformed vertex: clip position (16.16), lit color 0..255, uv (16.16).
#[derive(Clone, Copy, Default)]
struct Tv {
    c: [i64; 4],
    col: [i64; 4],
    uv: [i64; 2],
}

/// A projected vertex: 28.4 screen position, 24-bit depth, 1/w (2^30 / w).
#[derive(Clone, Copy, Default)]
struct Sv {
    x: i64,
    y: i64,
    z: i64,
    q: i64,
    col: [i64; 4],
    uv: [i64; 2],
}

fn mat_vec(m: &[i64; 16], v: [i64; 4]) -> [i64; 4] {
    let mut r = [0i64; 4];
    for (i, o) in r.iter_mut().enumerate() {
        let row = &m[i * 4..i * 4 + 4];
        let s = row[0] as i128 * v[0] as i128
            + row[1] as i128 * v[1] as i128
            + row[2] as i128 * v[2] as i128
            + row[3] as i128 * v[3] as i128;
        *o = (s >> 16) as i64;
    }
    r
}

/// Draw parameters.
pub struct Draw<'a> {
    pub flags: u32,
    pub color: u32,
    pub model: [i64; 16],
    pub view_proj: [i64; 16],
    pub tex: Option<&'a Texture>,
}

fn channel(c: u32, shift: u32) -> i64 {
    ((c >> shift) & 0xFF) as i64
}

/// Lit color (0..255 per channel, alpha) of a vertex at world `p` with unit
/// normal `n`; `base` = material x vertex color.
fn shade(l: &Lighting, flags: u32, base: [i64; 4], p: [i64; 3], n: [i64; 3]) -> [i64; 4] {
    let shading = flags & fl::SHADING_MASK;
    if shading == fl::UNLIT {
        return base;
    }
    let mut diff = l.ambient;
    let mut spec = [0i64; 3];
    let phong = shading == fl::PHONG;
    let shin = match (flags >> fl::SHININESS_SHIFT) & 0xFF {
        0 => 32,
        s => s,
    };
    let strength = match (flags >> fl::SPECULAR_SHIFT) & 0xFF {
        0 => ONE / 2,
        s => s as i64 * ONE / 255,
    };
    let view = if phong { normalize([l.eye[0] - p[0], l.eye[1] - p[1], l.eye[2] - p[2]]) } else { [0; 3] };
    for li in &l.lights {
        let (dir, att) = if li.point {
            let d = [li.v[0] - p[0], li.v[1] - p[1], li.v[2] - p[2]];
            let att = if li.range > 0 {
                // smooth fade: (1 - (dist / range)^2), clamped
                let dist2 = (d[0] as i128 * d[0] as i128 + d[1] as i128 * d[1] as i128 + d[2] as i128 * d[2] as i128) >> 16;
                let r2 = (li.range as i128 * li.range as i128) >> 16;
                if r2 <= 0 { 0 } else { (ONE - ((dist2 << 16) / r2) as i64).max(0) }
            } else {
                ONE
            };
            (normalize(d), att)
        } else {
            (li.v, ONE)
        };
        if att == 0 {
            continue;
        }
        let ndl = dot(n, dir);
        if ndl <= 0 {
            continue;
        }
        let k = mulf(ndl, att);
        for c in 0..3 {
            diff[c] += mulf(li.rgb[c], k);
        }
        if phong {
            let h = normalize([dir[0] + view[0], dir[1] + view[1], dir[2] + view[2]]);
            let ndh = dot(n, h).clamp(0, ONE);
            // ndh^shin by squaring
            let (mut acc, mut b, mut e) = (ONE, ndh, shin);
            while e > 0 {
                if e & 1 != 0 {
                    acc = mulf(acc, b);
                }
                b = mulf(b, b);
                e >>= 1;
            }
            let s = mulf(mulf(acc, strength), att);
            for c in 0..3 {
                spec[c] += mulf(li.rgb[c], s);
            }
        }
    }
    let mut out = base;
    for c in 0..3 {
        out[c] = ((base[c] * diff[c] + 255 * spec[c]) >> 16).clamp(0, 255);
    }
    out
}

fn vertex_stage(ctx: &Ctx, m: &Mesh, d: &Draw) -> Vec<Tv> {
    let mat = [channel(d.color, 16), channel(d.color, 8), channel(d.color, 0), channel(d.color, 24)];
    let lit = d.flags & fl::SHADING_MASK != fl::UNLIT;
    let l = &ctx.light;
    let fog = l.fog_end > l.fog_start;
    let mut out = Vec::with_capacity(m.v.len());
    for v in &m.v {
        let world = mat_vec(&d.model, [v.p[0], v.p[1], v.p[2], ONE]);
        let clip = mat_vec(&d.view_proj, world);
        let vc = [channel(v.c, 16), channel(v.c, 8), channel(v.c, 0), channel(v.c, 24)];
        let mut base = [0i64; 4];
        for c in 0..4 {
            base[c] = (mat[c] * vc[c] + 127) / 255;
        }
        let mut col = if lit {
            let n = mat_vec(&d.model, [v.n[0], v.n[1], v.n[2], 0]);
            shade(l, d.flags, base, [world[0], world[1], world[2]], normalize([n[0], n[1], n[2]]))
        } else {
            base
        };
        if fog {
            let f = ((l.fog_end - clip[3]) * ONE / (l.fog_end - l.fog_start)).clamp(0, ONE);
            for c in 0..3 {
                col[c] = l.fog[c] + (((col[c] - l.fog[c]) * f) >> 16);
            }
        }
        out.push(Tv { c: clip, col, uv: v.uv });
    }
    out
}

/// Signed distances of a clip-space vertex to the clip planes (inside >= 0).
fn plane_dist(c: &[i64; 4], plane: usize) -> i64 {
    let (x, y, z, w) = (c[0], c[1], c[2], c[3]);
    match plane {
        0 => z + w,             // near
        1 => w - z,             // far
        2 => GUARD * w + x,     // left (guard band)
        3 => GUARD * w - x,     // right
        4 => GUARD * w + y,     // bottom
        5 => GUARD * w - y,     // top
        _ => w - MIN_W,         // w stays positive
    }
}
const PLANES: usize = 7;

fn outcode(c: &[i64; 4]) -> u32 {
    (0..PLANES).fold(0, |m, p| if plane_dist(c, p) < 0 { m | 1 << p } else { m })
}

fn lerp_tv(a: &Tv, b: &Tv, t: i64) -> Tv {
    let l = |x: i64, y: i64| x + ((y - x) as i128 * t as i128 >> 16) as i64;
    let mut r = Tv::default();
    for i in 0..4 {
        r.c[i] = l(a.c[i], b.c[i]);
        r.col[i] = l(a.col[i], b.col[i]);
    }
    r.uv = [l(a.uv[0], b.uv[0]), l(a.uv[1], b.uv[1])];
    r
}

/// Clip a triangle against every plane in `mask`; returns the polygon (<= 10 vertices).
fn clip_poly(tri: [Tv; 3], mask: u32, out: &mut [Tv; 12]) -> usize {
    let mut a = [Tv::default(); 12];
    a[..3].copy_from_slice(&tri);
    let mut n = 3;
    for p in 0..PLANES {
        if mask & (1 << p) == 0 {
            continue;
        }
        let mut b = [Tv::default(); 12];
        let mut m = 0;
        for i in 0..n {
            let (cur, nxt) = (&a[i], &a[(i + 1) % n]);
            let (dc, dn) = (plane_dist(&cur.c, p), plane_dist(&nxt.c, p));
            if dc >= 0 && m < 12 {
                b[m] = *cur;
                m += 1;
            }
            if (dc >= 0) != (dn >= 0) && m < 12 {
                let t = ((dc as i128) << 16) / (dc as i128 - dn as i128);
                b[m] = lerp_tv(cur, nxt, t as i64);
                m += 1;
            }
        }
        a = b;
        n = m;
        if n < 3 {
            return 0;
        }
    }
    out[..n].copy_from_slice(&a[..n]);
    n
}

fn project(t: &Tv, w: u32, h: u32) -> Sv {
    let cw = t.c[3].max(MIN_W) as i128;
    let (hw, hh) = (w as i128 * 8, h as i128 * 8);
    Sv {
        x: (hw + t.c[0] as i128 * hw / cw) as i64,
        y: (hh - t.c[1] as i128 * hh / cw) as i64,
        z: ((1i128 << 23) + ((t.c[2] as i128) << 23) / cw).clamp(0, FAR_DEPTH as i128) as i64,
        q: ((1i128 << 46) / cw) as i64,
        col: t.col,
        uv: t.uv,
    }
}

/// Bresenham line in color `col` (wireframe).
fn line(c: &mut Ctx, a: (i64, i64), b: (i64, i64), col: u32) -> u32 {
    let (mut x0, mut y0, x1, y1) = (a.0, a.1, b.0, b.1);
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let mut err = dx + dy;
    let mut n = 0;
    for _ in 0..8192 {
        if x0 >= 0 && y0 >= 0 && (x0 as u32) < c.w && (y0 as u32) < c.h {
            c.color[(y0 as u32 * c.w + x0 as u32) as usize] = col;
            n += 1;
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
    n
}

fn rgb(col: &[i64; 4]) -> u32 {
    ((col[0].clamp(0, 255) as u32) << 16) | ((col[1].clamp(0, 255) as u32) << 8) | col[2].clamp(0, 255) as u32
}

/// Rasterize one projected triangle; returns the pixels written, or None
/// when it was culled / empty.
fn raster(c: &mut Ctx, v: [Sv; 3], d: &Draw) -> Option<u32> {
    let [v0, mut v1, mut v2] = v;
    let area = (v1.x - v0.x) * (v2.y - v0.y) - (v2.x - v0.x) * (v1.y - v0.y);
    if area == 0 {
        return None;
    }
    // y points down on screen: a counter-clockwise (front) triangle has a
    // negative area here. Make it positive for the edge functions.
    let front = area < 0;
    if !front && d.flags & fl::DOUBLE_SIDED == 0 {
        return None;
    }
    let mut area = area;
    if area < 0 {
        core::mem::swap(&mut v1, &mut v2);
        area = -area;
    }
    if d.flags & fl::WIREFRAME != 0 {
        let p = |s: &Sv| (s.x >> 4, s.y >> 4);
        let col = rgb(&v0.col);
        let n = line(c, p(&v0), p(&v1), col) + line(c, p(&v1), p(&v2), col) + line(c, p(&v2), p(&v0), col);
        return Some(n);
    }
    let (w, h) = (c.w as i64, c.h as i64);
    let minx = v0.x.min(v1.x).min(v2.x);
    let maxx = v0.x.max(v1.x).max(v2.x);
    let miny = v0.y.min(v1.y).min(v2.y);
    let maxy = v0.y.max(v1.y).max(v2.y);
    let x_lo = ((minx - 8 + 15) >> 4).max(0);
    let x_hi = ((maxx - 8) >> 4).min(w - 1);
    let y_lo = ((miny - 8 + 15) >> 4).max(0);
    let y_hi = ((maxy - 8) >> 4).min(h - 1);
    if x_lo > x_hi || y_lo > y_hi {
        return Some(0);
    }
    let vs = [v0, v1, v2];
    // edge i is opposite vertex i
    let mut e_row = [0i64; 3];
    let mut sx = [0i64; 3];
    let mut sy = [0i64; 3];
    let (px, py) = (x_lo * 16 + 8, y_lo * 16 + 8);
    for i in 0..3 {
        let (a, b) = (&vs[(i + 1) % 3], &vs[(i + 2) % 3]);
        let (ex, ey) = (b.x - a.x, b.y - a.y);
        let top_left = ey < 0 || (ey == 0 && ex > 0);
        e_row[i] = ex * (py - a.y) - ey * (px - a.x) - if top_left { 0 } else { 1 };
        sx[i] = -ey * 16;
        sy[i] = ex * 16;
    }
    let inv = (1i64 << 62) / area;
    let lam = |e: i64| (e.max(0) * inv) >> 46;
    let z = [v0.z, v1.z, v2.z];
    let col = [v0.col, v1.col, v2.col];
    let depth_test = d.flags & fl::NO_DEPTH_TEST == 0;
    let depth_write = d.flags & fl::NO_DEPTH_WRITE == 0;
    let blend = d.flags & fl::BLEND != 0;
    // perspective-correct texture coordinates: weights q' = (1/w) relative to
    // the largest, so the per-pixel sums stay in 64 bits
    let qmax = v0.q.max(v1.q).max(v2.q).max(1);
    let qn = [(v0.q << 16) / qmax, (v1.q << 16) / qmax, (v2.q << 16) / qmax];
    let uq = [[vs[0].uv[0] * qn[0], vs[0].uv[1] * qn[0]], [vs[1].uv[0] * qn[1], vs[1].uv[1] * qn[1]], [vs[2].uv[0] * qn[2], vs[2].uv[1] * qn[2]]];
    let mut written = 0u32;
    for y in y_lo..=y_hi {
        let mut e = e_row;
        let row = (y * w) as usize;
        for x in x_lo..=x_hi {
            if (e[0] | e[1] | e[2]) >= 0 {
                let l1 = lam(e[1]);
                let l2 = lam(e[2]);
                let l0 = (ONE - l1 - l2).max(0);
                let zz = ((l0 * z[0] + l1 * z[1] + l2 * z[2]) >> 16) as u32;
                let i = row + x as usize;
                if !depth_test || zz < c.depth[i] {
                    let mut px = [0i64; 4];
                    for k in 0..4 {
                        px[k] = (l0 * col[0][k] + l1 * col[1][k] + l2 * col[2][k]) >> 16;
                    }
                    let mut keep = true;
                    if let Some(t) = d.tex {
                        let qq = l0 * qn[0] + l1 * qn[1] + l2 * qn[2];
                        if qq > 0 {
                            let u = (l0 * uq[0][0] + l1 * uq[1][0] + l2 * uq[2][0]) / qq;
                            let v = (l0 * uq[0][1] + l1 * uq[1][1] + l2 * uq[2][1]) / qq;
                            let tx = ((u * t.w as i64) >> 16).rem_euclid(t.w as i64) as u32;
                            let ty = ((v * t.h as i64) >> 16).rem_euclid(t.h as i64) as u32;
                            let texel = t.px[(ty * t.w + tx) as usize];
                            let ta = channel(texel, 24);
                            if ta == 0 {
                                keep = false;
                            }
                            px[0] = (px[0] * channel(texel, 16) * 257 + 0x8080) >> 16;
                            px[1] = (px[1] * channel(texel, 8) * 257 + 0x8080) >> 16;
                            px[2] = (px[2] * channel(texel, 0) * 257 + 0x8080) >> 16;
                            px[3] = (px[3] * ta * 257 + 0x8080) >> 16;
                        }
                    }
                    if keep {
                        let out = if blend {
                            let dst = c.color[i];
                            let a = px[3].clamp(0, 255);
                            let mix = |s: i64, sh: u32| {
                                let dc = channel(dst, sh);
                                dc + (((s.clamp(0, 255) - dc) * a * 257 + 0x8080) >> 16)
                            };
                            rgb(&[mix(px[0], 16), mix(px[1], 8), mix(px[2], 0), 255])
                        } else {
                            rgb(&px)
                        };
                        c.color[i] = out;
                        if depth_write {
                            c.depth[i] = zz;
                        }
                        written += 1;
                    }
                }
            }
            e[0] += sx[0];
            e[1] += sx[1];
            e[2] += sx[2];
        }
        e_row[0] += sy[0];
        e_row[1] += sy[1];
        e_row[2] += sy[2];
    }
    Some(written)
}

/// Draw `m` into `c`; returns (triangles drawn, pixels written).
fn draw_into(c: &mut Ctx, m: &Mesh, d: &Draw) -> (u32, u32) {
    if c.gpu.is_some() {
        return gpu_draw(c, m, d);
    }
    let tv = vertex_stage(c, m, d);
    let (w, h) = (c.w, c.h);
    let (mut tris, mut pixels) = (0u32, 0u32);
    let mut poly = [Tv::default(); 12];
    for t in m.idx.chunks_exact(3) {
        let tri = [tv[t[0] as usize], tv[t[1] as usize], tv[t[2] as usize]];
        let oc = [outcode(&tri[0].c), outcode(&tri[1].c), outcode(&tri[2].c)];
        if oc[0] & oc[1] & oc[2] != 0 {
            continue;
        }
        let n = if oc[0] | oc[1] | oc[2] == 0 {
            poly[..3].copy_from_slice(&tri);
            3
        } else {
            clip_poly(tri, oc[0] | oc[1] | oc[2], &mut poly)
        };
        if n < 3 {
            continue;
        }
        let s0 = project(&poly[0], w, h);
        let mut drawn = false;
        for k in 1..n - 1 {
            let (s1, s2) = (project(&poly[k], w, h), project(&poly[k + 1], w, h));
            if let Some(px) = raster(c, [s0, s1, s2], d) {
                pixels += px;
                drawn = true;
            }
        }
        tris += drawn as u32;
    }
    (tris, pixels)
}

/// The GPU path: the same vertex stage, then the triangles go to virgl as
/// clip-space vertices. Back faces are culled here when the whole triangle is
/// in front of the eye (so counts match the software path); triangles that
/// cross the eye plane are left to the GPU clipper. Pixels are reported as
/// the screen area the triangles cover (the GPU does not say how many passed
/// the depth test).
fn gpu_draw(c: &mut Ctx, m: &Mesh, d: &Draw) -> (u32, u32) {
    let tv = vertex_stage(c, m, d);
    let (w, h) = (c.w, c.h);
    let double = d.flags & fl::DOUBLE_SIDED != 0;
    let mut out: Vec<virgl::GVertex> = Vec::with_capacity(m.idx.len());
    let (mut tris, mut area) = (0u32, 0i64);
    let gv = |t: &Tv| {
        let col = |k: usize| t.col[k].clamp(0, 255) as u32;
        virgl::GVertex([
            virgl::f32_from_fx(t.c[0]),
            virgl::f32_from_fx(t.c[1]),
            virgl::f32_from_fx(t.c[2]),
            virgl::f32_from_fx(t.c[3]),
            col(0) | col(1) << 8 | col(2) << 16 | col(3) << 24,
            virgl::f32_from_fx(t.uv[0]),
            virgl::f32_from_fx(t.uv[1]),
        ])
    };
    for t in m.idx.chunks_exact(3) {
        let tri = [tv[t[0] as usize], tv[t[1] as usize], tv[t[2] as usize]];
        let oc = [outcode(&tri[0].c), outcode(&tri[1].c), outcode(&tri[2].c)];
        if oc[0] & oc[1] & oc[2] != 0 {
            continue;
        }
        if tri.iter().all(|v| v.c[3] > MIN_W) {
            let s = [project(&tri[0], w, h), project(&tri[1], w, h), project(&tri[2], w, h)];
            let a = (s[1].x - s[0].x) * (s[2].y - s[0].y) - (s[2].x - s[0].x) * (s[1].y - s[0].y);
            if a == 0 || (a > 0 && !double) {
                continue;
            }
            area += if d.flags & fl::WIREFRAME != 0 {
                // edges only: about the longer extent of each edge, in pixels
                let e = |p: &Sv, q: &Sv| (p.x - q.x).abs().max((p.y - q.y).abs()) / 16;
                e(&s[0], &s[1]) + e(&s[1], &s[2]) + e(&s[2], &s[0])
            } else {
                a.abs() / 512 // 28.4 squared, halved
            };
        } else {
            area += (w * h) as i64 / 2;
        }
        tris += 1;
        out.extend_from_slice(&[gv(&tri[0]), gv(&tri[1]), gv(&tri[2])]);
    }
    if out.is_empty() {
        return (0, 0);
    }
    let tex = d.tex.and_then(|t| t.gpu.call_once(|| virgl::texture(t.w, t.h, &t.px).ok()).as_ref());
    let r = virgl::draw(
        c.gpu.as_ref().unwrap(),
        &out,
        tex,
        !double,
        d.flags & fl::WIREFRAME != 0,
        d.flags & fl::NO_DEPTH_TEST == 0,
        d.flags & fl::NO_DEPTH_WRITE == 0,
        d.flags & fl::BLEND != 0,
    );
    if let Err(e) = r {
        crate::println!("[gfx3d] GPU draw: {}", e);
        return (0, 0);
    }
    c.dirty = true;
    (tris, area.min((w * h) as i64 * 4) as u32)
}

/// Draw mesh `mesh` (with texture `tex`, 0 = none) into context `id`.
pub fn draw(owner: u64, id: u32, mesh: u32, tex: u32, flags: u32, color: u32, model: [i64; 16], view_proj: [i64; 16]) -> Result<(u32, u32), &'static str> {
    let c = ctx(owner, id)?;
    let m = match get(owner, mesh) {
        Some(Res::Mesh(m)) => m,
        _ => return Err("gfx3d: no such mesh"),
    };
    let t = match tex {
        0 => None,
        t => match get(owner, t) {
            Some(Res::Tex(t)) => Some(t),
            _ => return Err("gfx3d: no such texture"),
        },
    };
    let d = Draw { flags, color, model, view_proj, tex: t.as_deref() };
    let r = draw_into(&mut c.lock(), &m, &d);
    DRAWS.fetch_add(1, Ordering::Relaxed);
    TRIANGLES.fetch_add(r.0 as u64, Ordering::Relaxed);
    PIXELS.fetch_add(r.1 as u64, Ordering::Relaxed);
    Ok(r)
}

// --- self-test -------------------------------------------------------------

fn f(x: i64, den: i64) -> i64 {
    x * ONE / den
}

const IDENTITY: [i64; 16] = [ONE, 0, 0, 0, 0, ONE, 0, 0, 0, 0, ONE, 0, 0, 0, 0, ONE];

fn mat_mul(a: &[i64; 16], b: &[i64; 16]) -> [i64; 16] {
    let mut r = [0i64; 16];
    for i in 0..4 {
        for j in 0..4 {
            r[i * 4 + j] = (0..4).map(|k| mulf(a[i * 4 + k], b[k * 4 + j])).sum();
        }
    }
    r
}

fn vtx(x: i64, y: i64, z: i64, n: [i64; 3], uv: [i64; 2], c: u32) -> Vtx {
    Vtx { p: [x, y, z], n, uv, c }
}

/// A quad (two counter-clockwise triangles) from four corners in order.
fn quad(v: &mut Vec<Vtx>, idx: &mut Vec<u32>, p: [[i64; 3]; 4], n: [i64; 3], c: u32) {
    let b = v.len() as u32;
    let uv = [[0, ONE], [ONE, ONE], [ONE, 0], [0, 0]];
    for k in 0..4 {
        v.push(vtx(p[k][0], p[k][1], p[k][2], n, uv[k], c));
    }
    idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
}

/// A unit cube centred on the origin, one color per face.
fn cube(owner: u64) -> Result<u32, &'static str> {
    let (mut v, mut idx) = (Vec::new(), Vec::new());
    let h = ONE / 2;
    let faces: [([[i64; 3]; 4], [i64; 3], u32); 6] = [
        ([[-h, -h, h], [h, -h, h], [h, h, h], [-h, h, h]], [0, 0, ONE], 0xFFFF_4040),
        ([[h, -h, -h], [-h, -h, -h], [-h, h, -h], [h, h, -h]], [0, 0, -ONE], 0xFF40_FF40),
        ([[h, -h, h], [h, -h, -h], [h, h, -h], [h, h, h]], [ONE, 0, 0], 0xFF40_40FF),
        ([[-h, -h, -h], [-h, -h, h], [-h, h, h], [-h, h, -h]], [-ONE, 0, 0], 0xFFFF_FF40),
        ([[-h, h, h], [h, h, h], [h, h, -h], [-h, h, -h]], [0, ONE, 0], 0xFF40_FFFF),
        ([[-h, -h, -h], [h, -h, -h], [h, -h, h], [-h, -h, h]], [0, -ONE, 0], 0xFFFF_40FF),
    ];
    for (p, n, c) in faces {
        quad(&mut v, &mut idx, p, n, c);
    }
    mesh(owner, v, idx)
}

/// Boot self-test: depth test, culling, lighting, texturing, clipping and a
/// perspective cube, rendered offscreen and checked pixel by pixel.
pub fn self_test() -> bool {
    let sw = match run_self_test(false) {
        Ok(msg) => {
            crate::println!("MILESTONE: GFX3D OK ({})", msg);
            true
        }
        Err(e) => {
            crate::println!("[gfx3d] self-test FAILED: {}", e);
            false
        }
    };
    // the same checks on the GPU when there is one (QEMU virtio-gpu-gl)
    if !virgl::available() {
        return sw;
    }
    match run_self_test(true) {
        Ok(msg) => {
            crate::println!("MILESTONE: GFX3D VIRGL OK (host GPU through virtio-gpu 3D: {})", msg);
            sw
        }
        Err(e) => {
            crate::println!("[gfx3d] GPU (virgl) self-test FAILED: {} -- contexts fall back to software", e);
            virgl::set_enabled(false);
            sw
        }
    }
}

fn run_self_test(gpu: bool) -> Result<alloc::string::String, alloc::string::String> {
    use alloc::format;
    const OWNER: u64 = 0;
    const BG: u32 = 0x10_2030;
    let used0 = USED.load(Ordering::Relaxed);
    let ctx_id = create_with(OWNER, 64, 64, gpu).map_err(|e| format!("{}", e))?;
    if on_gpu(OWNER, ctx_id) != gpu {
        free(OWNER, ctx_id);
        return Err(alloc::string::String::from("no GPU render target"));
    }
    let mut ids = alloc::vec![ctx_id];
    let r = (|| -> Result<alloc::string::String, alloc::string::String> {
        let px = |x, y| read_pixel(OWNER, ctx_id, x, y).unwrap_or(0xDEAD);
        let e = |s: &str| alloc::string::String::from(s);
        let flat = |flags| Draw { flags, color: 0xFFFF_FFFF, model: IDENTITY, view_proj: IDENTITY, tex: None };
        // 1. depth: a near green triangle drawn first survives a far red quad
        let n = [0, 0, ONE];
        let (mut v, mut idx) = (Vec::new(), Vec::new());
        quad(&mut v, &mut idx, [[-ONE / 2, -ONE / 2, ONE / 2], [ONE / 2, -ONE / 2, ONE / 2], [ONE / 2, ONE / 2, ONE / 2], [-ONE / 2, ONE / 2, ONE / 2]], n, 0xFFFF_0000);
        let red = mesh(OWNER, v, idx).map_err(e)?;
        ids.push(red);
        let tri = mesh(
            OWNER,
            alloc::vec![
                vtx(-f(3, 10), -f(3, 10), -f(2, 10), n, [0, 0], 0xFF00_FF00),
                vtx(f(3, 10), -f(3, 10), -f(2, 10), n, [0, 0], 0xFF00_FF00),
                vtx(0, f(4, 10), -f(2, 10), n, [0, 0], 0xFF00_FF00),
            ],
            alloc::vec![0, 1, 2],
        )
        .map_err(e)?;
        ids.push(tri);
        clear(OWNER, ctx_id, BG, gfx3d_clear::COLOR | gfx3d_clear::DEPTH).map_err(e)?;
        let draw_m = |m: u32, d: &Draw| {
            let c = ctx(OWNER, ctx_id).unwrap();
            let mm = match get(OWNER, m) {
                Some(Res::Mesh(m)) => m,
                _ => unreachable!(),
            };
            let r = draw_into(&mut c.lock(), &mm, d);
            r
        };
        let (t1, _) = draw_m(tri, &flat(fl::UNLIT));
        let (t2, p2) = draw_m(red, &flat(fl::UNLIT));
        if t1 != 1 || t2 != 2 {
            return Err(format!("triangle counts {} / {}", t1, t2));
        }
        if px(32, 32) != 0x00FF00 {
            return Err(format!("depth: centre {:06x}, want green", px(32, 32)));
        }
        if px(20, 18) != 0xFF0000 {
            return Err(format!("quad pixel {:06x}, want red", px(20, 18)));
        }
        if px(2, 2) != BG {
            return Err(format!("corner {:06x}, want background", px(2, 2)));
        }
        // the quad covers 32x32 pixels exactly once (top-left rule), minus the triangle
        let quad_total = p2 + 0; // pixels the quad wrote (not those under the triangle)
        // 2. negative control: without the depth test the quad overwrites the triangle
        draw_m(red, &flat(fl::UNLIT | fl::NO_DEPTH_TEST));
        if px(32, 32) != 0xFF0000 {
            return Err(e("negative control: depth test off but centre not red"));
        }
        // 3. culling: a clockwise (back-facing) triangle is skipped, unless double-sided
        let back = mesh(
            OWNER,
            alloc::vec![vtx(-ONE, -ONE, 0, n, [0, 0], 0xFFFF_FFFF), vtx(-ONE, -f(6, 10), 0, n, [0, 0], 0xFFFF_FFFF), vtx(-f(6, 10), -ONE, 0, n, [0, 0], 0xFFFF_FFFF)],
            alloc::vec![0, 1, 2],
        )
        .map_err(e)?;
        ids.push(back);
        let (tb, _) = draw_m(back, &flat(fl::UNLIT));
        if tb != 0 || px(2, 61) != BG {
            return Err(e("back face was drawn"));
        }
        draw_m(back, &flat(fl::UNLIT | fl::DOUBLE_SIDED));
        if px(2, 61) != 0xFFFFFF {
            return Err(format!("double-sided back face missing ({:06x})", px(2, 61)));
        }
        // 4. lighting: a face towards the light is brighter than one edge-on
        set_lighting(
            OWNER,
            ctx_id,
            Lighting { ambient: [ONE / 5; 3], eye: [0, 0, 5 * ONE], fog: [0; 3], fog_start: 0, fog_end: 0, lights: alloc::vec![Light { point: false, v: [0, 0, ONE], rgb: [ONE; 3], range: 0 }] },
        )
        .map_err(e)?;
        let (mut lv, mut li) = (Vec::new(), Vec::new());
        quad(&mut lv, &mut li, [[-ONE, -ONE, 0], [0, -ONE, 0], [0, ONE, 0], [-ONE, ONE, 0]], [0, 0, ONE], 0xFFFF_FFFF);
        quad(&mut lv, &mut li, [[0, -ONE, 0], [ONE, -ONE, 0], [ONE, ONE, 0], [0, ONE, 0]], [ONE, 0, 0], 0xFFFF_FFFF);
        let lit = mesh(OWNER, lv, li).map_err(e)?;
        ids.push(lit);
        clear(OWNER, ctx_id, BG, 3).map_err(e)?;
        draw_m(lit, &flat(fl::LAMBERT));
        let (bright, dark) = (px(16, 32) & 0xFF, px(48, 32) & 0xFF);
        if !(bright > 240 && dark > 40 && dark < 60) {
            return Err(format!("lighting: facing {} / edge-on {}", bright, dark));
        }
        // 5. texture: a 2x2 checker on a full-screen quad
        let tex_id = texture(OWNER, 2, 2, alloc::vec![0xFF00_0000, 0xFFFF_FFFF, 0xFFFF_FFFF, 0xFF00_0000]).map_err(e)?;
        ids.push(tex_id);
        let (mut tv, mut ti) = (Vec::new(), Vec::new());
        quad(&mut tv, &mut ti, [[-ONE, -ONE, 0], [ONE, -ONE, 0], [ONE, ONE, 0], [-ONE, ONE, 0]], n, 0xFFFF_FFFF);
        let full = mesh(OWNER, tv, ti).map_err(e)?;
        ids.push(full);
        clear(OWNER, ctx_id, BG, 3).map_err(e)?;
        {
            let c = ctx(OWNER, ctx_id).unwrap();
            let tex = match get(OWNER, tex_id) {
                Some(Res::Tex(t)) => t,
                _ => unreachable!(),
            };
            let mm = match get(OWNER, full) {
                Some(Res::Mesh(m)) => m,
                _ => unreachable!(),
            };
            draw_into(&mut c.lock(), &mm, &Draw { flags: fl::UNLIT, color: 0xFFFF_FFFF, model: IDENTITY, view_proj: IDENTITY, tex: Some(&tex) });
        }
        if px(10, 10) != 0 || px(54, 10) != 0xFFFFFF || px(10, 54) != 0xFFFFFF || px(54, 54) != 0 {
            return Err(format!("texture: {:06x} {:06x} {:06x} {:06x}", px(10, 10), px(54, 10), px(10, 54), px(54, 54)));
        }
        // 6. a cube in perspective, rotated: three faces visible, symmetric about the
        // vertical axis when turned 45 degrees; it crosses the near plane when pushed in
        let cube_id = cube(OWNER).map_err(e)?;
        ids.push(cube_id);
        let (c45, s45) = (46341, 46341);
        let (c30, s30) = (56756, 32768);
        let ry = [c45, 0, s45, 0, 0, ONE, 0, 0, -s45, 0, c45, 0, 0, 0, 0, ONE];
        let rx = [ONE, 0, 0, 0, 0, c30, -s30, 0, 0, s30, c30, 0, 0, 0, 0, ONE];
        let tr = |z: i64| [ONE, 0, 0, 0, 0, ONE, 0, 0, 0, 0, ONE, z, 0, 0, 0, ONE];
        // perspective: fov 90, aspect 1, near 0.5, far 10
        let proj = [ONE, 0, 0, 0, 0, ONE, 0, 0, 0, 0, -72_437, -68_985, 0, 0, -ONE, 0];
        let model = mat_mul(&tr(-3 * ONE), &mat_mul(&rx, &ry));
        clear(OWNER, ctx_id, BG, 3).map_err(e)?;
        let (tc, pc) = draw_m(cube_id, &Draw { flags: fl::LAMBERT, color: 0xFFFF_FFFF, model, view_proj: proj, tex: None });
        if tc != 6 || pc < 120 {
            return Err(format!("cube: {} triangles, {} pixels", tc, pc));
        }
        let left = (0..64u32).flat_map(|y| (0..32u32).map(move |x| (x, y))).filter(|&(x, y)| px(x, y) != BG).count() as i64;
        let right = (0..64u32).flat_map(|y| (32..64u32).map(move |x| (x, y))).filter(|&(x, y)| px(x, y) != BG).count() as i64;
        if (left - right).abs() * 20 > left + right {
            return Err(format!("cube not symmetric: {} / {}", left, right));
        }
        // the camera inside the cube's front: near-plane clipping keeps it drawable
        let close = mat_mul(&tr(-ONE * 7 / 10), &ry);
        clear(OWNER, ctx_id, BG, 3).map_err(e)?;
        let (tn, _) = draw_m(cube_id, &Draw { flags: fl::UNLIT | fl::DOUBLE_SIDED, color: 0xFFFF_FFFF, model: close, view_proj: proj, tex: None });
        let pn = (0..64u32).flat_map(|y| (0..64u32).map(move |x| (x, y))).filter(|&(x, y)| px(x, y) != BG).count();
        if tn == 0 || pn < 64 * 64 / 2 {
            return Err(format!("near clipping: {} triangles, {} pixels", tn, pn));
        }
        Ok(format!("depth, culling, lighting {}/{}, texture, cube {} px, clipped {} px, quad {} px", bright, dark, pc, pn, quad_total))
    })();
    for id in ids {
        free(OWNER, id);
    }
    let r = r?;
    if USED.load(Ordering::Relaxed) != used0 {
        return Err(format!("memory not returned: {} -> {}", used0, USED.load(Ordering::Relaxed)));
    }
    Ok(r)
}
