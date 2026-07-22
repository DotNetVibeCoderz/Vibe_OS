//! Software rendering primitives (v0.6 "Daun"): a [`Canvas`] over a
//! `0x00RRGGBB` pixel buffer, with rectangle fills/outlines, alpha blending,
//! and text drawn from the Noto Sans Mono bitmap font. The compositor and
//! window manager render into a full-screen back buffer of these pixels.

use noto_sans_mono_bitmap::{get_raster, get_raster_width, FontWeight, RasterHeight};

pub const FONT_HEIGHT: RasterHeight = RasterHeight::Size16;
pub const FONT_WEIGHT: FontWeight = FontWeight::Regular;

pub type Color = u32; // 0x00RRGGBB

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

/// A mutable view over a rectangular pixel buffer.
pub struct Canvas<'a> {
    pub pixels: &'a mut [Color],
    pub width: usize,
    pub height: usize,
}

impl<'a> Canvas<'a> {
    pub fn new(pixels: &'a mut [Color], width: usize, height: usize) -> Self {
        Self { pixels, width, height }
    }

    #[inline]
    pub fn put(&mut self, x: usize, y: usize, color: Color) {
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x] = color;
        }
    }

    /// Alpha-blend `color` over the existing pixel (`alpha` 0..=255).
    #[inline]
    pub fn blend(&mut self, x: usize, y: usize, color: Color, alpha: u8) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = y * self.width + x;
        let dst = self.pixels[idx];
        let mix = |s: u32, d: u32| -> u32 {
            (s * alpha as u32 + d * (255 - alpha as u32)) / 255
        };
        let r = mix((color >> 16) & 0xFF, (dst >> 16) & 0xFF);
        let g = mix((color >> 8) & 0xFF, (dst >> 8) & 0xFF);
        let b = mix(color & 0xFF, dst & 0xFF);
        self.pixels[idx] = (r << 16) | (g << 8) | b;
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        let x0 = x.max(0) as usize;
        let y0 = y.max(0) as usize;
        let x1 = ((x + w).max(0) as usize).min(self.width);
        let y1 = ((y + h).max(0) as usize).min(self.height);
        for py in y0..y1 {
            let row = py * self.width;
            for px in x0..x1 {
                self.pixels[row + px] = color;
            }
        }
    }

    /// A vertical gradient fill from `top` to `bottom`.
    pub fn fill_gradient(&mut self, top: Color, bottom: Color) {
        let h = self.height.max(1);
        for y in 0..self.height {
            let t = y as u32 * 255 / h as u32;
            let lerp = |a: u32, b: u32| (a * (255 - t) + b * t) / 255;
            let r = lerp((top >> 16) & 0xFF, (bottom >> 16) & 0xFF);
            let g = lerp((top >> 8) & 0xFF, (bottom >> 8) & 0xFF);
            let b = lerp(top & 0xFF, bottom & 0xFF);
            let color = (r << 16) | (g << 8) | b;
            let row = y * self.width;
            for x in 0..self.width {
                self.pixels[row + x] = color;
            }
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

pub fn glyph_width() -> usize {
    get_raster_width(FONT_WEIGHT, FONT_HEIGHT)
}

pub fn glyph_height() -> usize {
    FONT_HEIGHT.val()
}
