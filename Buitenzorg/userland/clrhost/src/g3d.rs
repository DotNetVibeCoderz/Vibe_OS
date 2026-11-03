//! The native half of `Buitenzorg.Graphics3D` (in BZAPP.DLL): the
//! `[DllImport("buitenzorg")] G3d*` calls, over the GFX3D syscall (56).
//!
//! As in `bzapi.rs`, the marshalling is here: managed `float[]` / `int[]`
//! arrays are read out of the RustCLR heap and laid out as the kernel's
//! `Gfx3dCmd` / `Vertex3D` / `f32` blocks in host memory.

use alloc::vec::Vec;

use rustclr_core::{ArrayStorage, ClrArray, ExecResult, Interpreter, Value};

use crate::sys;

const GFX3D: u64 = 56;

// bz_abi::gfx3d_op
const CREATE: u32 = 0;
const FREE: u32 = 1;
const CLEAR: u32 = 2;
const MESH: u32 = 3;
const TEXTURE: u32 = 4;
const LIGHTS: u32 = 5;
const DRAW: u32 = 6;
const PRESENT: u32 = 7;
const READ_PIXEL: u32 = 8;
const READ: u32 = 9;

/// `bz_abi::Gfx3dCmd`.
#[repr(C)]
#[derive(Default)]
struct Gfx3dCmd {
    op: u32,
    target: u32,
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    ptr: u64,
    len: u64,
    ptr2: u64,
    len2: u64,
}

/// `bz_abi::Vertex3D`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Vertex3D {
    p: [f32; 3],
    n: [f32; 3],
    uv: [f32; 2],
    color: u32,
    _reserved: u32,
}

fn call(c: &Gfx3dCmd) -> u64 {
    sys::syscall(GFX3D, c as *const Gfx3dCmd as u64, 0, 0)
}

fn int(a: &[Value], i: usize) -> i64 {
    match a.get(i) {
        Some(Value::I32(v)) => *v as i64,
        Some(Value::I64(v)) | Some(Value::NativeInt(v)) => *v,
        _ => 0,
    }
}

fn ret_i32(v: i64) -> ExecResult<Option<Value>> {
    Ok(Some(Value::I32(v as i32)))
}

/// Result as an id / value, -1 for an error.
fn ret(r: u64) -> ExecResult<Option<Value>> {
    ret_i32(if sys::is_err(r) { -1 } else { r as i64 })
}

fn floats(interp: &Interpreter, a: &[Value], i: usize) -> Vec<f32> {
    let Some(Value::Obj(h)) = a.get(i) else { return Vec::new() };
    if h.is_null() {
        return Vec::new();
    }
    interp
        .heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::F32(v) => v.clone(),
            ArrayStorage::F64(v) => v.iter().map(|&x| x as f32).collect(),
            // an array made generically (List<float>.ToArray()) keeps Values
            ArrayStorage::Values(v) => v
                .iter()
                .map(|x| match x {
                    Value::F(f) => *f as f32,
                    Value::I32(i) => *i as f32,
                    _ => 0.0,
                })
                .collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

fn ints(interp: &Interpreter, a: &[Value], i: usize) -> Vec<u32> {
    let Some(Value::Obj(h)) = a.get(i) else { return Vec::new() };
    if h.is_null() {
        return Vec::new();
    }
    interp
        .heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::I32(v) => v.iter().map(|&p| p as u32).collect(),
            ArrayStorage::U32(v) => v.clone(),
            ArrayStorage::Values(v) => v
                .iter()
                .map(|x| match x {
                    Value::I32(i) => *i as u32,
                    Value::I64(i) => *i as u32,
                    _ => 0,
                })
                .collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

/// `G3dCreate(int width, int height)` -> context id, -1.
fn create(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    ret(call(&Gfx3dCmd { op: CREATE, a: int(a, 0) as u32, b: int(a, 1) as u32, ..Default::default() }))
}

/// `G3dFree(int id)` -> 0 / -1.
fn free(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    ret(call(&Gfx3dCmd { op: FREE, target: int(a, 0) as u32, ..Default::default() }))
}

/// `G3dClear(int context, int color, int bits)`.
fn clear(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    ret(call(&Gfx3dCmd { op: CLEAR, target: int(a, 0) as u32, a: int(a, 1) as u32, b: int(a, 2) as u32, ..Default::default() }))
}

/// `G3dMesh(float[] vertices, int[] colors, int[] indices)` -> mesh id, -1.
/// `vertices` has 8 floats per vertex (position, normal, uv); `colors` one
/// 0xAARRGGBB per vertex or null (white).
fn mesh(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let f = floats(i, a, 0);
    let colors = ints(i, a, 1);
    let idx = ints(i, a, 2);
    let n = f.len() / 8;
    if n == 0 {
        return ret_i32(-1);
    }
    let mut v = Vec::with_capacity(n);
    for k in 0..n {
        let s = &f[k * 8..k * 8 + 8];
        v.push(Vertex3D {
            p: [s[0], s[1], s[2]],
            n: [s[3], s[4], s[5]],
            uv: [s[6], s[7]],
            color: colors.get(k).copied().unwrap_or(0xFFFF_FFFF),
            _reserved: 0,
        });
    }
    ret(call(&Gfx3dCmd {
        op: MESH,
        a: n as u32,
        b: idx.len() as u32,
        ptr: v.as_ptr() as u64,
        len: (n * core::mem::size_of::<Vertex3D>()) as u64,
        ptr2: if idx.is_empty() { 0 } else { idx.as_ptr() as u64 },
        len2: (idx.len() * 4) as u64,
        ..Default::default()
    }))
}

/// `G3dTexture(int width, int height, int[] pixels)` -> texture id, -1.
fn texture(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (w, h) = (int(a, 0), int(a, 1));
    let px = ints(i, a, 2);
    if w <= 0 || h <= 0 || (px.len() as i64) < w * h {
        return ret_i32(-1);
    }
    ret(call(&Gfx3dCmd { op: TEXTURE, a: w as u32, b: h as u32, ptr: px.as_ptr() as u64, len: (px.len() * 4) as u64, ..Default::default() }))
}

/// `G3dLights(int context, float[] block)` (bz_abi::gfx3d_light layout).
fn lights(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let b = floats(i, a, 1);
    if b.is_empty() {
        return ret_i32(-1);
    }
    ret(call(&Gfx3dCmd { op: LIGHTS, target: int(a, 0) as u32, ptr: b.as_ptr() as u64, len: (b.len() * 4) as u64, ..Default::default() }))
}

/// `G3dDraw(int context, int mesh, int texture, int flags, int color,
/// float[] matrices)` -> pixels << 32 | triangles, -1.
fn draw(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let m = floats(i, a, 5);
    if m.len() < 32 {
        return Ok(Some(Value::I64(-1)));
    }
    let r = call(&Gfx3dCmd {
        op: DRAW,
        target: int(a, 0) as u32,
        a: int(a, 1) as u32,
        b: int(a, 2) as u32,
        c: int(a, 3) as u32,
        d: int(a, 4) as u32,
        ptr: m.as_ptr() as u64,
        len: 128,
        ..Default::default()
    });
    Ok(Some(Value::I64(if sys::is_err(r) { -1 } else { r as i64 })))
}

/// `G3dPresent(int context, int window, int x, int y)` -> 0 / -1.
fn present(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    ret(call(&Gfx3dCmd { op: PRESENT, target: int(a, 0) as u32, a: int(a, 1) as u32, b: int(a, 2) as u32, c: int(a, 3) as u32, ..Default::default() }))
}

/// `G3dReadPixel(int context, int x, int y)` -> 0xRRGGBB, -1.
fn read_pixel(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    ret(call(&Gfx3dCmd { op: READ_PIXEL, target: int(a, 0) as u32, a: int(a, 1) as u32, b: int(a, 2) as u32, ..Default::default() }))
}

/// `G3dRead(int context, int[] pixels)` -> 0 / -1: the whole color buffer.
fn read(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let Some(Value::Obj(h)) = a.get(1) else { return ret_i32(-1) };
    let len = i.heap.with::<ClrArray, _>(*h, |arr| match &arr.storage {
        ArrayStorage::I32(v) => v.len(),
        _ => 0,
    });
    let len = len.unwrap_or(0);
    if len == 0 {
        return ret_i32(-1);
    }
    let mut buf = alloc::vec![0u32; len];
    let r = call(&Gfx3dCmd { op: READ, target: int(a, 0) as u32, ptr: buf.as_mut_ptr() as u64, len: (len * 4) as u64, ..Default::default() });
    if sys::is_err(r) {
        return ret_i32(-1);
    }
    let _ = i.heap.with_mut::<ClrArray, _>(*h, |arr| {
        if let ArrayStorage::I32(v) = &mut arr.storage {
            for (d, s) in v.iter_mut().zip(&buf) {
                *d = *s as i32;
            }
        }
    });
    ret_i32(0)
}

/// Register the bindings.
pub fn install(interp: &mut Interpreter) {
    let natives: [(&str, rustclr_core::interp::NativeFn); 10] = [
        ("G3dCreate", create),
        ("G3dFree", free),
        ("G3dClear", clear),
        ("G3dMesh", mesh),
        ("G3dTexture", texture),
        ("G3dLights", lights),
        ("G3dDraw", draw),
        ("G3dPresent", present),
        ("G3dReadPixel", read_pixel),
        ("G3dRead", read),
    ];
    for (name, f) in natives {
        interp.register_native(alloc::format!("pinvoke:buitenzorg!{}", name), f);
    }
}
