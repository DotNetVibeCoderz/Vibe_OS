//! Pixel work for `Buitenzorg.Game`'s 2D canvas: an interpreter loop per pixel
//! is far too slow for sprites and tilemaps, so the canvas (an `int[]` of
//! 0xAARRGGBB) is written by these natives in place.
//!
//! - `PixFill(int[] dst, w, h, x, y, rw, rh, color)`
//! - `PixBlit(int[] dst, dw, dh, int[] src, sw, sh, sx, sy, srw, srh, dx, dy,
//!   drw, drh, flags)`: the source rectangle scaled (nearest) to the
//!   destination one; flags 1 = mirror horizontally, 2 = skip pixels whose
//!   alpha is 0 (sprites with transparent parts)
//! - `PixTiles(int[] dst, dw, dh, int[] tileset, tsw, tsh, tile, byte[] map,
//!   mw, mh, camx, camy)`: every tile of a map that is visible from camera
//!   (camx, camy) -- tile n (1-based) is the n-th tile-sized square of the
//!   tileset, row by row; 0 is empty
//!
//! - `PixOrient(int[] dst, int[] src, sw, sh, orientation)`: `src` turned
//!   and/or mirrored as an EXIF orientation (1..8) says; `dst` is sh x sw for
//!   5..8, else sw x sh (the image viewer)
//!
//! All clip to the destination. Each returns 0, or -1 for bad arguments.

use alloc::vec::Vec;

use rustclr_core::{ArrayStorage, ClrArray, ExecResult, Interpreter, Value};

use crate::bzapi::int;

fn ints(i: &Interpreter, a: &[Value], idx: usize) -> Vec<i32> {
    let Some(Value::Obj(h)) = a.get(idx) else { return Vec::new() };
    i.heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::I32(v) => v.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

fn bytes(i: &Interpreter, a: &[Value], idx: usize) -> Vec<u8> {
    let Some(Value::Obj(h)) = a.get(idx) else { return Vec::new() };
    i.heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::U8(v) => v.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

/// Run `f` on the destination `int[]` (argument 0) in place.
fn with_dst(i: &mut Interpreter, a: &[Value], f: impl FnOnce(&mut [i32])) -> bool {
    let Some(Value::Obj(h)) = a.first() else { return false };
    i.heap
        .with_mut::<ClrArray, _>(*h, |arr| match &mut arr.storage {
            ArrayStorage::I32(v) => {
                f(v);
                true
            }
            _ => false,
        })
        .unwrap_or(false)
}

fn ret(ok: bool) -> ExecResult<Option<Value>> {
    Ok(Some(Value::I32(if ok { 0 } else { -1 })))
}

fn pix_fill(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (w, h) = (int(a, 1), int(a, 2));
    let (x, y, rw, rh, c) = (int(a, 3), int(a, 4), int(a, 5), int(a, 6), int(a, 7) as i32);
    if w <= 0 || h <= 0 {
        return ret(false);
    }
    let (x0, y0) = (x.max(0), y.max(0));
    let (x1, y1) = ((x + rw).min(w), (y + rh).min(h));
    let ok = with_dst(i, a, |d| {
        if (d.len() as i64) < w * h {
            return;
        }
        for yy in y0..y1 {
            let row = (yy * w) as usize;
            for xx in x0..x1 {
                d[row + xx as usize] = c;
            }
        }
    });
    ret(ok)
}

#[allow(clippy::too_many_arguments)]
fn blit(d: &mut [i32], dw: i64, dh: i64, s: &[i32], sw: i64, sh: i64, sx: i64, sy: i64, srw: i64, srh: i64, dx: i64, dy: i64, drw: i64, drh: i64, flags: i64) {
    if srw <= 0 || srh <= 0 || drw <= 0 || drh <= 0 || (s.len() as i64) < sw * sh || (d.len() as i64) < dw * dh {
        return;
    }
    let flip = flags & 1 != 0;
    let keyed = flags & 2 != 0;
    for y in dy.max(0)..(dy + drh).min(dh) {
        let v = sy + (y - dy) * srh / drh;
        if v < 0 || v >= sh {
            continue;
        }
        for x in dx.max(0)..(dx + drw).min(dw) {
            let mut u = (x - dx) * srw / drw;
            if flip {
                u = srw - 1 - u;
            }
            let u = sx + u;
            if u < 0 || u >= sw {
                continue;
            }
            let p = s[(v * sw + u) as usize];
            if keyed && (p as u32) >> 24 == 0 {
                continue;
            }
            d[(y * dw + x) as usize] = p;
        }
    }
}

fn pix_blit(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let src = ints(i, a, 3);
    let g = |k| int(a, k);
    let (dw, dh, sw, sh) = (g(1), g(2), g(4), g(5));
    let (sx, sy, srw, srh, dx, dy, drw, drh, fl) = (g(6), g(7), g(8), g(9), g(10), g(11), g(12), g(13), g(14));
    let ok = with_dst(i, a, |d| blit(d, dw, dh, &src, sw, sh, sx, sy, srw, srh, dx, dy, drw, drh, fl));
    ret(ok)
}

fn pix_tiles(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let set = ints(i, a, 3);
    let map = bytes(i, a, 7);
    let g = |k| int(a, k);
    let (dw, dh, tsw, tsh, tile, mw, mh, camx, camy) = (g(1), g(2), g(4), g(5), g(6), g(8), g(9), g(10), g(11));
    if tile <= 0 || tsw < tile || mw <= 0 || mh <= 0 || (map.len() as i64) < mw * mh {
        return ret(false);
    }
    let per_row = tsw / tile;
    let ok = with_dst(i, a, |d| {
        let (tx0, ty0) = ((camx.div_euclid(tile)).max(0), (camy.div_euclid(tile)).max(0));
        let (tx1, ty1) = (((camx + dw) / tile + 1).min(mw), ((camy + dh) / tile + 1).min(mh));
        for ty in ty0..ty1 {
            for tx in tx0..tx1 {
                let t = map[(ty * mw + tx) as usize] as i64;
                if t == 0 {
                    continue;
                }
                let (sx, sy) = (((t - 1) % per_row) * tile, ((t - 1) / per_row) * tile);
                blit(d, dw, dh, &set, tsw, tsh, sx, sy, tile, tile, tx * tile - camx, ty * tile - camy, tile, tile, 2);
            }
        }
    });
    ret(ok)
}

/// Where logical pixel (lx, ly) of an EXIF-oriented image comes from in the
/// stored one (sw x sh).
pub fn orient_source(o: i64, lx: i64, ly: i64, sw: i64, sh: i64) -> (i64, i64) {
    match o {
        2 => (sw - 1 - lx, ly),
        3 => (sw - 1 - lx, sh - 1 - ly),
        4 => (lx, sh - 1 - ly),
        5 => (ly, lx),
        6 => (ly, sh - 1 - lx),
        7 => (sw - 1 - ly, sh - 1 - lx),
        8 => (sw - 1 - ly, lx),
        _ => (lx, ly),
    }
}

fn pix_orient(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let src = ints(i, a, 1);
    let (sw, sh, o) = (int(a, 2), int(a, 3), int(a, 4));
    if sw <= 0 || sh <= 0 || (src.len() as i64) < sw * sh {
        return ret(false);
    }
    let (lw, lh) = if (5..=8).contains(&o) { (sh, sw) } else { (sw, sh) };
    let ok = with_dst(i, a, |d| {
        if (d.len() as i64) < lw * lh {
            return;
        }
        for ly in 0..lh {
            for lx in 0..lw {
                let (x, y) = orient_source(o, lx, ly, sw, sh);
                d[(ly * lw + lx) as usize] = src[(y * sw + x) as usize];
            }
        }
    });
    ret(ok)
}

pub fn register(interp: &mut Interpreter) {
    let natives: [(&str, rustclr_core::interp::NativeFn); 4] = [("PixFill", pix_fill), ("PixBlit", pix_blit), ("PixTiles", pix_tiles), ("PixOrient", pix_orient)];
    for (name, f) in natives {
        interp.register_native(alloc::format!("pinvoke:buitenzorg!{}", name), f);
    }
}
