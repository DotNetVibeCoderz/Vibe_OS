//! A textured ray-casting renderer for managed games (Labirin, MLabirin):
//! one native call draws a whole first-person frame into an `int[]`, so the
//! interpreter only runs the game rules. Grid map of bytes (0 empty, 1 hedge,
//! 2 stone, 3 gate), procedural textures, distance shading, floor/ceiling
//! gradients and billboard sprites with a depth buffer.
//!
//! `RayRender(int[] pixels, int w, int h, byte[] map, int mapW, int mapH,
//!            int px, int py, int angle, int[] sprites)`:
//! positions in 1/1024 map cells, angle in 1/4096 turns, sprites as
//! (x, y, kind) triples (kind 1 flower, 2 fruit). Returns 0, or -1.

use alloc::vec::Vec;

use rustclr_core::{ArrayStorage, ClrArray, ExecResult, Interpreter, Value};

use crate::bzapi::int;

const TEX: usize = 64;

/// The wall textures (64x64 each), made on first use.
static TEXTURES: crate::netapi::Lock<Option<[Vec<u32>; 3]>> = crate::netapi::Lock::new(None);

fn hash(x: u32, y: u32, s: u32) -> u32 {
    let mut h = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ s.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

fn rgb(r: u32, g: u32, b: u32) -> u32 {
    (r.min(255) << 16) | (g.min(255) << 8) | b.min(255)
}

/// Clipped hedge: dark greens with leaf speckles.
fn hedge() -> Vec<u32> {
    let mut t = alloc::vec![0u32; TEX * TEX];
    for y in 0..TEX {
        for x in 0..TEX {
            let n = hash(x as u32 / 2, y as u32 / 2, 7) % 100;
            let leaf = hash(x as u32, y as u32, 3) % 9 == 0;
            let base = 60 + n / 3;
            t[y * TEX + x] = if leaf { rgb(90, 150 + n / 2, 70) } else { rgb(base / 3, base + 20, base / 3 + 10) };
        }
    }
    t
}

/// Old garden wall: stones with mortar lines.
fn stone() -> Vec<u32> {
    let mut t = alloc::vec![0u32; TEX * TEX];
    for y in 0..TEX {
        for x in 0..TEX {
            let row = y / 16;
            let off = if row % 2 == 0 { 0 } else { 16 };
            let mortar = y % 16 == 0 || (x + off) % 32 == 0;
            let n = hash(((x + off) / 32) as u32, row as u32, 11) % 40;
            let g = hash(x as u32, y as u32, 5) % 12;
            t[y * TEX + x] = if mortar { rgb(70, 66, 58) } else { rgb(150 + n + g, 140 + n + g, 120 + n) };
        }
    }
    t
}

/// The exit gate: wooden bars over a bright opening.
fn gate() -> Vec<u32> {
    let mut t = alloc::vec![0u32; TEX * TEX];
    for y in 0..TEX {
        for x in 0..TEX {
            let bar = x % 12 < 4 || y < 6 || y > 57;
            t[y * TEX + x] = if bar { rgb(120 + (hash(x as u32, y as u32, 9) % 30), 80, 40) } else { rgb(250, 230, 150) };
        }
    }
    t
}

/// Sprite texture (32x32, 0 = transparent).
fn sprite_pixel(kind: i32, u: usize, v: usize) -> u32 {
    let (dx, dy) = (u as i32 - 16, v as i32 - 16);
    match kind {
        1 => {
            // a golden flower on a stem
            if v > 18 && (u == 15 || u == 16) {
                return rgb(60, 140, 60);
            }
            let (fx, fy) = (dx, dy + 6);
            let r2 = fx * fx + fy * fy;
            if r2 < 9 {
                rgb(200, 110, 30)
            } else if r2 < 64 && ((fx.abs() < 3) || (fy.abs() < 3) || (fx - fy).abs() < 3 || (fx + fy).abs() < 3) {
                rgb(255, 210, 60)
            } else {
                0
            }
        }
        _ => {
            // a round red fruit with a leaf
            if dx * dx + (dy - 2) * (dy - 2) < 100 {
                let shine = dx < -2 && dy < 0 && dx * dx + dy * dy < 20;
                if shine { rgb(255, 170, 160) } else { rgb(210, 40, 40) }
            } else if dy < -8 && dy > -13 && dx > 0 && dx < 6 {
                rgb(60, 150, 60)
            } else {
                0
            }
        }
    }
}

fn shade(c: u32, f: u32) -> u32 {
    // f in 0..=256
    let r = ((c >> 16) & 255) * f >> 8;
    let g = ((c >> 8) & 255) * f >> 8;
    let b = (c & 255) * f >> 8;
    (r << 16) | (g << 8) | b
}

fn sin_cos(turns4096: i64) -> (f64, f64) {
    let a = (turns4096 as f64) * core::f64::consts::TAU / 4096.0;
    (libm::sin(a), libm::cos(a))
}

pub fn render(px_out: &mut [i32], w: usize, h: usize, map: &[u8], mw: usize, mh: usize, px: f64, py: f64, ang: i64, sprites: &[i32]) {
    let mut guard = TEXTURES.lock();
    let tex = guard.get_or_insert_with(|| [hedge(), stone(), gate()]);
    let (sa, ca) = sin_cos(ang);
    let (dir_x, dir_y) = (ca, sa);
    // camera plane: 66 degree field of view
    let (plane_x, plane_y) = (-sa * 0.66, ca * 0.66);
    let half = h / 2;
    // sky and lawn
    for y in 0..h {
        let c = if y < half {
            let t = (y * 256 / half.max(1)) as u32;
            rgb(120 + t * 60 / 256, 170 + t * 40 / 256, 220 - t * 30 / 256)
        } else {
            let t = ((y - half) * 256 / half.max(1)) as u32;
            rgb(40 + t * 30 / 256, 70 + t * 70 / 256, 35 + t * 20 / 256)
        };
        let row = &mut px_out[y * w..y * w + w];
        for p in row.iter_mut() {
            *p = c as i32;
        }
    }
    let mut zbuf = alloc::vec![f64::MAX; w];
    for x in 0..w {
        let cam = 2.0 * x as f64 / w as f64 - 1.0;
        let (rx, ry) = (dir_x + plane_x * cam, dir_y + plane_y * cam);
        let (mut mx, mut my) = (px as i64, py as i64);
        let ddx = if rx == 0.0 { 1e30 } else { (1.0 / rx).abs() };
        let ddy = if ry == 0.0 { 1e30 } else { (1.0 / ry).abs() };
        let (step_x, mut sdx) = if rx < 0.0 { (-1, (px - mx as f64) * ddx) } else { (1, (mx as f64 + 1.0 - px) * ddx) };
        let (step_y, mut sdy) = if ry < 0.0 { (-1, (py - my as f64) * ddy) } else { (1, (my as f64 + 1.0 - py) * ddy) };
        let mut side = 0;
        let mut cell = 0u8;
        for _ in 0..64 {
            if sdx < sdy {
                sdx += ddx;
                mx += step_x;
                side = 0;
            } else {
                sdy += ddy;
                my += step_y;
                side = 1;
            }
            if mx < 0 || my < 0 || mx >= mw as i64 || my >= mh as i64 {
                cell = 2;
                break;
            }
            cell = map[my as usize * mw + mx as usize];
            if cell != 0 {
                break;
            }
        }
        if cell == 0 {
            continue;
        }
        let dist = if side == 0 { sdx - ddx } else { sdy - ddy }.max(0.05);
        zbuf[x] = dist;
        let line = (h as f64 / dist) as i64;
        let y0 = (half as i64 - line / 2).max(0) as usize;
        let y1 = ((half as i64 + line / 2).min(h as i64 - 1)).max(0) as usize;
        let hit = if side == 0 { py + dist * ry } else { px + dist * rx };
        let mut tx = ((hit - libm::floor(hit)) * TEX as f64) as usize % TEX;
        if (side == 0 && rx > 0.0) || (side == 1 && ry < 0.0) {
            tx = TEX - 1 - tx;
        }
        let t = &tex[((cell as usize).max(1) - 1).min(2)];
        let fog = (256.0 / (1.0 + dist * 0.18)) as u32;
        let f = if side == 1 { fog * 3 / 4 } else { fog };
        for y in y0..=y1 {
            let ty = (((y as i64 - (half as i64 - line / 2)) * TEX as i64) / line.max(1)).clamp(0, TEX as i64 - 1) as usize;
            px_out[y * w + x] = shade(t[ty * TEX + tx], f) as i32;
        }
    }
    // sprites, far to near
    let mut sp: Vec<(f64, f64, i32)> = sprites.chunks_exact(3).map(|s| (s[0] as f64 / 1024.0, s[1] as f64 / 1024.0, s[2])).collect();
    sp.sort_by(|a, b| {
        let da = (a.0 - px) * (a.0 - px) + (a.1 - py) * (a.1 - py);
        let db = (b.0 - px) * (b.0 - px) + (b.1 - py) * (b.1 - py);
        db.partial_cmp(&da).unwrap_or(core::cmp::Ordering::Equal)
    });
    let inv = 1.0 / (plane_x * dir_y - dir_x * plane_y);
    for (sx, sy, kind) in sp {
        let (rx, ry) = (sx - px, sy - py);
        let tx = inv * (dir_y * rx - dir_x * ry);
        let tz = inv * (-plane_y * rx + plane_x * ry);
        if tz <= 0.1 {
            continue;
        }
        let screen_x = ((w as f64 / 2.0) * (1.0 + tx / tz)) as i64;
        let size = ((h as f64 / tz) * 0.5) as i64;
        if size <= 0 {
            continue;
        }
        let top = half as i64 + ((h as f64 / tz) * 0.5) as i64 - size; // standing on the lawn
        let fog = (256.0 / (1.0 + tz * 0.18)) as u32;
        for sxp in (screen_x - size / 2).max(0)..(screen_x + size / 2).min(w as i64) {
            if zbuf[sxp as usize] < tz {
                continue;
            }
            let u = (((sxp - (screen_x - size / 2)) * 32) / size).clamp(0, 31) as usize;
            for syp in top.max(0)..(top + size).min(h as i64) {
                let v = (((syp - top) * 32) / size).clamp(0, 31) as usize;
                let c = sprite_pixel(kind, u, v);
                if c != 0 {
                    px_out[syp as usize * w + sxp as usize] = shade(c, fog) as i32;
                }
            }
        }
    }
}

fn ints(i: &Interpreter, a: &[Value], idx: usize) -> Vec<i32> {
    let Some(Value::Obj(h)) = a.get(idx) else { return Vec::new() };
    i.heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::I32(v) => v.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

fn ray_render(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (w, h) = (int(a, 1).clamp(1, 2048) as usize, int(a, 2).clamp(1, 2048) as usize);
    let map: Vec<u8> = match a.get(3) {
        Some(Value::Obj(hd)) => i.heap.with::<ClrArray, _>(*hd, |arr| match &arr.storage {
            ArrayStorage::U8(v) => v.clone(),
            _ => Vec::new(),
        }).unwrap_or_default(),
        _ => Vec::new(),
    };
    let (mw, mh) = (int(a, 4).max(1) as usize, int(a, 5).max(1) as usize);
    if map.len() < mw * mh {
        return Ok(Some(Value::I32(-1)));
    }
    let (px, py, ang) = (int(a, 6) as f64 / 1024.0, int(a, 7) as f64 / 1024.0, int(a, 8));
    let sprites = ints(i, a, 9);
    let mut frame = alloc::vec![0i32; w * h];
    render(&mut frame, w, h, &map, mw, mh, px, py, ang, &sprites);
    let Some(Value::Obj(hd)) = a.first() else { return Ok(Some(Value::I32(-1))) };
    let ok = i
        .heap
        .with_mut::<ClrArray, _>(*hd, |arr| match &mut arr.storage {
            ArrayStorage::I32(v) if v.len() >= frame.len() => {
                v[..frame.len()].copy_from_slice(&frame);
                true
            }
            _ => false,
        })
        .unwrap_or(false);
    Ok(Some(Value::I32(if ok { 0 } else { -1 })))
}

pub fn register(interp: &mut Interpreter) {
    interp.register_native(alloc::string::String::from("pinvoke:buitenzorg!RayRender"), ray_render);
}
