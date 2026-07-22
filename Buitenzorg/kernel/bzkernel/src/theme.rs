//! Theme engine (v0.10 "Buah"): design tokens + style parameters, with a
//! dark/light system theme plus 8 built-in styles (requirements.md §15). The
//! compositor and window manager read [`current`] and render title bars,
//! borders, shadows, and the desktop per the theme's style tokens — switchable
//! live at runtime (`theme <name>` / cycle).

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
};

/// All built-in themes in cycle order.
pub const THEMES: [Theme; 10] = [
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
];

static CURRENT: Mutex<usize> = Mutex::new(0);

pub fn current() -> Theme {
    THEMES[*CURRENT.lock()]
}

pub fn name() -> &'static str {
    current().name
}

pub fn is_dark() -> bool {
    current().dark
}

/// Set the theme by name; returns true if found.
pub fn set_by_name(name: &str) -> bool {
    if let Some(idx) = THEMES.iter().position(|t| t.name == name) {
        *CURRENT.lock() = idx;
        true
    } else {
        false
    }
}

/// Advance to the next theme (live cycle); returns its name.
pub fn cycle() -> &'static str {
    let mut cur = CURRENT.lock();
    *cur = (*cur + 1) % THEMES.len();
    THEMES[*cur].name
}

/// Toggle only between the dark and light system themes.
pub fn toggle() -> &'static str {
    let mut cur = CURRENT.lock();
    *cur = if *cur == 0 { 1 } else { 0 };
    THEMES[*cur].name
}

/// All theme names, for `theme list`.
pub fn names() -> &'static [Theme; 10] {
    &THEMES
}
