//! Text console rendered into the boot framebuffer (v0.1: "gambar piksel").

use bootloader_api::info::{FrameBuffer, PixelFormat};
use core::fmt;
use noto_sans_mono_bitmap::{
    get_raster, get_raster_width, FontWeight, RasterHeight, RasterizedChar,
};
use spin::{Mutex, Once};

const FONT_HEIGHT: RasterHeight = RasterHeight::Size16;
const FONT_WEIGHT: FontWeight = FontWeight::Regular;
const LINE_SPACING: usize = 2;
const BORDER: usize = 8;

/// Buitenzorg green-on-dark default palette (dark system theme, §10.5).
const FG: (u8, u8, u8) = (0xB8, 0xE9, 0x94); // leaf green
const BG: (u8, u8, u8) = (0x0B, 0x12, 0x0B); // deep soil

pub struct Console {
    fb: FrameBuffer,
    x: usize,
    y: usize,
    glyph_w: usize,
}

impl Console {
    fn new(mut fb: FrameBuffer) -> Self {
        let bg = BG;
        let info = fb.info();
        let bpp = info.bytes_per_pixel;
        // Clear to background color.
        for px in fb.buffer_mut().chunks_exact_mut(bpp) {
            write_pixel_bytes(px, info.pixel_format, bg);
        }
        Self {
            fb,
            x: BORDER,
            y: BORDER,
            glyph_w: get_raster_width(FONT_WEIGHT, FONT_HEIGHT),
        }
    }

    pub fn info(&self) -> bootloader_api::info::FrameBufferInfo {
        self.fb.info()
    }

    /// Physical/virtual address of the framebuffer start (for the FB_INFO syscall).
    pub fn buffer_addr(&mut self) -> (u64, u64) {
        let buf = self.fb.buffer_mut();
        (buf.as_ptr() as u64, buf.len() as u64)
    }

    fn newline(&mut self) {
        self.x = BORDER;
        self.y += FONT_HEIGHT.val() + LINE_SPACING;
        let height = self.fb.info().height;
        if self.y + FONT_HEIGHT.val() >= height {
            self.scroll();
        }
    }

    fn scroll(&mut self) {
        let info = self.fb.info();
        let row_bytes = info.stride * info.bytes_per_pixel;
        let step = (FONT_HEIGHT.val() + LINE_SPACING) * row_bytes;
        let buf = self.fb.buffer_mut();
        buf.copy_within(step.., 0);
        let len = buf.len();
        let tail = &mut buf[len - step..];
        for px in tail.chunks_exact_mut(info.bytes_per_pixel) {
            write_pixel_bytes(px, info.pixel_format, BG);
        }
        self.y -= FONT_HEIGHT.val() + LINE_SPACING;
    }

    fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.newline(),
            '\r' => self.x = BORDER,
            c => {
                if self.x + self.glyph_w >= self.fb.info().width - BORDER {
                    self.newline();
                }
                let raster = get_raster(c, FONT_WEIGHT, FONT_HEIGHT)
                    .unwrap_or_else(|| get_raster('?', FONT_WEIGHT, FONT_HEIGHT).unwrap());
                self.draw_glyph(&raster);
                self.x += self.glyph_w;
            }
        }
    }

    fn draw_glyph(&mut self, glyph: &RasterizedChar) {
        let info = self.fb.info();
        let (x0, y0) = (self.x, self.y);
        let buf = self.fb.buffer_mut();
        for (dy, row) in glyph.raster().iter().enumerate() {
            for (dx, intensity) in row.iter().enumerate() {
                let x = x0 + dx;
                let y = y0 + dy;
                if x >= info.width || y >= info.height {
                    continue;
                }
                let color = blend(*intensity);
                let offset = (y * info.stride + x) * info.bytes_per_pixel;
                let px = &mut buf[offset..offset + info.bytes_per_pixel];
                write_pixel_bytes(px, info.pixel_format, color);
            }
        }
    }
}

fn blend(intensity: u8) -> (u8, u8, u8) {
    let mix = |f: u8, b: u8| -> u8 {
        ((f as u16 * intensity as u16 + b as u16 * (255 - intensity as u16)) / 255) as u8
    };
    (mix(FG.0, BG.0), mix(FG.1, BG.1), mix(FG.2, BG.2))
}

fn write_pixel_bytes(px: &mut [u8], format: PixelFormat, (r, g, b): (u8, u8, u8)) {
    match format {
        PixelFormat::Rgb => {
            px[0] = r;
            px[1] = g;
            px[2] = b;
        }
        PixelFormat::Bgr => {
            px[0] = b;
            px[1] = g;
            px[2] = r;
        }
        PixelFormat::U8 => {
            px[0] = ((r as u16 + g as u16 + b as u16) / 3) as u8;
        }
        _ => {
            // Unknown layout: write grayscale into the first byte as a best effort.
            px[0] = g;
        }
    }
}

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            self.write_char(c);
        }
        Ok(())
    }
}

impl Console {
    /// v0.3 "Batang" milestone "gambar piksel": paint direct-pixel color
    /// swatches (a growth gradient, soil → canopy) in the top-right corner.
    fn draw_pixel_demo(&mut self) {
        const SWATCHES: [(u8, u8, u8); 8] = [
            (0x3E, 0x2A, 0x1D), // soil
            (0x6B, 0x4A, 0x2B), // roots
            (0x8A, 0x6D, 0x3B), // stem base
            (0x5C, 0x8A, 0x3B), // young stem
            (0x4F, 0xA3, 0x3F), // leaf
            (0x6F, 0xC1, 0x4E), // bright leaf
            (0xB8, 0xE9, 0x94), // foliage (console foreground)
            (0xE8, 0xF5, 0xC0), // canopy light
        ];
        const SIZE: usize = 18;
        let info = self.fb.info();
        let x0 = info.width.saturating_sub(SWATCHES.len() * SIZE + BORDER);
        let buf = self.fb.buffer_mut();
        for (i, &color) in SWATCHES.iter().enumerate() {
            for dy in 0..SIZE {
                for dx in 0..SIZE {
                    let x = x0 + i * SIZE + dx;
                    let y = BORDER + dy;
                    if x >= info.width || y >= info.height {
                        continue;
                    }
                    let edge = dx == 0 || dy == 0 || dx == SIZE - 1 || dy == SIZE - 1;
                    let px_color = if edge { BG } else { color };
                    let offset = (y * info.stride + x) * info.bytes_per_pixel;
                    let px = &mut buf[offset..offset + info.bytes_per_pixel];
                    write_pixel_bytes(px, info.pixel_format, px_color);
                }
            }
        }
    }
}

/// Draw the direct-pixel demo; returns false when no framebuffer exists.
pub fn draw_pixel_demo() -> bool {
    if let Some(console) = CONSOLE.get() {
        x86_64::instructions::interrupts::without_interrupts(|| {
            console.lock().draw_pixel_demo();
        });
        true
    } else {
        false
    }
}

impl Console {
    /// Framebuffer dimensions in pixels.
    fn dimensions(&self) -> (usize, usize) {
        let info = self.fb.info();
        (info.width, info.height)
    }

    /// Blit a full-screen back buffer of `0x00RRGGBB` pixels to the hardware
    /// framebuffer, converting to its pixel format (v0.6 compositor present).
    fn present(&mut self, back: &[u32]) {
        let info = self.fb.info();
        let bpp = info.bytes_per_pixel;
        let (w, h) = (info.width, info.height);
        let format = info.pixel_format;
        let buffer = self.fb.buffer_mut();
        for y in 0..h {
            for x in 0..w {
                let src = back[y * w + x];
                let color = (
                    (src >> 16) as u8,
                    (src >> 8) as u8,
                    src as u8,
                );
                let off = (y * info.stride + x) * bpp;
                write_pixel_bytes(&mut buffer[off..off + bpp], format, color);
            }
        }
    }
}

pub static CONSOLE: Once<Mutex<Console>> = Once::new();

pub fn init(fb: FrameBuffer) {
    CONSOLE.call_once(|| Mutex::new(Console::new(fb)));
}

/// Framebuffer dimensions in pixels, if a framebuffer exists.
pub fn dimensions() -> Option<(usize, usize)> {
    CONSOLE.get().map(|c| c.lock().dimensions())
}

/// Present a `0x00RRGGBB` back buffer (width*height) to the screen.
pub fn present(back: &[u32]) {
    if let Some(console) = CONSOLE.get() {
        x86_64::instructions::interrupts::without_interrupts(|| {
            console.lock().present(back);
        });
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    if let Some(console) = CONSOLE.get() {
        x86_64::instructions::interrupts::without_interrupts(|| {
            console.lock().write_fmt(args).ok();
        });
    }
}
