//! App icons for the desktop and the start menu. The artwork is AI-generated
//! (assets/icons/*.jpg); `bzimage/build.rs` downsamples each one, cuts the
//! same anti-aliased rounded-square silhouette into its alpha channel and
//! packs them into `/disk/ICONS.BIN` ("BZIC" v1: count, big/small size, then
//! per icon a 12-byte name + big + small ARGB). `wm.rs` blends them over the
//! wallpaper; when the pack is unavailable (boot media without a /disk driver)
//! it falls back to drawing a distinct colored tile per app.
//!
//! **Icon packs:** more BZIC files in `/disk/ICONS/*.BIN` (a `.bzpkg` can
//! ship one as `ICONS/NAME.BIN`). Selecting one (`icons NAME`, the ICON_PACK
//! setting) lays it over the built-in pack: an app the pack has an icon for
//! uses it, the others keep theirs. A pack must use the built-in sizes.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::{Mutex, Once};

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
/// The selected icon pack (name, icons) laid over [`PACK`].
static OVERLAY: Mutex<Option<(String, Pack)>> = Mutex::new(None);
/// Bumped whenever the overlay changes (frame caches key on it).
static GENERATION: AtomicU64 = AtomicU64::new(0);
const PACK_DIR: &str = "/disk/ICONS";

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

/// A frame-cache input: the base pack's presence and the overlay generation.
pub fn cache_key() -> u64 {
    (PACK.get().is_some() as u64) | GENERATION.load(Ordering::Relaxed) << 1
}

/// The icon packs in /disk/ICONS (names without `.BIN`, upper case).
pub fn packs() -> Vec<String> {
    let mut v: Vec<String> = crate::vfs::list_entries(PACK_DIR)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !e.is_dir && e.name.to_ascii_uppercase().ends_with(".BIN"))
        .map(|e| e.name.to_ascii_uppercase().trim_end_matches(".BIN").into())
        .collect();
    v.sort();
    v
}

/// The selected pack's name (None = the built-in icons).
pub fn pack_name() -> Option<String> {
    OVERLAY.lock().as_ref().map(|(n, _)| n.clone())
}

/// Select icon pack `name` (None = back to the built-in icons).
pub fn set_pack(name: Option<&str>) -> Result<(), &'static str> {
    let Some(name) = name else {
        *OVERLAY.lock() = None;
        GENERATION.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    };
    let upper = name.to_ascii_uppercase();
    let bytes = crate::vfs::read(&alloc::format!("{}/{}.BIN", PACK_DIR, upper)).map_err(|_| "no such icon pack")?;
    let pack = parse(&bytes).ok_or("not a BZIC icon pack")?;
    let (big, small) = (size(true) as usize, size(false) as usize);
    if pack.big != big || pack.small != small {
        return Err("the pack's icon sizes differ from the desktop's");
    }
    *OVERLAY.lock() = Some((upper, pack));
    GENERATION.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Self-test: every pack in /disk/ICONS loads and replaces icons; back to
/// the built-in ones afterwards.
pub fn self_test() -> bool {
    let list = packs();
    let before = pack_name();
    let mut ok = true;
    for p in &list {
        let good = set_pack(Some(p)).is_ok() && OVERLAY.lock().as_ref().is_some_and(|(_, pk)| !pk.icons.is_empty());
        ok &= good;
    }
    let refused = set_pack(Some("TIDAKADA")).is_err();
    let _ = set_pack(before.as_deref());
    if ok && refused {
        crate::println!("[kernel] MILESTONE: ICON PACKS OK ({} pack(s) in {}, laid over the built-in icons)", list.len(), PACK_DIR);
    } else {
        crate::println!("[icons] self-test: packs {:?} ok {} refused {}", list, ok, refused);
    }
    ok && refused
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
    let matches = |i: &&Icon| {
        let len = i.name.iter().position(|&c| c == 0).unwrap_or(12);
        &i.name[..len] == name.as_bytes()
    };
    if let Some(over) = OVERLAY.try_lock() {
        if let Some((_, pack)) = over.as_ref() {
            if let Some(icon) = pack.icons.iter().find(matches) {
                blit(canvas, pack, icon, x, y, big);
                return true;
            }
        }
    }
    let Some(pack) = PACK.get() else { return false };
    let Some(icon) = pack.icons.iter().find(matches) else {
        return false;
    };
    blit(canvas, pack, icon, x, y, big);
    true
}

fn blit(canvas: &mut Canvas, pack: &Pack, icon: &Icon, x: i32, y: i32, big: bool) {
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
}
