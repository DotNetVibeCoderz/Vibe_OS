//! TrueType / OpenType fonts: the desktop's typefaces.
//!
//! Font files live in `/disk/FONTS` (`*.TTF`, `*.OTF`) and are registered at
//! boot by [`load_all`]. Each registered *face* (a family in one weight) gets
//! a stable id (1, 2, ...; 0 = the built-in bitmap font). A variable font with
//! a weight axis is registered twice: its default instance (Regular) and a
//! `wght` = 700 instance (Bold).
//!
//! * outlines + metrics come from `ttf-parser`, coverage from
//!   `ab_glyph_rasterizer` (anti-aliased, no hinting);
//! * kerning from the `kern` table, else the GPOS `kern` feature's pair
//!   adjustments (format 1 and 2), cached per glyph pair;
//! * glyph bitmaps are cached per (face, pixel size, character) within a
//!   byte budget; a character a face lacks is taken from another registered
//!   face that has it, else from the bitmap font.
//!
//! The compositor draws the shell's text (window titles, Start menu, taskbar,
//! toolkit text) in the **UI font** and the terminal / window text lines in
//! the **monospace font** on the bitmap font's 8 x 16 cell grid, so choosing a
//! font never changes a layout. Both are settings ([`set_ui`], [`set_mono`],
//! kept in `/disk/FONTS.CFG`); with no font files the bitmap font is used.
//!
//! **Allocation:** laying text out allocates (glyph cache, placement list), so
//! callers that must never allocate (the boot splash inside `_print`) use the
//! bitmap functions in `gfx` directly.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

/// Where font files are kept.
pub const DIR: &str = "/disk/FONTS";
const CONFIG: &str = "/disk/FONTS.CFG";
/// Glyph cache budget in bytes of coverage.
const CACHE_BUDGET: usize = 3 << 20;
/// Largest pixel size drawn.
pub const MAX_PX: u32 = 200;
/// UI text sizes: body / strong lines are 16 px tall, headings 24 px.
const UI_BODY_PX: u32 = 14;
const UI_HEADING_PX: u32 = 21;
/// Text scale of the UI fonts in percent (the UI_SCALE setting).
static SCALE_PCT: AtomicU32 = AtomicU32::new(100);

/// The UI text scale choices (percent).
pub const SCALES: [u32; 3] = [100, 110, 125];

/// Set the UI text scale (percent, one of [`SCALES`]); the glyph caches key
/// by pixel size, and the generation makes frame caches redraw.
pub fn set_scale(pct: u32) -> bool {
    if !SCALES.contains(&pct) {
        return false;
    }
    if SCALE_PCT.swap(pct, Ordering::Relaxed) != pct {
        bump();
    }
    true
}

/// The UI text scale (percent).
pub fn scale() -> u32 {
    SCALE_PCT.load(Ordering::Relaxed)
}
/// The monospace cell (the bitmap font's): 8 px wide.
const MONO_CELL: f32 = 8.0;

/// One registered face.
struct Face {
    family: &'static str,
    bold: bool,
    mono: bool,
    /// 8.3 file name in `/disk/FONTS`.
    file: &'static str,
    removed: bool,
    face: ttf_parser::Face<'static>,
    upem: f32,
    ascender: f32,
    descender: f32,
    gap: f32,
    /// GPOS lookups of the `kern` feature.
    kern_lookups: Vec<u16>,
    kern_cache: BTreeMap<(u16, u16), i16>,
}

/// A rasterized glyph: coverage rows, its box relative to the pen on the
/// baseline (`left` right of the pen, `top` above the baseline) and the
/// advance in pixels.
pub struct Glyph {
    pub w: u16,
    pub h: u16,
    pub left: i16,
    pub top: i16,
    pub advance: f32,
    pub cov: Vec<u8>,
}

static FACES: Mutex<Vec<Face>> = Mutex::new(Vec::new());
static CACHE: Mutex<GlyphCache> = Mutex::new(GlyphCache { map: BTreeMap::new(), bytes: 0, misses: 0, hits: 0 });
/// UI font: regular id | bold id << 16 (0 = bitmap).
static UI: AtomicU32 = AtomicU32::new(0);
/// Monospace font: id | pixel size << 16 (0 = bitmap).
static MONO: AtomicU32 = AtomicU32::new(0);
/// Bumped on every change of the registered fonts or the choices.
static GENERATION: AtomicU64 = AtomicU64::new(1);

struct GlyphCache {
    map: BTreeMap<(u16, u16, u32), Arc<Glyph>>,
    bytes: usize,
    misses: u64,
    hits: u64,
}

/// Changes whenever what text looks like changes (fonts added / removed,
/// another UI or monospace font): frame caches mix it into their keys.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

fn bump() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

// --- registration -------------------------------------------------------------

/// Typographic family name (name id 16), else the family (1).
fn family_name(face: &ttf_parser::Face) -> Option<String> {
    let mut fallback = None;
    for n in face.names() {
        if n.name_id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY || n.name_id == ttf_parser::name_id::FAMILY {
            if let Some(s) = name_text(&n) {
                if n.name_id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY {
                    return Some(s);
                }
                fallback.get_or_insert(s);
            }
        }
    }
    fallback
}

/// A name record as text: UTF-16 BE (Unicode / Windows platforms) or Mac Roman (ASCII part).
fn name_text(n: &ttf_parser::name::Name) -> Option<String> {
    if n.is_unicode() {
        let units: Vec<u16> = n.name.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        return Some(char::decode_utf16(units).map(|r| r.unwrap_or('?')).collect());
    }
    if n.platform_id == ttf_parser::PlatformId::Macintosh {
        return Some(n.name.iter().map(|&b| if b < 128 { b as char } else { '?' }).collect());
    }
    None
}

fn leak_str(s: &str) -> &'static str {
    alloc::boxed::Box::leak(s.to_string().into_boxed_str())
}

/// Indices of the GPOS lookups of the `kern` feature.
fn kern_lookups(face: &ttf_parser::Face) -> Vec<u16> {
    let mut out = Vec::new();
    if let Some(gpos) = face.tables().gpos {
        for i in 0..gpos.features.len() {
            if let Some(f) = gpos.features.get(i) {
                if f.tag == ttf_parser::Tag::from_bytes(b"kern") {
                    for l in f.lookup_indices {
                        if !out.contains(&l) {
                            out.push(l);
                        }
                    }
                }
            }
        }
    }
    out
}

/// Register the faces of one font file (already read). Returns the ids.
fn register_bytes(file: &str, data: Vec<u8>) -> Result<Vec<u16>, &'static str> {
    let data: &'static [u8] = alloc::boxed::Box::leak(data.into_boxed_slice());
    let probe = ttf_parser::Face::parse(data, 0).map_err(|_| "not a TrueType / OpenType font")?;
    let family = family_name(&probe).ok_or("font has no family name")?;
    let family = leak_str(family.trim());
    let file = leak_str(file);
    let has_wght = probe
        .variation_axes()
        .into_iter()
        .any(|a| a.tag == ttf_parser::Tag::from_bytes(b"wght") && a.max_value >= 700.0);
    let mut instances: Vec<(bool, Option<f32>)> = Vec::new();
    if has_wght {
        instances.push((false, Some(400.0)));
        instances.push((true, Some(700.0)));
    } else {
        let bold = probe.is_bold() || probe.weight().to_number() >= 600;
        instances.push((bold, None));
    }
    let mut ids = Vec::new();
    let mut faces = FACES.lock();
    for (bold, wght) in instances {
        if faces.iter().any(|f| !f.removed && f.family == family && f.bold == bold) {
            continue; // the same face from another file: keep the first
        }
        let mut face = ttf_parser::Face::parse(data, 0).map_err(|_| "font parse failed")?;
        if let Some(w) = wght {
            face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), w);
        }
        let mono = face.is_monospaced() || {
            let a = face.glyph_index('i').and_then(|g| face.glyph_hor_advance(g));
            let b = face.glyph_index('W').and_then(|g| face.glyph_hor_advance(g));
            a.is_some() && a == b
        };
        let kern = kern_lookups(&face);
        faces.push(Face {
            family,
            bold,
            mono,
            file,
            removed: false,
            upem: face.units_per_em().max(1) as f32,
            ascender: face.ascender() as f32,
            descender: face.descender() as f32,
            gap: face.line_gap() as f32,
            kern_lookups: kern,
            kern_cache: BTreeMap::new(),
            face,
        });
        ids.push(faces.len() as u16);
    }
    drop(faces);
    bump();
    Ok(ids)
}

/// Register `/disk/FONTS/<file>`.
pub fn register_file(file: &str) -> Result<Vec<u16>, &'static str> {
    let data = crate::vfs::read(&alloc::format!("{}/{}", DIR, file)).map_err(|_| "cannot read the font file")?;
    register_bytes(file, data)
}

fn is_font_name(n: &str) -> bool {
    let u = n.to_ascii_uppercase();
    u.ends_with(".TTF") || u.ends_with(".OTF")
}

/// Register every font in `/disk/FONTS` and apply `/disk/FONTS.CFG` (or the
/// defaults: Plus Jakarta Sans for the UI, JetBrains Mono for text). Safe to
/// call again (files already registered are skipped).
pub fn load_all() -> usize {
    let entries = crate::vfs::list_entries(DIR).unwrap_or_default();
    let mut n = 0;
    for e in entries.iter().filter(|e| !e.is_dir && is_font_name(&e.name)) {
        let known = FACES.lock().iter().any(|f| !f.removed && f.file.eq_ignore_ascii_case(&e.name));
        if known {
            continue;
        }
        match register_file(&e.name) {
            Ok(ids) => n += ids.len(),
            Err(err) => crate::println!("[font] {}: {}", e.name, err),
        }
    }
    apply_config();
    let total = FACES.lock().iter().filter(|f| !f.removed).count();
    if n > 0 {
        crate::println!(
            "[font] {} face(s) from {} -- UI: {}, monospace: {}",
            total,
            DIR,
            ui_family().unwrap_or("bitmap"),
            mono_family().unwrap_or("bitmap")
        );
    }
    n
}

fn apply_config() {
    let cfg = crate::vfs::read(CONFIG).ok().and_then(|b| String::from_utf8(b).ok()).unwrap_or_default();
    let mut ui = Some("Plus Jakarta Sans".to_string());
    let mut mono = Some("JetBrains Mono".to_string());
    for line in cfg.lines() {
        if let Some(v) = line.strip_prefix("ui ") {
            ui = if v.trim() == "bitmap" { None } else { Some(v.trim().to_string()) };
        } else if let Some(v) = line.strip_prefix("mono ") {
            mono = if v.trim() == "bitmap" { None } else { Some(v.trim().to_string()) };
        }
    }
    select_ui(ui.as_deref());
    select_mono(mono.as_deref());
}

fn save_config() {
    let text = alloc::format!("ui {}\nmono {}\n", ui_family().unwrap_or("bitmap"), mono_family().unwrap_or("bitmap"));
    if let Err(e) = crate::vfs::write(CONFIG, text.as_bytes()) {
        crate::println!("[font] cannot save {}: {}", CONFIG, e);
    }
}

fn find_face(faces: &[Face], family: &str, bold: bool) -> Option<u16> {
    faces
        .iter()
        .position(|f| !f.removed && f.family.eq_ignore_ascii_case(family) && f.bold == bold)
        .map(|i| i as u16 + 1)
}

/// The face of `family` in that weight (falls back to the other weight).
pub fn find(family: &str, bold: bool) -> Option<u16> {
    let faces = FACES.lock();
    find_face(&faces, family, bold).or_else(|| find_face(&faces, family, !bold))
}

fn select_ui(family: Option<&str>) -> bool {
    let v = match family {
        None => 0,
        Some(f) => match (find(f, false), find(f, true)) {
            (Some(r), Some(b)) => r as u32 | (b as u32) << 16,
            _ => return false,
        },
    };
    UI.store(v, Ordering::Relaxed);
    bump();
    true
}

fn select_mono(family: Option<&str>) -> bool {
    let v = match family {
        None => 0,
        Some(f) => {
            let Some(id) = find(f, false) else { return false };
            // The size whose advance fills the 8 px cell.
            let px = {
                let faces = FACES.lock();
                let face = &faces[id as usize - 1];
                let adv = face.face.glyph_index('M').and_then(|g| face.face.glyph_hor_advance(g)).unwrap_or(600) as f32;
                ((MONO_CELL * face.upem / adv.max(1.0)) + 0.5) as u32
            };
            id as u32 | px.clamp(6, 40) << 16
        }
    };
    MONO.store(v, Ordering::Relaxed);
    bump();
    true
}

/// Family of the UI font (None = bitmap).
pub fn ui_family() -> Option<&'static str> {
    let v = UI.load(Ordering::Relaxed);
    if v == 0 {
        return None;
    }
    FACES.lock().get((v & 0xFFFF) as usize - 1).map(|f| f.family)
}

/// Family of the monospace font (None = bitmap).
pub fn mono_family() -> Option<&'static str> {
    let v = MONO.load(Ordering::Relaxed);
    if v == 0 {
        return None;
    }
    FACES.lock().get((v & 0xFFFF) as usize - 1).map(|f| f.family)
}

/// Families with a regular face, in registration order; `mono_only` keeps monospace ones.
pub fn families(mono_only: bool) -> Vec<&'static str> {
    let faces = FACES.lock();
    let mut out: Vec<&'static str> = Vec::new();
    for f in faces.iter().filter(|f| !f.removed && (!mono_only || f.mono)) {
        if !out.contains(&f.family) {
            out.push(f.family);
        }
    }
    out
}

/// Use `family` (None = bitmap) for the shell's text and remember it.
pub fn set_ui(family: Option<&str>) -> bool {
    let ok = select_ui(family);
    if ok {
        save_config();
    }
    ok
}

/// Use `family` (None = bitmap) for the terminal and text windows and remember it.
pub fn set_mono(family: Option<&str>) -> bool {
    let ok = select_mono(family);
    if ok {
        save_config();
    }
    ok
}

/// (UI font id, pixel size) for a shell text style, or None for the bitmap font.
pub fn for_style(bold: bool, heading: bool) -> Option<(u16, u32)> {
    let v = UI.load(Ordering::Relaxed);
    if v == 0 {
        return None;
    }
    let id = if bold || heading { (v >> 16) as u16 } else { (v & 0xFFFF) as u16 };
    let px = if heading { UI_HEADING_PX } else { UI_BODY_PX };
    Some((id, (px * scale() + 50) / 100))
}

/// (monospace font id, pixel size) or None for the bitmap font.
pub fn mono() -> Option<(u16, u32)> {
    let v = MONO.load(Ordering::Relaxed);
    if v == 0 {
        None
    } else {
        Some(((v & 0xFFFF) as u16, v >> 16))
    }
}

/// Resolve an id from user space: 1.. = a face, 0xFFF0 + style = the UI font
/// for a toolkit style (0 body, 1 strong, 2 heading), 0xFFFE = monospace.
pub fn resolve(id: u32) -> Option<u16> {
    match id {
        0xFFF0 => for_style(false, false).map(|f| f.0),
        0xFFF1 => for_style(true, false).map(|f| f.0),
        0xFFF2 => for_style(true, true).map(|f| f.0),
        0xFFFE => mono().map(|f| f.0),
        n if n >= 1 => {
            let faces = FACES.lock();
            faces.get(n as usize - 1).filter(|f| !f.removed).map(|_| n as u16)
        }
        _ => None,
    }
}

/// Facts about face `id`: (family, bold, monospace, file).
pub fn info(id: u16) -> Option<(&'static str, bool, bool, &'static str)> {
    let faces = FACES.lock();
    let f = faces.get((id as usize).checked_sub(1)?)?;
    (!f.removed).then_some((f.family, f.bold, f.mono, f.file))
}

/// Number of face slots (ids 1..=count; removed ones answer None to [`info`]).
pub fn count() -> usize {
    FACES.lock().len()
}

/// (ascent, descent, line height) in pixels at `px`; descent is positive.
pub fn metrics(id: u16, px: u32) -> Option<(i32, i32, i32)> {
    let faces = FACES.lock();
    let f = faces.get((id as usize).checked_sub(1)?)?;
    let s = px as f32 / f.upem;
    let asc = (f.ascender * s + 0.5) as i32;
    let desc = (-f.descender * s + 0.5) as i32;
    let line = ((f.ascender - f.descender + f.gap) * s + 0.5) as i32;
    Some((asc, desc, line.max(asc + desc)))
}

/// Unregister the faces from `file` and delete it; ids stay reserved. The
/// UI / monospace choice falls back to the bitmap font if it used them.
pub fn remove_file(file: &str) -> Result<usize, &'static str> {
    let mut n = 0;
    {
        let mut faces = FACES.lock();
        for f in faces.iter_mut().filter(|f| !f.removed && f.file.eq_ignore_ascii_case(file)) {
            f.removed = true;
            n += 1;
        }
    }
    if n == 0 {
        // a file whose faces were never registered (the same family from another file)
        let path = alloc::format!("{}/{}", DIR, file);
        if crate::vfs::stat(&path).is_ok() && is_font_name(file) {
            crate::vfs::remove(&path).map_err(|_| "cannot delete the font file")?;
            return Ok(0);
        }
        return Err("no such font file");
    }
    // their glyphs are useless now (the font data itself stays allocated: it was leaked)
    let mut c = CACHE.lock();
    c.map.clear();
    c.bytes = 0;
    drop(c);
    let ui = UI.load(Ordering::Relaxed);
    let faces = FACES.lock();
    let gone = |id: u32| id != 0 && faces.get(id as usize - 1).map_or(true, |f| f.removed);
    let ui_gone = gone(ui & 0xFFFF) || gone(ui >> 16);
    let mono_gone = gone(MONO.load(Ordering::Relaxed) & 0xFFFF);
    drop(faces);
    if ui_gone {
        UI.store(0, Ordering::Relaxed);
    }
    if mono_gone {
        MONO.store(0, Ordering::Relaxed);
    }
    let _ = crate::vfs::remove(&alloc::format!("{}/{}", DIR, file));
    if ui_gone || mono_gone {
        save_config();
    }
    bump();
    Ok(n)
}

/// Install font data as `/disk/FONTS/<file>` after checking it parses (and,
/// when given, its SHA-256), then register it.
pub fn install(file: &str, data: &[u8], sha256: Option<&[u8; 32]>) -> Result<Vec<u16>, &'static str> {
    if !is_font_name(file) || file.contains('/') || file.len() > 12 {
        return Err("font file names are 8.3 .TTF / .OTF names");
    }
    if let Some(want) = sha256 {
        use sha2::Digest;
        let got: [u8; 32] = sha2::Sha256::digest(data).into();
        if &got != want {
            return Err("SHA-256 does not match the catalog");
        }
    }
    ttf_parser::Face::parse(data, 0).map_err(|_| "not a TrueType / OpenType font")?;
    let _ = crate::vfs::mkdir(DIR);
    crate::vfs::write(&alloc::format!("{}/{}", DIR, file), data).map_err(|_| "cannot write the font file")?;
    if FACES.lock().iter().any(|f| !f.removed && f.file.eq_ignore_ascii_case(file)) {
        return Ok(Vec::new()); // replaced on disk; registered at the next boot
    }
    register_bytes(file, data.to_vec())
}

// --- glyphs + layout ---------------------------------------------------------

struct Outline {
    r: ab_glyph_rasterizer::Rasterizer,
    s: f32,
    dx: f32,
    dy: f32,
    last: ab_glyph_rasterizer::Point,
    start: ab_glyph_rasterizer::Point,
}

impl Outline {
    fn p(&self, x: f32, y: f32) -> ab_glyph_rasterizer::Point {
        ab_glyph_rasterizer::point(x * self.s - self.dx, self.dy - y * self.s)
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.last = self.p(x, y);
        self.start = self.last;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.r.draw_line(self.last, p);
        self.last = p;
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p) = (self.p(x1, y1), self.p(x, y));
        self.r.draw_quad(self.last, c, p);
        self.last = p;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = (self.p(x1, y1), self.p(x2, y2), self.p(x, y));
        self.r.draw_cubic(self.last, c1, c2, p);
        self.last = p;
    }
    fn close(&mut self) {
        if self.last != self.start {
            self.r.draw_line(self.last, self.start);
        }
        self.last = self.start;
    }
}

fn floor(v: f32) -> i32 {
    let i = v as i32;
    if (i as f32) > v { i - 1 } else { i }
}

fn ceil(v: f32) -> i32 {
    let i = v as i32;
    if (i as f32) < v { i + 1 } else { i }
}

/// Rasterize glyph `gid` of `face` at `px`, shifted right by `sub` quarters
/// of a pixel (subpixel positioning: pens are not whole pixels).
fn rasterize(face: &Face, gid: ttf_parser::GlyphId, px: u32, sub: u32) -> Glyph {
    let s = px as f32 / face.upem;
    let shift = sub as f32 * 0.25;
    let advance = face.face.glyph_hor_advance(gid).unwrap_or(0) as f32 * s;
    let empty = Glyph { w: 0, h: 0, left: 0, top: 0, advance, cov: Vec::new() };
    let Some(bb) = face.face.glyph_bounding_box(gid) else { return empty };
    let (x0, x1) = (floor(bb.x_min as f32 * s + shift), ceil(bb.x_max as f32 * s + shift));
    let (y0, y1) = (floor(bb.y_min as f32 * s), ceil(bb.y_max as f32 * s));
    let (w, h) = ((x1 - x0).max(0) as usize, (y1 - y0).max(0) as usize);
    if w == 0 || h == 0 || w > 4 * MAX_PX as usize || h > 4 * MAX_PX as usize {
        return empty;
    }
    let mut o = Outline {
        r: ab_glyph_rasterizer::Rasterizer::new(w, h),
        s,
        dx: x0 as f32 - shift,
        dy: y1 as f32,
        last: ab_glyph_rasterizer::point(0.0, 0.0),
        start: ab_glyph_rasterizer::point(0.0, 0.0),
    };
    if face.face.outline_glyph(gid, &mut o).is_none() {
        return empty;
    }
    let mut cov = alloc::vec![0u8; w * h];
    o.r.for_each_pixel(|i, a| {
        let v = (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
        cov[i] = v.min(255) as u8;
    });
    Glyph { w: w as u16, h: h as u16, left: x0 as i16, top: y1 as i16, advance, cov }
}

/// A glyph of the bitmap font as a [`Glyph`] (for characters no face has).
fn bitmap_glyph(ch: char, px: u32) -> Glyph {
    use noto_sans_mono_bitmap::{get_raster, FontWeight, RasterHeight};
    let r = get_raster(ch, FontWeight::Regular, RasterHeight::Size16).or_else(|| get_raster('?', FontWeight::Regular, RasterHeight::Size16));
    let Some(r) = r else { return Glyph { w: 0, h: 0, left: 0, top: 0, advance: px as f32 * 0.55, cov: Vec::new() } };
    let rows = r.raster();
    let (h, w) = (rows.len(), rows.first().map_or(0, |x| x.len()));
    let mut cov = Vec::with_capacity(w * h);
    for row in rows {
        cov.extend_from_slice(row);
    }
    // the bitmap cell's baseline is ~4 px above its bottom
    Glyph { w: w as u16, h: h as u16, left: 0, top: (h as i16) - 4, advance: w as f32, cov }
}

/// The cached glyph for `ch` of face `id` at `px` and subpixel offset `sub`
/// (0..=3 quarters), resolving fallbacks.
fn glyph(faces: &[Face], id: u16, px: u32, ch: char, sub: u32) -> Arc<Glyph> {
    let key = (id, px as u16, (ch as u32) << 2 | (sub & 3));
    {
        let mut c = CACHE.lock();
        if let Some(g) = c.map.get(&key).cloned() {
            c.hits += 1;
            return g;
        }
    }
    let face = &faces[id as usize - 1];
    let g = match face.face.glyph_index(ch).filter(|g| g.0 != 0) {
        Some(gid) => rasterize(face, gid, px, sub & 3),
        None => {
            // another registered face with this character, scaled to the same ascent
            let other = faces
                .iter()
                .find(|f| !f.removed && f.bold == face.bold && f.face.glyph_index(ch).is_some_and(|g| g.0 != 0))
                .or_else(|| faces.iter().find(|f| !f.removed && f.face.glyph_index(ch).is_some_and(|g| g.0 != 0)));
            match other {
                Some(o) => rasterize(o, o.face.glyph_index(ch).unwrap(), px, sub & 3),
                None => bitmap_glyph(ch, px),
            }
        }
    };
    let g = Arc::new(g);
    let mut c = CACHE.lock();
    c.misses += 1;
    if c.bytes + g.cov.len() > CACHE_BUDGET {
        c.map.clear();
        c.bytes = 0;
    }
    c.bytes += g.cov.len() + 32;
    c.map.insert(key, g.clone());
    g
}

/// Kerning between two glyphs of `face` in design units.
fn kerning(face: &mut Face, l: ttf_parser::GlyphId, r: ttf_parser::GlyphId) -> i16 {
    if let Some(&k) = face.kern_cache.get(&(l.0, r.0)) {
        return k;
    }
    let mut k = 0i16;
    if let Some(kern) = face.face.tables().kern {
        if let Some(v) = kern.subtables.into_iter().filter(|s| s.horizontal && !s.variable).find_map(|s| s.glyphs_kerning(l, r)) {
            k = v;
        }
    }
    if k == 0 && !face.kern_lookups.is_empty() {
        if let Some(gpos) = face.face.tables().gpos {
            'lookups: for &li in face.kern_lookups.iter() {
                let Some(lookup) = gpos.lookups.get(li) else { continue };
                for si in 0..lookup.subtables.len() {
                    let Some(ttf_parser::gpos::PositioningSubtable::Pair(pair)) = lookup.subtables.get::<ttf_parser::gpos::PositioningSubtable>(si) else {
                        continue;
                    };
                    let v = match pair {
                        ttf_parser::gpos::PairAdjustment::Format1 { coverage, sets } => {
                            coverage.get(l).and_then(|i| sets.get(i)).and_then(|set| set.get(r)).map(|(a, _)| a.x_advance)
                        }
                        ttf_parser::gpos::PairAdjustment::Format2 { coverage, classes, matrix } => {
                            if coverage.get(l).is_some() {
                                matrix.get((classes.0.get(l), classes.1.get(r))).map(|(a, _)| a.x_advance)
                            } else {
                                None
                            }
                        }
                    };
                    if let Some(v) = v {
                        k = v;
                        break 'lookups;
                    }
                }
            }
        }
    }
    if face.kern_cache.len() < 8192 {
        face.kern_cache.insert((l.0, r.0), k);
    }
    k
}

/// Pen positions (x offset in pixels from the start) and glyphs of `text`,
/// plus the total width. Kerning applied; `cell` > 0 puts every character in
/// a fixed cell instead (monospace grid).
pub fn layout(id: u16, px: u32, text: &str, cell: i32) -> (Vec<(i32, Arc<Glyph>)>, i32) {
    let px = px.clamp(4, MAX_PX);
    let mut faces = FACES.lock();
    let mut out = Vec::with_capacity(text.len());
    if id == 0 || id as usize > faces.len() {
        return (out, 0);
    }
    let mut pen = 0f32;
    let mut prev: Option<ttf_parser::GlyphId> = None;
    let s = px as f32 / faces[id as usize - 1].upem;
    for (i, ch) in text.chars().enumerate() {
        if cell > 0 {
            let g = glyph(&faces, id, px, ch, 0);
            // centre glyphs narrower or wider than the cell
            let off = ((cell as f32 - g.advance) / 2.0 + 0.5) as i32;
            out.push((i as i32 * cell + off.max(-2), g));
            continue;
        }
        let gid = faces[id as usize - 1].face.glyph_index(ch);
        if let (Some(l), Some(r)) = (prev, gid) {
            pen += kerning(&mut faces[id as usize - 1], l, r) as f32 * s;
        }
        // whole pixels + a quarter-pixel glyph variant (FreeType-like spacing)
        let mut base = floor(pen);
        let mut sub = ((pen - base as f32) * 4.0 + 0.5) as u32;
        if sub >= 4 {
            base += 1;
            sub = 0;
        }
        let g = glyph(&faces, id, px, ch, sub);
        out.push((base, g.clone()));
        pen += g.advance;
        prev = gid;
    }
    let width = if cell > 0 { text.chars().count() as i32 * cell } else { (pen + 0.5) as i32 };
    (out, width)
}

/// Width of `text` in pixels (kerned), without rasterizing.
pub fn measure(id: u16, px: u32, text: &str) -> i32 {
    let px = px.clamp(4, MAX_PX);
    let mut faces = FACES.lock();
    if id == 0 || id as usize > faces.len() {
        return 0;
    }
    let s = px as f32 / faces[id as usize - 1].upem;
    let mut pen = 0f32;
    let mut prev: Option<ttf_parser::GlyphId> = None;
    for ch in text.chars() {
        let gid = faces[id as usize - 1].face.glyph_index(ch).filter(|g| g.0 != 0);
        match gid {
            Some(g) => {
                if let Some(l) = prev {
                    pen += kerning(&mut faces[id as usize - 1], l, g) as f32 * s;
                }
                pen += faces[id as usize - 1].face.glyph_hor_advance(g).unwrap_or(0) as f32 * s;
            }
            None => {
                drop(faces);
                // a fallback glyph: its advance comes from the glyph itself
                let faces2 = FACES.lock();
                pen += glyph(&faces2, id, px, ch, 0).advance;
                drop(faces2);
                faces = FACES.lock();
            }
        }
        prev = gid;
    }
    (pen + 0.5) as i32
}

/// Ascent in pixels of face `id` at `px` (the baseline below a line's top).
pub fn ascent(id: u16, px: u32) -> i32 {
    metrics(id, px).map_or(px as i32 * 4 / 5, |m| m.0)
}

/// Glyph cache statistics: (entries, bytes, hits, misses).
pub fn cache_stats() -> (usize, usize, u64, u64) {
    let c = CACHE.lock();
    (c.map.len(), c.bytes, c.hits, c.misses)
}

// --- self-test --------------------------------------------------------------------

/// Checks the shipped fonts: registered, proportional vs monospace advances,
/// kerning, rasterized coverage, fallback, the UI / monospace choice and
/// switching it -> `MILESTONE: FONTS OK`.
pub fn self_test() -> bool {
    let mut fails: Vec<&str> = Vec::new();
    let ui = find("Plus Jakarta Sans", false);
    let ui_b = find("Plus Jakarta Sans", true);
    let mono_id = find("JetBrains Mono", false);
    let (Some(ui), Some(ui_b), Some(mono_id)) = (ui, ui_b, mono_id) else {
        crate::println!("[font] self-test skipped: the shipped fonts are not in {}", DIR);
        return false;
    };
    // proportional: 'i' narrower than 'W'; monospace: equal
    if !(measure(ui, 16, "i") < measure(ui, 16, "W")) {
        fails.push("proportional advances");
    }
    if measure(mono_id, 16, "i") != measure(mono_id, 16, "W") {
        fails.push("monospace advances");
    }
    if !info(mono_id).is_some_and(|i| i.2) || info(ui).is_some_and(|i| i.2) {
        fails.push("monospace detection");
    }
    // bold is wider than regular
    if !(measure(ui_b, 16, "Buitenzorg") > measure(ui, 16, "Buitenzorg")) {
        fails.push("bold face");
    }
    // kerning: "AV" / "To" closer than the sum of their parts in a proportional face
    let kerned = ["AV", "To", "Ty", "LT"].iter().any(|p| {
        let mut cs = p.chars();
        let (a, b) = (cs.next().unwrap(), cs.next().unwrap());
        let mut s1 = String::new();
        s1.push(a);
        let mut s2 = String::new();
        s2.push(b);
        measure(ui, 32, p) < measure(ui, 32, &s1) + measure(ui, 32, &s2)
    });
    if !kerned {
        fails.push("kerning");
    }
    // coverage: an 'O' at 24 px has a hole and anti-aliased edges
    let (placed, w) = layout(ui, 24, "O", 0);
    let ok_o = placed.first().is_some_and(|(_, g)| {
        let (gw, gh) = (g.w as usize, g.h as usize);
        let full = g.cov.iter().filter(|&&a| a == 255).count();
        let partial = g.cov.iter().filter(|&&a| a > 0 && a < 255).count();
        gw > 8 && gh > 10 && g.cov[(gh / 2) * gw + gw / 2] == 0 && full > 10 && partial > 10
    });
    if !ok_o || w <= 0 {
        fails.push("rasterized coverage");
    }
    // a character none of the faces has comes from the bitmap font
    let (pl, _) = layout(ui, 16, "\u{2591}", 0);
    if !pl.first().is_some_and(|(_, g)| g.w > 0) {
        fails.push("fallback glyph");
    }
    // the choice: switch the UI font off and back, generation moves
    let (g0, before) = (generation(), ui_family());
    select_ui(None);
    let off = for_style(false, false).is_none();
    select_ui(before);
    let back = ui_family() == before;
    if !(off && back && generation() > g0) {
        fails.push("UI font switch");
    }
    let (entries, bytes, _, _) = cache_stats();
    // (a FACES guard inside the println! arguments would live to the end of
    // the statement, and ui_family() below takes FACES again: deadlock)
    let faces = FACES.lock().iter().filter(|f| !f.removed).count();
    if fails.is_empty() {
        crate::println!(
            "[kernel] MILESTONE: FONTS OK ({} face(s); UI {} / monospace {}; kerning, bold, AA coverage, fallback; cache {} glyphs, {} KiB)",
            faces,
            ui_family().unwrap_or("bitmap"),
            mono_family().unwrap_or("bitmap"),
            entries,
            bytes / 1024
        );
        true
    } else {
        crate::println!("[font] self-test FAILED: {:?}", fails);
        false
    }
}

// --- catalog + shell command ---------------------------------------------------------

/// One style of a font in the catalog of installable fonts
/// (`/disk/FONTS/CATALOG.TXT`, like fontist's formulas).
pub struct CatalogEntry {
    pub name: String,
    pub family: String,
    pub style: String,
    pub file: String,
    pub size: u64,
    pub sha256: String,
    pub licence: String,
    pub url: String,
}

/// Licences installed without asking.
pub fn free_licence(l: &str) -> bool {
    matches!(l, "OFL-1.1" | "Apache-2.0" | "MIT")
}

/// The catalog's entries.
pub fn catalog() -> Vec<CatalogEntry> {
    let text = crate::vfs::read(&alloc::format!("{}/CATALOG.TXT", DIR)).ok().and_then(|b| String::from_utf8(b).ok()).unwrap_or_default();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 8).then(|| CatalogEntry {
                name: f[0].to_string(),
                family: f[1].to_string(),
                style: f[2].to_string(),
                file: f[3].to_string(),
                size: f[4].parse().unwrap_or(0),
                sha256: f[5].to_string(),
                licence: f[6].to_string(),
                url: f[7].to_string(),
            })
        })
        .collect()
}

fn installed_file(file: &str) -> bool {
    crate::vfs::stat(&alloc::format!("{}/{}", DIR, file)).is_ok()
}

/// Shell `font ...`.
pub fn command(args: &[&str]) -> Vec<String> {
    use alloc::format;
    let rest = |i: usize| args.get(i..).map(|a| a.join(" ")).unwrap_or_default();
    match args.first().copied().unwrap_or("list") {
        "list" | "ls" => {
            let mut out = Vec::new();
            for id in 1..=count() as u16 {
                if let Some((family, bold, mono, file)) = info(id) {
                    out.push(format!(
                        "{:>3}  {:<24} {:<8} {:<10} {}",
                        id,
                        family,
                        if bold { "Bold" } else { "Regular" },
                        if mono { "monospace" } else { "" },
                        file
                    ));
                }
            }
            if out.is_empty() {
                out.push(format!("font: no fonts in {} -- the bitmap font is used", DIR));
            }
            let (n, bytes, hits, misses) = cache_stats();
            out.push(format!(
                "UI: {}   monospace: {}   cache: {} glyphs, {} KiB, {} hits / {} rasterized",
                ui_family().unwrap_or("bitmap"),
                mono_family().unwrap_or("bitmap"),
                n,
                bytes / 1024,
                hits,
                misses
            ));
            out
        }
        "ui" | "mono" if args.len() >= 2 => {
            let fam = rest(1);
            let pick = if fam.eq_ignore_ascii_case("bitmap") { None } else { Some(fam.as_str()) };
            let ok = if args[0] == "ui" { set_ui(pick) } else { set_mono(pick) };
            if ok {
                crate::wm::present_now();
                alloc::vec![format!("font: {} font is now {}", args[0], pick.unwrap_or("bitmap"))]
            } else {
                alloc::vec![format!("font: no installed family '{}' (font list / font catalog)", fam)]
            }
        }
        "catalog" | "search" => {
            let q = rest(1).to_ascii_lowercase();
            let all = catalog();
            let mut seen: Vec<&str> = Vec::new();
            let mut out = Vec::new();
            for e in all.iter().filter(|e| q.is_empty() || e.name.contains(&q) || e.family.to_ascii_lowercase().contains(&q)) {
                if seen.contains(&e.name.as_str()) {
                    continue;
                }
                seen.push(&e.name);
                let installed = find(&e.family, false).is_some();
                out.push(format!(
                    "{:<14} {:<24} {:<11} {:>5} KiB  {}",
                    e.name,
                    e.family,
                    e.licence,
                    e.size / 1024,
                    if installed { "terpasang" } else { "" }
                ));
            }
            if out.is_empty() {
                out.push("font: nothing in the catalog matches".into());
            }
            out
        }
        "info" if args.len() >= 2 => {
            let name = args[1].to_ascii_lowercase();
            let c: Vec<CatalogEntry> = catalog().into_iter().filter(|e| e.name == name).collect();
            if c.is_empty() {
                return alloc::vec![format!("font: '{}' is not in the catalog", name)];
            }
            let mut out = alloc::vec![format!("{} ({}), licence {}", c[0].family, c[0].name, c[0].licence)];
            for e in c.iter() {
                out.push(format!(
                    "  {:<8} {} {} KiB  sha256 {}..  {}",
                    e.style,
                    e.file,
                    e.size / 1024,
                    &e.sha256[..e.sha256.len().min(12)],
                    e.url
                ));
            }
            out
        }
        "install" | "pasang" if args.len() >= 2 => {
            let name = args[1].to_ascii_lowercase();
            let accept = args.iter().any(|a| *a == "--accept-license" || *a == "--setuju");
            let c: Vec<CatalogEntry> = catalog().into_iter().filter(|e| e.name == name).collect();
            let Some(first) = c.first() else {
                return alloc::vec![format!("font: '{}' is not in the catalog (font catalog)", name)];
            };
            if c.iter().all(|e| installed_file(&e.file)) {
                return alloc::vec![format!("font: {} is already installed", first.family)];
            }
            if !free_licence(&first.licence) && !accept {
                return alloc::vec![
                    format!("font: {} is licensed under {}, not a free font licence.", first.family, first.licence),
                    format!("      Read its terms ({}), then: font install {} --accept-license", first.url, name),
                ];
            }
            // HTTPS lives in the managed runtime: the Settings app downloads it
            match crate::app::spawn_with_args("msetting", &format!("--font-install {}", name)) {
                Ok(pid) => alloc::vec![format!(
                    "font: downloading {} over HTTPS (pid {}); it appears in 'font list' when done ('logs msetting')",
                    first.family, pid
                )],
                Err(e) => alloc::vec![format!("font: cannot start the downloader: {}", e)],
            }
        }
        "remove" | "hapus" if args.len() >= 2 => {
            let name = args[1];
            let mut files: Vec<String> = catalog().into_iter().filter(|e| e.name.eq_ignore_ascii_case(name)).map(|e| e.file).collect();
            if files.is_empty() {
                files.push(name.to_ascii_uppercase());
            }
            files.dedup();
            let mut out = Vec::new();
            for f in files {
                match remove_file(&f) {
                    Ok(n) => out.push(format!("font: removed {} ({} face(s))", f, n)),
                    Err(e) => out.push(format!("font: {}: {}", f, e)),
                }
            }
            crate::wm::present_now();
            out
        }
        // a file already on the device (bz font install --device pushes it):
        // font add SOURCE FILE.TTF [SHA256]
        "add" if args.len() >= 3 => {
            let (src, file) = (args[1], args[2].to_ascii_uppercase());
            let mut want = [0u8; 32];
            let sha = match args.get(3) {
                Some(h) if h.len() == 64 => {
                    for i in 0..32 {
                        match u8::from_str_radix(&h[i * 2..i * 2 + 2], 16) {
                            Ok(b) => want[i] = b,
                            Err(_) => return alloc::vec!["font: the SHA-256 is not hex".into()],
                        }
                    }
                    Some(&want)
                }
                Some(_) => return alloc::vec!["font: a SHA-256 has 64 hex digits".into()],
                None => None,
            };
            let Ok(data) = crate::vfs::read(src) else { return alloc::vec![format!("font: cannot read {}", src)] };
            match install(&file, &data, sha) {
                Ok(ids) => {
                    if !src.eq_ignore_ascii_case(&format!("{}/{}", DIR, file)) {
                        let _ = crate::vfs::remove(src);
                    }
                    crate::wm::present_now();
                    alloc::vec![format!("font: {} installed ({} new face(s))", file, ids.len())]
                }
                Err(e) => alloc::vec![format!("font: {}: {}", file, e)],
            }
        }
        "test" => {
            let ok = self_test();
            alloc::vec![format!("font self-test: {}", if ok { "ok" } else { "FAILED (see the log)" })]
        }
        _ => alloc::vec![
            "font [list]                     installed faces, the UI and monospace font".into(),
            "font ui <family|bitmap>         the desktop's font".into(),
            "font mono <family|bitmap>       the terminal's font".into(),
            "font catalog [text]             fonts that can be installed".into(),
            "font info <name>                a catalog entry".into(),
            "font install <name>             download (HTTPS), check SHA-256, install".into(),
            "font add <file> <NAME.TTF> [sha256]  install a font file already on the device".into(),
            "font remove <name|FILE.TTF>     uninstall".into(),
        ],
    }
}
