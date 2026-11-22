//! The desktop's personal settings for applications (the SETTING syscall):
//! theme, wallpaper, screensaver and its delay, animations, cursor size --
//! what the shell's `theme` / `bg` / `saver` / `anim` / `cursor` commands
//! change, so a Settings app can show and change the same things -- plus the
//! screen resolution (next start) and the UI text scale.
//!
//! **Kept on disk:** `/disk/DESKTOP.CFG` holds the theme, wallpaper,
//! screensaver + delay, animations, cursor, text scale and resolution.
//! `autosave` (desktop loop, every ~3 s) writes it when something changed --
//! whoever changed it (Settings, the shell's `theme` / `bg`, the Start menu);
//! `restore` applies it once the desktop's background start-up has loaded the
//! wallpapers; `boot_resolution` is read before the window system exists.

use alloc::format;
use alloc::string::String;
use bz_abi::setting_key as key;
use bz_abi::syserr::INVAL;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;

const CONFIG: &str = "/disk/DESKTOP.CFG";

/// Resolutions offered (std VGA can do all of them; the kernel heap decides
/// at start-up which fit).
pub const RESOLUTIONS: [(usize, usize, &str); 8] = [
    (800, 600, "800x600"),
    (1024, 768, "1024x768"),
    (1280, 720, "1280x720"),
    (1280, 800, "1280x800"),
    (1366, 768, "1366x768"),
    (1440, 900, "1440x900"),
    (1600, 900, "1600x900"),
    (1920, 1080, "1920x1080"),
];
const SCALE_NAMES: [&str; 3] = ["100 %", "110 %", "125 %"];

/// The resolution asked for (index), usize::MAX = the firmware's.
static WANTED_RES: AtomicUsize = AtomicUsize::new(usize::MAX);
/// `restore` ran (until then nothing is saved: the defaults must not
/// overwrite the user's file).
static RESTORED: AtomicBool = AtomicBool::new(false);
static LAST_SAVED: Mutex<Option<String>> = Mutex::new(None);

fn current_resolution() -> usize {
    let w = WANTED_RES.load(Ordering::Relaxed);
    if w != usize::MAX {
        return w;
    }
    let (sw, sh) = crate::framebuffer::dimensions().unwrap_or((0, 0));
    RESOLUTIONS.iter().position(|(w, h, _)| *w == sw && *h == sh).unwrap_or(2)
}

/// What DESKTOP.CFG holds now.
fn snapshot() -> String {
    let (anim, _) = crate::wm::options();
    let mut s = format!(
        "theme={}\r\nwallpaper={}\r\nsaver={}\r\ndelay={}\r\nanim={}\r\ncursor={}\r\nscale={}\r\n",
        crate::theme::name(),
        crate::wallpaper::label(),
        crate::screensaver::name(),
        crate::screensaver::timeout_seconds(),
        anim as u8,
        crate::wm::cursor_scale(),
        crate::font::scale()
    );
    if let Some(p) = crate::icons::pack_name() {
        s.push_str(&format!("icons={}\r\n", p));
    }
    let r = WANTED_RES.load(Ordering::Relaxed);
    if let Some((_, _, n)) = RESOLUTIONS.get(r) {
        s.push_str(&format!("resolution={}\r\n", n));
    }
    s
}

fn config_lines() -> alloc::vec::Vec<(String, String)> {
    let Ok(bytes) = crate::vfs::read(CONFIG) else { return alloc::vec::Vec::new() };
    let text = String::from_utf8_lossy(&bytes);
    text.lines()
        .filter_map(|l| l.trim().split_once('=').map(|(k, v)| (String::from(k.trim()), String::from(v.trim()))))
        .collect()
}

/// The resolution DESKTOP.CFG asks for (read at start-up, before the window
/// system), also remembered as the setting's value.
pub fn boot_resolution() -> Option<(usize, usize)> {
    let (_, v) = config_lines().into_iter().find(|(k, _)| k == "resolution")?;
    let i = RESOLUTIONS.iter().position(|(_, _, n)| *n == v)?;
    WANTED_RES.store(i, Ordering::Relaxed);
    Some((RESOLUTIONS[i].0, RESOLUTIONS[i].1))
}

/// Apply DESKTOP.CFG (once, on a normal start, after the wallpapers can be
/// loaded). Returns how many values it applied.
pub fn restore() -> usize {
    if RESTORED.swap(true, Ordering::Relaxed) {
        return 0;
    }
    let mut n = 0;
    for (k, v) in config_lines() {
        let num = v.parse::<u64>().ok();
        let ok = match k.as_str() {
            "theme" => crate::theme::set_by_name(&v),
            "wallpaper" => crate::wallpaper::set_official(&v).is_ok() || crate::wallpaper::set_builtin(&v),
            "saver" => crate::screensaver::set(&v),
            "delay" => num.is_some_and(|d| (5..=3600).contains(&d)) && {
                crate::screensaver::set_timeout_seconds(num.unwrap_or(12));
                true
            },
            "anim" => {
                let (_, rounded) = crate::wm::options();
                crate::wm::set_options(v == "1", rounded);
                true
            }
            "cursor" => {
                crate::wm::set_cursor_scale(if v == "2" { 2 } else { 1 });
                true
            }
            "scale" => num.is_some_and(|p| crate::font::set_scale(p as u32)),
            "icons" => crate::icons::set_pack(Some(&v)).is_ok(),
            _ => false,
        };
        n += ok as usize;
    }
    *LAST_SAVED.lock() = Some(snapshot());
    if n > 0 {
        crate::println!("[settings] {} desktop setting(s) restored from {}", n, CONFIG);
        crate::wm::present_now();
    }
    n
}

/// Write DESKTOP.CFG when the settings changed since the last write (only
/// after `restore`, and only when /disk is mounted).
pub fn autosave() {
    if !RESTORED.load(Ordering::Relaxed) || !crate::vfs::mounts().iter().any(|m| m == "/disk") {
        return;
    }
    let snap = snapshot();
    let mut last = LAST_SAVED.lock();
    if last.as_deref() == Some(snap.as_str()) {
        return;
    }
    if crate::vfs::write(CONFIG, snap.as_bytes()).is_ok() {
        *last = Some(snap);
    }
}

/// Self-test boot: a change is saved, a restore reads it back, and the file
/// is then put back as it was.
pub fn self_test() -> bool {
    if !crate::vfs::mounts().iter().any(|m| m == "/disk") {
        crate::println!("[kernel] MILESTONE: SETTINGS SAVED OK (no /disk: nothing to keep)");
        return true;
    }
    let before = crate::vfs::read(CONFIG).ok();
    let (saver0, delay0, scale0) = (crate::screensaver::name(), crate::screensaver::timeout_seconds(), crate::font::scale());
    RESTORED.store(true, Ordering::Relaxed);
    *LAST_SAVED.lock() = None;
    crate::screensaver::set_timeout_seconds(77);
    crate::font::set_scale(110);
    autosave();
    let text = crate::vfs::read(CONFIG).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let saved = text.contains("delay=77") && text.contains("scale=110") && text.contains("theme=");
    crate::screensaver::set_timeout_seconds(delay0);
    crate::font::set_scale(scale0);
    RESTORED.store(false, Ordering::Relaxed);
    let applied = restore();
    let restored = crate::screensaver::timeout_seconds() == 77 && crate::font::scale() == 110;
    // put everything back (the user's file, the values of this boot)
    crate::screensaver::set_timeout_seconds(delay0);
    crate::screensaver::set(saver0);
    crate::font::set_scale(scale0);
    match before {
        Some(b) => {
            let _ = crate::vfs::write(CONFIG, &b);
        }
        None => {
            let _ = crate::vfs::remove(CONFIG);
        }
    }
    RESTORED.store(false, Ordering::Relaxed);
    *LAST_SAVED.lock() = None;
    let res = label(key::RESOLUTION, 2, false) == Some("1280x720") && count(key::UI_SCALE) == 3;
    let ok = saved && applied >= 2 && restored && res;
    if ok {
        crate::println!("[kernel] MILESTONE: SETTINGS SAVED OK (desktop settings written to {}, restored, resolution + text scale listed)", CONFIG);
    } else {
        crate::println!("[settings] self-test: saved {} applied {} restored {} lists {}", saved, applied, restored, res);
    }
    ok
}

/// Number of choices of a list setting (themes, wallpapers, screensavers + "off").
pub fn count(k: u64) -> u64 {
    match k {
        key::THEME => crate::theme::names().len() as u64,
        key::WALLPAPER => crate::wallpaper::OFFICIAL.len() as u64,
        key::SCREENSAVER => crate::screensaver::NAMES.len() as u64 + 1,
        key::ANIMATIONS | key::LARGE_CURSOR | key::CLOCK_24H => 2,
        key::TIMEZONE => crate::clock::ZONES.len() as u64,
        key::LANGUAGE => crate::clock::LANGUAGES.len() as u64,
        key::UI_FONT => 1 + crate::font::families(false).len() as u64,
        key::MONO_FONT => 1 + crate::font::families(true).len() as u64,
        key::RESOLUTION => RESOLUTIONS.len() as u64,
        key::UI_SCALE => crate::font::SCALES.len() as u64,
        key::ICON_PACK => 1 + crate::icons::packs().len() as u64,
        _ => 0,
    }
}

/// Current value: an index for list settings, seconds for the delay, 0/1 for switches.
pub fn get(k: u64) -> u64 {
    match k {
        key::THEME => {
            let cur = crate::theme::name();
            crate::theme::names().iter().position(|t| t.name == cur).map_or(INVAL, |i| i as u64)
        }
        key::WALLPAPER => {
            let cur = crate::wallpaper::label();
            crate::wallpaper::OFFICIAL.iter().position(|(n, _, _)| *n == cur).map_or(INVAL, |i| i as u64)
        }
        key::SCREENSAVER => {
            let cur = crate::screensaver::name();
            crate::screensaver::NAMES.iter().position(|n| *n == cur).unwrap_or(crate::screensaver::NAMES.len()) as u64
        }
        key::SCREENSAVER_DELAY => crate::screensaver::timeout_seconds(),
        key::ANIMATIONS => crate::wm::options().0 as u64,
        key::LARGE_CURSOR => (crate::wm::cursor_scale() > 1) as u64,
        key::TIMEZONE => crate::clock::zone() as u64,
        key::LANGUAGE => crate::clock::language() as u64,
        key::CLOCK_24H => crate::clock::h24() as u64,
        key::CLOCK => crate::clock::local_unix().max(0) as u64,
        key::UI_FONT => font_index(crate::font::ui_family(), false),
        key::MONO_FONT => font_index(crate::font::mono_family(), true),
        key::RESOLUTION => current_resolution() as u64,
        key::UI_SCALE => crate::font::SCALES.iter().position(|p| *p == crate::font::scale()).unwrap_or(0) as u64,
        key::ICON_PACK => match crate::icons::pack_name() {
            None => 0,
            Some(n) => crate::icons::packs().iter().position(|p| *p == n).map_or(0, |i| i as u64 + 1),
        },
        _ => INVAL,
    }
}

/// Change a setting; returns 0, or INVAL for an unknown key / value.
pub fn set(k: u64, v: u64) -> u64 {
    let ok = match k {
        key::THEME => match crate::theme::names().get(v as usize) {
            Some(t) => crate::theme::set_by_name(t.name),
            None => false,
        },
        key::WALLPAPER => match crate::wallpaper::OFFICIAL.get(v as usize) {
            Some((n, _, _)) => crate::wallpaper::set_official(n).is_ok(),
            None => false,
        },
        key::SCREENSAVER => match crate::screensaver::NAMES.get(v as usize) {
            Some(n) => crate::screensaver::set(n),
            None if v as usize == crate::screensaver::NAMES.len() => crate::screensaver::set("off"),
            None => false,
        },
        key::SCREENSAVER_DELAY if (5..=3600).contains(&v) => {
            crate::screensaver::set_timeout_seconds(v);
            true
        }
        key::ANIMATIONS if v <= 1 => {
            let (_, rounded) = crate::wm::options();
            crate::wm::set_options(v == 1, rounded);
            true
        }
        key::LARGE_CURSOR if v <= 1 => {
            crate::wm::set_cursor_scale(if v == 1 { 2 } else { 1 });
            true
        }
        key::TIMEZONE => crate::clock::set_zone(v as usize),
        key::LANGUAGE => crate::clock::set_language(v as usize),
        key::CLOCK_24H if v <= 1 => {
            crate::clock::set_h24(v == 1);
            true
        }
        // 2000-01-01 .. 2099-12-31
        key::CLOCK if (946_684_800..4_102_444_800).contains(&v) => {
            crate::clock::set_local(v as i64);
            true
        }
        key::UI_FONT if v == 0 => crate::font::set_ui(None),
        key::UI_FONT => crate::font::families(false).get(v as usize - 1).is_some_and(|f| crate::font::set_ui(Some(f))),
        key::MONO_FONT if v == 0 => crate::font::set_mono(None),
        key::MONO_FONT => crate::font::families(true).get(v as usize - 1).is_some_and(|f| crate::font::set_mono(Some(f))),
        // the next start switches the display (std VGA); saved at once
        key::RESOLUTION if (v as usize) < RESOLUTIONS.len() => {
            WANTED_RES.store(v as usize, Ordering::Relaxed);
            autosave();
            true
        }
        key::UI_SCALE => crate::font::SCALES.get(v as usize).is_some_and(|p| crate::font::set_scale(*p)),
        key::ICON_PACK if v == 0 => crate::icons::set_pack(None).is_ok(),
        key::ICON_PACK => crate::icons::packs().get(v as usize - 1).is_some_and(|p| crate::icons::set_pack(Some(p)).is_ok()),
        _ => false,
    };
    if ok {
        crate::wm::present_now(); // the desktop shows the change at once
        0
    } else {
        INVAL
    }
}

/// Name (`detail` false) or description (`detail` true) of choice `index`.
pub fn label(k: u64, index: usize, detail: bool) -> Option<&'static str> {
    match k {
        key::THEME => crate::theme::names().get(index).map(|t| t.name),
        key::WALLPAPER => crate::wallpaper::OFFICIAL.get(index).map(|(n, _, d)| if detail { *d } else { *n }),
        key::SCREENSAVER => {
            if index == crate::screensaver::NAMES.len() {
                Some("off")
            } else {
                crate::screensaver::NAMES.get(index).copied()
            }
        }
        key::TIMEZONE => crate::clock::ZONES.get(index).map(|(n, d, _)| if detail { *d } else { *n }),
        key::LANGUAGE => crate::clock::LANGUAGES.get(index).map(|(c, n)| if detail { *c } else { *n }),
        key::RESOLUTION => RESOLUTIONS.get(index).map(|(_, _, n)| if detail { "berlaku setelah komputer dimulai ulang" } else { *n }),
        key::UI_SCALE => SCALE_NAMES.get(index).map(|n| if detail { "ukuran teks desktop dan aplikasi" } else { *n }),
        key::ICON_PACK if index == 0 => Some(if detail { "ikon asli Buitenzorg" } else { "bawaan" }),
        key::ICON_PACK => {
            let packs = crate::icons::packs();
            let name = packs.get(index - 1)?;
            // the label must outlive this call: leak each pack name once
            static NAMES: Mutex<alloc::vec::Vec<&'static str>> = Mutex::new(alloc::vec::Vec::new());
            let mut names = NAMES.lock();
            let lower = name.to_ascii_lowercase();
            let s = match names.iter().find(|n| **n == lower) {
                Some(s) => *s,
                None => {
                    let s: &'static str = alloc::boxed::Box::leak(lower.into_boxed_str());
                    names.push(s);
                    s
                }
            };
            Some(if detail { "paket ikon dari /disk/ICONS" } else { s })
        }
        key::UI_FONT | key::MONO_FONT => {
            let mono = k == key::MONO_FONT;
            if index == 0 {
                return Some(if detail { "built-in 8 x 16 bitmap font" } else { "Bitmap (Noto Sans Mono)" });
            }
            let family = *crate::font::families(mono).get(index - 1)?;
            if !detail {
                return Some(family);
            }
            let id = crate::font::find(family, false)?;
            Some(match crate::font::info(id) {
                Some((_, _, true, _)) => "monospace",
                _ => "proportional",
            })
        }
        _ => None,
    }
}

/// Index of a font family in its settings list (0 = the bitmap font).
fn font_index(family: Option<&str>, mono: bool) -> u64 {
    match family {
        None => 0,
        Some(f) => crate::font::families(mono).iter().position(|x| *x == f).map_or(0, |i| i as u64 + 1),
    }
}
