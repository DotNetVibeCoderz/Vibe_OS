//! Theme engine (v0.10 "Buah"): design tokens + style parameters, with a
//! dark/light system theme plus 8 built-in styles (requirements.md §15). The
//! compositor and window manager read [`current`] and render title bars,
//! borders, shadows, and the desktop per the theme's style tokens — switchable
//! live at runtime (`theme <name>` / cycle).

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::gfx::{rgb, Color};

/// Title-bar rendering style.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TitleStyle {
    /// Flat colored bar.
    Flat,
    /// 3D beveled bar (Classic Windows).
    Beveled,
    /// Short tab in the top-left (BeOS).
    Tab,
}

/// Drop-shadow style.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShadowStyle {
    None,
    /// Soft shadow offset a few px.
    Soft,
    /// Hard solid offset block (Neo Brutalism).
    HardOffset,
}

/// Design tokens + style parameters consumed by the renderer.
#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub dark: bool,
    pub desktop_top: Color,
    pub desktop_bottom: Color,
    pub gradient: bool, // false = flat desktop fill (desktop_top)
    pub win_body: Color,
    pub win_title_active: Color,
    pub win_title_inactive: Color,
    pub win_border: Color,
    pub border_thickness: i32,
    pub title_style: TitleStyle,
    pub shadow_style: ShadowStyle,
    pub text: Color,
    pub title_text: Color,
    pub taskbar_bg: Color,
    pub accent: Color,
    pub cursor: Color,
    pub shadow: Color,
    /// Opacity (0..=255) of the desktop shell's glass panels — taskbar, start
    /// menu, icon labels — blended over the wallpaper. 0 = solid (classic
    /// themes keep their opaque look).
    pub glass: u8,
}

// --- The system dark/light themes (Buitenzorg leaf palette) -----------------

const DARK: Theme = Theme {
    name: "dark",
    dark: true,
    desktop_top: rgb(0x10, 0x1A, 0x12),
    desktop_bottom: rgb(0x1E, 0x2E, 0x1C),
    gradient: true,
    win_body: rgb(0x14, 0x1C, 0x16),
    win_title_active: rgb(0x4F, 0xA3, 0x3F),
    win_title_inactive: rgb(0x2C, 0x3A, 0x2C),
    win_border: rgb(0x0A, 0x0F, 0x0A),
    border_thickness: 2,
    title_style: TitleStyle::Flat,
    shadow_style: ShadowStyle::Soft,
    text: rgb(0xC8, 0xE9, 0xB0),
    title_text: rgb(0x0B, 0x12, 0x0B),
    taskbar_bg: rgb(0x0A, 0x14, 0x0C),
    accent: rgb(0x6F, 0xC1, 0x4E),
    cursor: rgb(0xF0, 0xFF, 0xE0),
    shadow: rgb(0x05, 0x08, 0x05),
    glass: 0,
};

const LIGHT: Theme = Theme {
    name: "light",
    dark: false,
    desktop_top: rgb(0xE8, 0xF0, 0xDE),
    desktop_bottom: rgb(0xCB, 0xDD, 0xC0),
    gradient: true,
    win_body: rgb(0xF6, 0xFA, 0xF0),
    win_title_active: rgb(0x4F, 0xA3, 0x3F),
    win_title_inactive: rgb(0xB8, 0xCB, 0xAE),
    win_border: rgb(0x8A, 0x9E, 0x82),
    border_thickness: 1,
    title_style: TitleStyle::Flat,
    shadow_style: ShadowStyle::Soft,
    text: rgb(0x1C, 0x2A, 0x18),
    title_text: rgb(0xF6, 0xFA, 0xF0),
    taskbar_bg: rgb(0xD6, 0xE4, 0xCC),
    accent: rgb(0x2F, 0x7A, 0x24),
    cursor: rgb(0x14, 0x20, 0x10),
    shadow: rgb(0xA8, 0xB8, 0x9E),
    glass: 0,
};

// --- The 8 built-in styles (requirements.md §15) ----------------------------

const NEO_BRUTALISM: Theme = Theme {
    name: "neo-brutalism",
    dark: false,
    desktop_top: rgb(0xF2, 0xE9, 0x2C), // bold yellow
    desktop_bottom: rgb(0xF2, 0xE9, 0x2C),
    gradient: false,
    win_body: rgb(0xFF, 0xFF, 0xFF),
    win_title_active: rgb(0xFF, 0x4D, 0x4D), // bold red
    win_title_inactive: rgb(0xD0, 0xD0, 0xD0),
    win_border: rgb(0x00, 0x00, 0x00),
    border_thickness: 4,
    title_style: TitleStyle::Flat,
    shadow_style: ShadowStyle::HardOffset,
    text: rgb(0x00, 0x00, 0x00),
    title_text: rgb(0x00, 0x00, 0x00),
    taskbar_bg: rgb(0x00, 0x00, 0x00),
    accent: rgb(0x2E, 0x5B, 0xFF), // bold blue
    cursor: rgb(0x00, 0x00, 0x00),
    shadow: rgb(0x00, 0x00, 0x00),
    glass: 0,
};

const CLEAN: Theme = Theme {
    name: "clean",
    dark: false,
    desktop_top: rgb(0xFB, 0xFB, 0xFA),
    desktop_bottom: rgb(0xF0, 0xF0, 0xEE),
    gradient: true,
    win_body: rgb(0xFF, 0xFF, 0xFF),
    win_title_active: rgb(0xFF, 0xFF, 0xFF),
    win_title_inactive: rgb(0xFA, 0xFA, 0xFA),
    win_border: rgb(0xE2, 0xE2, 0xE0),
    border_thickness: 1,
    title_style: TitleStyle::Flat,
    shadow_style: ShadowStyle::None,
    text: rgb(0x33, 0x33, 0x33),
    title_text: rgb(0x22, 0x22, 0x22),
    taskbar_bg: rgb(0xFF, 0xFF, 0xFF),
    accent: rgb(0x4A, 0x90, 0xE2),
    cursor: rgb(0x33, 0x33, 0x33),
    shadow: rgb(0xE8, 0xE8, 0xE6),
    glass: 0,
};

const MATERIAL: Theme = Theme {
    name: "material",
    dark: false,
    desktop_top: rgb(0x3F, 0x51, 0xB5), // indigo
    desktop_bottom: rgb(0x1A, 0x23, 0x7E),
    gradient: true,
    win_body: rgb(0xFF, 0xFF, 0xFF),
    win_title_active: rgb(0x67, 0x3A, 0xB7), // deep purple
    win_title_inactive: rgb(0xB3, 0x9D, 0xDB),
    win_border: rgb(0xCF, 0xCF, 0xCF),
    border_thickness: 1,
    title_style: TitleStyle::Flat,
    shadow_style: ShadowStyle::Soft,
    text: rgb(0x21, 0x21, 0x21),
    title_text: rgb(0xFF, 0xFF, 0xFF),
    taskbar_bg: rgb(0x31, 0x2A, 0x5E),
    accent: rgb(0xFF, 0x40, 0x81), // pink accent
    cursor: rgb(0x21, 0x21, 0x21),
    shadow: rgb(0x18, 0x18, 0x30),
    glass: 0,
};

const BENTO: Theme = Theme {
    name: "bento",
    dark: true,
    desktop_top: rgb(0x1B, 0x1B, 0x1F),
    desktop_bottom: rgb(0x24, 0x24, 0x2A),
    gradient: true,
    win_body: rgb(0x2A, 0x2A, 0x30),
    win_title_active: rgb(0xF5, 0xA6, 0x23), // warm amber
    win_title_inactive: rgb(0x3A, 0x3A, 0x42),
    win_border: rgb(0x3A, 0x3A, 0x42),
    border_thickness: 2,
    title_style: TitleStyle::Flat,
    shadow_style: ShadowStyle::Soft,
    text: rgb(0xE4, 0xE4, 0xEA),
    title_text: rgb(0x1B, 0x1B, 0x1F),
    taskbar_bg: rgb(0x16, 0x16, 0x1A),
    accent: rgb(0x4E, 0xD1, 0xA1),
    cursor: rgb(0xFF, 0xFF, 0xFF),
    shadow: rgb(0x0E, 0x0E, 0x12),
    glass: 0,
};

const CLASSIC_LINUX: Theme = Theme {
    name: "classic-linux",
    dark: false,
    desktop_top: rgb(0x4E, 0x6E, 0x8E), // steel blue CDE
    desktop_bottom: rgb(0x4E, 0x6E, 0x8E),
    gradient: false,
    win_body: rgb(0xC0, 0xC0, 0xB8),
    win_title_active: rgb(0x6A, 0x6A, 0x8A),
    win_title_inactive: rgb(0x90, 0x90, 0x98),
    win_border: rgb(0x60, 0x60, 0x60),
    border_thickness: 2,
    title_style: TitleStyle::Beveled,
    shadow_style: ShadowStyle::None,
    text: rgb(0x10, 0x10, 0x10),
    title_text: rgb(0xF0, 0xF0, 0xF0),
    taskbar_bg: rgb(0xA8, 0xA8, 0xA0),
    accent: rgb(0x40, 0x60, 0x80),
    cursor: rgb(0x00, 0x00, 0x00),
    shadow: rgb(0x50, 0x50, 0x50),
    glass: 0,
};

const CLASSIC_WINDOWS: Theme = Theme {
    name: "classic-windows",
    dark: false,
    desktop_top: rgb(0x00, 0x80, 0x80), // teal
    desktop_bottom: rgb(0x00, 0x80, 0x80),
    gradient: false,
    win_body: rgb(0xC0, 0xC0, 0xC0),
    win_title_active: rgb(0x00, 0x00, 0x80), // navy
    win_title_inactive: rgb(0x80, 0x80, 0x80),
    win_border: rgb(0x80, 0x80, 0x80),
    border_thickness: 2,
    title_style: TitleStyle::Beveled,
    shadow_style: ShadowStyle::None,
    text: rgb(0x00, 0x00, 0x00),
    title_text: rgb(0xFF, 0xFF, 0xFF),
    taskbar_bg: rgb(0xC0, 0xC0, 0xC0),
    accent: rgb(0x00, 0x00, 0x80),
    cursor: rgb(0x00, 0x00, 0x00),
    shadow: rgb(0x40, 0x40, 0x40),
    glass: 0,
};

const SUN: Theme = Theme {
    name: "sun",
    dark: false,
    desktop_top: rgb(0x63, 0x5A, 0x7A), // purple-gray CDE
    desktop_bottom: rgb(0x63, 0x5A, 0x7A),
    gradient: false,
    win_body: rgb(0xB8, 0xB0, 0xC8),
    win_title_active: rgb(0x4A, 0x3F, 0x6A),
    win_title_inactive: rgb(0x8A, 0x82, 0xA0),
    win_border: rgb(0x38, 0x30, 0x50),
    border_thickness: 3,
    title_style: TitleStyle::Beveled,
    shadow_style: ShadowStyle::None,
    text: rgb(0x18, 0x14, 0x24),
    title_text: rgb(0xF0, 0xEC, 0xF8),
    taskbar_bg: rgb(0x4A, 0x42, 0x60),
    accent: rgb(0xC8, 0x9B, 0x3C),
    cursor: rgb(0x10, 0x0C, 0x18),
    shadow: rgb(0x30, 0x28, 0x44),
    glass: 0,
};

const BEOS: Theme = Theme {
    name: "beos",
    dark: false,
    desktop_top: rgb(0x9C, 0xB0, 0xC8), // cool gray-blue
    desktop_bottom: rgb(0x9C, 0xB0, 0xC8),
    gradient: false,
    win_body: rgb(0xDD, 0xDD, 0xDD),
    win_title_active: rgb(0xFF, 0xD5, 0x1E), // signature yellow tab
    win_title_inactive: rgb(0xCF, 0xCF, 0xCF),
    win_border: rgb(0x60, 0x60, 0x60),
    border_thickness: 1,
    title_style: TitleStyle::Tab,
    shadow_style: ShadowStyle::Soft,
    text: rgb(0x10, 0x10, 0x10),
    title_text: rgb(0x20, 0x18, 0x00),
    taskbar_bg: rgb(0xC8, 0xC8, 0xC8),
    accent: rgb(0xFF, 0xD5, 0x1E),
    cursor: rgb(0x00, 0x00, 0x00),
    shadow: rgb(0x70, 0x80, 0x90),
    glass: 0,
};

/// All built-in themes in cycle order.
/// "Kebun" — the default look, taken from the Bogor Botanical Gardens wallpaper:
/// deep-moss glass panels, mist text, and the pink of the garden's lotus pond
/// as the one accent (selection, the active window, the Start leaf).
const KEBUN: Theme = Theme {
    name: "kebun",
    dark: true,
    desktop_top: rgb(0x0F, 0x24, 0x1B),
    desktop_bottom: rgb(0x1B, 0x3A, 0x2B),
    gradient: true,
    win_body: rgb(0x10, 0x20, 0x1A),
    win_title_active: rgb(0x1E, 0x3B, 0x2E),
    win_title_inactive: rgb(0x15, 0x29, 0x1F),
    win_border: rgb(0x2E, 0x54, 0x42),
    border_thickness: 1,
    title_style: TitleStyle::Flat,
    shadow_style: ShadowStyle::Soft,
    text: rgb(0xE6, 0xEF, 0xE7),
    title_text: rgb(0xE6, 0xEF, 0xE7),
    taskbar_bg: rgb(0x0F, 0x24, 0x1B),
    accent: rgb(0xEE, 0x8F, 0xB0),
    cursor: rgb(0xF0, 0xFF, 0xE0),
    shadow: rgb(0x05, 0x0D, 0x09),
    glass: 205,
};

/// Secondary text on the kebun panels ("lichen") and the time ("pollen").
pub const LICHEN: Color = rgb(0x9D, 0xB8, 0xA8);
pub const POLLEN: Color = rgb(0xF3, 0xC9, 0x69);

/// Index of the default theme in `THEMES`.
const DEFAULT: usize = 10;

pub const THEMES: [Theme; 11] = [
    DARK,
    LIGHT,
    NEO_BRUTALISM,
    CLEAN,
    MATERIAL,
    BENTO,
    CLASSIC_LINUX,
    CLASSIC_WINDOWS,
    SUN,
    BEOS,
    KEBUN,
];

static CURRENT: Mutex<usize> = Mutex::new(DEFAULT);
/// Themes from `/disk/THEMES/*.THM` (third-party theme packages), after the
/// built-in ones: index `THEMES.len() + i`.
static EXTRA: Mutex<Vec<Theme>> = Mutex::new(Vec::new());

const DIR: &str = "/disk/THEMES";

pub fn current() -> Theme {
    let i = *CURRENT.lock();
    if i < THEMES.len() {
        return THEMES[i];
    }
    EXTRA.lock().get(i - THEMES.len()).copied().unwrap_or(THEMES[DEFAULT])
}

pub fn name() -> &'static str {
    current().name
}

pub fn is_dark() -> bool {
    current().dark
}

/// Set the theme by name; returns true if found.
pub fn set_by_name(name: &str) -> bool {
    if let Some(idx) = all().iter().position(|t| t.name.eq_ignore_ascii_case(name)) {
        *CURRENT.lock() = idx;
        true
    } else {
        false
    }
}

/// Advance to the next theme (live cycle); returns its name.
pub fn cycle() -> &'static str {
    let n = THEMES.len() + EXTRA.lock().len();
    let next = (*CURRENT.lock() + 1) % n;
    *CURRENT.lock() = next;
    current().name
}

/// Toggle between light and the default dark-ish look (light -> kebun,
/// anything else -> light).
pub fn toggle() -> &'static str {
    let mut cur = CURRENT.lock();
    *cur = if *cur == 1 { DEFAULT } else { 1 };
    THEMES[*cur].name
}

/// Return to the default theme ("kebun").
pub fn set_default() {
    *CURRENT.lock() = DEFAULT;
}

/// Every theme: the built-in ones, then the installed packages.
pub fn all() -> Vec<Theme> {
    let mut v: Vec<Theme> = THEMES.to_vec();
    v.extend(EXTRA.lock().iter().copied());
    v
}

/// All themes, for `theme list` and the THEME setting.
pub fn names() -> Vec<Theme> {
    all()
}

/// Whether `name` is a theme package (not built in).
pub fn is_package(name: &str) -> bool {
    !THEMES.iter().any(|t| t.name == name) && EXTRA.lock().iter().any(|t| t.name == name)
}

fn color(v: &str) -> Option<Color> {
    let h = v.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some(rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

/// Parse a `.THM` theme package: `key=value` lines (`#` comments), colors as
/// `#RRGGBB`; keys left out come from `base` (a built-in theme, default
/// kebun). None without a valid `name` (letters, digits, `-`, <= 24).
pub fn parse(text: &str) -> Option<Theme> {
    let pairs: Vec<(&str, &str)> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.trim(), v.trim())))
        .collect();
    let get = |k: &str| pairs.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
    let name = get("name")?.to_ascii_lowercase();
    if name.is_empty() || name.len() > 24 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return None;
    }
    let mut t = get("base").and_then(|b| THEMES.iter().find(|t| t.name == b)).copied().unwrap_or(THEMES[DEFAULT]);
    for (k, v) in &pairs {
        let flag = *v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes");
        match *k {
            "dark" => t.dark = flag,
            "gradient" => t.gradient = flag,
            "border" => t.border_thickness = v.parse::<i32>().unwrap_or(t.border_thickness).clamp(0, 6),
            "glass" => t.glass = v.parse::<u32>().unwrap_or(t.glass as u32).min(255) as u8,
            "title_style" => {
                t.title_style = match *v {
                    "beveled" => TitleStyle::Beveled,
                    "tab" => TitleStyle::Tab,
                    _ => TitleStyle::Flat,
                }
            }
            "shadow_style" => {
                t.shadow_style = match *v {
                    "none" => ShadowStyle::None,
                    "hard" => ShadowStyle::HardOffset,
                    _ => ShadowStyle::Soft,
                }
            }
            _ => {
                let Some(c) = color(v) else { continue };
                match *k {
                    "desktop_top" => t.desktop_top = c,
                    "desktop_bottom" => t.desktop_bottom = c,
                    "win_body" => t.win_body = c,
                    "win_title_active" => t.win_title_active = c,
                    "win_title_inactive" => t.win_title_inactive = c,
                    "win_border" => t.win_border = c,
                    "text" => t.text = c,
                    "title_text" => t.title_text = c,
                    "taskbar_bg" => t.taskbar_bg = c,
                    "accent" => t.accent = c,
                    "cursor" => t.cursor = c,
                    "shadow" => t.shadow = c,
                    _ => {}
                }
            }
        }
    }
    // the name lives as long as the kernel (a few bytes per package)
    t.name = alloc::boxed::Box::leak(name.into_boxed_str());
    Some(t)
}

/// (Re)load the theme packages in /disk/THEMES. A package with a built-in
/// theme's name is skipped. Returns how many packages are registered.
pub fn load_packages() -> usize {
    let entries = crate::vfs::list_entries(DIR).unwrap_or_default();
    let current_name = name();
    let mut found: Vec<Theme> = Vec::new();
    for e in entries.iter().filter(|e| !e.is_dir && e.name.to_ascii_uppercase().ends_with(".THM")) {
        let path = alloc::format!("{}/{}", DIR, e.name);
        let Some(t) = crate::vfs::read(&path).ok().and_then(|b| String::from_utf8(b).ok()).and_then(|s| parse(&s)) else {
            crate::println!("[theme] {}: not a theme package", path);
            continue;
        };
        if THEMES.iter().any(|b| b.name == t.name) || found.iter().any(|f| f.name == t.name) {
            continue;
        }
        found.push(t);
    }
    let n = found.len();
    // keep the current theme selected if it is still there
    let still = found.iter().position(|t| t.name == current_name);
    {
        let mut extra = EXTRA.lock();
        let mut cur = CURRENT.lock();
        if *cur >= THEMES.len() {
            *cur = still.map_or(DEFAULT, |i| THEMES.len() + i);
        }
        *extra = found;
    }
    if n > 0 {
        crate::println!("[theme] {} theme package(s) from {}", n, DIR);
    }
    n
}

/// What `/disk/THEMES` looks like now: names, sizes, first clusters and
/// change times of its `.THM` files (a rewrite gets a new cluster chain, so
/// an edit within FAT's 2-second clock still shows).
fn packages_signature() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    for e in crate::vfs::list_entries(DIR).unwrap_or_default() {
        if e.is_dir || !e.name.to_ascii_uppercase().ends_with(".THM") {
            continue;
        }
        for b in e.name.bytes() {
            mix(b as u64);
        }
        mix(e.size as u64);
        mix(e.cluster as u64);
        mix((e.date as u64) << 16 | e.time as u64);
    }
    h
}

static WATCHED: spin::Mutex<Option<u64>> = spin::Mutex::new(None);

/// Hot reload (the desktop loop calls it every ~2 s): when a theme package
/// was added, removed or edited, the packages are read again and the
/// current theme -- if it is one of them -- shows the new values at once.
/// The first call only records the state. True when something was reloaded.
pub fn hot_reload() -> bool {
    let sig = packages_signature();
    let mut w = WATCHED.lock();
    let changed = matches!(*w, Some(old) if old != sig);
    *w = Some(sig);
    drop(w);
    if changed {
        let n = load_packages();
        crate::println!("[theme] {} changed: {} package(s) reloaded, current theme '{}'", DIR, n, name());
    }
    changed
}

/// A package written, edited and removed on `/disk/THEMES` is seen by
/// `hot_reload` each time (the edit keeps the file size: the new cluster
/// chain must give it away). None = failed.
fn hot_reload_test() -> Option<&'static str> {
    if crate::vfs::list_entries(DIR).is_err() {
        return Some("no /disk/THEMES: hot reload not tried");
    }
    let path = alloc::format!("{}/UJIPANAS.THM", DIR);
    let accent_of = || all().iter().find(|t| t.name == "uji-panas").map(|t| t.accent);
    hot_reload(); // record the folder as it is
    let wrote = crate::vfs::write(&path, b"name=uji-panas
base=kebun
accent=#112233
").is_ok();
    let added = wrote && hot_reload() && accent_of() == Some(rgb(0x11, 0x22, 0x33));
    let rewrote = crate::vfs::write(&path, b"name=uji-panas
base=kebun
accent=#445566
").is_ok();
    let edited = rewrote && hot_reload() && accent_of() == Some(rgb(0x44, 0x55, 0x66));
    let _ = crate::vfs::remove(&path);
    let removed = hot_reload() && accent_of().is_none();
    let quiet = !hot_reload();
    if added && edited && removed && quiet {
        Some("hot reload: add / edit / remove seen")
    } else {
        crate::println!("[theme] hot reload test: added {} edited {} removed {} quiet {}", added, edited, removed, quiet);
        None
    }
}

/// Self-test: a package parses (base + overrides), registers, can be
/// selected, and a broken one is refused.
pub fn self_test() -> bool {
    let sample = "name=uji-tema\nbase=light\naccent=#123456\nglass=300\ntitle_style=tab\n";
    let parsed = parse(sample)
        .filter(|t| t.accent == rgb(0x12, 0x34, 0x56) && t.glass == 255 && !t.dark && t.title_style == TitleStyle::Tab);
    let refused = parse("name=bad name\n").is_none() && parse("accent=#000000\n").is_none();
    let loaded = load_packages();
    let before = name();
    let pkg = all().iter().find(|t| is_package(t.name)).map(|t| t.name);
    let switched = pkg.is_none_or(|n| set_by_name(n) && name() == n);
    set_by_name(before);
    let hot = hot_reload_test();
    let ok = parsed.is_some() && refused && switched && hot.is_some();
    if ok {
        crate::println!(
            "[kernel] MILESTONE: THEME PACKAGES OK ({} package(s) in {}, base + overrides parsed, broken files refused, {})",
            loaded,
            DIR,
            hot.unwrap_or("")
        );
    } else {
        crate::println!("[theme] self-test: parsed {} refused {} switched {} hot reload {}", parsed.is_some(), refused, switched, hot.is_some());
    }
    ok
}
