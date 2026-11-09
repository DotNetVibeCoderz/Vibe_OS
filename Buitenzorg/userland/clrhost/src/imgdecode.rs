//! Image decoding for managed apps (Buitenzorg.App `Images`): BMP, PNG
//! (`zune-png`), baseline + progressive JPEG (`zune-jpeg`) and GIF (here:
//! LZW, palettes, transparency, frame disposal -- every frame of an
//! animation). Pixels come out as 0xAARRGGBB `int`s, ready for a Blit.
//!
//! Decoding in the interpreter would take seconds per picture under
//! emulation; here it is native code. Two calls: `ImageInfo(data)` (size,
//! frame count) sizes the managed array, `ImageDecodeInto(data, frame,
//! pixels)` fills it -- the last decode is cached so the pair costs one
//! decode.

use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

pub struct Picture {
    pub width: usize,
    pub height: usize,
    /// One ARGB buffer per frame, with each frame's delay in milliseconds.
    pub frames: Vec<(Vec<u32>, u32)>,
}

/// The last decode, behind a small spin lock (managed threads may decode at once).
struct Cache {
    busy: AtomicBool,
    slot: UnsafeCell<Option<(u64, Picture)>>,
}
unsafe impl Sync for Cache {}
static CACHE: Cache = Cache { busy: AtomicBool::new(false), slot: UnsafeCell::new(None) };

fn key(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ data.len() as u64;
    for b in data.iter().take(4096).chain(data.iter().rev().take(256)) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Decode (or take the cached decode of) `data`, then run `f` on it.
pub fn with_picture<R>(data: &[u8], f: impl FnOnce(&Picture) -> R) -> Option<R> {
    let k = key(data);
    while CACHE.busy.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
    let c = unsafe { &mut *CACHE.slot.get() };
    if c.as_ref().map(|(ck, _)| *ck) != Some(k) {
        *c = decode(data).map(|p| (k, p));
    }
    let r = c.as_ref().map(|(_, p)| f(p));
    CACHE.busy.store(false, Ordering::Release);
    r
}

pub fn decode(d: &[u8]) -> Option<Picture> {
    if d.len() >= 2 && &d[..2] == b"BM" {
        return bmp(d);
    }
    if d.len() >= 8 && &d[..8] == b"\x89PNG\r\n\x1a\n" {
        return png(d);
    }
    if d.len() >= 3 && d[0] == 0xFF && d[1] == 0xD8 {
        return jpeg(d);
    }
    if d.len() >= 6 && (&d[..6] == b"GIF87a" || &d[..6] == b"GIF89a") {
        return gif(d);
    }
    None
}

fn one(width: usize, height: usize, px: Vec<u32>) -> Option<Picture> {
    (width > 0 && height > 0 && px.len() == width * height).then(|| Picture { width, height, frames: alloc::vec![(px, 0)] })
}

fn bmp(d: &[u8]) -> Option<Picture> {
    let u32_at = |i: usize| Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?));
    let off = u32_at(10)? as usize;
    let w = u32_at(18)? as i32;
    let h = u32_at(22)? as i32;
    let bpp = u16::from_le_bytes(d.get(28..30)?.try_into().ok()?) as usize;
    if w <= 0 || h == 0 || (bpp != 24 && bpp != 32) || w > 8192 || h.abs() > 8192 {
        return None;
    }
    let (w, top_down, ah) = (w as usize, h < 0, h.unsigned_abs() as usize);
    let row = (w * bpp / 8 + 3) & !3;
    let mut px = alloc::vec![0u32; w * ah];
    for y in 0..ah {
        let src = off + (if top_down { y } else { ah - 1 - y }) * row;
        for x in 0..w {
            let p = src + x * bpp / 8;
            let (b, g, r) = (*d.get(p)? as u32, *d.get(p + 1)? as u32, *d.get(p + 2)? as u32);
            px[y * w + x] = 0xFF00_0000 | r << 16 | g << 8 | b;
        }
    }
    one(w, ah, px)
}

/// 8-bit samples of `channels` (1 gray, 2 gray+alpha, 3 RGB, 4 RGBA) to ARGB.
fn to_argb(raw: &[u8], channels: usize, w: usize, h: usize) -> Option<Vec<u32>> {
    if channels == 0 || raw.len() < w * h * channels {
        return None;
    }
    Some(
        raw.chunks_exact(channels)
            .take(w * h)
            .map(|c| {
                let (r, g, b, a) = match channels {
                    1 => (c[0], c[0], c[0], 255),
                    2 => (c[0], c[0], c[0], c[1]),
                    3 => (c[0], c[1], c[2], 255),
                    _ => (c[0], c[1], c[2], c[3]),
                };
                (a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
            })
            .collect(),
    )
}

fn png(d: &[u8]) -> Option<Picture> {
    let opts = zune_core::options::DecoderOptions::default().png_set_strip_to_8bit(true).set_max_width(8192).set_max_height(8192);
    let mut dec = zune_png::PngDecoder::new_with_options(d, opts);
    let raw = dec.decode_raw().ok()?;
    let (w, h) = dec.get_dimensions()?;
    let channels = dec.get_colorspace()?.num_components();
    one(w, h, to_argb(&raw, channels, w, h)?)
}

fn jpeg(d: &[u8]) -> Option<Picture> {
    let opts = zune_core::options::DecoderOptions::default()
        .jpeg_set_out_colorspace(zune_core::colorspace::ColorSpace::RGB)
        .set_max_width(8192)
        .set_max_height(8192);
    let mut dec = zune_jpeg::JpegDecoder::new_with_options(d, opts);
    let raw = dec.decode().ok()?;
    let (w, h) = dec.dimensions()?;
    one(w, h, to_argb(&raw, 3, w, h)?)
}

// ---- GIF ------------------------------------------------------------------------

fn gif(d: &[u8]) -> Option<Picture> {
    let w = u16::from_le_bytes([*d.get(6)?, *d.get(7)?]) as usize;
    let h = u16::from_le_bytes([*d.get(8)?, *d.get(9)?]) as usize;
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let flags = d[10];
    let bg_index = d[11] as usize;
    let mut p = 13;
    let mut global: Vec<u32> = Vec::new();
    if flags & 0x80 != 0 {
        let n = 2usize << (flags & 7);
        global = palette(d.get(p..p + n * 3)?);
        p += n * 3;
    }
    let mut canvas = alloc::vec![0u32; w * h];
    let mut frames: Vec<(Vec<u32>, u32)> = Vec::new();
    let (mut delay, mut transparent, mut disposal): (u32, Option<usize>, u8) = (0, None, 0);
    while p < d.len() && frames.len() < 256 {
        match d[p] {
            0x21 => {
                // extension: graphic control (delay, transparency, disposal), others skipped
                let label = *d.get(p + 1)?;
                p += 2;
                if label == 0xF9 && *d.get(p)? >= 4 {
                    let pf = d[p + 1];
                    delay = u16::from_le_bytes([d[p + 2], d[p + 3]]) as u32 * 10;
                    transparent = (pf & 1 != 0).then_some(d[p + 4] as usize);
                    disposal = (pf >> 2) & 7;
                }
                p = skip_blocks(d, p)?;
            }
            0x2C => {
                let fx = u16::from_le_bytes([*d.get(p + 1)?, *d.get(p + 2)?]) as usize;
                let fy = u16::from_le_bytes([*d.get(p + 3)?, *d.get(p + 4)?]) as usize;
                let fw = u16::from_le_bytes([*d.get(p + 5)?, *d.get(p + 6)?]) as usize;
                let fh = u16::from_le_bytes([*d.get(p + 7)?, *d.get(p + 8)?]) as usize;
                let lf = *d.get(p + 9)?;
                p += 10;
                let local;
                let pal: &[u32] = if lf & 0x80 != 0 {
                    let n = 2usize << (lf & 7);
                    local = palette(d.get(p..p + n * 3)?);
                    p += n * 3;
                    &local
                } else {
                    &global
                };
                let min_code = *d.get(p)? as u32;
                p += 1;
                let mut data = Vec::new();
                loop {
                    let n = *d.get(p)? as usize;
                    p += 1;
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(d.get(p..p + n)?);
                    p += n;
                }
                let indices = lzw(&data, min_code, fw * fh)?;
                let before = if disposal == 3 { Some(canvas.clone()) } else { None };
                let interlaced = lf & 0x40 != 0;
                for (k, &ix) in indices.iter().enumerate().take(fw * fh) {
                    let row = if interlaced { interlace_row(k / fw, fh) } else { k / fw };
                    let (x, y) = (fx + k % fw, fy + row);
                    if x >= w || y >= h || Some(ix as usize) == transparent {
                        continue;
                    }
                    if let Some(c) = pal.get(ix as usize) {
                        canvas[y * w + x] = *c;
                    }
                }
                frames.push((canvas.clone(), delay.max(20)));
                // disposal for the next frame: 2 = clear the area, 3 = restore
                match disposal {
                    2 => {
                        let bg = if transparent.is_some() { 0 } else { global.get(bg_index).copied().unwrap_or(0) };
                        for y in fy..(fy + fh).min(h) {
                            for x in fx..(fx + fw).min(w) {
                                canvas[y * w + x] = bg;
                            }
                        }
                    }
                    3 => {
                        if let Some(b) = before {
                            canvas = b;
                        }
                    }
                    _ => {}
                }
                transparent = None;
                disposal = 0;
                delay = 0;
            }
            0x3B => break,
            _ => return None,
        }
    }
    if frames.is_empty() {
        return None;
    }
    if frames.len() == 1 {
        frames[0].1 = 0;
    }
    Some(Picture { width: w, height: h, frames })
}

fn palette(b: &[u8]) -> Vec<u32> {
    b.chunks_exact(3).map(|c| 0xFF00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32).collect()
}

fn skip_blocks(d: &[u8], mut p: usize) -> Option<usize> {
    loop {
        let n = *d.get(p)? as usize;
        p += 1 + n;
        if n == 0 {
            return Some(p);
        }
    }
}

/// The row an interlaced GIF's `k`-th stored row lands on (passes 8/8/4/2).
fn interlace_row(k: usize, h: usize) -> usize {
    let passes = [(0usize, 8usize), (4, 8), (2, 4), (1, 2)];
    let mut k = k;
    for (start, step) in passes {
        let n = if h > start { (h - start).div_ceil(step) } else { 0 };
        if k < n {
            return start + k * step;
        }
        k -= n;
    }
    0
}

/// Write the string of dictionary entry `c` (walking the prefixes back).
fn emit(prefix: &[u16], suffix: &[u8], c: u32, stack: &mut Vec<u8>, out: &mut Vec<u8>) {
    stack.clear();
    let mut c = c as usize;
    while c != u16::MAX as usize && c < prefix.len() && stack.len() < 4096 {
        stack.push(suffix[c]);
        c = prefix[c] as usize;
    }
    out.extend(stack.iter().rev());
}

/// GIF's variable-width LZW: `min` = the minimum code size.
fn lzw(data: &[u8], min: u32, max_out: usize) -> Option<Vec<u8>> {
    if !(2..=11).contains(&min) {
        return None;
    }
    let clear = 1u32 << min;
    let end = clear + 1;
    let mut size = min + 1;
    // the dictionary as (prefix, last byte, first byte, length)
    let mut prefix: Vec<u16> = Vec::with_capacity(4096);
    let mut suffix: Vec<u8> = Vec::with_capacity(4096);
    let mut first: Vec<u8> = Vec::with_capacity(4096);
    let reset = |prefix: &mut Vec<u16>, suffix: &mut Vec<u8>, first: &mut Vec<u8>| {
        prefix.clear();
        suffix.clear();
        first.clear();
        for i in 0..clear as usize {
            prefix.push(u16::MAX);
            suffix.push(i as u8);
            first.push(i as u8);
        }
        prefix.push(u16::MAX);
        suffix.push(0);
        first.push(0); // clear
        prefix.push(u16::MAX);
        suffix.push(0);
        first.push(0); // end
    };
    reset(&mut prefix, &mut suffix, &mut first);
    let mut out = Vec::with_capacity(max_out);
    let (mut bits, mut nbits, mut pos) = (0u32, 0u32, 0usize);
    let mut prev: Option<u32> = None;
    let mut stack = Vec::with_capacity(4096);
    loop {
        while nbits < size {
            if pos >= data.len() {
                return Some(out);
            }
            bits |= (data[pos] as u32) << nbits;
            nbits += 8;
            pos += 1;
        }
        let code = bits & ((1 << size) - 1);
        bits >>= size;
        nbits -= size;
        if code == clear {
            reset(&mut prefix, &mut suffix, &mut first);
            size = min + 1;
            prev = None;
            continue;
        }
        if code == end {
            return Some(out);
        }
        let n = prefix.len() as u32;
        match prev {
            None => {
                if code >= n {
                    return None;
                }
                emit(&prefix, &suffix, code, &mut stack, &mut out);
            }
            Some(pc) => {
                let fb = if code < n { first[code as usize] } else if code == n { first[pc as usize] } else { return None };
                if n < 4096 {
                    prefix.push(pc as u16);
                    suffix.push(fb);
                    first.push(first[pc as usize]);
                }
                emit(&prefix, &suffix, code, &mut stack, &mut out);
                if prefix.len() as u32 == (1 << size) && size < 12 {
                    size += 1;
                }
            }
        }
        prev = Some(code);
        if out.len() >= max_out {
            return Some(out);
        }
    }
}
