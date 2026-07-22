//! Screen saver (v0.11 "Cahaya"): a framework that activates after an idle
//! timeout and built-in savers in the spirit of Windows 3.1 / 98 (Starfield,
//! Mystify, 3D Pipes, Marquee, Bouncing). Each renders a frame given a frame
//! counter; the desktop loop drives activation/dismissal and drawing.

use spin::Mutex;

use crate::gfx::{rgb, Canvas};

/// Idle timeout in timer ticks before the saver kicks in (PIT ~18.2 Hz).
pub const IDLE_TIMEOUT_TICKS: u64 = 18 * 12; // ~12 s

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Saver {
    Starfield,
    Mystify,
    Pipes,
    Marquee,
    Bouncing,
    Blank,
}

pub const NAMES: [&str; 6] = ["starfield", "mystify", "pipes", "marquee", "bouncing", "blank"];

struct Config {
    saver: Saver,
    enabled: bool,
    timeout: u64,
}

static CONFIG: Mutex<Config> = Mutex::new(Config {
    saver: Saver::Starfield,
    enabled: true,
    timeout: IDLE_TIMEOUT_TICKS,
});

pub fn set(name: &str) -> bool {
    let s = match name {
        "starfield" => Saver::Starfield,
        "mystify" => Saver::Mystify,
        "pipes" => Saver::Pipes,
        "marquee" => Saver::Marquee,
        "bouncing" => Saver::Bouncing,
        "blank" => Saver::Blank,
        "off" | "none" => {
            CONFIG.lock().enabled = false;
            return true;
        }
        _ => return false,
    };
    let mut c = CONFIG.lock();
    c.saver = s;
    c.enabled = true;
    true
}

pub fn name() -> &'static str {
    let c = CONFIG.lock();
    if !c.enabled {
        return "off";
    }
    NAMES[c.saver as usize]
}

pub fn enabled() -> bool {
    CONFIG.lock().enabled
}

pub fn timeout() -> u64 {
    CONFIG.lock().timeout
}

/// Render one screensaver frame into `canvas` (full-screen) at `frame`.
pub fn render(canvas: &mut Canvas, frame: u64) {
    let saver = CONFIG.lock().saver;
    let (w, h) = (canvas.width as i32, canvas.height as i32);
    // Fade the background in over the first ~8 frames (micro-interaction).
    canvas.fill_rect(0, 0, w, h, rgb(0, 0, 0));
    match saver {
        Saver::Starfield => starfield(canvas, frame, w, h),
        Saver::Mystify => mystify(canvas, frame, w, h),
        Saver::Pipes => pipes(canvas, frame, w, h),
        Saver::Marquee => marquee(canvas, frame, w, h),
        Saver::Bouncing => bouncing(canvas, frame, w, h),
        Saver::Blank => {}
    }
}

// A tiny deterministic PRNG so stars/segments are stable per index.
fn hash(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x
}

fn starfield(canvas: &mut Canvas, frame: u64, w: i32, h: i32) {
    let (cx, cy) = (w / 2, h / 2);
    for i in 0..220u64 {
        let hx = hash(i);
        // Angle and speed per star.
        let ang = (hx % 628) as i32; // 0..628 ~ 0..2pi*100
        let speed = 2 + (hx >> 8) % 4;
        // Distance grows with frame, wrapping.
        let dist = ((frame * speed + (hx >> 16) % 400) % 400) as i32;
        // Approximate cos/sin via a small table.
        let (sx, sy) = unit(ang);
        let x = cx + sx * dist / 100;
        let y = cy + sy * dist / 100;
        if x >= 0 && x < w && y >= 0 && y < h {
            let b = (120 + dist / 4).min(255) as u8;
            let c = rgb(b, b, b);
            let sz = 1 + dist / 200;
            canvas.fill_rect(x, y, sz, sz, c);
        }
    }
}

fn mystify(canvas: &mut Canvas, frame: u64, w: i32, h: i32) {
    // Two bouncing polygons trailing color.
    for poly in 0..2 {
        let color = if poly == 0 { rgb(0x4E, 0xD1, 0xFF) } else { rgb(0xFF, 0x6E, 0xC7) };
        let mut pts = [(0i32, 0i32); 4];
        for (k, p) in pts.iter_mut().enumerate() {
            let seed = (poly * 4 + k) as u64;
            let px = bounce(frame + hash(seed) % 200, w - 20, (hash(seed) % 3 + 2) as i32) + 10;
            let py = bounce(frame + hash(seed + 99) % 200, h - 20, (hash(seed + 1) % 3 + 2) as i32) + 10;
            *p = (px, py);
        }
        for k in 0..4 {
            let (x0, y0) = pts[k];
            let (x1, y1) = pts[(k + 1) % 4];
            canvas.draw_line(x0, y0, x1, y1, color);
        }
    }
}

fn pipes(canvas: &mut Canvas, frame: u64, w: i32, h: i32) {
    // A growing "pipe" turning at grid cells.
    let cell = 24;
    let steps = (frame % 400) as i32;
    let (mut x, mut y) = (w / 2 / cell, h / 2 / cell);
    let mut dir = 0;
    for s in 0..steps {
        let seed = hash(s as u64);
        if s % 5 == 0 {
            dir = (dir + 1 + (seed % 3) as i32) % 4;
        }
        let (dx, dy) = [(1, 0), (0, 1), (-1, 0), (0, -1)][dir as usize];
        x = (x + dx).clamp(0, w / cell - 1);
        y = (y + dy).clamp(0, h / cell - 1);
        let hue = (s * 6) as u8;
        let c = rgb(0x40 + hue / 2, 0xC0, 0x80 + hue / 3);
        canvas.ellipse(x * cell + cell / 2 - 6, y * cell + cell / 2 - 6, 12, 12, c, true);
    }
}

fn marquee(canvas: &mut Canvas, frame: u64, w: i32, h: i32) {
    let text = "Buitenzorg OS  --  zonder zorg, tanpa kekhawatiran  --  ";
    let tw = text.len() as i32 * 8;
    let x = w - ((frame * 6) % (w + tw) as u64) as i32;
    let y = h / 2 - 8;
    canvas.draw_text(x, y, text, rgb(0x6F, 0xC1, 0x4E), w);
    canvas.draw_text(x, y + 20, text, rgb(0x2C, 0x6A, 0x24), w);
}

fn bouncing(canvas: &mut Canvas, frame: u64, w: i32, h: i32) {
    for i in 0..6u64 {
        let hx = hash(i);
        let sz = 30 + (hx % 40) as i32;
        let x = bounce(frame + hx % 300, w - sz, (hx % 4 + 2) as i32);
        let y = bounce(frame + (hx >> 8) % 300, h - sz, ((hx >> 4) % 4 + 2) as i32);
        let c = rgb((hx & 0xFF) as u8 | 0x40, ((hx >> 8) & 0xFF) as u8 | 0x40, ((hx >> 16) & 0xFF) as u8 | 0x40);
        canvas.ellipse(x, y, sz, sz, c, true);
    }
}

/// Ping-pong a value in 0..range at the given speed.
fn bounce(frame: u64, range: i32, speed: i32) -> i32 {
    if range <= 0 {
        return 0;
    }
    let period = (2 * range) as u64;
    let t = (frame * speed as u64) % period;
    let t = t as i32;
    if t < range {
        t
    } else {
        2 * range - t
    }
}

/// Unit vector * 100 for an angle in 0..628 (~radians*100), via a 16-step table.
fn unit(ang: i32) -> (i32, i32) {
    const COS: [i32; 16] = [
        100, 92, 71, 38, 0, -38, -71, -92, -100, -92, -71, -38, 0, 38, 71, 92,
    ];
    let idx = ((ang / 39) % 16) as usize;
    let sidx = (idx + 12) % 16; // sin = cos(a - 90deg)
    (COS[idx], COS[sidx])
}
