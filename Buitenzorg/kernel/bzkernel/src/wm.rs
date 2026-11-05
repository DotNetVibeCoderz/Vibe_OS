//! Window manager + compositor (v0.6 "Daun" + v0.7 "Kanopi"). Floating windows
//! with title bars, z-order, hit testing, and mouse-driven move/resize, plus
//! virtual desktops (workspaces) and theme-driven colors. The compositor
//! renders the desktop, the current workspace's windows, a taskbar (with a
//! workspace indicator), and the cursor into a full-screen back buffer.

use alloc::{string::String, vec, vec::Vec};
use spin::Mutex;

use crate::gfx::{self, Canvas, Color};
use crate::theme;

const TITLE_H: i32 = 24;
const BORDER: i32 = 2;
const RESIZE_GRIP: i32 = 14;
const MIN_W: i32 = 120;
const MIN_H: i32 = 60;
const TASKBAR_H: i32 = 34;
const BTN_W: i32 = 20; // title-bar button width
pub const WORKSPACES: u8 = 4;

// Desktop shell (v0.16 "Panen" UX pass): a Start button + start menu + desktop
// icons + a tray clock, blending macOS (centered, rounded, a top-corner clock),
// Ubuntu/GNOME (a launcher of apps) and Windows XP (a green Start button + a
// two-part start menu). Geometry:
const START_W: i32 = 76; // Start button width on the taskbar
// Start menu ("herbarium drawer"): a category rail on the left, the chosen
// category's apps as icon cards on the right, power actions in the footer.
const SM_W: i32 = 528; // start-menu width
const SM_H: i32 = 412; // start-menu height (two rows of app cards)
const SM_HDR: i32 = 46; // header band height
const SM_FTR: i32 = 46; // footer band height
const SM_RAIL_W: i32 = 168; // category rail width
const SM_RAIL_ROW: i32 = 40; // category rail row pitch
const SM_CELL_W: i32 = 112; // app card width
const SM_CELL_H: i32 = 92; // app card height
const ICON_W: i32 = 72; // desktop icon cell width
const ICON_H: i32 = 70; // desktop icon cell height (44 px icon + label)

/// A start-menu category: its name, its own leaf color (the rail marker and
/// the heading rule — it identifies the category, it is not decoration) and
/// its entries as (launch-name, label). A launch-name starting with '@' is a
/// system action the desktop loop performs instead of starting an app.
struct Category {
    name: &'static str,
    leaf: Color,
    apps: &'static [(&'static str, &'static str)],
}

const CATEGORIES: [Category; 5] = [
    Category {
        name: "Kantor",
        leaf: gfx::rgb(0x7F, 0xB3, 0xE0),
        // Interactive managed apps (RustCLR + Buitenzorg.UI, every architecture);
        // the native boot-demo programs stay reachable from the terminal (`run calc`).
        apps: &[("mnotes", "Catatan"), ("mcalc", "Kalkulator"), ("mfiles", "Berkas")],
    },
    Category {
        name: "Multimedia",
        leaf: gfx::rgb(0xC7, 0x92, 0xEA),
        apps: &[("mimage", "Gambar"), ("mpiano", "Piano"), ("mpaint", "Paint"), ("mgallery", "Galeri Kontrol")],
    },
    Category {
        name: "Permainan",
        leaf: gfx::rgb(0xF3, 0xC9, 0x69),
        apps: &[("m2048", "2048"), ("mxox", "Tic-Tac-Toe"), ("mruang", "Ruang 3D"), ("mbuah", "Panen Buah 3D")],
    },
    Category {
        name: "Utilitas",
        leaf: gfx::rgb(0x7C, 0xC4, 0xA4),
        // `hellogui` is a managed C# app (/disk/HELLOGUI.DLL, RustCLR).
        apps: &[("mclock", "Jam"), ("mtasks", "Task Manager"), ("mhwlab", "Lab Perangkat"), ("hellogui", "Jam C#")],
    },
    Category {
        name: "Pengaturan",
        leaf: gfx::rgb(0xD9, 0xB3, 0x8C),
        apps: &[("msetting", "Pengaturan"), ("mwaktu", "Tanggal & Waktu"), ("@wallpaper", "Wallpaper"), ("@theme", "Tema")],
    },
];

/// The desktop's right-click menu, as (launch-name or @action, label).
const CTX_ITEMS: [(&str, &str); 7] = [
    ("msetting", "Pengaturan"),
    ("@wallpaper", "Wallpaper berikutnya"),
    ("@theme", "Ganti tema"),
    ("@tile", "Rapikan jendela"),
    ("mfiles", "Berkas"),
    ("mtasks", "Task Manager"),
    ("managed", "Semua aplikasi"),
];
const CTX_W: i32 = 230;
const CTX_ITEM_H: i32 = 30;
const CTX_PAD: i32 = 6;

/// Screen rectangle of right-click menu item `i` for a menu at (x, y).
fn ctx_item_rect(x: i32, y: i32, i: usize) -> (i32, i32, i32, i32) {
    (x + CTX_PAD, y + CTX_PAD + i as i32 * CTX_ITEM_H, CTX_W - 2 * CTX_PAD, CTX_ITEM_H)
}

fn ctx_height() -> i32 {
    2 * CTX_PAD + CTX_ITEMS.len() as i32 * CTX_ITEM_H
}

/// Desktop launcher icons (top-left), as (launch-name, label).
const DESKTOP: [(&str, &str); 5] = [
    ("mfiles", "Berkas"),
    ("mnotes", "Catatan"),
    ("mcalc", "Kalkulator"),
    ("mtasks", "Task Manager"),
    // the folder of managed (RustCLR + Buitenzorg.UI) apps: /disk/MANAGED.DLL
    ("managed", "Managed Apps"),
];

/// Fallback tile colors when the icon pack is unavailable (no /disk driver on
/// this boot medium): each app still gets its own color, never a shared glyph.
fn fallback_color(name: &str) -> Color {
    match name {
        "files" => gfx::rgb(0xF5, 0x8A, 0x1F),
        "managed" => gfx::rgb(0x3F, 0x7A, 0x58),
        "editor" => gfx::rgb(0x2F, 0x7B, 0xE8),
        "calc" => gfx::rgb(0x3A, 0x3F, 0x47),
        "store" => gfx::rgb(0x12, 0x9E, 0x6E),
        "imgview" => gfx::rgb(0xC8, 0x34, 0xB4),
        "clock" => gfx::rgb(0x34, 0x2E, 0x8C),
        "piano" => gfx::rgb(0xB8, 0x1E, 0x2E),
        "2048" => gfx::rgb(0xD9, 0xB0, 0x6A),
        "taskmgr" => gfx::rgb(0x0F, 0x3B, 0x52),
        "paint" => gfx::rgb(0x1C, 0xC8, 0xC8),
        "xox" => gfx::rgb(0x6A, 0x2C, 0xB8),
        "audiopanel" => gfx::rgb(0xE0, 0x7A, 0x14),
        "@wallpaper" => gfx::rgb(0x1F, 0x7A, 0x52),
        "@theme" => gfx::rgb(0xD0, 0x4A, 0x78),
        _ => gfx::rgb(0x4C, 0x9A, 0x6A),
    }
}

/// Draw an app's icon (from the icon pack — keyed by launch name without the
/// '@' — or a colored tile with its initial).
/// The icon-pack name for a launch name: a managed app uses the picture of
/// the native app it stands in for.
fn icon_name(name: &str) -> &str {
    match name {
        "mnotes" => "editor",
        "mcalc" => "calc",
        "mfiles" => "files",
        "m2048" => "2048",
        "mxox" => "xox",
        "mclock" | "hellogui" => "clock",
        "mtasks" => "taskmgr",
        "mimage" => "imgview",
        "mpiano" => "piano",
        "mpaint" => "paint",
        n => n.trim_start_matches('@'),
    }
}

fn draw_app_icon(canvas: &mut Canvas, name: &str, label: &str, x: i32, y: i32, big: bool) {
    if crate::icons::draw(canvas, icon_name(name), x, y, big) {
        return;
    }
    let sz = crate::icons::size(big);
    let c = fallback_color(name);
    fill_rounded_gradient(canvas, x, y, sz, sz, if big { 10 } else { 4 }, shift(c, 30), c);
    let initial = &label[..1];
    let gw = gfx::glyph_width() as i32;
    canvas.draw_text(x + (sz - gw) / 2, y + (sz - 16) / 2, initial, gfx::rgb(0xFF, 0xFF, 0xFF), x + sz);
}

/// Day/month names for the tray and start-menu date.

/// Weekday (0 = Sunday) by Sakamoto's method.
fn weekday(y: i32, m: i32, d: i32) -> usize {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    ((y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d).rem_euclid(7)) as usize
}

/// Today as (weekday name, day, month name), local time (`clock.rs`), in
/// the interface language.
fn today() -> (&'static str, u8, &'static str) {
    let (y, mo, d, _, _, _) = crate::clock::now();
    let mo = mo.clamp(1, 12);
    (crate::clock::day_name(weekday(y as i32, mo as i32, d as i32)), d, crate::clock::month_name(mo))
}

/// Format `n` as two digits into `buf` (no allocation in the render path).
fn two_digits(buf: &mut [u8], n: u8) {
    buf[0] = b'0' + (n / 10) % 10;
    buf[1] = b'0' + n % 10;
}

/// Local (hour, minute) for the tray (`clock.rs`: RTC + time zone).
fn read_clock() -> (u8, u8) {
    let (_, _, _, h, m, _) = crate::clock::now();
    (h, m)
}

/// The tray clock text ("16:05", or "4:05 PM" with the 12-hour clock).
fn tray_time() -> ([u8; 8], usize) {
    let (hh, mm) = read_clock();
    let mut t = [b' '; 8];
    if crate::clock::h24() {
        two_digits(&mut t[0..2], hh);
        t[2] = b':';
        two_digits(&mut t[3..5], mm);
        (t, 5)
    } else {
        let h12 = if hh % 12 == 0 { 12 } else { hh % 12 };
        let mut n = 0;
        if h12 >= 10 {
            t[n] = b'1';
            n += 1;
        }
        t[n] = b'0' + h12 % 10;
        t[n + 1] = b':';
        two_digits(&mut t[n + 2..n + 4], mm);
        t[n + 4] = b' ';
        t[n + 5] = if hh < 12 { b'A' } else { b'P' };
        t[n + 6] = b'M';
        (t, n + 7)
    }
}

/// Workspace button `ws` (0-based) at the right end of the taskbar.
fn ws_button_rect(width: i32, height: i32, ws: u8) -> (i32, i32, i32, i32) {
    let i = (WORKSPACES - 1 - ws) as i32; // rightmost = the last workspace
    (width - 10 - 22 - i * 26, height - TASKBAR_H + 7, 22, TASKBAR_H - 14)
}

/// The tray's date + time block (left x, width); clicking it opens the
/// "Tanggal & Waktu" app.
fn clock_area(width: i32) -> (i32, i32) {
    let right = width - 10 - WORKSPACES as i32 * 26 - 10;
    let (t, n) = tray_time();
    let tw = gfx::text_width(core::str::from_utf8(&t[..n]).unwrap_or(""), gfx::TextStyle::Strong);
    let (day, d, mon) = today();
    let dd = if d >= 10 { 2 } else { 1 };
    let date_w = gfx::text_width(&day[..3], gfx::TextStyle::Body)
        + dd * gfx::glyph_width() as i32
        + gfx::text_width(mon, gfx::TextStyle::Body)
        + 2 * gfx::glyph_width() as i32;
    let w = tw + 12 + date_w + 12;
    (right - w, w)
}

/// Pixel canvas owned by an app window (v0.8): the client area apps draw
/// into through the WIN_CMD syscall.
///
/// SMP: each canvas has its **own lock** (`SharedCanvas`), separate from the
/// window-manager lock `WM`. `WM` guards the structure (window list, geometry,
/// menus, cursor) and is held only briefly: drawing into a window looks the
/// canvas up under `WM`, drops `WM`, then locks just that canvas; a compose
/// clones the structure under `WM` (canvases are shared `Arc`s, so this is
/// cheap), drops `WM`, and composites the snapshot, locking one canvas at a
/// time. So apps keep drawing while another CPU composes the desktop. Lock
/// order: never take `WM` while holding a canvas lock.
pub struct AppCanvas {
    pub w: i32,
    pub h: i32,
    /// What the app draws into.
    pub pixels: Vec<u32>,
    /// What the compositor shows, when the window is double-buffered: a
    /// process window becomes so on its first CLEAR (a toolkit redrawing the
    /// whole frame, e.g. the managed Buitenzorg.UI), and WIN_PRESENT copies
    /// `pixels` here. Without it a compose on another CPU could catch the
    /// frame half drawn -- the window flickered on a 4-core Raspberry Pi.
    pub front: Option<Vec<u32>>,
    /// Bumped by every change to what the window shows (draws, blits,
    /// presents): the compositor's damage tracking compares it.
    pub gen: u64,
}

/// A window's canvas, shared between the window list and in-flight composes.
pub type SharedCanvas = alloc::sync::Arc<spin::Mutex<AppCanvas>>;

fn shared(c: AppCanvas) -> SharedCanvas {
    alloc::sync::Arc::new(spin::Mutex::new(c))
}

/// Draw calls that ran while a compose was in progress (evidence that the
/// split lock lets them overlap).
static COMPOSING: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static DRAWS_DURING_COMPOSE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Number of window draw calls that overlapped a compose on another CPU.
pub fn draws_during_compose() -> u64 {
    DRAWS_DURING_COMPOSE.load(core::sync::atomic::Ordering::Relaxed)
}

fn note_draw() {
    use core::sync::atomic::Ordering;
    if COMPOSING.load(Ordering::Relaxed) > 0 {
        DRAWS_DURING_COMPOSE.fetch_add(1, Ordering::Relaxed);
    }
}

/// Window state (v0.11 window controls).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WinState {
    Normal,
    Minimized,
    Maximized,
}

/// Title-bar control buttons.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TitleButton {
    Minimize,
    Maximize,
    Close,
}

#[derive(Clone)]
pub struct Window {
    pub id: u32,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub lines: Vec<String>,
    pub workspace: u8,
    pub canvas: Option<SharedCanvas>,
    pub state: WinState,
    pub saved: (i32, i32, i32, i32), // geometry to restore from max/min
    /// Process that created the window (0 = kernel/shell). Keyboard focus
    /// follows the topmost window's owner (multi-process increment 1).
    pub owner: u64,
    /// Pointer events for the owning program (WIN_EVENT), packed as
    /// `bz_abi::pointer_event` describes; consecutive moves are coalesced.
    pub events: alloc::collections::VecDeque<u64>,
}

/// Pack a pointer event (see `bz_abi::pointer_event`).
fn pack_event(kind: u64, x: i32, y: i32, buttons: u64) -> u64 {
    (kind << 56) | ((buttons & 0xFF) << 32) | (((y as i16) as u16 as u64) << 16) | ((x as i16) as u16 as u64)
}

impl Window {
    /// Queue a pointer event (64 at most; a move replaces a queued move).
    fn push_event(&mut self, ev: u64) {
        let kind = ev >> 56;
        if kind == bz_abi::pointer_event::MOVE {
            if let Some(last) = self.events.back_mut() {
                if *last >> 56 == bz_abi::pointer_event::MOVE {
                    *last = ev;
                    return;
                }
            }
        }
        if self.events.len() >= 64 {
            self.events.pop_front();
        }
        self.events.push_back(ev);
    }

    /// Client-area origin on the screen.
    fn client_origin(&self) -> (i32, i32) {
        (self.x + BORDER, self.y + BORDER + TITLE_H)
    }

    /// (x, y) relative to the client area, if it lies inside it.
    fn client_point(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        let (ox, oy) = self.client_origin();
        let (cw, ch) = (self.w - 2 * BORDER, self.h - TITLE_H - 2 * BORDER);
        let (lx, ly) = (x - ox, y - oy);
        (self.canvas.is_some() && lx >= 0 && ly >= 0 && lx < cw && ly < ch).then_some((lx, ly))
    }
}

#[derive(Clone, Copy)]
enum Drag {
    None,
    Move { id: u32, dx: i32, dy: i32 },
    Resize { id: u32 },
}

#[derive(Clone)]
struct Wm {
    windows: Vec<Window>, // back-to-front z-order (last = top)
    next_id: u32,
    cursor: (i32, i32),
    drag: Drag,
    prev_left: bool,
    width: i32,
    height: i32,
    workspace: u8,
    /// (window id, button) currently hovered, for micro-interaction highlight.
    hover: Option<(u32, TitleButton)>,
    /// App window holding the pointer capture (a press in its client area,
    /// until the release).
    capture: Option<u32>,
    /// App window the pointer was last over (for LEAVE).
    pointer_in: Option<u32>,
    /// Active click ripple: (x, y, start_frame).
    ripple: Option<(i32, i32, u64)>,
    /// Animation frame counter (advanced by the desktop loop).
    frame: u64,
    /// UI options (Personalization).
    animations: bool,
    rounded: bool,
    cursor_scale: i32,
    /// Desktop shell state (v0.16 UX pass).
    start_open: bool,
    /// Category shown in the start menu (index into CATEGORIES).
    start_cat: usize,
    /// A launch requested by a start-menu / desktop-icon click, drained by the
    /// desktop loop (the WM cannot run a ring-3 app itself).
    pending: Option<String>,
    /// A power action requested from the start menu: 1 = shutdown, 2 = restart.
    pending_power: u8,
    /// Owners of windows the user closed, for the desktop loop to reap.
    closed_owners: Vec<u64>,
    /// (desktop-icon index, frame) of the last icon click, for double-click.
    last_icon: (usize, u64),
    /// The open right-click menu's top-left corner.
    ctx_menu: Option<(i32, i32)>,
}

static WM: Mutex<Option<Wm>> = Mutex::new(None);

pub fn init(width: usize, height: usize) {
    gfx::warm_glyphs();
    *WM.lock() = Some(Wm {
        windows: Vec::new(),
        next_id: 1,
        cursor: (width as i32 / 2, height as i32 / 2),
        drag: Drag::None,
        prev_left: false,
        width: width as i32,
        height: height as i32,
        workspace: 0,
        hover: None,
        capture: None,
        pointer_in: None,
        ripple: None,
        frame: 0,
        animations: true,
        rounded: true,
        cursor_scale: 1,
        start_open: false,
        start_cat: 0,
        pending: None,
        pending_power: 0,
        closed_owners: Vec::new(),
        last_icon: (usize::MAX, 0),
        ctx_menu: None,
    });
}

/// Drain a pending app-launch requested from the desktop shell (start menu /
/// desktop icon). The desktop loop calls this and runs the app.
pub fn take_pending_launch() -> Option<String> {
    WM.lock().as_mut().and_then(|wm| wm.pending.take())
}

/// Drain the owners of windows the user has closed since the last call.
pub fn take_closed_owners() -> Vec<u64> {
    WM.lock().as_mut().map(|wm| core::mem::take(&mut wm.closed_owners)).unwrap_or_default()
}

/// Reassign window `id` to process `owner` (kernel self-tests).
pub fn set_window_owner(id: u32, owner: u64) {
    if let Some(wm) = WM.lock().as_mut() {
        if let Some(w) = wm.windows.iter_mut().find(|w| w.id == id) {
            w.owner = owner;
        }
    }
}

/// Remove every window whose owning program has exited — the boot demos leave
/// their final frames behind, and a frozen window (a "monitor" stuck at
/// "Uptime 6s") misinforms. Kernel windows (owner 0: Welcome, Terminal, …)
/// stay. Returns how many were removed.
pub fn close_orphan_windows() -> usize {
    let mut guard = WM.lock();
    let Some(wm) = guard.as_mut() else { return 0 };
    let before = wm.windows.len();
    wm.windows.retain(|w| w.owner == 0 || crate::process::is_alive(w.owner));
    before - wm.windows.len()
}

/// Close every window of the current workspace except the terminal, as if
/// its close button was pressed (a live owner left without windows is ended
/// by `app::reap_closed_windows`). Returns how many were closed.
pub fn close_workspace_windows() -> usize {
    let mut guard = WM.lock();
    let Some(wm) = guard.as_mut() else { return 0 };
    let ws = wm.workspace;
    let ids: Vec<u32> = wm
        .windows
        .iter()
        .filter(|w| w.workspace == ws && w.title != "Terminal" && !w.title.starts_with("widget:"))
        .map(|w| w.id)
        .collect();
    for &id in &ids {
        wm.press_button(id, TitleButton::Close);
    }
    ids.len()
}

/// Arrange the windows of the current workspace side by side (shelf packing,
/// left to right, wrapping into rows) right of the desktop icons and above the
/// taskbar, keeping each window's size; z-order is unchanged. Maximized and
/// minimized windows and widget-board windows are left alone. Returns how
/// many windows were moved.
pub fn tile() -> usize {
    const GAP: i32 = 12;
    let mut guard = WM.lock();
    let Some(wm) = guard.as_mut() else { return 0 };
    let ws = wm.workspace;
    let (left, top) = (ICON_W + 2 * GAP, GAP);
    let right = wm.width - GAP;
    let bottom = wm.height - TASKBAR_H - GAP;
    let (mut x, mut y, mut row_h, mut moved) = (left, top, 0, 0);
    for win in wm.windows.iter_mut() {
        if win.workspace != ws || win.state != WinState::Normal || win.title.starts_with("widget:") {
            continue;
        }
        if x > left && x + win.w > right {
            x = left;
            y += row_h + GAP;
            row_h = 0;
        }
        let ny = y.min((bottom - win.h).max(top));
        (win.x, win.y) = (x, ny);
        win.saved = (win.x, win.y, win.w, win.h);
        x += win.w + GAP;
        row_h = row_h.max(win.h);
        moved += 1;
    }
    moved
}

/// Place the first window titled `title` (current workspace) at x, y with
/// size w x h, restoring it if minimized/maximized. Returns true if found.
pub fn place_window(title: &str, x: i32, y: i32, w: i32, h: i32) -> bool {
    let mut guard = WM.lock();
    let Some(wm) = guard.as_mut() else { return false };
    let ws = wm.workspace;
    let Some(win) = wm.windows.iter_mut().find(|win| win.title == title && win.workspace == ws) else {
        return false;
    };
    (win.x, win.y, win.w, win.h) = (x, y, w, h);
    win.saved = (x, y, w, h);
    win.state = WinState::Normal;
    true
}

/// Id of the first window titled `title`.
pub fn window_id(title: &str) -> Option<u32> {
    WM.lock().as_ref()?.windows.iter().find(|w| w.title == title).map(|w| w.id)
}

/// Rename window `id`.
pub fn set_title(id: u32, title: &str) {
    if let Some(wm) = WM.lock().as_mut() {
        if let Some(w) = wm.windows.iter_mut().find(|w| w.id == id) {
            w.title = String::from(title);
        }
    }
}

/// Bring the first window titled `title` to the front.
pub fn focus_title(title: &str) {
    let mut guard = WM.lock();
    let Some(wm) = guard.as_mut() else { return };
    if let Some(id) = wm.windows.iter().find(|w| w.title == title).map(|w| w.id) {
        wm.focus(id);
    }
}

/// True if process `owner` still has at least one window.
pub fn has_windows(owner: u64) -> bool {
    WM.lock().as_ref().map(|wm| wm.windows.iter().any(|w| w.owner == owner)).unwrap_or(false)
}

/// Drain a pending power action from the start menu (1 = shutdown, 2 = restart).
pub fn take_pending_power() -> u8 {
    WM.lock().as_mut().map(|wm| core::mem::take(&mut wm.pending_power)).unwrap_or(0)
}

/// Is the start menu open? (The desktop loop re-renders while it is.)
/// A right-button press at (x, y): on the empty desktop it opens the
/// right-click menu there (kept on screen); anywhere else it closes menus.
pub fn right_click(x: i32, y: i32) {
    let mut guard = WM.lock();
    let Some(wm) = guard.as_mut() else { return };
    wm.start_open = false;
    let on_desktop = y < wm.height - TASKBAR_H && wm.window_at(x, y).is_none();
    wm.ctx_menu = if on_desktop {
        let mx = x.min(wm.width - CTX_W - 4).max(0);
        let my = y.min(wm.height - TASKBAR_H - ctx_height() - 4).max(0);
        Some((mx, my))
    } else {
        None
    };
}

pub fn start_menu_open() -> bool {
    WM.lock().as_ref().map(|wm| wm.start_open).unwrap_or(false)
}

/// Self-test the desktop-shell click routing (Start button -> open menu ->
/// click an app row -> a launch is queued). Drives `handle_mouse` the same way
/// the mouse IRQ would. Returns true if clicking the "files" row queued it.
/// Leaves the shell closed and the launch queue empty.
pub fn self_test() -> bool {
    let height = { WM.lock().as_ref().map(|w| w.height).unwrap_or(0) };
    if height == 0 {
        return false;
    }
    // Click the Start button (bottom-left corner of the taskbar).
    handle_mouse(40, height - 4, true);
    handle_mouse(40, height - 4, false);
    let opened = start_menu_open();
    // The menu opens on the first category (Kantor): its first card is the
    // editor. Then pick the "Permainan" rail entry and its first card (2048).
    let (cx0, cy0) = card_center(height, 0);
    handle_mouse(cx0, cy0, true);
    handle_mouse(cx0, cy0, false);
    let first = matches!(take_pending_launch().as_deref(), Some("mnotes"));
    handle_mouse(40, height - 4, true);
    handle_mouse(40, height - 4, false);
    let (rx, ry) = rail_center(height, 2);
    handle_mouse(rx, ry, true);
    handle_mouse(rx, ry, false);
    handle_mouse(cx0, cy0, true);
    handle_mouse(cx0, cy0, false);
    let game = matches!(take_pending_launch().as_deref(), Some("m2048"));
    // "Utilitas" -> its fourth card, on the second row: the managed C# app.
    handle_mouse(40, height - 4, true);
    handle_mouse(40, height - 4, false);
    let (ux, uy) = rail_center(height, 3);
    handle_mouse(ux, uy, true);
    handle_mouse(ux, uy, false);
    let (c3x, c3y) = card_center(height, 3);
    handle_mouse(c3x, c3y, true);
    handle_mouse(c3x, c3y, false);
    let managed = matches!(take_pending_launch().as_deref(), Some("hellogui"));
    // Right-click on the desktop opens the menu; its fourth item tidies windows.
    // (Boot demos leave many windows: find a spot of bare desktop first.)
    let spot = {
        let g = WM.lock();
        g.as_ref().and_then(|w| {
            (1..(w.height - TASKBAR_H) / 24).flat_map(|r| (4..w.width / 24).map(move |c| (c * 24, r * 24)))
                .find(|&(x, y)| w.window_at(x, y).is_none() && !(0..DESKTOP.len()).any(|i| { let (ix, iy) = icon_rect(i); x >= ix && x < ix + ICON_W && y >= iy && y < iy + ICON_H }))
        })
    };
    let (sx, sy) = spot.unwrap_or((400, 300));
    right_click(sx, sy);
    let ctx_open = WM.lock().as_ref().map_or(false, |w| w.ctx_menu.is_some());
    let (ix, iy, iw, ih) = WM.lock().as_ref().and_then(|w| w.ctx_menu).map(|(x, y)| ctx_item_rect(x, y, 3)).unwrap_or((0, 0, 0, 0));
    handle_mouse(ix + iw / 2, iy + ih / 2, true);
    handle_mouse(ix + iw / 2, iy + ih / 2, false);
    let ctx = ctx_open && matches!(take_pending_launch().as_deref(), Some("@tile"));
    crate::println!("[wm] self-test: start menu {} / {} / {}, right-click menu {}", first, game, managed, ctx);
    let launched = first && game && managed && ctx;
    // Reset shell state so the real desktop starts clean.
    if let Some(wm) = WM.lock().as_mut() {
        wm.start_open = false;
        wm.start_cat = 0;
        wm.pending = None;
        wm.last_icon = (usize::MAX, 0);
        wm.ctx_menu = None;
    }
    opened && launched
}

/// Top-left of the start menu on a screen of `height`.
fn menu_origin(height: i32) -> (i32, i32) {
    (6, height - TASKBAR_H - SM_H - 6)
}

/// Rect (x, y, w, h) of category rail row `i`.
fn rail_rect(height: i32, i: usize) -> (i32, i32, i32, i32) {
    let (x, y) = menu_origin(height);
    (x + 10, y + SM_HDR + 12 + i as i32 * SM_RAIL_ROW, SM_RAIL_W - 20, SM_RAIL_ROW - 6)
}

/// Rect (x, y, w, h) of app card `j` in the content pane (3 per row).
fn card_rect(height: i32, j: usize) -> (i32, i32, i32, i32) {
    let (x, y) = menu_origin(height);
    let (col, row) = ((j % 3) as i32, (j / 3) as i32);
    (x + SM_RAIL_W + 14 + col * (SM_CELL_W + 4), y + SM_HDR + 70 + row * (SM_CELL_H + 4), SM_CELL_W, SM_CELL_H)
}

fn rail_center(height: i32, i: usize) -> (i32, i32) {
    let (x, y, w, h) = rail_rect(height, i);
    (x + w / 2, y + h / 2)
}

fn card_center(height: i32, j: usize) -> (i32, i32) {
    let (x, y, w, h) = card_rect(height, j);
    (x + w / 2, y + h / 2)
}

/// Footer power actions: (x, w, power code, label). 1 = shutdown, 2 =
/// restart, 3 = sleep; laid out right-to-left.
fn power_buttons(height: i32) -> [(i32, i32, u8, &'static str); 3] {
    let (x, _) = menu_origin(height);
    let labels: [(u8, &str); 3] = [(1, "Matikan"), (2, "Mulai ulang"), (3, "Tidur")];
    let mut out = [(0, 0, 0u8, ""); 3];
    let mut right = x + SM_W - 12;
    for (k, (code, label)) in labels.iter().enumerate() {
        let w = gfx::text_width(label, gfx::TextStyle::Body) + 20;
        right -= w;
        out[k] = (right, w, *code, *label);
        right -= 6;
    }
    out
}

fn hit(px: i32, py: i32, (x, y, w, h): (i32, i32, i32, i32)) -> bool {
    px >= x && px < x + w && py >= y && py < y + h
}

/// Current cursor size (1 = normal, 2 = large).
pub fn cursor_scale() -> i32 {
    WM.lock().as_ref().map_or(1, |w| w.cursor_scale)
}

/// Set cursor size (1 = normal, 2 = large) — Personalization.
pub fn set_cursor_scale(scale: i32) {
    if let Some(wm) = WM.lock().as_mut() {
        wm.cursor_scale = scale.clamp(1, 3);
    }
}

/// Advance the animation frame counter (called each tick by the desktop loop
/// when animating). Returns true if a redraw is warranted (ripple in flight).
pub fn tick_animation(frame: u64) -> bool {
    let mut guard = WM.lock();
    let Some(wm) = guard.as_mut() else { return false };
    wm.frame = frame;
    if let Some((_, _, start)) = wm.ripple {
        if frame.saturating_sub(start) > 8 {
            wm.ripple = None;
        }
        return true;
    }
    false
}

/// Toggle animations / rounded corners (Personalization).
pub fn set_options(animations: bool, rounded: bool) {
    if let Some(wm) = WM.lock().as_mut() {
        wm.animations = animations;
        wm.rounded = rounded;
    }
}

pub fn options() -> (bool, bool) {
    WM.lock().as_ref().map(|w| (w.animations, w.rounded)).unwrap_or((true, true))
}

pub fn create_window(title: &str, x: i32, y: i32, w: i32, h: i32, lines: &[&str]) -> u32 {
    let mut guard = WM.lock();
    let Some(wm) = guard.as_mut() else { return 0 };
    let id = wm.next_id;
    wm.next_id += 1;
    let workspace = wm.workspace;
    wm.windows.push(Window {
        id,
        title: String::from(title),
        x,
        y,
        w,
        h,
        lines: lines.iter().map(|s| String::from(*s)).collect(),
        workspace,
        canvas: None,
        state: WinState::Normal,
        saved: (x, y, w, h),
        owner: crate::aspace::current_id(),
        events: alloc::collections::VecDeque::new(),
    });
    id
}

/// Owner process of the window that has keyboard focus: the topmost
/// non-minimized window on the current workspace (0 = kernel/terminal).
pub fn focus_owner() -> u64 {
    let guard = WM.lock();
    let Some(wm) = guard.as_ref() else { return 0 };
    wm.windows
        .iter()
        .rev()
        .find(|w| w.workspace == wm.workspace && w.state != WinState::Minimized)
        .map(|w| w.owner)
        .unwrap_or(0)
}

/// Create an app window with a drawable pixel canvas (v0.8 WIN_CREATE).
/// A window whose title starts with "widget:" is docked on the widget board
/// along the right edge (v0.9 widget variant) instead of cascading.
pub fn create_app_window(title: &str, w: i32, h: i32) -> u32 {
    let mut guard = WM.lock();
    let Some(wm) = guard.as_mut() else { return 0 };
    let id = wm.next_id;
    wm.next_id += 1;

    if let Some(name) = title.strip_prefix("widget:") {
        // Dock on the widget board (right edge), stacked top-to-bottom.
        let ww = 220;
        let board_x = wm.width - ww - 12;
        let used: i32 = wm
            .windows
            .iter()
            .filter(|w| w.title.starts_with("widget:"))
            .map(|w| w.h + 12)
            .sum();
        let y = 12 + used;
        let cw = (ww - 2 * BORDER).max(1);
        let ch = (h - TITLE_H - 2 * BORDER).max(1);
        let body = theme::current().win_body;
        let workspace = wm.workspace;
        wm.windows.push(Window {
            id,
            title: String::from(name),
            x: board_x,
            y,
            w: ww,
            h,
            lines: Vec::new(),
            workspace,
            canvas: Some(shared(AppCanvas {
                gen: 0,
                w: cw,
                h: ch,
                pixels: vec![body; (cw * ch) as usize],
                front: None,
            })),
            state: WinState::Normal,
            saved: (board_x, y, ww, h),
            owner: crate::aspace::current_id(),
            events: alloc::collections::VecDeque::new(),
        });
        return id;
    }

    let n = wm.windows.len() as i32;
    let x = (60 + n * 30).min(wm.width - w - 20).max(0);
    let y = (60 + n * 26).min(wm.height - h - TASKBAR_H - 20).max(0);
    let cw = (w - 2 * BORDER).max(1);
    let ch = (h - TITLE_H - 2 * BORDER).max(1);
    let body = theme::current().win_body;
    let workspace = wm.workspace;
    wm.windows.push(Window {
        id,
        title: String::from(title),
        x,
        y,
        w,
        h,
        lines: Vec::new(),
        workspace,
        canvas: Some(shared(AppCanvas {
                gen: 0,
            w: cw,
            h: ch,
            pixels: vec![body; (cw * ch) as usize],
            front: None,
        })),
        state: WinState::Normal,
        saved: (x, y, w, h),
        owner: crate::aspace::current_id(),
        events: alloc::collections::VecDeque::new(),
    });
    id
}

/// WIN_PRESENT: the double-buffered windows of process `owner` show what
/// was drawn since the last present.
pub fn flip_owned(owner: u64) {
    let canvases: Vec<SharedCanvas> = match WM.lock().as_ref() {
        Some(wm) => wm.windows.iter().filter(|w| w.owner == owner).filter_map(|w| w.canvas.clone()).collect(),
        None => return,
    };
    for c in canvases {
        let mut c = c.lock();
        let c = &mut *c;
        c.gen += 1;
        if let Some(front) = c.front.as_mut() {
            if front.len() == c.pixels.len() {
                front.copy_from_slice(&c.pixels);
            } else {
                *front = c.pixels.clone();
            }
        }
    }
}

/// Apply a draw command to an app window's canvas (v0.8 WIN_CMD).
pub fn draw_on_window(id: u32, cmd: &bz_abi::DrawCmd, text: Option<&str>) -> Result<(), &'static str> {
    let mut guard = WM.lock();
    let wm = guard.as_mut().ok_or("wm not initialized")?;
    let win = wm.windows.iter_mut().find(|w| w.id == id).ok_or("no such window")?;
    let shared = win.canvas.clone().ok_or("window has no canvas")?;
    let owner = win.owner;
    drop(guard); // draw with only this window's canvas locked
    note_draw();
    let mut canvas = shared.lock();
    let canvas = &mut *canvas;
    canvas.gen += 1;
    if cmd.op == bz_abi::draw_op::CLEAR && canvas.front.is_none() && owner != 0 {
        // a full redraw starts: from now on show only presented frames
        canvas.front = Some(canvas.pixels.clone());
    }
    let (cw, ch) = (canvas.w as usize, canvas.h as usize);
    let mut c = Canvas::new(&mut canvas.pixels, cw, ch);
    match cmd.op {
        bz_abi::draw_op::FILL_RECT => c.fill_rect(cmd.x, cmd.y, cmd.w, cmd.h, cmd.color),
        bz_abi::draw_op::CLEAR => c.fill_rect(0, 0, cw as i32, ch as i32, cmd.color),
        bz_abi::draw_op::DRAW_TEXT => {
            let text = text.ok_or("draw_text without text")?;
            c.draw_text(cmd.x, cmd.y, text, cmd.color, cw as i32);
        }
        bz_abi::draw_op::LINE => c.draw_line(cmd.x, cmd.y, cmd.x + cmd.w, cmd.y + cmd.h, cmd.color),
        bz_abi::draw_op::ELLIPSE => c.ellipse(cmd.x, cmd.y, cmd.w, cmd.h, cmd.color, false),
        bz_abi::draw_op::FILL_ELLIPSE => c.ellipse(cmd.x, cmd.y, cmd.w, cmd.h, cmd.color, true),
        bz_abi::draw_op::RECT => c.rect_outline(cmd.x, cmd.y, cmd.w, cmd.h, 1, cmd.color),
        // Toolkit ops (managed Buitenzorg.UI): `_pad` is the second parameter.
        bz_abi::draw_op::FILL_ROUND_RECT => fill_rounded(&mut c, cmd.x, cmd.y, cmd.w, cmd.h, cmd._pad as i32, cmd.color),
        bz_abi::draw_op::ROUND_RECT => outline_rounded(&mut c, cmd.x, cmd.y, cmd.w, cmd.h, cmd._pad as i32, 1, cmd.color),
        bz_abi::draw_op::GRADIENT_V => gradient_v(&mut c, cmd.x, cmd.y, cmd.w, cmd.h, cmd.color, cmd._pad),
        bz_abi::draw_op::FILL_ROUND_GRADIENT_V => {
            fill_rounded_gradient(&mut c, cmd.x, cmd.y, cmd.w, cmd.h, cmd.text_len as i32, cmd.color, cmd._pad)
        }
        bz_abi::draw_op::DRAW_TEXT_STYLED => {
            let text = text.ok_or("draw_text without text")?;
            let style = match cmd._pad {
                1 => gfx::TextStyle::Strong,
                2 => gfx::TextStyle::Heading,
                _ => gfx::TextStyle::Body,
            };
            c.draw_text_styled(cmd.x, cmd.y, text, cmd.color, cw as i32, style);
        }
        bz_abi::draw_op::BLEND_ROUND_RECT => {
            let (radius, alpha) = ((cmd._pad & 0xFFFF) as i32, ((cmd._pad >> 16) & 0xFF) as u8);
            blend_rounded(&mut c, cmd.x, cmd.y, cmd.w, cmd.h, radius, cmd.color, alpha)
        }
        _ => return Err("unknown draw op"),
    }
    Ok(())
}

/// Blit a client ARGB pixel buffer into a window's canvas at (x, y), with
/// clipping (v0.16 "Panen" BLIT op — client-side software rendering). The
/// canvas stores 0x00RRGGBB, so alpha is dropped (the client composites first).
///
/// `src_ptr` is a user-space pointer to `bw*bh` ARGB `u32` pixels; it is
/// copied row by row through a raw pointer (never turned into a `&[u32]`
/// slice — a slice over user memory would let the optimizer assume the whole
/// region is dereferenceable/immutable, which corrupted the boot).
pub fn blit_on_window(
    id: u32,
    x: i32,
    y: i32,
    bw: i32,
    bh: i32,
    src_ptr: u64,
) -> Result<(), &'static str> {
    let mut guard = WM.lock();
    let wm = guard.as_mut().ok_or("wm not initialized")?;
    let win = wm.windows.iter_mut().find(|w| w.id == id).ok_or("no such window")?;
    let shared = win.canvas.clone().ok_or("window has no canvas")?;
    drop(guard); // blit with only this window's canvas locked
    note_draw();
    let mut canvas = shared.lock();
    canvas.gen += 1;
    let (cw, ch) = (canvas.w as i32, canvas.h as i32);
    let base = src_ptr as *const u32;
    // Clip once, then one block copy per row (G1: this was a volatile read
    // per pixel). The source is user memory the syscall layer validated; it
    // is copied through a raw pointer -- never a slice over user memory (see
    // CLAUDE.md) -- and the alpha byte is cleared afterwards, two pixels per
    // step.
    let x0 = x.max(0);
    let x1 = (x + bw).min(cw);
    if x1 <= x0 {
        return Ok(());
    }
    let n = (x1 - x0) as usize;
    for row in 0..bh {
        let dy = y + row;
        if dy < 0 || dy >= ch {
            continue;
        }
        let d = (dy * cw + x0) as usize;
        let dst = &mut canvas.pixels[d..d + n];
        unsafe { core::ptr::copy_nonoverlapping(base.add((row * bw + (x0 - x)) as usize), dst.as_mut_ptr(), n) };
        let (head, words, tail) = unsafe { dst.align_to_mut::<u64>() };
        for p in head.iter_mut().chain(tail.iter_mut()) {
            *p &= 0x00FF_FFFF;
        }
        for w in words.iter_mut() {
            *w &= 0x00FF_FFFF_00FF_FFFF;
        }
    }
    Ok(())
}

/// Reusable compose buffer for [`present_now`] — allocated once and kept, so a
/// present from a shell-launched app never needs a fresh multi-MiB allocation
/// on a full/fragmented heap (that failed and panicked the kernel once).
static PRESENT_BUF: spin::Mutex<alloc::vec::Vec<u32>> = spin::Mutex::new(alloc::vec::Vec::new());

/// Compose + present immediately (v0.8 WIN_PRESENT syscall).
/// Set by a present that found another CPU already composing: that CPU
/// composes once more after it finishes, so the newer drawing is shown.
static PRESENT_AGAIN: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn present_now() {
    use core::sync::atomic::Ordering;
    if let Some((w, h)) = crate::framebuffer::dimensions() {
        // Presents coalesce: a full compose takes long, and queueing every
        // caller behind it (spinning) only repeats the same work. If another
        // CPU is composing, ask it to go round once more and return.
        let Some(mut back) = PRESENT_BUF.try_lock() else {
            PRESENT_AGAIN.store(true, Ordering::Release);
            return;
        };
        if back.len() != w * h {
            back.resize(w * h, 0);
        }
        for _ in 0..4 {
            PRESENT_AGAIN.store(false, Ordering::Release);
            // Profiler zones (wm::compose, fb::present): the compositor is
            // the deepest thing a WIN_PRESENT syscall runs. Inert unless
            // profiling is enabled.
            compose_and_present(&mut back, w, h);
            if !PRESENT_AGAIN.load(Ordering::Acquire) {
                break;
            }
        }
    }
}

/// Replace a window's body text (used by the terminal to push scrollback).
pub fn set_window_lines(id: u32, lines: Vec<String>) {
    if let Some(wm) = WM.lock().as_mut() {
        if let Some(w) = wm.windows.iter_mut().find(|w| w.id == id) {
            w.lines = lines;
        }
    }
}

/// Move a window to a workspace.
pub fn set_window_workspace(id: u32, workspace: u8) {
    if let Some(wm) = WM.lock().as_mut() {
        if let Some(w) = wm.windows.iter_mut().find(|w| w.id == id) {
            w.workspace = workspace % WORKSPACES;
        }
    }
}

pub fn window_rect(id: u32) -> Option<(i32, i32, i32, i32)> {
    let guard = WM.lock();
    let wm = guard.as_ref()?;
    wm.windows.iter().find(|w| w.id == id).map(|w| (w.x, w.y, w.w, w.h))
}

/// Window state (for the window-controls demo/tests).
pub fn window_state(id: u32) -> Option<WinState> {
    WM.lock().as_ref().and_then(|wm| wm.windows.iter().find(|w| w.id == id).map(|w| w.state))
}

/// Maximize/restore a window (v0.11 window controls).
pub fn maximize(id: u32) {
    if let Some(wm) = WM.lock().as_mut() {
        wm.toggle_maximize(id);
    }
}

/// Minimize a window.
pub fn minimize(id: u32) {
    if let Some(wm) = WM.lock().as_mut() {
        wm.press_button(id, TitleButton::Minimize);
    }
}

/// Close (remove) a window.
pub fn close(id: u32) {
    if let Some(wm) = WM.lock().as_mut() {
        wm.press_button(id, TitleButton::Close);
    }
}

/// Switch to virtual desktop `n` (0-based, wrapped). Returns the new one.
pub fn switch_workspace(n: u8) -> u8 {
    let mut guard = WM.lock();
    let Some(wm) = guard.as_mut() else { return 0 };
    wm.workspace = n % WORKSPACES;
    wm.drag = Drag::None;
    wm.workspace
}

/// Open windows on the current workspace (tests, the ARM64 desktop frame).
#[allow(dead_code)]
pub fn window_count() -> usize {
    WM.lock().as_ref().map(|w| w.windows.iter().filter(|x| x.workspace == w.workspace).count()).unwrap_or(0)
}

pub fn current_workspace() -> u8 {
    WM.lock().as_ref().map(|w| w.workspace).unwrap_or(0)
}

pub fn set_cursor(x: i32, y: i32) {
    if let Some(wm) = WM.lock().as_mut() {
        wm.cursor = (x.clamp(0, wm.width - 1), y.clamp(0, wm.height - 1));
    }
}

/// Feed one mouse sample (absolute position + left button) into the WM.
/// Mouse wheel notches (positive = scroll up) for the window under the
/// pointer. True if an app window took them.
/// Id of the top-most window under the pointer (current workspace).
pub fn window_at_cursor() -> Option<u32> {
    let guard = WM.lock();
    let wm = guard.as_ref()?;
    let (cx, cy) = wm.cursor;
    if wm.start_open || cy >= wm.height - TASKBAR_H {
        return None;
    }
    wm.window_at(cx, cy).map(|i| wm.windows[i].id)
}

pub fn wheel(notches: i64) -> bool {
    let mut guard = WM.lock();
    let Some(wm) = guard.as_mut() else { return false };
    wm.route_wheel(notches)
}

pub fn handle_mouse(x: i32, y: i32, left: bool) {
    let mut guard = WM.lock();
    let Some(wm) = guard.as_mut() else { return };
    let (px, py) = wm.cursor;
    wm.cursor = (x.clamp(0, wm.width - 1), y.clamp(0, wm.height - 1));
    let (cx, cy) = wm.cursor;
    let (mdx, mdy) = (cx - px, cy - py);

    // Hover tracking (micro-interaction highlight on title-bar buttons).
    wm.hover = wm.title_button_at(cx, cy);
    // Start menu: resting on a category in the rail shows its apps.
    if wm.start_open {
        if let Some(i) = (0..CATEGORIES.len()).find(|&i| hit(cx, cy, rail_rect(wm.height, i))) {
            wm.start_cat = i;
        }
    }

    let pressed = left && !wm.prev_left;
    let released = !left && wm.prev_left;
    wm.prev_left = left;
    wm.route_pointer(cx, cy, left, pressed, released, (mdx, mdy) != (0, 0));

    if pressed {
        let frame = wm.frame;
        wm.ripple = Some((cx, cy, frame)); // click ripple
        if wm.shell_click(cx, cy) {
            // Consumed by the Start button, start menu, or a desktop icon.
        } else if cy >= wm.height - TASKBAR_H {
            wm.taskbar_click(cx);
        } else if let Some((id, btn)) = wm.title_button_at(cx, cy) {
            wm.press_button(id, btn);
        } else {
            wm.begin_drag(cx, cy);
        }
    } else if released {
        wm.drag = Drag::None;
    } else if left {
        wm.continue_drag(mdx, mdy);
    }
}

impl Wm {
    /// Deliver the pointer to app windows (WIN_EVENT): moves and presses
    /// over a client area go to the top-most window there, a press captures
    /// the pointer until the release, and leaving a client area is reported.
    fn route_pointer(&mut self, cx: i32, cy: i32, left: bool, pressed: bool, released: bool, moved: bool) {
        use bz_abi::pointer_event::{DOWN, LEAVE, MOVE, UP};
        let buttons = left as u64;
        if let Some(id) = self.capture {
            if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                let (ox, oy) = w.client_origin();
                let (lx, ly) = (cx - ox, cy - oy);
                if released {
                    w.push_event(pack_event(UP, lx, ly, 0));
                } else if moved {
                    w.push_event(pack_event(MOVE, lx, ly, buttons));
                }
            }
            if released {
                self.capture = None;
            }
            return;
        }
        let over_shell = self.start_open || cy >= self.height - TASKBAR_H;
        let target = if over_shell {
            None
        } else {
            self.window_at(cx, cy).and_then(|i| {
                let w = &self.windows[i];
                (w.owner != 0).then(|| w.client_point(cx, cy).map(|p| (w.id, p))).flatten()
            })
        };
        let now_in = target.map(|(id, _)| id);
        if self.pointer_in != now_in {
            if let Some(old) = self.pointer_in {
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == old) {
                    w.push_event(pack_event(LEAVE, 0, 0, buttons));
                }
            }
            self.pointer_in = now_in;
        }
        let Some((id, (lx, ly))) = target else { return };
        let Some(w) = self.windows.iter_mut().find(|w| w.id == id) else { return };
        if pressed {
            w.push_event(pack_event(DOWN, lx, ly, buttons));
            self.capture = Some(id);
        } else if moved || released {
            w.push_event(pack_event(if released { UP } else { MOVE }, lx, ly, buttons));
        }
    }

    /// Wheel notches to the app window under the pointer (not over the shell).
    fn route_wheel(&mut self, notches: i64) -> bool {
        let (cx, cy) = self.cursor;
        if self.start_open || cy >= self.height - TASKBAR_H {
            return false;
        }
        let Some(i) = self.window_at(cx, cy) else { return false };
        let w = &mut self.windows[i];
        if w.owner == 0 {
            return false;
        }
        let Some((lx, ly)) = w.client_point(cx, cy) else { return false };
        let n = notches.clamp(-32768, 32767) as i16 as u16 as u64;
        w.push_event(pack_event(bz_abi::pointer_event::WHEEL, lx, ly, 0) | (n << 40));
        true
    }

    /// Index of the top-most window on the current workspace at (x, y).
    fn window_at(&self, x: i32, y: i32) -> Option<usize> {
        self.windows.iter().rposition(|w| {
            w.workspace == self.workspace
                && x >= w.x
                && x < w.x + w.w
                && y >= w.y
                && y < w.y + w.h
        })
    }

    fn begin_drag(&mut self, x: i32, y: i32) {
        let Some(idx) = self.window_at(x, y) else { return };
        let win = self.windows.remove(idx);
        let id = win.id;
        self.windows.push(win);
        let w = self.windows.last().unwrap();

        let in_resize = x >= w.x + w.w - RESIZE_GRIP && y >= w.y + w.h - RESIZE_GRIP;
        let in_title = y < w.y + TITLE_H;
        self.drag = if in_resize {
            Drag::Resize { id }
        } else if in_title {
            Drag::Move { id, dx: x - w.x, dy: y - w.y }
        } else {
            Drag::None
        };
    }

    fn continue_drag(&mut self, mdx: i32, mdy: i32) {
        let (cx, cy) = self.cursor;
        match self.drag {
            Drag::Move { id, dx, dy } => {
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.x = (cx - dx).clamp(0, self.width - 40);
                    w.y = (cy - dy).clamp(0, self.height - TASKBAR_H - 20);
                }
            }
            Drag::Resize { id } => {
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.w = (w.w + mdx).max(MIN_W).min(self.width - w.x);
                    w.h = (w.h + mdy).max(MIN_H).min(self.height - TASKBAR_H - w.y);
                }
            }
            Drag::None => {}
        }
    }

    /// The three title-bar button rects (min, max, close) for a window.
    fn button_rects(&self, w: &Window) -> [(i32, TitleButton); 3] {
        let right = w.x + w.w - BORDER;
        [
            (right - 3 * BTN_W, TitleButton::Minimize),
            (right - 2 * BTN_W, TitleButton::Maximize),
            (right - BTN_W, TitleButton::Close),
        ]
    }

    /// Handle a click on the desktop shell (Start button, open start menu, or a
    /// desktop icon). Returns true if the click was consumed. Launch/power
    /// requests are queued in `pending`/`pending_power` for the desktop loop.
    fn shell_click(&mut self, cx: i32, cy: i32) -> bool {
        // Right-click menu open: an item runs, anywhere else just closes it.
        if let Some((mx, my)) = self.ctx_menu.take() {
            if let Some(i) = (0..CTX_ITEMS.len()).find(|&i| hit(cx, cy, ctx_item_rect(mx, my, i))) {
                self.pending = Some(String::from(CTX_ITEMS[i].0));
            }
            return true;
        }
        // Start menu open: route the click into it, or close on an outside click.
        if self.start_open {
            let (x, y) = menu_origin(self.height);
            if cx >= x && cx < x + SM_W && cy >= y && cy < y + SM_H {
                for i in 0..CATEGORIES.len() {
                    if hit(cx, cy, rail_rect(self.height, i)) {
                        self.start_cat = i;
                        return true;
                    }
                }
                let cat = &CATEGORIES[self.start_cat];
                for (j, (name, _)) in cat.apps.iter().enumerate() {
                    if hit(cx, cy, card_rect(self.height, j)) {
                        self.pending = Some(String::from(*name));
                        self.start_open = false;
                        return true;
                    }
                }
                let py = y + SM_H - SM_FTR + 10;
                for (bx, bw, code, _) in power_buttons(self.height) {
                    if hit(cx, cy, (bx, py, bw, 26)) {
                        self.pending_power = code;
                        self.start_open = false;
                    }
                }
                return true;
            }
            self.start_open = false;
            return true; // outside click closes the menu (and is consumed)
        }
        // Start button on the taskbar.
        if cy >= self.height - TASKBAR_H && cx >= 6 && cx < 6 + START_W {
            self.start_open = true;
            return true;
        }
        // Desktop icons: double-click launches (two clicks within ~0.6 s).
        for i in 0..DESKTOP.len() {
            let (ix, iy) = icon_rect(i);
            if cx >= ix && cx < ix + ICON_W && cy >= iy && cy < iy + ICON_H {
                let now = crate::interrupts::ticks();
                if self.last_icon.0 == i && now.saturating_sub(self.last_icon.1) < 11 {
                    self.pending = Some(String::from(DESKTOP[i].0));
                    self.last_icon = (usize::MAX, 0);
                } else {
                    self.last_icon = (i, now);
                }
                return true;
            }
        }
        false
    }

    /// Which title button (if any) is under (x, y), on the top-most window there.
    fn title_button_at(&self, x: i32, y: i32) -> Option<(u32, TitleButton)> {
        let idx = self.window_at(x, y)?;
        let w = &self.windows[idx];
        if y >= w.y + TITLE_H {
            return None;
        }
        for (bx, btn) in self.button_rects(w) {
            if x >= bx && x < bx + BTN_W {
                return Some((w.id, btn));
            }
        }
        None
    }

    fn press_button(&mut self, id: u32, btn: TitleButton) {
        match btn {
            TitleButton::Close => {
                if let Some(w) = self.windows.iter().find(|w| w.id == id) {
                    if w.owner != 0 {
                        self.closed_owners.push(w.owner);
                    }
                }
                self.windows.retain(|w| w.id != id);
            }
            TitleButton::Minimize => {
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    if w.state != WinState::Minimized {
                        if w.state == WinState::Normal {
                            w.saved = (w.x, w.y, w.w, w.h);
                        }
                        w.state = WinState::Minimized;
                    }
                }
            }
            TitleButton::Maximize => self.toggle_maximize(id),
        }
    }

    fn toggle_maximize(&mut self, id: u32) {
        let (width, height) = (self.width, self.height);
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            match w.state {
                WinState::Maximized => {
                    let (sx, sy, sw, sh) = w.saved;
                    w.x = sx;
                    w.y = sy;
                    w.w = sw;
                    w.h = sh;
                    w.state = WinState::Normal;
                }
                _ => {
                    if w.state == WinState::Normal {
                        w.saved = (w.x, w.y, w.w, w.h);
                    }
                    w.x = 0;
                    w.y = 0;
                    w.w = width;
                    w.h = height - TASKBAR_H;
                    w.state = WinState::Maximized;
                }
            }
        }
    }

    /// Raise a window to the top and restore it if minimized.
    fn focus(&mut self, id: u32) {
        if let Some(idx) = self.windows.iter().position(|w| w.id == id) {
            if self.windows[idx].state == WinState::Minimized {
                let (sx, sy, sw, sh) = self.windows[idx].saved;
                let w = &mut self.windows[idx];
                w.x = sx;
                w.y = sy;
                w.w = sw;
                w.h = sh;
                w.state = WinState::Normal;
            }
            let win = self.windows.remove(idx);
            self.windows.push(win);
        }
    }

    /// Handle a click on the taskbar: desktop buttons 1-4, the clock (opens
    /// "Tanggal & Waktu"), or a window button (restore/focus).
    fn taskbar_click(&mut self, x: i32) {
        let y = self.height - TASKBAR_H + TASKBAR_H / 2;
        for ws in 0..WORKSPACES {
            if hit(x, y, ws_button_rect(self.width, self.height, ws)) {
                self.workspace = ws;
                self.drag = Drag::None;
                return;
            }
        }
        let (cx0, cw) = clock_area(self.width);
        if x >= cx0 && x < cx0 + cw {
            self.pending = Some(String::from("mwaktu"));
            return;
        }
        // Mirror the running-window button strip in draw_taskbar (after the
        // Start button). Restores/focuses the clicked window.
        let mut bx = 6 + START_W + 8;
        let ids: Vec<u32> = self
            .windows
            .iter()
            .filter(|w| w.workspace == self.workspace)
            .map(|w| w.id)
            .collect();
        for id in ids {
            if x >= bx && x < bx + 110 {
                // Un-minimize on taskbar click, then focus.
                if let Some(win) = self.windows.iter_mut().find(|w| w.id == id) {
                    if win.state == WinState::Minimized {
                        win.state = WinState::Normal;
                    }
                }
                self.focus(id);
                return;
            }
            bx += 116;
        }
    }

    fn compose(&self, canvas: &mut Canvas) {
        let th = theme::current();
        {
            let _z = crate::profile::Guard::new("compose:wallpaper");
            crate::wallpaper::paint(canvas, &th, self.workspace);
        }
        {
            let _z = crate::profile::Guard::new("compose:icons");
            let (mx, my) = self.cursor;
            let hot = (0..DESKTOP.len()).position(|i| {
                let (ix, iy) = icon_rect(i);
                mx >= ix && mx < ix + ICON_W && my >= iy && my < iy + ICON_H
            });
            let mut key = gfx::mix(theme_key(&th), hot.map_or(u64::MAX, |i| i as u64));
            key = gfx::mix(key, crate::icons::loaded() as u64);
            let (_, last_y) = icon_rect(DESKTOP.len() - 1);
            cached_layer(&ICON_LAYER, canvas, 0, 0, ICON_W + 40, last_y + ICON_H + 8, key, |c| self.draw_desktop_icons(c, &th));
        }
        let zw = crate::profile::Guard::new("compose:windows");

        let visible: Vec<&Window> = self
            .windows
            .iter()
            .filter(|w| w.workspace == self.workspace && w.state != WinState::Minimized)
            .collect();
        let top_id = visible.last().map(|w| w.id);
        let bt = th.border_thickness;
        let radius = if self.rounded && th.title_style != theme::TitleStyle::Beveled {
            8
        } else {
            0
        };

        for win in &visible {
            // In a parallel compose, a window (with its shadow) entirely in
            // the other band is that band's work.
            if !canvas.rows_visible(win.y - 10, win.h + 20) {
                continue;
            }
            let active = Some(win.id) == top_id;

            // Drop shadow per theme style.
            match th.shadow_style {
                theme::ShadowStyle::None => {}
                theme::ShadowStyle::Soft => {
                    // Only where the window does not cover it (G1: the body
                    // used to be painted over a full-size shadow).
                    shadow_rounded(canvas, win.x, win.y, win.w, win.h, 4, 5, radius, th.shadow);
                }
                theme::ShadowStyle::HardOffset => {
                    canvas.fill_rect(win.x + win.w, win.y + 8, 8, win.h, th.shadow);
                    canvas.fill_rect(win.x + 8, win.y + win.h, win.w - 8, 8, th.shadow);
                }
            }

            // Body + border (rounded). An app window's client area is about
            // to be covered by its canvas: paint only around it.
            match &win.canvas {
                Some(app) => {
                    let (aw, ah) = {
                        let a = app.lock();
                        (a.w, a.h)
                    };
                    let top = BORDER + TITLE_H;
                    let vis_w = aw.min(win.w - 2 * BORDER).max(0);
                    let vis_h = ah.min(win.h - TITLE_H - 2 * BORDER).max(0);
                    fill_rounded_rows(canvas, win.x, win.y, win.w, win.h, radius, th.win_body, 0, top);
                    fill_rounded_rows(canvas, win.x, win.y, win.w, win.h, radius, th.win_body, top + vis_h, win.h);
                    canvas.fill_rect(win.x, win.y + top, BORDER, vis_h, th.win_body);
                    canvas.fill_rect(win.x + BORDER + vis_w, win.y + top, win.w - BORDER - vis_w, vis_h, th.win_body);
                }
                None => fill_rounded(canvas, win.x, win.y, win.w, win.h, radius, th.win_body),
            }
            outline_rounded(canvas, win.x, win.y, win.w, win.h, radius, bt, th.win_border);

            let title_bg = if active { th.win_title_active } else { th.win_title_inactive };
            let title_color = if active {
                th.title_text
            } else if th.glass > 0 {
                crate::theme::LICHEN
            } else {
                th.text
            };
            match th.title_style {
                theme::TitleStyle::Tab => {
                    let tab_w = (win.w / 2).min(180).max(80);
                    canvas.fill_rect(win.x + bt, win.y + bt, tab_w, TITLE_H, title_bg);
                    canvas.draw_text(win.x + 8, win.y + 5, &win.title, title_color, win.x + tab_w);
                }
                theme::TitleStyle::Beveled => {
                    canvas.fill_rect(win.x + bt, win.y + bt, win.w - 2 * bt, TITLE_H, title_bg);
                    let light = shift(title_bg, 60);
                    canvas.fill_rect(win.x + bt, win.y + bt, win.w - 2 * bt, 2, light);
                    canvas.fill_rect(win.x + bt, win.y + bt, 2, TITLE_H, light);
                    canvas.fill_rect(win.x + bt, win.y + bt + TITLE_H - 2, win.w - 2 * bt, 2, th.win_border);
                    canvas.draw_text(win.x + 8, win.y + 5, &win.title, title_color, win.x + win.w - 8);
                }
                theme::TitleStyle::Flat => {
                    // Rounded top only: fill the title area, corners masked by body.
                    canvas.fill_rect(win.x + bt, win.y + bt, win.w - 2 * bt, TITLE_H, title_bg);
                    canvas.draw_text(win.x + 8, win.y + 5, &win.title, title_color, win.x + win.w - 3 * BTN_W - 4);
                }
            }

            // Glass themes mark the focused window with a lotus hairline.
            if active && th.glass > 0 {
                canvas.fill_rect(win.x + radius, win.y, win.w - 2 * radius, 2, th.accent);
            }

            // Title-bar control buttons (minimize / maximize / close).
            self.draw_title_buttons(canvas, win, &th, title_color);

            if let Some(app) = &win.canvas {
                let app = app.lock();
                let shown = app.front.as_ref().unwrap_or(&app.pixels);
                // Blit the app canvas into the client area, clipped to both
                // the current window size and the screen.
                let ox = win.x + BORDER;
                let oy = win.y + BORDER + TITLE_H;
                let vis_w = app.w.min(win.w - 2 * BORDER).max(0);
                let vis_h = app.h.min(win.h - TITLE_H - 2 * BORDER).max(0);
                // Row copies, clipped to the screen once (G1: this was a
                // bounds-checked put() per pixel, most of a compose).
                let cw = canvas.width as i32;
                let x0 = ox.max(0);
                let x1 = (ox + vis_w).min(cw);
                let y0 = oy.max(canvas.y0 as i32);
                let y1 = (oy + vis_h).min(canvas.y1 as i32);
                if x1 > x0 {
                    let n = (x1 - x0) as usize;
                    let src_col = (x0 - ox) as usize;
                    for py in y0..y1 {
                        let src = ((py - oy) as usize) * app.w as usize + src_col;
                        let dst = canvas.at(x0 as usize, py as usize);
                        canvas.pixels[dst..dst + n].copy_from_slice(&shown[src..src + n]);
                    }
                }
            } else {
                let mut ty = win.y + TITLE_H + 8;
                let body_bottom = win.y + win.h - 6;
                for line in &win.lines {
                    if ty > body_bottom {
                        break;
                    }
                    canvas.draw_text(win.x + 8, ty, line, th.text, win.x + win.w - 6);
                    ty += gfx::glyph_height() as i32 + 3;
                }
            }
            for i in 0..3 {
                let gx = win.x + win.w - 4 - i * 4;
                let gy = win.y + win.h - 4;
                canvas.fill_rect(gx, gy - i * 4, 3, 3, th.win_title_active);
            }
        }

        drop(zw);
        let zt = crate::profile::Guard::new("compose:taskbar");
        let key = self.taskbar_key(&th);
        cached_layer(&TASKBAR_LAYER, canvas, 0, self.height - TASKBAR_H, self.width, TASKBAR_H, key, |c| self.draw_taskbar(c, &th));
        drop(zt);
        self.draw_start_menu(canvas, &th);
        self.draw_ctx_menu(canvas, &th);
        self.draw_ripple(canvas, &th);
    }

    /// What this frame shows, element by element, for damage tracking.
    fn scene_desc(&self) -> SceneDesc {
        let th = theme::current();
        let mut global = gfx::mix(theme_key(&th), self.workspace as u64);
        global = gfx::mix_bytes(global, crate::wallpaper::label().as_bytes());
        global = gfx::mix(global, (self.width as u64) << 32 | self.height as u64);
        global = gfx::mix(global, (self.rounded as u64) << 2 | (self.animations as u64) << 1 | crate::icons::loaded() as u64);
        global = gfx::mix(global, self.cursor_scale as u64);
        // Menus and the click ripple follow the pointer over large areas:
        // while one is up (or was, see `damage`) every frame is a full one.
        let overlay = self.start_open || self.ctx_menu.is_some() || self.ripple.is_some();
        if overlay {
            global = gfx::mix(global, (self.cursor.0 as u32 as u64) << 32 | self.cursor.1 as u32 as u64);
            global = gfx::mix(global, self.start_cat as u64);
            global = gfx::mix(global, self.frame);
        }
        let (mx, my) = self.cursor;
        let hot = (0..DESKTOP.len()).position(|i| {
            let (ix, iy) = icon_rect(i);
            mx >= ix && mx < ix + ICON_W && my >= iy && my < iy + ICON_H
        });
        let top_id = self
            .windows
            .iter()
            .filter(|w| w.workspace == self.workspace && w.state != WinState::Minimized)
            .next_back()
            .map(|w| w.id);
        let windows = self
            .windows
            .iter()
            .filter(|w| w.workspace == self.workspace && w.state != WinState::Minimized)
            .map(|w| {
                let mut k = gfx::mix_bytes(gfx::mix(0, (Some(w.id) == top_id) as u64), w.title.as_bytes());
                for line in &w.lines {
                    k = gfx::mix_bytes(k, line.as_bytes());
                }
                if let Some((id, b)) = self.hover {
                    if id == w.id {
                        k = gfx::mix(k, 1 + b as u64);
                    }
                }
                if let Some(c) = &w.canvas {
                    k = gfx::mix(k, c.lock().gen);
                }
                (w.id, (w.x, w.y, w.w, w.h), k)
            })
            .collect();
        SceneDesc {
            global,
            overlay,
            icons: hot.map_or(u64::MAX, |i| i as u64),
            taskbar: self.taskbar_key(&th),
            windows,
            cursor: cursor_rect(self.cursor, self.cursor_scale),
        }
    }

    /// Everything a cursor-only frame must not change: when this key is the
    /// same as at the last full frame, moving the pointer changes nothing but
    /// the cursor's own pixels. None = not eligible (a menu is open, a drag
    /// or a ripple is running: the pointer position matters to the scene).
    fn cursor_key(&self) -> Option<u64> {
        if self.start_open || self.ctx_menu.is_some() || self.ripple.is_some() || !matches!(self.drag, Drag::None) {
            return None;
        }
        let th = theme::current();
        let (mx, my) = self.cursor;
        let hot = (0..DESKTOP.len()).position(|i| {
            let (ix, iy) = icon_rect(i);
            mx >= ix && mx < ix + ICON_W && my >= iy && my < iy + ICON_H
        });
        let mut k = gfx::mix(self.taskbar_key(&th), hot.map_or(u64::MAX, |i| i as u64));
        k = gfx::mix(k, self.hover.map_or(u64::MAX, |(id, b)| (id as u64) << 8 | b as u64));
        k = gfx::mix(k, self.cursor_scale as u64);
        k = gfx::mix(gfx::mix(k, self.rounded as u64), self.animations as u64);
        k = gfx::mix_bytes(k, crate::wallpaper::label().as_bytes());
        // A safety net for changes made without a present: the windows of
        // this desktop, where they are and what they show.
        for w in self.windows.iter().filter(|w| w.workspace == self.workspace) {
            k = gfx::mix(k, (w.x as u32 as u64) << 32 | w.y as u32 as u64);
            k = gfx::mix(k, (w.w as u32 as u64) << 32 | w.h as u32 as u64);
            for line in &w.lines {
                k = gfx::mix_bytes(k, line.as_bytes());
            }
        }
        Some(gfx::mix(k, self.capture.map_or(u64::MAX, |c| c as u64)))
    }

    /// Draw the three title-bar buttons with a hover highlight.
    fn draw_title_buttons(&self, canvas: &mut Canvas, win: &Window, th: &theme::Theme, fg: Color) {
        let ty = win.y + BORDER;
        for (bx, btn) in self.button_rects(win) {
            let hovered = self.hover == Some((win.id, btn));
            if hovered {
                let hl = if btn == TitleButton::Close {
                    gfx::rgb(0xE0, 0x48, 0x3B)
                } else {
                    shift(th.win_title_active, 40)
                };
                canvas.fill_rect(bx, ty, BTN_W, TITLE_H - 1, hl);
            }
            let cx = bx + BTN_W / 2;
            let cy = ty + TITLE_H / 2;
            let c = if hovered && btn == TitleButton::Close { gfx::rgb(0xFF, 0xFF, 0xFF) } else { fg };
            match btn {
                TitleButton::Minimize => canvas.fill_rect(cx - 4, cy + 4, 8, 2, c),
                TitleButton::Maximize => {
                    canvas.rect_outline(cx - 5, cy - 5, 10, 10, 1, c);
                }
                TitleButton::Close => {
                    canvas.draw_line(cx - 4, cy - 4, cx + 4, cy + 4, c);
                    canvas.draw_line(cx + 4, cy - 4, cx - 4, cy + 4, c);
                }
            }
        }
    }

    /// Expanding click ripple (a micro-interaction) for a few frames.
    fn draw_ripple(&self, canvas: &mut Canvas, th: &theme::Theme) {
        if !self.animations {
            return;
        }
        if let Some((rx, ry, start)) = self.ripple {
            let age = self.frame.saturating_sub(start) as i32;
            if age <= 8 {
                let r = 4 + age * 4;
                canvas.ellipse(rx - r, ry - r, 2 * r, 2 * r, th.accent, false);
            }
        }
    }

    /// Everything `draw_taskbar` reads besides the pixels under the bar (the
    /// layer cache hashes those itself). Keep in sync with `draw_taskbar`.
    fn taskbar_key(&self, th: &theme::Theme) -> u64 {
        let mut k = gfx::mix(theme_key(th), self.start_open as u64);
        k = gfx::mix(k, self.workspace as u64);
        let (cx, cy) = self.cursor;
        let ws_hover = (0..WORKSPACES).position(|ws| hit(cx, cy, ws_button_rect(self.width, self.height, ws)));
        k = gfx::mix(k, ws_hover.map_or(u64::MAX, |i| i as u64));
        let (cx0, cw) = clock_area(self.width);
        k = gfx::mix(k, hit(cx, cy, (cx0, self.height - TASKBAR_H + 5, cw, TASKBAR_H - 10)) as u64);
        let (t, tn) = tray_time();
        k = gfx::mix_bytes(k, &t[..tn]);
        let (day, d, mon) = today();
        k = gfx::mix_bytes(gfx::mix_bytes(gfx::mix(k, d as u64), day.as_bytes()), mon.as_bytes());
        let top_id = self
            .windows
            .iter()
            .filter(|w| w.workspace == self.workspace && w.state != WinState::Minimized)
            .next_back()
            .map(|w| w.id);
        k = gfx::mix(k, top_id.map_or(u64::MAX, |i| i as u64));
        for w in self.windows.iter().filter(|w| w.workspace == self.workspace) {
            let st = match w.state {
                WinState::Normal => 0,
                WinState::Minimized => 1,
                WinState::Maximized => 2,
            };
            k = gfx::mix_bytes(gfx::mix(gfx::mix(k, w.id as u64), st), w.title.as_bytes());
        }
        k
    }

    fn draw_taskbar(&self, canvas: &mut Canvas, th: &theme::Theme) {
        let y = self.height - TASKBAR_H;
        let glass = th.glass > 0;
        // The bar: canopy glass over the wallpaper with a moss hairline on top
        // (classic themes keep their solid gradient + accent rule).
        if glass {
            blend_rounded(canvas, 0, y, self.width, TASKBAR_H, 0, th.taskbar_bg, th.glass);
            canvas.fill_rect(0, y, self.width, 1, th.win_border);
        } else {
            gradient_v(canvas, 0, y, self.width, TASKBAR_H, shift(th.taskbar_bg, 14), th.taskbar_bg);
            canvas.fill_rect(0, y, self.width, 1, th.accent);
        }

        // --- Start button: a leaf mark + "Mulai" ---------------------------
        if glass {
            let bg = if self.start_open { shift(th.taskbar_bg, 34) } else { shift(th.taskbar_bg, 16) };
            fill_rounded(canvas, 6, y + 5, START_W, TASKBAR_H - 10, 7, bg);
            if self.start_open {
                canvas.fill_rect(14, y + TASKBAR_H - 7, START_W - 16, 2, th.accent);
            }
        } else {
            let base = if self.start_open { shift(th.win_title_active, 30) } else { th.win_title_active };
            fill_rounded_gradient(canvas, 6, y + 4, START_W, TASKBAR_H - 8, 8, shift(base, 40), base);
        }
        let leaf = if glass { th.accent } else { th.title_text };
        canvas.ellipse(14, y + 9, 9, 15, leaf, true); // the leaf
        canvas.draw_line(18, y + 11, 18, y + 23, if glass { th.taskbar_bg } else { th.win_title_active }); // midrib
        let label_c = if glass { th.text } else { th.title_text };
        canvas.draw_text_styled(28, y + 9, "Mulai", label_c, 6 + START_W - 2, gfx::TextStyle::Strong);

        // --- tray (right): date + time (opens Tanggal & Waktu), desktops 1-4 --
        for ws in 0..WORKSPACES {
            let (bx, by, bw, bh) = ws_button_rect(self.width, self.height, ws);
            let sel = ws == self.workspace;
            let hover = hit(self.cursor.0, self.cursor.1, (bx, by, bw, bh));
            let bg = if sel { th.accent } else if hover { shift(th.taskbar_bg, 40) } else { shift(th.taskbar_bg, 18) };
            fill_rounded(canvas, bx, by, bw, bh, 5, bg);
            let label = [b'1' + ws];
            let ls = core::str::from_utf8(&label).unwrap();
            let fg = if sel { th.taskbar_bg } else { th.text };
            let lx = bx + (bw - gfx::text_width(ls, gfx::TextStyle::Strong)) / 2;
            canvas.draw_text_styled(lx, by + (bh - 16) / 2, ls, fg, bx + bw, gfx::TextStyle::Strong);
        }
        let (cx0, cw) = clock_area(self.width);
        if hit(self.cursor.0, self.cursor.1, (cx0, y + 5, cw, TASKBAR_H - 10)) {
            fill_rounded(canvas, cx0, y + 5, cw, TASKBAR_H - 10, 6, shift(th.taskbar_bg, 18));
        }
        let mut rx = cx0 + cw - 6;
        let (t, tn) = tray_time();
        let ts = core::str::from_utf8(&t[..tn]).unwrap();
        let tw = gfx::text_width(ts, gfx::TextStyle::Strong);
        rx -= tw;
        let time_c = if glass { crate::theme::POLLEN } else { th.accent };
        canvas.draw_text_styled(rx, y + 9, ts, time_c, rx + tw + 1, gfx::TextStyle::Strong);
        let (day, d, mon) = today();
        let mut dbuf = [b' '; 2];
        two_digits(&mut dbuf, d);
        let dd = if dbuf[0] == b'0' { core::str::from_utf8(&dbuf[1..]).unwrap() } else { core::str::from_utf8(&dbuf).unwrap() };
        let date_c = if glass { crate::theme::LICHEN } else { th.text };
        // "Sab 1 Nov" — the short day keeps the tray compact.
        let short = &day[..3];
        let date_w = gfx::text_width(short, gfx::TextStyle::Body)
            + gfx::text_width(dd, gfx::TextStyle::Body)
            + gfx::text_width(mon, gfx::TextStyle::Body)
            + 2 * gfx::glyph_width() as i32;
        rx -= date_w + 12;
        let mut dx = canvas.draw_text_styled(rx, y + 9, short, date_c, rx + date_w, gfx::TextStyle::Body);
        dx = canvas.draw_text_styled(dx + gfx::glyph_width() as i32, y + 9, dd, date_c, rx + date_w + 2, gfx::TextStyle::Body);
        canvas.draw_text_styled(dx + gfx::glyph_width() as i32, y + 9, mon, date_c, rx + date_w + 4, gfx::TextStyle::Body);

        // --- running windows: quiet labels, the focused one underlined -------
        let top_id = self
            .windows
            .iter()
            .filter(|w| w.workspace == self.workspace && w.state != WinState::Minimized)
            .next_back()
            .map(|w| w.id);
        let start_x = 6 + START_W + 8; // keep in sync with taskbar_click
        let mut bx = start_x;
        for win in self.windows.iter().filter(|w| w.workspace == self.workspace) {
            if bx + 116 > rx - 4 {
                break; // no room before the tray
            }
            let active = Some(win.id) == top_id;
            if glass {
                if active {
                    fill_rounded(canvas, bx, y + 5, 110, TASKBAR_H - 10, 6, shift(th.taskbar_bg, 20));
                    canvas.fill_rect(bx + 10, y + TASKBAR_H - 7, 90, 2, th.accent);
                }
                let tc = if active {
                    th.text
                } else if win.state == WinState::Minimized {
                    darken(crate::theme::LICHEN, 50)
                } else {
                    crate::theme::LICHEN
                };
                canvas.draw_text(bx + 10, y + 9, &win.title, tc, bx + 104);
            } else {
                if active {
                    fill_rounded_gradient(canvas, bx, y + 5, 110, TASKBAR_H - 10, 5, shift(th.win_title_active, 26), th.win_title_active);
                } else if win.state == WinState::Minimized {
                    fill_rounded(canvas, bx, y + 5, 110, TASKBAR_H - 10, 5, shift(th.taskbar_bg, 22));
                } else {
                    fill_rounded(canvas, bx, y + 5, 110, TASKBAR_H - 10, 5, th.win_title_inactive);
                }
                let tc = if active { th.title_text } else { th.text };
                canvas.draw_text(bx + 8, y + 9, &win.title, tc, bx + 108);
            }
            bx += 116;
        }
    }

    /// The start menu — a herbarium drawer: header (name + today), category
    /// rail with each category's leaf color, the chosen category's apps as
    /// icon cards, and power actions in the footer.
    /// The desktop's right-click menu: a panel of rows, the one under the
    /// pointer highlighted.
    fn draw_ctx_menu(&self, canvas: &mut Canvas, th: &theme::Theme) {
        let Some((x, y)) = self.ctx_menu else { return };
        let h = ctx_height();
        blend_rounded(canvas, x + 3, y + 4, CTX_W, h, 10, th.shadow, 100);
        panel(canvas, th, x, y, CTX_W, h, 10);
        if th.glass > 0 {
            blend_rounded(canvas, x, y, CTX_W, h, 10, th.taskbar_bg, 120);
        }
        outline_rounded(canvas, x, y, CTX_W, h, 10, 1, th.win_border);
        let (cx, cy) = self.cursor;
        for (i, (name, label)) in CTX_ITEMS.iter().enumerate() {
            let (rx, ry, rw, rh) = ctx_item_rect(x, y, i);
            if hit(cx, cy, (rx, ry, rw, rh)) {
                fill_rounded(canvas, rx, ry, rw, rh, 6, shift(th.taskbar_bg, 40));
            }
            if !name.starts_with('@') {
                draw_app_icon(canvas, name, label, rx + 6, ry + (rh - crate::icons::size(false)) / 2, false);
            }
            canvas.draw_text(rx + 34, ry + (rh - 16) / 2, label, th.text, rx + rw);
            if i == 0 || i == 3 {
                canvas.fill_rect(rx + 4, ry + rh - 1, rw - 8, 1, th.win_border);
            }
        }
    }

    fn draw_start_menu(&self, canvas: &mut Canvas, th: &theme::Theme) {
        if !self.start_open {
            return;
        }
        let (x, y) = menu_origin(self.height);
        let (mx, my) = self.cursor;
        let glass = th.glass > 0;
        let text = th.text;
        let muted = if glass { crate::theme::LICHEN } else { darken(th.text, 40) };
        let line = th.win_border;

        // Panel with a soft shadow, then hairline frame.
        blend_rounded(canvas, x + 4, y + 6, SM_W, SM_H, 12, th.shadow, 110);
        panel(canvas, th, x, y, SM_W, SM_H, 12);
        if glass {
            // A second pass makes the menu denser than the taskbar, so app
            // labels stay legible over any wallpaper.
            blend_rounded(canvas, x, y, SM_W, SM_H, 12, th.taskbar_bg, 120);
        }
        outline_rounded(canvas, x, y, SM_W, SM_H, 12, 1, line);

        // Header: OS name (strong) + today's date (muted), hairline below.
        canvas.draw_text_styled(x + 18, y + 15, "Buitenzorg OS", text, x + SM_W / 2, gfx::TextStyle::Strong);
        let (day, d, mon) = today();
        let mut db = [0u8; 2];
        two_digits(&mut db, d);
        let dd = if db[0] == b'0' { core::str::from_utf8(&db[1..]).unwrap() } else { core::str::from_utf8(&db).unwrap() };
        let date_w = gfx::text_width(day, gfx::TextStyle::Body) + gfx::text_width(dd, gfx::TextStyle::Body)
            + gfx::text_width(mon, gfx::TextStyle::Body) + 2 * gfx::glyph_width() as i32 + 8;
        let mut tx = x + SM_W - 18 - date_w;
        tx = canvas.draw_text_styled(tx, y + 15, day, muted, x + SM_W, gfx::TextStyle::Body);
        tx = canvas.draw_text_styled(tx, y + 15, ",", muted, x + SM_W, gfx::TextStyle::Body);
        tx = canvas.draw_text_styled(tx + 8, y + 15, dd, muted, x + SM_W, gfx::TextStyle::Body);
        canvas.draw_text_styled(tx + 8, y + 15, mon, muted, x + SM_W, gfx::TextStyle::Body);
        canvas.fill_rect(x + 1, y + SM_HDR, SM_W - 2, 1, line);

        // Category rail.
        for (i, cat) in CATEGORIES.iter().enumerate() {
            let (rx, ry, rw, rh) = rail_rect(self.height, i);
            let sel = i == self.start_cat;
            if sel {
                fill_rounded(canvas, rx, ry, rw, rh, 7, shift(th.taskbar_bg, 26));
                fill_rounded(canvas, rx, ry + 7, 4, rh - 14, 2, cat.leaf);
            } else {
                canvas.ellipse(rx + 2, ry + rh / 2 - 3, 6, 6, cat.leaf, true);
            }
            let style = if sel { gfx::TextStyle::Strong } else { gfx::TextStyle::Body };
            canvas.draw_text_styled(rx + 16, ry + (rh - 16) / 2, cat.name, if sel { text } else { muted }, rx + rw - 22, style);
            // How many apps the category holds.
            let n = [b'0' + cat.apps.len() as u8];
            let ns = core::str::from_utf8(&n).unwrap();
            canvas.draw_text_styled(rx + rw - 14, ry + (rh - 16) / 2, ns, muted, rx + rw, gfx::TextStyle::Body);
        }
        canvas.fill_rect(x + SM_RAIL_W, y + SM_HDR + 10, 1, SM_H - SM_HDR - SM_FTR - 20, line);

        // Content pane: the category name as a heading, ruled in its leaf color.
        let cat = &CATEGORIES[self.start_cat];
        let px = x + SM_RAIL_W + 20;
        canvas.draw_text_styled(px, y + SM_HDR + 18, cat.name, text, x + SM_W - 16, gfx::TextStyle::Heading);
        canvas.fill_rect(px + 1, y + SM_HDR + 50, 36, 3, cat.leaf);

        for (j, (name, label)) in cat.apps.iter().enumerate() {
            let (cx, cy, cw, ch) = card_rect(self.height, j);
            let hot = hit(mx, my, (cx, cy, cw, ch));
            if hot {
                fill_rounded(canvas, cx, cy, cw, ch, 10, shift(th.taskbar_bg, 24));
                outline_rounded(canvas, cx, cy, cw, ch, 10, 1, shift(line, 20));
            }
            let isz = crate::icons::size(true);
            draw_app_icon(canvas, name, label, cx + (cw - isz) / 2, cy + 10, true);
            let lw = gfx::text_width(label, gfx::TextStyle::Body).min(cw - 6);
            canvas.draw_text(cx + (cw - lw) / 2, cy + 10 + isz + 10, label, if hot { text } else { muted }, cx + cw - 3);
        }

        // Footer: credit on the left, power actions on the right.
        let fy = y + SM_H - SM_FTR;
        canvas.fill_rect(x + 1, fy, SM_W - 2, 1, line);
        canvas.draw_text_styled(x + 18, fy + 15, "Gravicode Studios", muted, x + SM_W / 2, gfx::TextStyle::Body);
        for (bx, bw, code, label) in power_buttons(self.height) {
            let by = fy + 10;
            let hot = hit(mx, my, (bx, by, bw, 26));
            if hot {
                let c = if code == 1 { gfx::rgb(0xC0, 0x39, 0x2B) } else { shift(th.taskbar_bg, 30) };
                fill_rounded(canvas, bx, by, bw, 26, 7, c);
            } else {
                outline_rounded(canvas, bx, by, bw, 26, 7, 1, line);
            }
            let c = if hot && code == 1 { gfx::rgb(0xFF, 0xFF, 0xFF) } else if hot { text } else { muted };
            canvas.draw_text(bx + 10, by + 5, label, c, bx + bw - 4);
        }
    }

    /// Desktop launcher icons down the top-left, each label on a small glass
    /// pill so it reads over bright or busy wallpapers.
    fn draw_desktop_icons(&self, canvas: &mut Canvas, th: &theme::Theme) {
        let (mx, my) = self.cursor;
        for (i, (name, label)) in DESKTOP.iter().enumerate() {
            let (ix, iy) = icon_rect(i);
            let hot = mx >= ix && mx < ix + ICON_W && my >= iy && my < iy + ICON_H;
            if hot {
                blend_rounded(canvas, ix, iy, ICON_W, ICON_H, 8, th.taskbar_bg, 110);
            }
            let isz = crate::icons::size(true);
            let tx = ix + (ICON_W - isz) / 2;
            blend_rounded(canvas, tx + 1, iy + 6, isz, isz, 10, th.shadow, 120);
            draw_app_icon(canvas, name, label, tx, iy + 3, true);
            let lw = gfx::text_width(label, gfx::TextStyle::Body);
            let lx = ix + (ICON_W - lw) / 2;
            let ly = iy + 3 + isz + 4;
            blend_rounded(canvas, lx - 5, ly - 1, lw + 10, 18, 8, th.taskbar_bg, if th.glass > 0 { 150 } else { 0 });
            canvas.draw_text(lx, ly, label, if th.glass > 0 { th.text } else { th.title_text }, ix + ICON_W + 8);
        }
    }

}

/// What a presented frame showed, element by element (damage tracking).
#[derive(Clone, PartialEq)]
struct SceneDesc {
    /// Theme, wallpaper, desktop, options, open menus: any change = full frame.
    global: u64,
    /// A menu or the ripple was up (then the next frame is a full one too).
    overlay: bool,
    icons: u64,
    taskbar: u64,
    /// Visible windows back to front: id, rect, content key.
    windows: Vec<(u32, (i32, i32, i32, i32), u64)>,
    cursor: (i32, i32, i32, i32),
}

/// What is on the screen now (None = unknown: present everything next time).
static ON_SCREEN: Mutex<Option<SceneDesc>> = Mutex::new(None);
/// Damage frames since the last full present (a full one every so often is
/// the safety net for any change the description misses).
static SINCE_FULL: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static DAMAGE_FRAMES: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static DAMAGE_ROWS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// (frames presented by damage, rows they presented).
pub fn damage_stats() -> (u64, u64) {
    use core::sync::atomic::Ordering::Relaxed;
    (DAMAGE_FRAMES.load(Relaxed), DAMAGE_ROWS.load(Relaxed))
}

/// Something other than the compositor drew on the screen (screensaver,
/// splash, a direct full present): the next frame presents everything.
pub fn screen_overwritten() {
    if let Some(mut g) = ON_SCREEN.try_lock() {
        *g = None;
    } else {
        SINCE_FULL.store(u32::MAX / 2, core::sync::atomic::Ordering::Relaxed);
    }
    // the screensaver and friends must not show the pointer floating above
    hide_hw_cursors();
}

// --- Hardware cursors (G2) ---------------------------------------------------
//
// An output that can show the pointer itself gets frames without the
// software cursor and a move command per pointer move: a virtio GPU (its
// cursor queue) and the Raspberry Pi's firmware framebuffer (an HVS plane,
// through the mailbox). The other outputs keep the software cursor.

struct HwCursor {
    /// Image key (theme colors + scale) of the uploaded image; 0 = none.
    key: u64,
    gpu: bool,
    fb: bool,
    pos: (i32, i32),
}

static HW_CURSOR: Mutex<HwCursor> = Mutex::new(HwCursor { key: 0, gpu: false, fb: false, pos: (-1, -1) });
static HW_CURSOR_ENABLED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

/// Use hardware cursors where an output has one (default on).
pub fn set_hw_cursor(on: bool) {
    HW_CURSOR_ENABLED.store(on, core::sync::atomic::Ordering::Relaxed);
    if !on {
        hide_hw_cursors();
    }
}

/// (virtio GPU, board framebuffer) currently show the cursor themselves.
pub fn hw_cursor_outputs() -> (bool, bool) {
    let h = HW_CURSOR.lock();
    (h.gpu, h.fb)
}

fn hide_hw_cursors() {
    let Some(mut h) = HW_CURSOR.try_lock() else { return };
    if h.gpu {
        crate::virtio::gpu::hide_cursor();
    }
    if h.fb {
        crate::hwio::fb_cursor_state(false, 0, 0);
    }
    *h = HwCursor { key: 0, gpu: false, fb: false, pos: (-1, -1) };
}

/// The cursor as an ARGB image of `size`^2 (opaque shape, clear elsewhere).
fn cursor_image(th: &theme::Theme, scale: i32, size: usize) -> Vec<u32> {
    let mut img = vec![0u32; size * size];
    for dy in 0..12 {
        let len = 12 - dy;
        for dx in 0..len {
            let edge = dx == 0 || dx == len - 1 || dy == 0;
            let c = 0xFF00_0000 | if edge { th.win_border } else { th.cursor };
            for sy in 0..scale {
                for sx in 0..scale {
                    let (x, y) = ((dx * scale + sx) as usize, (dy * scale + sy) as usize);
                    if x < size && y < size {
                        img[y * size + x] = c;
                    }
                }
            }
        }
    }
    img
}

/// Bring the hardware cursors to `cursor` (uploading the image when the
/// theme or scale changed). Returns (virtio GPU shows it, board fb shows it).
fn sync_hw_cursors(th: &theme::Theme, cursor: (i32, i32), scale: i32) -> (bool, bool) {
    if !HW_CURSOR_ENABLED.load(core::sync::atomic::Ordering::Relaxed) {
        return (false, false);
    }
    let Some(mut h) = HW_CURSOR.try_lock() else { return (false, false) };
    let key = gfx::mix(gfx::mix(gfx::mix(1, th.cursor as u64), th.win_border as u64), scale as u64);
    if key != h.key {
        h.gpu = crate::virtio::gpu::cursor_capable()
            && crate::virtio::gpu::set_cursor(&cursor_image(th, scale, crate::virtio::gpu::CURSOR_SIZE), cursor.0, cursor.1);
        let fs = crate::hwio::fb_cursor_size();
        let was_fb = h.fb;
        h.fb = fs >= (12 * scale) as usize
            && crate::hwio::fb_cursor_image(&cursor_image(th, scale, fs))
            && crate::hwio::fb_cursor_state(true, cursor.0, cursor.1);
        if h.fb && !was_fb {
            crate::println!("[wm] firmware hardware cursor on ({}x{} ARGB plane)", fs, fs);
        }
        h.key = key;
        h.pos = cursor;
    } else if h.pos != cursor {
        if h.gpu {
            crate::virtio::gpu::move_cursor(cursor.0, cursor.1);
        }
        if h.fb {
            h.fb = crate::hwio::fb_cursor_state(true, cursor.0, cursor.1);
        }
        h.pos = cursor;
    }
    (h.gpu, h.fb)
}

/// Rows that differ between two scenes, merged; None = present everything.
fn damage(old: &SceneDesc, new: &SceneDesc, width: i32, height: i32) -> Option<Vec<(usize, usize)>> {
    if old.global != new.global || old.overlay || new.overlay {
        return None;
    }
    let mut rows: Vec<(i32, i32)> = Vec::new();
    let mut add = |r: (i32, i32, i32, i32), margin: i32| rows.push((r.1 - margin, r.1 + r.3 + margin));
    if old.icons != new.icons {
        let (_, last_y) = icon_rect(DESKTOP.len() - 1);
        add((0, 0, ICON_W + 40, last_y + ICON_H + 8), 0);
    }
    if old.taskbar != new.taskbar {
        add((0, height - TASKBAR_H, width, TASKBAR_H), 0);
    }
    if old.cursor != new.cursor {
        add(old.cursor, 0);
        add(new.cursor, 0);
    }
    // A window counts as changed when its place in the stack, its rect or
    // its content changed; its old and new rects (with the shadow) are damage.
    const SHADOW: i32 = 10;
    for (i, w) in new.windows.iter().enumerate() {
        match old.windows.iter().position(|o| o.0 == w.0) {
            Some(j) if j == i && old.windows[j] == *w => {}
            Some(j) => {
                add(old.windows[j].1, SHADOW);
                add(w.1, SHADOW);
            }
            None => add(w.1, SHADOW),
        }
    }
    for (j, o) in old.windows.iter().enumerate() {
        match new.windows.iter().position(|w| w.0 == o.0) {
            Some(i) if i == j => {}
            Some(_) => add(o.1, SHADOW),
            None => add(o.1, SHADOW),
        }
    }
    rows.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (a, b) in rows {
        let (a, b) = (a.clamp(0, height) as usize, b.clamp(0, height) as usize);
        if b <= a {
            continue;
        }
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    Some(out)
}

/// A composed frame's pointer: where, how large, and (for the desktop's own
/// frames) the cursor key a later cursor-only frame must match.
struct FramePointer {
    pos: (i32, i32),
    scale: i32,
    key: Option<u64>,
}

/// Present a composed frame `back` (without the pointer) showing `scene`: the
/// rows that changed since the frame on the screen, or everything when
/// unknown. Outputs with a hardware cursor get the frame as it is and a
/// cursor move; for the others the software cursor is drawn in first.
/// `record` keeps what a cursor-only frame needs (the desktop's frames).
fn present_scene(back: &mut [u32], width: usize, height: usize, scene: SceneDesc, ptr: FramePointer, record: bool) {
    use core::sync::atomic::Ordering::Relaxed;
    let th = theme::current();
    let (gpu_hw, fb_hw) = sync_hw_cursors(&th, ptr.pos, ptr.scale);
    let gpu_on = crate::virtio::gpu::active();
    let mut on_screen = ON_SCREEN.lock();
    let bands = match on_screen.as_ref() {
        Some(old) if SINCE_FULL.load(Relaxed) < 64 => damage(old, &scene, width as i32, height as i32),
        _ => None,
    };
    match &bands {
        Some(bands) => {
            SINCE_FULL.fetch_add(1, Relaxed);
            DAMAGE_FRAMES.fetch_add(1, Relaxed);
            for &(y0, y1) in bands {
                DAMAGE_ROWS.fetch_add((y1 - y0) as u64, Relaxed);
            }
        }
        None => SINCE_FULL.store(0, Relaxed),
    }
    let to_gpu = |back: &[u32]| match &bands {
        Some(b) => crate::virtio::gpu::present_bands(back, b),
        None => crate::virtio::gpu::present(back),
    };
    // 1. a GPU that draws the pointer itself gets the frame without it
    if gpu_on && gpu_hw {
        to_gpu(back);
    }
    // 2. the software cursor, for the outputs without one
    let r = clip_rect(cursor_rect(ptr.pos, ptr.scale), width, height);
    let sw = !fb_hw || (gpu_on && !gpu_hw);
    if record {
        let mut under = Vec::new();
        save_under(back, width, r, &mut under);
        *CURSOR_FRAME.lock() = ptr.key.map(|key| CursorFrame {
            seq: COMPOSE_SEQ.load(core::sync::atomic::Ordering::Acquire),
            key,
            rect: r,
            under,
            sw,
            hw: (gpu_hw, fb_hw),
        });
    }
    if sw {
        draw_cursor_at(&mut Canvas::new(back, width, height), &th, ptr.pos, ptr.scale);
    }
    // 3. the console framebuffer, and a GPU without its own cursor
    crate::framebuffer::present_console(back, bands.as_deref());
    if gpu_on && !gpu_hw {
        to_gpu(back);
    }
    *on_screen = Some(scene);
}

/// The pointer's bounding box: (x, y, w, h).
fn cursor_rect(cursor: (i32, i32), scale: i32) -> (i32, i32, i32, i32) {
    (cursor.0, cursor.1, 12 * scale, 12 * scale)
}

fn draw_cursor_at(canvas: &mut Canvas, th: &theme::Theme, cursor: (i32, i32), scale: i32) {
    {
        let (cx, cy) = cursor;
        let s = scale;
        for dy in 0..12 {
            let len = 12 - dy;
            for dx in 0..len {
                let edge = dx == 0 || dx == len - 1 || dy == 0;
                let color = if edge { th.win_border } else { th.cursor };
                // Scale each cursor pixel into an s×s block.
                canvas.fill_rect(cx + dx * s, cy + dy * s, s, s, color);
            }
        }
    }
}

/// Lighten/shift a color by `amount` per channel (per-workspace wallpaper).
pub fn shift(color: Color, amount: u32) -> Color {
    let ch = |sh: u32| -> u32 { (((color >> sh) & 0xFF) + amount).min(0xFF) };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// Darken a color by `amount` per channel (saturating at 0).
pub fn darken(color: Color, amount: u32) -> Color {
    let ch = |sh: u32| -> u32 { ((color >> sh) & 0xFF).saturating_sub(amount) };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// Horizontal inset of a rounded corner at row `dy` from the nearest corner
/// (0..radius), i.e. how many pixels to skip so the corner looks round.
fn corner_inset(dy: i32, radius: i32) -> i32 {
    if radius <= 0 || dy >= radius {
        return 0;
    }
    // inset = radius - sqrt(radius^2 - (radius-1-dy)^2)
    let d = radius - 1 - dy;
    let r2 = (radius * radius) as i64;
    let inner = r2 - (d * d) as i64;
    let s = isqrt(inner.max(0) as u64) as i32;
    (radius - s).max(0)
}

fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Fill a rectangle with rounded corners.
fn fill_rounded(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, radius: i32, color: Color) {
    if radius <= 0 {
        canvas.fill_rect(x, y, w, h, color);
        return;
    }
    for row in 0..h {
        let inset = if row < radius {
            corner_inset(row, radius)
        } else if row >= h - radius {
            corner_inset(h - 1 - row, radius)
        } else {
            0
        };
        canvas.fill_rect(x + inset, y + row, w - 2 * inset, 1, color);
    }
}

/// Rows `from..to` (relative to `y`) of a rounded rectangle.
fn fill_rounded_rows(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, radius: i32, color: Color, from: i32, to: i32) {
    for row in from.max(0)..to.min(h) {
        let inset = if radius > 0 && row < radius {
            corner_inset(row, radius)
        } else if radius > 0 && row >= h - radius {
            corner_inset(h - 1 - row, radius)
        } else {
            0
        };
        canvas.fill_rect(x + inset, y + row, w - 2 * inset, 1, color);
    }
}

/// The part of a rounded drop shadow (the window's shape moved by dx, dy)
/// that the window itself does not cover: everything right of / below the
/// window, plus the bands where the window's own corners are cut away.
fn shadow_rounded(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, dx: i32, dy: i32, radius: i32, color: Color) {
    let (sx, sy) = (x + dx, y + dy);
    for row in 0..h {
        let inset = if radius > 0 && row < radius {
            corner_inset(row, radius)
        } else if radius > 0 && row >= h - radius {
            corner_inset(h - 1 - row, radius)
        } else {
            0
        };
        let py = sy + row;
        let (l, r) = (sx + inset, sx + w - inset);
        // rows of the window itself (outside its corner bands) hide the
        // shadow up to the window's right edge
        let wrow = py - y;
        let covered_to = if wrow >= radius && wrow < h - radius { x + w } else { l };
        let from = l.max(covered_to);
        if r > from {
            canvas.fill_rect(from, py, r - from, 1, color);
        }
    }
}

/// Blend a rounded rectangle of `color` at `alpha` over what is underneath —
/// the glass panels of themes with `glass > 0`.
fn blend_rounded(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, radius: i32, color: Color, alpha: u8) {
    for row in 0..h {
        let inset = if radius > 0 && row < radius {
            corner_inset(row, radius)
        } else if radius > 0 && row >= h - radius {
            corner_inset(h - 1 - row, radius)
        } else {
            0
        };
        let py = y + row;
        if py < 0 {
            continue;
        }
        for px in (x + inset)..(x + w - inset) {
            if px >= 0 {
                canvas.blend(px as usize, py as usize, color, alpha);
            }
        }
    }
}

/// A shell panel: glass over the wallpaper, or a solid fill on classic themes.
fn panel(canvas: &mut Canvas, th: &theme::Theme, x: i32, y: i32, w: i32, h: i32, radius: i32) {
    if th.glass > 0 {
        blend_rounded(canvas, x, y, w, h, radius, th.taskbar_bg, th.glass);
    } else {
        fill_rounded(canvas, x, y, w, h, radius, shift(th.win_body, 6));
    }
}

/// Linearly interpolate each channel of `a`..`b` at row t/total.
fn lerp(a: Color, b: Color, t: i32, total: i32) -> Color {
    if total <= 0 {
        return a;
    }
    let ch = |sh: u32| -> u32 {
        let ca = ((a >> sh) & 0xFF) as i32;
        let cb = ((b >> sh) & 0xFF) as i32;
        (ca + (cb - ca) * t / total).clamp(0, 255) as u32
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// Fill a plain rectangle with a top→bottom vertical gradient.
fn gradient_v(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, top: Color, bottom: Color) {
    for row in 0..h {
        canvas.fill_rect(x, y + row, w, 1, lerp(top, bottom, row, h - 1));
    }
}

/// Fill a rounded rectangle with a top→bottom vertical gradient.
fn fill_rounded_gradient(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, radius: i32, top: Color, bottom: Color) {
    for row in 0..h {
        let inset = if row < radius {
            corner_inset(row, radius)
        } else if row >= h - radius {
            corner_inset(h - 1 - row, radius)
        } else {
            0
        };
        canvas.fill_rect(x + inset, y + row, w - 2 * inset, 1, lerp(top, bottom, row, h - 1));
    }
}

/// Top-left screen rect of desktop icon `i` (a vertical column).
/// Theme fields the shell layers are drawn with (a frame-cache input).
fn theme_key(th: &theme::Theme) -> u64 {
    let mut k = gfx::mix_bytes(0x7468_656d_65, th.name.as_bytes());
    for c in [th.taskbar_bg, th.win_border, th.accent, th.text, th.title_text, th.win_title_active, th.win_title_inactive, th.shadow, th.cursor] {
        k = gfx::mix(k, c as u64);
    }
    gfx::mix(gfx::mix(k, th.glass as u64), th.dark as u64)
}

/// A rectangle of the composed frame kept with the key it was drawn under.
struct LayerCache {
    key: u64,
    rect: (usize, usize, usize, usize),
    px: Vec<u32>,
}

static ICON_LAYER: Mutex<Option<LayerCache>> = Mutex::new(None);
static TASKBAR_LAYER: Mutex<Option<LayerCache>> = Mutex::new(None);
static LAYER_HITS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static LAYER_MISSES: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// (cache hits, misses) of the shell layers since boot.
pub fn layer_stats() -> (u64, u64) {
    use core::sync::atomic::Ordering::Relaxed;
    (LAYER_HITS.load(Relaxed), LAYER_MISSES.load(Relaxed))
}

/// Draw a layer that only touches the rectangle (x, y, w, h), reusing the last
/// result when both the pixels already there and `inputs` (everything else
/// the drawing reads) are unchanged. `draw` must not touch pixels outside the
/// rectangle: on a hit only the rectangle is restored.
fn cached_layer(cache: &Mutex<Option<LayerCache>>, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, inputs: u64, draw: impl FnOnce(&mut Canvas)) {
    use core::sync::atomic::Ordering::Relaxed;
    // Clipped to the rows this canvas holds (a band in a parallel compose:
    // each band then caches its own part -- the rect is part of the key).
    let x0 = x.max(0) as usize;
    let y0 = (y.max(0) as usize).max(canvas.y0);
    let x1 = ((x + w).max(0) as usize).min(canvas.width);
    let y1 = ((y + h).max(0) as usize).min(canvas.y1);
    if x1 <= x0 || y1 <= y0 {
        return; // nothing of it in this band (`draw` stays inside the rect)
    }
    // Another CPU composing at the same time: just draw.
    let guard = cache.try_lock();
    let Some(mut guard) = guard else {
        draw(canvas);
        return;
    };
    let rw = x1 - x0;
    let mut key = gfx::mix(inputs, (x0 | y0 << 16 | x1 << 32 | y1 << 48) as u64);
    for row in y0..y1 {
        let r = canvas.at(x0, row);
        key = gfx::hash_px(key, &canvas.pixels[r..r + rw]);
    }
    if let Some(c) = guard.as_ref() {
        if c.key == key && c.rect == (x0, y0, x1, y1) {
            for (i, row) in (y0..y1).enumerate() {
                let r = canvas.at(x0, row);
                canvas.pixels[r..r + rw].copy_from_slice(&c.px[i * rw..(i + 1) * rw]);
            }
            LAYER_HITS.fetch_add(1, Relaxed);
            return;
        }
    }
    draw(canvas);
    LAYER_MISSES.fetch_add(1, Relaxed);
    let mut px = guard.take().map(|c| c.px).unwrap_or_default();
    px.clear();
    px.reserve(rw * (y1 - y0));
    for row in y0..y1 {
        let r = canvas.at(x0, row);
        px.extend_from_slice(&canvas.pixels[r..r + rw]);
    }
    *guard = Some(LayerCache { key, rect: (x0, y0, x1, y1), px });
}

fn icon_rect(i: usize) -> (i32, i32) {
    (14, 14 + i as i32 * (ICON_H + 8))
}

/// Outline a rounded rectangle with `thickness`-px border.
fn outline_rounded(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, radius: i32, thickness: i32, color: Color) {
    if radius <= 0 {
        canvas.rect_outline(x, y, w, h, thickness, color);
        return;
    }
    for row in 0..h {
        let inset = if row < radius {
            corner_inset(row, radius)
        } else if row >= h - radius {
            corner_inset(h - 1 - row, radius)
        } else {
            0
        };
        let on_curve = row < radius || row >= h - radius;
        if on_curve {
            // Draw the curved edge pixels (a few px thick).
            canvas.fill_rect(x + inset, y + row, thickness.max(1), 1, color);
            canvas.fill_rect(x + w - inset - thickness.max(1), y + row, thickness.max(1), 1, color);
        } else {
            canvas.fill_rect(x, y + row, thickness, 1, color);
            canvas.fill_rect(x + w - thickness, y + row, thickness, 1, color);
        }
    }
    // Top/bottom straight edges.
    canvas.fill_rect(x + radius, y, w - 2 * radius, thickness, color);
    canvas.fill_rect(x + radius, y + h - thickness, w - 2 * radius, thickness, color);
}

// --- Parallel compose -------------------------------------------------------------
//
// A full compose is split in two row bands: the caller draws the top one and
// a helper kernel task -- running without the BKL, on another CPU when one is
// free -- the bottom one, into the two disjoint halves of the same buffer.
// Every drawing primitive clips to its canvas's band (`Canvas::band`).

/// The bottom band of a compose, for the helper. The pointers stay valid
/// because the caller waits for `BAND_DONE` before it returns.
struct BandJob {
    wm: *const Wm,
    ptr: *mut u32,
    len: usize,
    width: usize,
    height: usize,
    y0: usize,
}

// SAFETY: handed to exactly one other task, and the caller outlives its use.
unsafe impl Send for BandJob {}

static BAND_JOB: Mutex<Option<BandJob>> = Mutex::new(None);
static BAND_PENDING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static BAND_DONE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);
/// One compose at a time uses the helper; others compose alone.
static HELPER_BUSY: Mutex<()> = Mutex::new(());
static HELPER_STARTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static PARALLEL_COMPOSE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);
static PARALLEL_FRAMES: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// Kernel futex keys (above every user address, apart from process waits).
const HELPER_KEY: u64 = 0xC0 << 48 | 1;
const DONE_KEY: u64 = 0xC0 << 48 | 2;

/// Frames composed in two bands so far.
pub fn parallel_frames() -> u64 {
    PARALLEL_FRAMES.load(core::sync::atomic::Ordering::Relaxed)
}

/// Switch parallel compose on or off (shell `prof`-style experiments).
pub fn set_parallel_compose(on: bool) {
    PARALLEL_COMPOSE.store(on, core::sync::atomic::Ordering::Relaxed);
}

/// Body of the band helper task: wait for a job, compose it, report.
fn band_helper() {
    use core::sync::atomic::Ordering;
    loop {
        crate::task::block_while(HELPER_KEY, || !BAND_PENDING.load(Ordering::Acquire));
        if !BAND_PENDING.swap(false, Ordering::AcqRel) {
            continue;
        }
        let job = BAND_JOB.lock().take();
        if let Some(j) = job {
            let _z = crate::profile::Guard::new("wm::compose-band");
            // SAFETY: kernel memory of the caller's frame buffer (its bottom
            // rows, which the caller does not touch until BAND_DONE), and its
            // Wm snapshot; both outlive the job (see BandJob).
            let pixels = unsafe { core::slice::from_raw_parts_mut(j.ptr, j.len) };
            let mut canvas = Canvas::band(pixels, j.width, j.height, j.y0, j.height);
            unsafe { (*j.wm).compose(&mut canvas) };
        }
        BAND_DONE.store(true, Ordering::Release);
        crate::task::futex_wake(DONE_KEY, 1);
    }
}

/// Compose `wm` into `back`, in two bands when a second CPU can help.
fn compose_parallel(wm: &Wm, back: &mut [u32], width: usize, height: usize) {
    use core::sync::atomic::Ordering;
    let able = PARALLEL_COMPOSE.load(Ordering::Relaxed)
        && crate::task::is_started()
        // waiting for the helper blocks: only where blocking is allowed
        && crate::arch::cpu::interrupts_enabled()
        && crate::smp::cpus_online() >= 2
        && height >= 64
        && back.len() >= width * height;
    let busy = if able { HELPER_BUSY.try_lock() } else { None };
    let Some(_busy) = busy else {
        wm.compose(&mut Canvas::new(back, width, height));
        return;
    };
    if !HELPER_STARTED.swap(true, Ordering::AcqRel) {
        crate::task::spawn_unlocked("compose-band", band_helper);
    }
    let split = height / 2;
    let (top, bottom) = back[..width * height].split_at_mut(split * width);
    *BAND_JOB.lock() = Some(BandJob {
        wm: wm as *const Wm,
        ptr: bottom.as_mut_ptr(),
        len: bottom.len(),
        width,
        height,
        y0: split,
    });
    BAND_DONE.store(false, Ordering::Release);
    BAND_PENDING.store(true, Ordering::Release);
    crate::task::futex_wake(HELPER_KEY, 1);
    wm.compose(&mut Canvas::band(top, width, height, 0, split));
    {
        let _z = crate::profile::Guard::new("wm::compose-wait");
        while !BAND_DONE.load(Ordering::Acquire) {
            crate::task::block_while(DONE_KEY, || !BAND_DONE.load(Ordering::Acquire));
        }
    }
    PARALLEL_FRAMES.fetch_add(1, Ordering::Relaxed);
}

/// Compose into `back` and present it, by damage.
fn compose_and_present(back: &mut [u32], width: usize, height: usize) {
    let composed = {
        let _z = crate::profile::Guard::new("wm::compose");
        compose_scene(back, width, height)
    };
    let _z = crate::profile::Guard::new("fb::present");
    match composed {
        Some((scene, ptr)) => present_scene(back, width, height, scene, ptr, false),
        None => crate::framebuffer::present_full(back),
    }
}

/// Full frames composed so far (any buffer): a cursor-only frame is valid
/// only if no other compose happened since the desktop's last full frame.
static COMPOSE_SEQ: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static CURSOR_ONLY_FRAMES: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Cursor-only frames drawn (the pointer moved and nothing else changed).
pub fn cursor_only_frames() -> u64 {
    CURSOR_ONLY_FRAMES.load(core::sync::atomic::Ordering::Relaxed)
}

/// The desktop's last full frame: the compose it came from, its cursor key,
/// where the cursor was drawn and the pixels it covered.
struct CursorFrame {
    seq: u64,
    key: u64,
    rect: (usize, usize, usize, usize),
    under: Vec<u32>,
    /// A software cursor was drawn (some output has no hardware cursor).
    sw: bool,
    /// (virtio GPU, board fb) showed a hardware cursor.
    hw: (bool, bool),
}

static CURSOR_FRAME: Mutex<Option<CursorFrame>> = Mutex::new(None);

/// Snapshot the structure (cheap: canvases are shared) and compose without
/// holding `WM`, so other CPUs can create, move and draw into windows
/// meanwhile. The pointer is not drawn: `present_scene` does that per output.
fn compose_scene(back: &mut [u32], width: usize, height: usize) -> Option<(SceneDesc, FramePointer)> {
    use core::sync::atomic::Ordering;
    let snapshot = WM.lock().clone();
    let wm = snapshot?;
    COMPOSE_SEQ.fetch_add(1, Ordering::AcqRel);
    COMPOSING.fetch_add(1, Ordering::Relaxed);
    // Described before drawing: a canvas changed during the compose then
    // carries a newer generation than the one recorded, so the next frame
    // presents it again.
    let scene = wm.scene_desc();
    compose_parallel(&wm, back, width, height);
    COMPOSING.fetch_sub(1, Ordering::Relaxed);
    Some((scene, FramePointer { pos: wm.cursor, scale: wm.cursor_scale, key: wm.cursor_key() }))
}

fn clip_rect(r: (i32, i32, i32, i32), width: usize, height: usize) -> (usize, usize, usize, usize) {
    let x0 = r.0.clamp(0, width as i32) as usize;
    let y0 = r.1.clamp(0, height as i32) as usize;
    let x1 = (r.0 + r.2).clamp(0, width as i32) as usize;
    let y1 = (r.1 + r.3).clamp(0, height as i32) as usize;
    (x0, y0, x1.max(x0), y1.max(y0))
}

fn save_under(back: &[u32], width: usize, r: (usize, usize, usize, usize), out: &mut Vec<u32>) {
    out.clear();
    for y in r.1..r.3 {
        out.extend_from_slice(&back[y * width + r.0..y * width + r.2]);
    }
}

/// The desktop loop's frame. When only the pointer moved since its last full
/// frame (same cursor key, no compose in between), put back the pixels under
/// the old cursor, draw the new one and present just those rows -- instead of
/// composing and presenting the whole screen.
pub fn render_frame(back: &mut Vec<u32>, width: usize, height: usize) {
    render_frame_hint(back, width, height, false);
}

/// [`render_frame`] for the desktop loop, which knows whether the only input
/// since its last frame was pointer movement (`pointer_only`): typing into the
/// terminal, for one, changes the scene without changing the cursor key.
pub fn render_frame_hint(back: &mut Vec<u32>, width: usize, height: usize, pointer_only: bool) {
    use core::sync::atomic::Ordering;
    if back.len() != width * height {
        *back = vec![0u32; width * height];
        *CURSOR_FRAME.lock() = None;
    }
    let now = {
        let guard = WM.lock();
        guard.as_ref().map(|wm| (wm.cursor_key(), wm.cursor, wm.cursor_scale))
    };
    if let (true, Some((Some(key), cursor, scale))) = (pointer_only, now) {
        let mut frame = CURSOR_FRAME.lock();
        let valid = frame.as_ref().is_some_and(|f| f.key == key && f.seq == COMPOSE_SEQ.load(Ordering::Acquire));
        // hardware cursors: one move command each (an output whose cursor
        // mode changed since the last full frame needs a proper one)
        let th = theme::current();
        let hw = if valid { Some(sync_hw_cursors(&th, cursor, scale)) } else { None };
        if let (Some(f), Some(hw)) = (frame.as_mut(), hw) {
            if hw == f.hw {
                let _z = crate::profile::Guard::new("wm::cursor-only");
                let new = clip_rect(cursor_rect(cursor, scale), width, height);
                let old = f.rect;
                if f.sw {
                    let rw = old.2 - old.0;
                    for (i, y) in (old.1..old.3).enumerate() {
                        back[y * width + old.0..y * width + old.2].copy_from_slice(&f.under[i * rw..(i + 1) * rw]);
                    }
                    save_under(back, width, new, &mut f.under);
                    draw_cursor_at(&mut Canvas::new(back, width, height), &th, cursor, scale);
                }
                f.rect = new;
                let (y0, y1) = (old.1.min(new.1), old.3.max(new.3));
                // The screen now shows the cursor here: tell damage tracking.
                let mut on_screen = ON_SCREEN.lock();
                if let Some(sc) = on_screen.as_mut() {
                    sc.cursor = cursor_rect(cursor, scale);
                }
                if f.sw {
                    let (gpu_hw, fb_hw) = hw;
                    if !fb_hw {
                        crate::framebuffer::present_console(back, Some(&[(y0, y1)]));
                    }
                    if !gpu_hw {
                        crate::virtio::gpu::present_bands(back, &[(y0, y1)]);
                    }
                }
                drop(on_screen);
                CURSOR_ONLY_FRAMES.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    }
    full_frame(back, width, height);
}

/// The desktop loop's full frame: compose, then present by damage.
fn full_frame(back: &mut [u32], width: usize, height: usize) {
    let composed = {
        let _z = crate::profile::Guard::new("wm::compose");
        compose_scene(back, width, height)
    };
    crate::boot::phase("compose");
    let _z = crate::profile::Guard::new("fb::present");
    match composed {
        Some((scene, ptr)) => present_scene(back, width, height, scene, ptr, true),
        None => {
            *CURSOR_FRAME.lock() = None;
            crate::framebuffer::present_full(back);
        }
    }
}

/// WIN_EVENT: the next pointer event of window `id`, if `owner` owns it.
pub fn take_event(id: u32, owner: u64) -> Option<u64> {
    let mut guard = WM.lock();
    let wm = guard.as_mut()?;
    let w = wm.windows.iter_mut().find(|w| w.id == id && w.owner == owner)?;
    Some(w.events.pop_front().unwrap_or(0))
}

/// The (outer width, outer height) of a window whose client area is
/// `cw` x `ch` (WIN_CREATE with `win_flags::CLIENT_SIZE`).
pub fn outer_size(cw: i32, ch: i32) -> (i32, i32) {
    (cw + 2 * BORDER, ch + TITLE_H + 2 * BORDER)
}
