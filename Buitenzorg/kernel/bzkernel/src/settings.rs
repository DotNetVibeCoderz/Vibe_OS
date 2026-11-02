//! The desktop's personal settings for applications (the SETTING syscall):
//! theme, wallpaper, screensaver and its delay, animations, cursor size --
//! what the shell's `theme` / `bg` / `saver` / `anim` / `cursor` commands
//! change, so a Settings app can show and change the same things.

use bz_abi::setting_key as key;
use bz_abi::syserr::INVAL;

/// Number of choices of a list setting (themes, wallpapers, screensavers + "off").
pub fn count(k: u64) -> u64 {
    match k {
        key::THEME => crate::theme::names().len() as u64,
        key::WALLPAPER => crate::wallpaper::OFFICIAL.len() as u64,
        key::SCREENSAVER => crate::screensaver::NAMES.len() as u64 + 1,
        key::ANIMATIONS | key::LARGE_CURSOR => 2,
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
        _ => None,
    }
}
