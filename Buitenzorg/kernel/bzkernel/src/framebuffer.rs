//! Text console rendered into the boot framebuffer (v0.1: "gambar piksel").

use crate::boot::{Framebuffer as FbInfo, PixelFormat};
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

/// The boot framebuffer: its description plus the mapped pixel memory.
pub struct FrameBuffer {
    info: FbInfo,
    buf: &'static mut [u8],
}

impl FrameBuffer {
    /// # Safety
    /// `info` must describe a mapping that stays valid and is used only here.
    pub unsafe fn new(info: FbInfo) -> Self {
        let buf = unsafe { core::slice::from_raw_parts_mut(info.addr as *mut u8, info.len) };
        FrameBuffer { info, buf }
    }
    pub fn info(&self) -> FbInfo {
        self.info
    }
    pub fn buffer_mut(&mut self) -> &mut [u8] {
        self.buf
    }
}

pub struct Console {
    fb: FrameBuffer,
    x: usize,
    y: usize,
    glyph_w: usize,
    /// Hash of every TILE-wide piece of every row as last presented (G1:
    /// only pieces whose hash changed are written to the framebuffer, which
    /// is slow to write -- uncached on a Raspberry Pi, byte-wise on a 24-bit
    /// VBE mode). Empty = not valid (the console drew over the screen).
    tiles: alloc::vec::Vec<u64>,
    /// Tile pieces written / skipped (diagnostics: `fb_stats`).
    written: u64,
    skipped: u64,
}

/// Width in pixels of a present tile (one row high).
const TILE: usize = 64;

fn tile_hash(px: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &p in px {
        h = (h ^ p as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl Console {
    fn new(mut fb: FrameBuffer) -> Self {
        let bg = BG;
        let info = fb.info();
        let bpp = info.bytes_per_pixel;
        // Clear to background color.
        for px in fb.buffer_mut().chunks_exact_mut(bpp) {
            write_pixel_bytes(px, info.format, bg);
        }
        Self {
            fb,
            x: BORDER,
            y: BORDER,
            glyph_w: get_raster_width(FONT_WEIGHT, FONT_HEIGHT),
            tiles: alloc::vec::Vec::new(),
            written: 0,
            skipped: 0,
        }
    }

    pub fn info(&self) -> FbInfo {
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
            write_pixel_bytes(px, info.format, BG);
        }
        self.y -= FONT_HEIGHT.val() + LINE_SPACING;
    }

    fn write_char(&mut self, c: char) {
        self.tiles.clear(); // the console draws straight to the screen
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
                write_pixel_bytes(px, info.format, color);
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
        PixelFormat::Gray => {
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
        self.tiles.clear();
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
                    write_pixel_bytes(px, info.format, px_color);
                }
            }
        }
    }
}

/// Draw the direct-pixel demo; returns false when no framebuffer exists.
pub fn draw_pixel_demo() -> bool {
    if let Some(console) = CONSOLE.get() {
        crate::arch::cpu::without_interrupts(|| {
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
        let h = self.fb.info().height;
        self.present_rows(back, 0, h);
    }

    /// Like `present`, for screen rows `y0..y1` only, taken from `back` which
    /// holds just those rows (the boot splash's bar: a full frame through the
    /// byte-wise path takes seconds under emulation).
    fn present_rows(&mut self, back: &[u32], y0: usize, y1: usize) {
        let info = self.fb.info();
        let bpp = info.bytes_per_pixel;
        let (w, h) = (info.width, info.height);
        let y1 = y1.min(h);
        let format = info.format;
        let tiles_x = w.div_ceil(TILE);
        let fresh = self.tiles.len() != tiles_x * h;
        if fresh {
            self.tiles.clear();
            self.tiles.resize(tiles_x * h, 0);
        }
        let mut tiles = core::mem::take(&mut self.tiles);
        let (mut written, mut skipped) = (0u64, 0u64);
        // `dirty(y, tx)`: hash the piece, compare, remember; true = write it
        let mut dirty = |y: usize, tx: usize, row: &[u32]| -> bool {
            let x0 = tx * TILE;
            let hsh = tile_hash(&row[x0..(x0 + TILE).min(w)]);
            let slot = &mut tiles[y * tiles_x + tx];
            if !fresh && *slot == hsh {
                skipped += 1;
                return false;
            }
            *slot = hsh;
            written += 1;
            true
        };
        let buffer = self.fb.buffer_mut();
        // Fast paths for 32-bit pixels: whole rows (BGR = the compositor's own
        // 0x00RRGGBB words) or one 32-bit store per pixel (RGB). Byte-wise
        // writes cost 3-4 uncached stores per pixel -- on a Raspberry Pi 4 a
        // 1280x720 present took long enough to show half-drawn frames.
        if y1 <= y0 || back.len() < (y1 - y0) * w {
            self.tiles = tiles;
            return;
        }
        if bpp == 4 && buffer.len() >= info.stride * h * 4 {
            let dst = buffer.as_mut_ptr() as *mut u32;
            match format {
                PixelFormat::Bgr => {
                    for y in y0..y1 {
                        let row = &back[(y - y0) * w..(y - y0 + 1) * w];
                        for tx in 0..tiles_x {
                            if dirty(y, tx, row) {
                                let x0 = tx * TILE;
                                let n = TILE.min(w - x0);
                                unsafe { core::ptr::copy_nonoverlapping(row.as_ptr().add(x0), dst.add(y * info.stride + x0), n) };
                            }
                        }
                    }
                    self.tiles = tiles;
                    self.written += written;
                    self.skipped += skipped;
                    return;
                }
                PixelFormat::Rgb => {
                    for y in y0..y1 {
                        let row = &back[(y - y0) * w..(y - y0 + 1) * w];
                        for tx in 0..tiles_x {
                            if dirty(y, tx, row) {
                                for x in tx * TILE..((tx + 1) * TILE).min(w) {
                                    let s = row[x];
                                    let v = ((s & 0xFF) << 16) | (s & 0xFF00) | ((s >> 16) & 0xFF);
                                    unsafe { dst.add(y * info.stride + x).write_volatile(v) };
                                }
                            }
                        }
                    }
                    self.tiles = tiles;
                    self.written += written;
                    self.skipped += skipped;
                    return;
                }
                _ => {}
            }
        }
        for y in y0..y1 {
            let row = &back[(y - y0) * w..(y - y0 + 1) * w];
            for tx in 0..tiles_x {
                if !dirty(y, tx, row) {
                    continue;
                }
                for x in tx * TILE..((tx + 1) * TILE).min(w) {
                    let src = row[x];
                    let color = ((src >> 16) as u8, (src >> 8) as u8, src as u8);
                    let off = (y * info.stride + x) * bpp;
                    write_pixel_bytes(&mut buffer[off..off + bpp], format, color);
                }
            }
        }
        self.tiles = tiles;
        self.written += written;
        self.skipped += skipped;
    }
}

pub static CONSOLE: Once<Mutex<Console>> = Once::new();

pub fn init(info: FbInfo) {
    // SAFETY: the loader mapped the framebuffer for the kernel; only the
    // console touches it.
    let fb = unsafe { FrameBuffer::new(info) };
    CONSOLE.call_once(|| Mutex::new(Console::new(fb)));
}

/// Framebuffer dimensions in pixels, if a framebuffer exists.
pub fn dimensions() -> Option<(usize, usize)> {
    CONSOLE.get().map(|c| c.lock().dimensions())
}

/// Present a `0x00RRGGBB` back buffer (width*height) to the screen. Held
/// back while the boot splash owns the screen (the boot demos' windows must
/// not flash up mid-boot); the splash itself uses [`present_splash`].
pub fn present(back: &[u32]) {
    if crate::splash::active() {
        return;
    }
    present_splash(back);
}

/// Present screen rows `y0..y1` from `rows` (just those rows, `width` pixels
/// each), unconditionally: the boot splash's progress strip. (A virtio GPU
/// keeps the splash's first full frame; it takes whole frames only.)
pub fn present_splash_rows(rows: &[u32], y0: usize, y1: usize) {
    if let Some(console) = CONSOLE.get() {
        crate::arch::cpu::without_interrupts(|| {
            console.lock().present_rows(rows, y0, y1);
        });
    }
}

/// (tile pieces written, skipped as unchanged) by presents so far.
pub fn present_stats() -> (u64, u64) {
    CONSOLE.get().map_or((0, 0), |c| {
        let c = c.lock();
        (c.written, c.skipped)
    })
}

/// Present unconditionally (the boot splash's own frames).
pub fn present_splash(back: &[u32]) {
    if let Some(console) = CONSOLE.get() {
        crate::arch::cpu::without_interrupts(|| {
            console.lock().present(back);
        });
    }
    // Mirror to a virtio GPU, if one is up (multi-arch M2; virtio-PCI on PCs,
    // virtio-mmio on ARM64).
    crate::virtio::gpu::present(back);
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    // the remote log of the deploy service keeps the last 16 KiB of output
    crate::logring::push_fmt(args);
    crate::task::note_output();
    // Once the interactive desktop owns the framebuffer, kernel log lines go
    // to serial only: drawing them here scrolled the whole desktop up (the
    // console scrolls the framebuffer) and left log text under the taskbar.
    if crate::interactive::is_active() {
        return;
    }
    // Under the boot splash the log feeds its status line instead.
    if crate::splash::active() {
        let mut line = LineBuf { buf: [0; 160], len: 0 };
        line.write_fmt(args).ok();
        if let Ok(s) = core::str::from_utf8(&line.buf[..line.len]) {
            crate::splash::note(s);
        }
        return;
    }
    if let Some(console) = CONSOLE.get() {
        crate::arch::cpu::without_interrupts(|| {
            console.lock().write_fmt(args).ok();
        });
    }
}

/// A log line formatted on the stack (no allocation: `_print` may run while
/// the heap lock is held), truncated at 160 bytes, ASCII only.
struct LineBuf {
    buf: [u8; 160],
    len: usize,
}

impl core::fmt::Write for LineBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &b in s.as_bytes() {
            if self.len < self.buf.len() {
                self.buf[self.len] = if b.is_ascii() { b } else { b'?' };
                self.len += 1;
            }
        }
        Ok(())
    }
}
