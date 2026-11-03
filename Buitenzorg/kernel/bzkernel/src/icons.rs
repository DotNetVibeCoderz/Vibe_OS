//! App icons for the desktop and the start menu. The artwork is AI-generated
//! (assets/icons/*.jpg); `bzimage/build.rs` downsamples each one, cuts the
//! same anti-aliased rounded-square silhouette into its alpha channel and
//! packs them into `/disk/ICONS.BIN` ("BZIC" v1: count, big/small size, then
//! per icon a 12-byte name + big + small ARGB). `wm.rs` blends them over the
//! wallpaper; when the pack is unavailable (boot media without a /disk driver)
//! it falls back to drawing a distinct colored tile per app.

use alloc::vec::Vec;
use spin::Once;

use crate::gfx::Canvas;

struct Icon {
    name: [u8; 12],
    big: Vec<u32>,
    small: Vec<u32>,
}

struct Pack {
    big: usize,
    small: usize,
    icons: Vec<Icon>,
}

static PACK: Once<Pack> = Once::new();

fn parse(b: &[u8]) -> Option<Pack> {
    if b.len() < 8 || &b[0..4] != b"BZIC" || b[4] != 1 {
        return None;
    }
    let (count, big, small) = (b[5] as usize, b[6] as usize, b[7] as usize);
    let word = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let mut off = 8;
    let mut icons = Vec::new();
    for _ in 0..count {
        let need = 12 + (big * big + small * small) * 4;
        if off + need > b.len() {
            return None;
        }
        let mut name = [0u8; 12];
        name.copy_from_slice(&b[off..off + 12]);
        off += 12;
        let big_px = (0..big * big).map(|i| word(off + i * 4)).collect();
        off += big * big * 4;
        let small_px = (0..small * small).map(|i| word(off + i * 4)).collect();
        off += small * small * 4;
        icons.push(Icon { name, big: big_px, small: small_px });
    }
    Some(Pack { big, small, icons })
}

/// Load the icon pack from /disk. Returns how many icons are available.
pub fn load() -> usize {
    if let Some(p) = PACK.get() {
        return p.icons.len();
    }
    match crate::vfs::read("/disk/ICONS.BIN").ok().and_then(|b| parse(&b)) {
        Some(pack) => PACK.call_once(|| pack).icons.len(),
        None => 0,
    }
}

/// Whether the icon pack has been loaded (a frame-cache input).
pub fn loaded() -> bool {
    PACK.get().is_some()
}

/// Edge length of the big (desktop) or small (start menu) icon.
pub fn size(big: bool) -> i32 {
    match PACK.get() {
        Some(p) => (if big { p.big } else { p.small }) as i32,
        None => if big { 44 } else { 18 },
    }
}

/// Blend app `name`'s icon at (x, y). Returns false if no icon is loaded for it.
pub fn draw(canvas: &mut Canvas, name: &str, x: i32, y: i32, big: bool) -> bool {
    let Some(pack) = PACK.get() else { return false };
    let Some(icon) = pack.icons.iter().find(|i| {
        let len = i.name.iter().position(|&c| c == 0).unwrap_or(12);
        &i.name[..len] == name.as_bytes()
    }) else {
        return false;
    };
    let (px, n) = if big { (&icon.big, pack.big) } else { (&icon.small, pack.small) };
    for row in 0..n {
        for col in 0..n {
            let argb = px[row * n + col];
            let a = (argb >> 24) as u8;
            let (cx, cy) = (x + col as i32, y + row as i32);
            if a == 0 || cx < 0 || cy < 0 {
                continue;
            }
            canvas.blend(cx as usize, cy as usize, argb & 0x00FF_FFFF, a);
        }
    }
    true
}
