//! Desktop wallpaper (v0.11 "Cahaya" personalization). The default follows the
//! active theme's desktop gradient; built-in patterns, the **official
//! Buitenzorg OS wallpapers** (`OFFICIAL`: AI-generated artwork shipped on
//! /disk as 800x450 BMPs, built from `assets/wallpapers/*.jpg` by
//! `bzimage/build.rs`) and any user 24-bit BMP from the VFS can replace it.
//! `paint` renders the current wallpaper into the compositor's back buffer;
//! an image is resampled (bilinear) to the screen size once and cached, so a
//! frame is a row copy instead of a per-pixel stretch.

use alloc::{string::String, vec::Vec};
use spin::Mutex;

use crate::gfx::{rgb, Canvas};
use crate::theme::Theme;

enum Kind {
    /// Follow the active theme's desktop colors (default).
    Theme,
    /// A built-in procedural pattern by index.
    Pattern(u8),
    /// An image (RGB pixels) plus its screen-sized resample, built lazily.
    Image { w: i32, h: i32, pixels: Vec<u32>, scaled: Option<(i32, i32, Vec<u32>)> },
}

struct State {
    kind: Kind,
    label: String,
}

/// Official wallpapers: (name for `bg`, file on /disk, description). Keep in
/// sync with `OFFICIAL_WALLPAPERS` in bzimage/build.rs.
pub const OFFICIAL: [(&str, &str, &str); 8] = [
    ("nature", "/disk/WPNATURE.BMP", "Kebun Raya Bogor saat fajar"),
    ("futuristic", "/disk/WPFUTURE.BMP", "kota taman neon masa depan"),
    ("city", "/disk/WPCITY.BMP", "kota hujan di jam biru"),
    ("art", "/disk/WPART.BMP", "seni batik kawung-parang modern"),
    ("surreal", "/disk/WPSURREA.BMP", "beringin terbalik di atas laut"),
    ("imaginary", "/disk/WPIMAGIN.BMP", "pulau hutan melayang"),
    ("space", "/disk/WPSPACE.BMP", "planet bercincin & nebula"),
    ("minimal", "/disk/WPMINIMA.BMP", "pegunungan senja minimalis"),
];

/// Load official wallpaper `name` from /disk. Returns its (w, h).
pub fn set_official(name: &str) -> Result<(i32, i32), &'static str> {
    let (_, path, _) = OFFICIAL.iter().find(|(n, _, _)| *n == name).ok_or("unknown wallpaper")?;
    let bytes = crate::vfs::read(path).map_err(|_| "wallpaper file not on /disk")?;
    load_bmp(&bytes, name)
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Names of built-in wallpapers (plus "theme" default and any loaded image).
pub const BUILTINS: [&str; 5] = ["theme", "waves", "grid", "dots", "aurora"];

fn ensure() {
    let mut s = STATE.lock();
    if s.is_none() {
        *s = Some(State { kind: Kind::Theme, label: String::from("theme") });
    }
}

pub fn label() -> String {
    ensure();
    STATE.lock().as_ref().unwrap().label.clone()
}

/// Select a built-in wallpaper by name; returns true if recognized.
pub fn set_builtin(name: &str) -> bool {
    let kind = match name {
        "theme" => Kind::Theme,
        "waves" => Kind::Pattern(0),
        "grid" => Kind::Pattern(1),
        "dots" => Kind::Pattern(2),
        "aurora" => Kind::Pattern(3),
        _ => return false,
    };
    *STATE.lock() = Some(State { kind, label: String::from(name) });
    true
}

/// Set a user image wallpaper from decoded RGB pixels.
pub fn set_image(w: i32, h: i32, pixels: Vec<u32>, label: &str) {
    *STATE.lock() = Some(State {
        kind: Kind::Image { w, h, pixels, scaled: None },
        label: String::from(label),
    });
}

/// Load a 24-bit uncompressed BMP into a wallpaper. Returns (w, h) on success.
pub fn load_bmp(bytes: &[u8], label: &str) -> Result<(i32, i32), &'static str> {
    let (w, h, pixels) = decode_bmp(bytes)?;
    set_image(w, h, pixels, label);
    Ok((w, h))
}

/// Decode a 24-bit uncompressed BMP to (w, h, 0x00RRGGBB pixels).
pub fn decode_bmp(bytes: &[u8]) -> Result<(i32, i32, Vec<u32>), &'static str> {
    if bytes.len() < 54 || &bytes[0..2] != b"BM" {
        return Err("not a BMP");
    }
    let data_off = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as usize;
    let w = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
    let h_signed = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
    let bpp = u16::from_le_bytes([bytes[28], bytes[29]]);
    if bpp != 24 {
        return Err("only 24-bit BMP supported");
    }
    let h = h_signed.abs();
    let top_down = h_signed < 0;
    if w <= 0 || h <= 0 || w > 4096 || h > 4096 {
        return Err("bad BMP dimensions");
    }
    let row_stride = (((w * 3) + 3) & !3) as usize; // padded to 4 bytes
    let mut pixels = alloc::vec![0u32; (w * h) as usize];
    for row in 0..h {
        let src_row = if top_down { row } else { h - 1 - row };
        let base = data_off + src_row as usize * row_stride;
        if base + (w as usize * 3) > bytes.len() {
            return Err("BMP data truncated");
        }
        for col in 0..w {
            let p = base + col as usize * 3;
            let b = bytes[p];
            let g = bytes[p + 1];
            let r = bytes[p + 2];
            pixels[(row * w + col) as usize] = rgb(r, g, b);
        }
    }
    Ok((w, h, pixels))
}

/// Render the current wallpaper into `canvas` (full-screen).
pub fn paint(canvas: &mut Canvas, th: &Theme, workspace: u8) {
    ensure();
    let mut guard = STATE.lock();
    let state = guard.as_mut().unwrap();
    let (cw, ch) = (canvas.width as i32, canvas.height as i32);
    match &mut state.kind {
        Kind::Theme => {
            let tint = workspace as u32 * 10;
            let top = crate::wm::shift(th.desktop_top, tint);
            let bottom = crate::wm::shift(th.desktop_bottom, tint);
            if th.gradient {
                canvas.fill_gradient(top, bottom);
            } else {
                canvas.fill_rect(0, 0, cw, ch, top);
            }
        }
        Kind::Pattern(p) => paint_pattern(canvas, *p, th, cw, ch),
        Kind::Image { w, h, pixels, scaled } => {
            if !matches!(scaled, Some((sw, sh, _)) if *sw == cw && *sh == ch) {
                *scaled = Some((cw, ch, resample(pixels, *w, *h, cw, ch)));
            }
            let (_, _, img) = scaled.as_ref().unwrap();
            if canvas.pixels.len() == img.len() {
                canvas.pixels.copy_from_slice(img);
            }
        }
    }
}

/// Bilinear resample of `src` (`sw` x `sh`) to `dw` x `dh`, in 8.8 fixed point
/// (the kernel is soft-float). Done once per wallpaper change.
pub fn resample(src: &[u32], sw: i32, sh: i32, dw: i32, dh: i32) -> Vec<u32> {
    let mut out = alloc::vec![0u32; (dw * dh) as usize];
    let ch = |p: u32, s: u32| (p >> s) & 0xFF;
    for y in 0..dh {
        let fy = ((y * 2 + 1) * sh * 128 / dh - 128).max(0); // source y in 1/256 px
        let (y0, wy) = ((fy >> 8).min(sh - 1), (fy & 0xFF) as u32);
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..dw {
            let fx = ((x * 2 + 1) * sw * 128 / dw - 128).max(0);
            let (x0, wx) = ((fx >> 8).min(sw - 1), (fx & 0xFF) as u32);
            let x1 = (x0 + 1).min(sw - 1);
            let p = |xx: i32, yy: i32| src[(yy * sw + xx) as usize];
            let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
            let mut v = 0u32;
            for s in [16u32, 8, 0] {
                let top = ch(a, s) * (256 - wx) + ch(b, s) * wx;
                let bot = ch(c, s) * (256 - wx) + ch(d, s) * wx;
                v |= ((top * (256 - wy) + bot * wy) >> 16) << s;
            }
            out[(y * dw + x) as usize] = v;
        }
    }
    out
}

fn paint_pattern(canvas: &mut Canvas, p: u8, th: &Theme, cw: i32, ch: i32) {
    let base = th.desktop_top;
    let alt = crate::wm::shift(base, 22);
    match p {
        // waves: horizontal bands.
        0 => {
            for y in 0..ch {
                let band = ((y / 20) & 1) == 0;
                let c = if band { base } else { alt };
                canvas.fill_rect(0, y, cw, 1, c);
            }
        }
        // grid.
        1 => {
            canvas.fill_rect(0, 0, cw, ch, base);
            let line = crate::wm::shift(base, 30);
            let mut x = 0;
            while x < cw {
                canvas.fill_rect(x, 0, 1, ch, line);
                x += 40;
            }
            let mut y = 0;
            while y < ch {
                canvas.fill_rect(0, y, cw, 1, line);
                y += 40;
            }
        }
        // dots.
        2 => {
            canvas.fill_rect(0, 0, cw, ch, base);
            let dot = th.accent;
            let mut y = 20;
            while y < ch {
                let mut x = 20;
                while x < cw {
                    canvas.ellipse(x - 3, y - 3, 6, 6, dot, true);
                    x += 44;
                }
                y += 44;
            }
        }
        // aurora: vertical gradient toward the accent.
        _ => {
            let _ = (cw, ch);
            canvas.fill_gradient(th.desktop_bottom, th.accent);
        }
    }
}
