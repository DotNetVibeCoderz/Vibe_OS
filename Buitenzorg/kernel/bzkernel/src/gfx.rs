//! Software rendering primitives (v0.6 "Daun"): a [`Canvas`] over a
//! `0x00RRGGBB` pixel buffer, with rectangle fills/outlines, alpha blending,
//! and text drawn from the Noto Sans Mono bitmap font. The compositor and
//! window manager render into a full-screen back buffer of these pixels.

use alloc::vec::Vec;
use noto_sans_mono_bitmap::{get_raster, get_raster_width, FontWeight, RasterHeight};

pub const FONT_HEIGHT: RasterHeight = RasterHeight::Size16;
pub const FONT_WEIGHT: FontWeight = FontWeight::Regular;

pub type Color = u32; // 0x00RRGGBB

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

/// A mutable view over a rectangular pixel buffer.
///
/// It may hold only a band of the frame's rows (`y0..y1`, parallel compose):
/// coordinates stay those of the whole `width` x `height` frame, drawing
/// outside the band is clipped, and row `y` lives at `(y - y0) * width`.
/// Code indexing `pixels` directly must go through [`Canvas::at`] /
/// [`Canvas::band`].
pub struct Canvas<'a> {
    pub pixels: &'a mut [Color],
    pub width: usize,
    pub height: usize,
    /// First row held in `pixels`.
    pub y0: usize,
    /// One past the last row held.
    pub y1: usize,
}

impl<'a> Canvas<'a> {
    pub fn new(pixels: &'a mut [Color], width: usize, height: usize) -> Self {
        Self { pixels, width, height, y0: 0, y1: height }
    }

    /// Rows `y0..y1` of a `width` x `height` frame; `pixels` holds exactly them.
    pub fn band(pixels: &'a mut [Color], width: usize, height: usize, y0: usize, y1: usize) -> Self {
        Self { pixels, width, height, y0, y1 }
    }

    /// Index of (x, y) in `pixels` (y must be within the band).
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> usize {
        (y - self.y0) * self.width + x
    }

    /// Whether rows `y..y + h` touch this canvas's band (whole elements
    /// outside it are skipped in a parallel compose).
    #[inline]
    pub fn rows_visible(&self, y: i32, h: i32) -> bool {
        y + h > self.y0 as i32 && y < self.y1 as i32
    }

    #[inline]
    pub fn put(&mut self, x: usize, y: usize, color: Color) {
        if x < self.width && y >= self.y0 && y < self.y1 {
            let i = self.at(x, y);
            self.pixels[i] = color;
        }
    }

    /// Alpha-blend `color` over the existing pixel (`alpha` 0..=255).
    #[inline]
    pub fn blend(&mut self, x: usize, y: usize, color: Color, alpha: u8) {
        if x >= self.width || y < self.y0 || y >= self.y1 {
            return;
        }
        let idx = self.at(x, y);
        self.pixels[idx] = blend_px(self.pixels[idx], color, alpha);
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        let x0 = x.max(0) as usize;
        let y0 = (y.max(0) as usize).max(self.y0);
        let x1 = ((x + w).max(0) as usize).min(self.width);
        let y1 = ((y + h).max(0) as usize).min(self.y1);
        if x1 <= x0 {
            return;
        }
        if y1 <= y0 {
            return;
        }
        // First row by doubling, every further row a copy of it: block copies
        // (`memcpy`, `rep movsb` on x86) cost a fraction of a store per pixel,
        // above all under an emulator (G1: a window body fill took longer
        // than copying the whole wallpaper).
        let n = x1 - x0;
        let first = self.at(x0, y0);
        fill_span(&mut self.pixels[first..first + n], color);
        for py in y0 + 1..y1 {
            let dst = self.at(x0, py);
            let (head, tail) = self.pixels.split_at_mut(dst);
            tail[..n].copy_from_slice(&head[first..first + n]);
        }
    }

    /// A vertical gradient fill from `top` to `bottom`.
    pub fn fill_gradient(&mut self, top: Color, bottom: Color) {
        let h = self.height.max(1);
        for y in self.y0..self.y1 {
            let t = y as u32 * 255 / h as u32;
            let lerp = |a: u32, b: u32| (a * (255 - t) + b * t) / 255;
            let r = lerp((top >> 16) & 0xFF, (bottom >> 16) & 0xFF);
            let g = lerp((top >> 8) & 0xFF, (bottom >> 8) & 0xFF);
            let b = lerp(top & 0xFF, bottom & 0xFF);
            let color = (r << 16) | (g << 8) | b;
            let row = self.at(0, y);
            let w = self.width;
            fill_span(&mut self.pixels[row..row + w], color);
        }
    }

    pub fn rect_outline(&mut self, x: i32, y: i32, w: i32, h: i32, thickness: i32, color: Color) {
        self.fill_rect(x, y, w, thickness, color); // top
        self.fill_rect(x, y + h - thickness, w, thickness, color); // bottom
        self.fill_rect(x, y, thickness, h, color); // left
        self.fill_rect(x + w - thickness, y, thickness, h, color); // right
    }

    /// Bresenham line from (x0,y0) to (x1,y1).
    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        loop {
            if x >= 0 && y >= 0 {
                self.put(x as usize, y as usize, color);
            }
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Ellipse in the bounding box (x,y,w,h). Filled or outline. Integer-only
    /// (the kernel is no_std with no libm): for each row solve the ellipse
    /// equation `dx = a * sqrt(1 - dy^2/b^2)` via integer sqrt.
    pub fn ellipse(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color, fill: bool) {
        if w <= 1 || h <= 1 {
            return;
        }
        let (a, b) = (w / 2, h / 2);
        let (cx, cy) = (x + a, y + b);
        let (a2, b2) = ((a as i64) * a as i64, (b as i64) * b as i64);
        for dy in -b..=b {
            // dx = sqrt(a^2 * (b^2 - dy^2) / b^2)
            let num = a2 * (b2 - (dy as i64) * dy as i64);
            if num < 0 {
                continue;
            }
            let dx = isqrt((num / b2) as u64) as i32;
            let py = cy + dy;
            if fill {
                self.fill_rect(cx - dx, py, 2 * dx + 1, 1, color);
            } else if py >= 0 {
                if cx - dx >= 0 {
                    self.put((cx - dx) as usize, py as usize, color);
                }
                if cx + dx >= 0 {
                    self.put((cx + dx) as usize, py as usize, color);
                }
            }
        }
    }

    /// Draw a single glyph at (x, y); returns the advance width.
    pub fn draw_char(&mut self, x: i32, y: i32, c: char, color: Color) -> i32 {
        let raster = get_raster(c, FONT_WEIGHT, FONT_HEIGHT)
            .or_else(|| get_raster('?', FONT_WEIGHT, FONT_HEIGHT))
            .unwrap();
        for (dy, row) in raster.raster().iter().enumerate() {
            for (dx, &intensity) in row.iter().enumerate() {
                if intensity > 0 {
                    let px = x + dx as i32;
                    let py = y + dy as i32;
                    if px >= 0 && py >= 0 {
                        self.blend(px as usize, py as usize, color, intensity);
                    }
                }
            }
        }
        raster.width() as i32
    }

    /// Draw a string; long text is clipped at `max_x`.
    pub fn draw_text(&mut self, x: i32, y: i32, text: &str, color: Color, max_x: i32) {
        // The console font is TextStyle::Body: same rasters, and its path
        // has the glyph cache (window text used to go pixel by pixel here).
        if matches!(FONT_WEIGHT, FontWeight::Regular) && FONT_HEIGHT.val() == 16 {
            self.draw_text_styled(x, y, text, color, max_x, TextStyle::Body);
            return;
        }
        let mut cx = x;
        for c in text.chars() {
            if cx + glyph_width() as i32 > max_x {
                break;
            }
            cx += self.draw_char(cx, y, c, color);
        }
    }
}

/// Integer square root (Newton's method) for the ellipse rasterizer.
fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Text style for the desktop shell's type scale: Noto Sans Mono is the one
/// face the kernel carries, so hierarchy comes from weight (Bold) and size
/// (24 px headings vs 16 px body).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextStyle {
    Body,
    Strong,
    Heading,
}

impl TextStyle {
    fn font(self) -> (FontWeight, RasterHeight) {
        match self {
            TextStyle::Body => (FontWeight::Regular, RasterHeight::Size16),
            TextStyle::Strong => (FontWeight::Bold, RasterHeight::Size16),
            TextStyle::Heading => (FontWeight::Bold, RasterHeight::Size24),
        }
    }
}

/// Line height of `style` in pixels (the raster height).
pub fn line_height(style: TextStyle) -> i32 {
    let (_, h) = style.font();
    h.val() as i32
}

/// Pixel width of `text` in `style`.
pub fn text_width(text: &str, style: TextStyle) -> i32 {
    let (w, h) = style.font();
    (text.chars().count() * get_raster_width(w, h)) as i32
}

impl<'a> Canvas<'a> {
    /// Draw `text` in `style`, clipped at `max_x`. Returns the x after it.
    pub fn draw_text_styled(&mut self, x: i32, y: i32, text: &str, color: Color, max_x: i32, style: TextStyle) -> i32 {
        let (weight, height) = style.font();
        let gw = get_raster_width(weight, height) as i32;
        if !self.rows_visible(y, height.val() as i32) {
            // Outside this band: only the advance matters to the caller.
            let n = text.chars().count() as i32;
            let room = if gw > 0 { ((max_x - x) / gw).max(0) } else { 0 };
            return x + n.min(room) * gw;
        }
        let mut cx = x;
        for c in text.chars() {
            if cx + gw > max_x {
                break;
            }
            if c == ' ' {
                cx += gw;
                continue;
            }
            // ASCII from the glyph cache: only the covered pixels, and no
            // bounds checks when the glyph lies inside the canvas.
            if let Some(g) = GLYPHS[glyph_slot(style)].get().and_then(|t| t.get((c as usize).wrapping_sub(32))) {
                let inside = cx >= 0 && y >= self.y0 as i32 && (cx + gw) as usize <= self.width && (y + g.height as i32) as usize <= self.y1;
                if inside {
                    let origin = self.at(cx as usize, y as usize);
                    for &(dx, dy, a) in g.pixels.iter() {
                        let i = origin + dy as usize * self.width + dx as usize;
                        self.pixels[i] = if a == 255 { color } else { blend_px(self.pixels[i], color, a) };
                    }
                    cx += gw;
                    continue;
                }
            }
            let raster = get_raster(c, weight, height)
                .or_else(|| get_raster('?', weight, height))
                .unwrap();
            // Bounds once per row, not per pixel; full-coverage pixels are a
            // plain store (G1: text was most of a text window's compose).
            let (w, top, h) = (self.width as i32, self.y0 as i32, self.y1 as i32);
            for (dy, row) in raster.raster().iter().enumerate() {
                let py = y + dy as i32;
                if py < top {
                    continue;
                }
                if py >= h {
                    break;
                }
                let base = self.at(0, py as usize);
                for (dx, &intensity) in row.iter().enumerate() {
                    let px = cx + dx as i32;
                    if intensity == 0 || px < 0 {
                        continue;
                    }
                    if px >= w {
                        break;
                    }
                    let i = base + px as usize;
                    self.pixels[i] = if intensity == 255 { color } else { blend_px(self.pixels[i], color, intensity) };
                }
            }
            cx += gw;
        }
        cx
    }
}

impl<'a> Canvas<'a> {
    /// Width of `text` drawn by [`Canvas::draw_text_scaled`].
    pub fn text_width_scaled(&self, text: &str, style: TextStyle, scale: i32, tracking: i32) -> i32 {
        let n = text.chars().count() as i32;
        if n == 0 {
            return 0;
        }
        let (weight, height) = style.font();
        n * get_raster_width(weight, height) as i32 * scale + (n - 1) * tracking
    }

    /// Draw `text` with every glyph pixel enlarged to `scale` x `scale`,
    /// `tracking` extra pixels between letters, at `alpha` (boot splash title).
    pub fn draw_text_scaled(&mut self, x: i32, y: i32, text: &str, color: Color, style: TextStyle, scale: i32, tracking: i32, alpha: u8) {
        let (weight, height) = style.font();
        let gw = get_raster_width(weight, height) as i32 * scale + tracking;
        let mut cx = x;
        for c in text.chars() {
            if let Some(raster) = get_raster(c, weight, height).or_else(|| get_raster('?', weight, height)) {
                // Bilinear upscale of the glyph's coverage (8.8 fixed point),
                // so a scaled title has smooth edges instead of blocks.
                let rows = raster.raster();
                let (srh, srw) = (rows.len() as i32, rows.first().map_or(0, |r| r.len()) as i32);
                let at = |x: i32, y: i32| -> i32 {
                    if x < 0 || y < 0 || x >= srw || y >= srh { 0 } else { rows[y as usize][x as usize] as i32 }
                };
                for oy in 0..srh * scale {
                    let fy = ((oy * 2 + 1) * 128) / scale - 128;
                    let (y0, wy) = (fy >> 8, fy & 0xFF);
                    for ox in 0..srw * scale {
                        let fx = ((ox * 2 + 1) * 128) / scale - 128;
                        let (x0, wx) = (fx >> 8, fx & 0xFF);
                        let top = at(x0, y0) * (256 - wx) + at(x0 + 1, y0) * wx;
                        let bot = at(x0, y0 + 1) * (256 - wx) + at(x0 + 1, y0 + 1) * wx;
                        let v = (top * (256 - wy) + bot * wy) >> 16;
                        if v <= 0 {
                            continue;
                        }
                        let (px, py) = (cx + ox, y + oy);
                        if px >= 0 && py >= 0 {
                            self.blend(px as usize, py as usize, color, (v.min(255) as u32 * alpha as u32 / 255) as u8);
                        }
                    }
                }
            }
            cx += gw;
        }
    }
}

pub fn glyph_width() -> usize {
    get_raster_width(FONT_WEIGHT, FONT_HEIGHT)
}

pub fn glyph_height() -> usize {
    FONT_HEIGHT.val()
}

/// One cached glyph: its covered pixels as (dx, dy, coverage).
pub struct Glyph {
    pixels: Vec<(u8, u8, u8)>,
    height: u8,
}

static GLYPHS: [spin::Once<Vec<Glyph>>; 3] = [spin::Once::new(), spin::Once::new(), spin::Once::new()];

fn glyph_slot(style: TextStyle) -> usize {
    match style {
        TextStyle::Body => 0,
        TextStyle::Strong => 1,
        TextStyle::Heading => 2,
    }
}

/// Build the glyph cache (printable ASCII of every style). Call at a point
/// where allocating is safe: drawing only *reads* it (text is also drawn from
/// inside `_print` by the boot splash, which must never allocate), and falls
/// back to the font's rasters until it exists.
pub fn warm_glyphs() {
    for style in [TextStyle::Body, TextStyle::Strong, TextStyle::Heading] {
        glyphs(style);
    }
}

/// Printable ASCII (32..=126) of `style`.
fn glyphs(style: TextStyle) -> &'static Vec<Glyph> {
    GLYPHS[glyph_slot(style)].call_once(|| {
        let (weight, height) = style.font();
        (32u8..=126)
            .map(|c| {
                let mut pixels = Vec::new();
                let mut h = 0u8;
                if let Some(r) = get_raster(c as char, weight, height) {
                    h = r.raster().len() as u8;
                    for (dy, row) in r.raster().iter().enumerate() {
                        for (dx, &a) in row.iter().enumerate() {
                            if a > 0 {
                                pixels.push((dx as u8, dy as u8, a));
                            }
                        }
                    }
                }
                Glyph { pixels, height: h }
            })
            .collect()
    })
}

/// `color` over `dst` at coverage `alpha` (0 = dst, 255 = color): red+blue
/// and green each in one multiply, scaled to 0..=256 instead of divided by
/// 255 three times.
#[inline]
pub fn blend_px(dst: Color, color: Color, alpha: u8) -> Color {
    let a = alpha as u32 + (alpha as u32 >> 7);
    let na = 256 - a;
    let rb = ((color & 0xFF_00FF) * a + (dst & 0xFF_00FF) * na) >> 8;
    let g = ((color & 0x00_FF00) * a + (dst & 0x00_FF00) * na) >> 8;
    (rb & 0xFF_00FF) | (g & 0x00_FF00)
}

/// Fill `span` with `color`: a few stores, then doubling block copies.
pub fn fill_span(span: &mut [u32], color: Color) {
    const SEED: usize = 16;
    let n = span.len();
    if n <= SEED {
        span.fill(color);
        return;
    }
    span[..SEED].fill(color);
    let mut done = SEED;
    while done < n {
        let k = done.min(n - done);
        let (a, b) = span.split_at_mut(done);
        b[..k].copy_from_slice(&a[..k]);
        done += k;
    }
}

// --- Hashing for frame caches ----------------------------------------------------

const HASH_M: u64 = 0x9E37_79B9_7F4A_7C15;

/// Fold one value into a running hash (cache keys: theme, state, text...).
#[inline]
pub fn mix(h: u64, v: u64) -> u64 {
    let x = (h ^ v).wrapping_mul(HASH_M);
    x ^ (x >> 31)
}

/// Fold a byte string into a running hash.
pub fn mix_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    for c in bytes.chunks(8) {
        let mut w = [0u8; 8];
        w[..c.len()].copy_from_slice(c);
        h = mix(h, u64::from_le_bytes(w));
    }
    mix(h, bytes.len() as u64)
}

/// Hash a run of pixels: FNV-1a over 64-bit words, two pixels per step.
/// Under an emulator the cost is the number of guest instructions per pixel,
/// which this keeps lowest (measured: a four-lane variant with more
/// instructions made presents 1.5x slower under QEMU TCG). Used to tell
/// whether a screen tile or a cached layer changed.
pub fn hash_px(seed: u64, px: &[u32]) -> u64 {
    const P: u64 = 0x0000_0100_0000_01b3;
    let mut h = seed;
    // SAFETY: u32 -> u64 reinterpretation of the aligned middle part.
    let (head, words, tail) = unsafe { px.align_to::<u64>() };
    for &p in head {
        h = (h ^ p as u64).wrapping_mul(P);
    }
    for &w in words {
        h = (h ^ w).wrapping_mul(P);
    }
    for &p in tail {
        h = (h ^ p as u64).wrapping_mul(P);
    }
    h
}
