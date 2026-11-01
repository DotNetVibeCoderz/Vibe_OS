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
const SM_H: i32 = 316; // start-menu height
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
        apps: &[("editor", "Editor"), ("calc", "Kalkulator"), ("files", "Files")],
    },
    Category {
        name: "Multimedia",
        leaf: gfx::rgb(0xC7, 0x92, 0xEA),
        apps: &[("imgview", "Gambar"), ("piano", "Piano"), ("paint", "Paint")],
    },
    Category {
        name: "Permainan",
        leaf: gfx::rgb(0xF3, 0xC9, 0x69),
        apps: &[("2048", "2048"), ("xox", "Tic-Tac-Toe")],
    },
    Category {
        name: "Utilitas",
        leaf: gfx::rgb(0x7C, 0xC4, 0xA4),
        apps: &[("clock", "Jam"), ("taskmgr", "Task Manager"), ("store", "App Store")],
    },
    Category {
        name: "Pengaturan",
        leaf: gfx::rgb(0xD9, 0xB3, 0x8C),
        apps: &[("audiopanel", "Audio"), ("@wallpaper", "Wallpaper"), ("@theme", "Tema")],
    },
];

/// Desktop launcher icons (top-left), as (launch-name, label).
const DESKTOP: [(&str, &str); 4] = [
    ("files", "Files"),
    ("editor", "Editor"),
    ("calc", "Kalkulator"),
    ("store", "App Store"),
];

/// Fallback tile colors when the icon pack is unavailable (no /disk driver on
/// this boot medium): each app still gets its own color, never a shared glyph.
fn fallback_color(name: &str) -> Color {
    match name {
        "files" => gfx::rgb(0xF5, 0x8A, 0x1F),
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
fn draw_app_icon(canvas: &mut Canvas, name: &str, label: &str, x: i32, y: i32, big: bool) {
    if crate::icons::draw(canvas, name.trim_start_matches('@'), x, y, big) {
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
const DAYS: [&str; 7] = ["Minggu", "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];

/// Weekday (0 = Sunday) by Sakamoto's method.
fn weekday(y: i32, m: i32, d: i32) -> usize {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    ((y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d).rem_euclid(7)) as usize
}

/// Today as (weekday name, day, month name), from the CMOS clock.
fn today() -> (&'static str, u8, &'static str) {
    let (y, mo, d, _, _, _) = crate::rtc::read();
    let mo = mo.clamp(1, 12);
    (DAYS[weekday(y as i32, mo as i32, d as i32)], d, MONTHS[(mo - 1) as usize])
}

/// Format `n` as two digits into `buf` (no allocation in the render path).
fn two_digits(buf: &mut [u8], n: u8) {
    buf[0] = b'0' + (n / 10) % 10;
    buf[1] = b'0' + n % 10;
}

/// Read wall-clock (hour, minute) from the CMOS RTC (see `rtc.rs`).
fn read_clock() -> (u8, u8) {
    crate::rtc::read_hm()
}

/// Pixel canvas owned by an app window (v0.8): the client area apps draw
/// into through the WIN_CMD syscall.
pub struct AppCanvas {
    pub w: i32,
    pub h: i32,
    pub pixels: Vec<u32>,
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

pub struct Window {
    pub id: u32,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub lines: Vec<String>,
    pub workspace: u8,
    pub canvas: Option<AppCanvas>,
    pub state: WinState,
    pub saved: (i32, i32, i32, i32), // geometry to restore from max/min
    /// Process that created the window (0 = kernel/shell). Keyboard focus
    /// follows the topmost window's owner (multi-process increment 1).
    pub owner: u64,
}

#[derive(Clone, Copy)]
enum Drag {
    None,
    Move { id: u32, dx: i32, dy: i32 },
    Resize { id: u32 },
}

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
}

static WM: Mutex<Option<Wm>> = Mutex::new(None);

pub fn init(width: usize, height: usize) {
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
    let first = matches!(take_pending_launch().as_deref(), Some("editor"));
    handle_mouse(40, height - 4, true);
    handle_mouse(40, height - 4, false);
    let (rx, ry) = rail_center(height, 2);
    handle_mouse(rx, ry, true);
    handle_mouse(rx, ry, false);
    handle_mouse(cx0, cy0, true);
    handle_mouse(cx0, cy0, false);
    let game = matches!(take_pending_launch().as_deref(), Some("2048"));
    let launched = first && game;
    // Reset shell state so the real desktop starts clean.
    if let Some(wm) = WM.lock().as_mut() {
        wm.start_open = false;
        wm.start_cat = 0;
        wm.pending = None;
        wm.last_icon = (usize::MAX, 0);
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
            canvas: Some(AppCanvas {
                w: cw,
                h: ch,
                pixels: vec![body; (cw * ch) as usize],
            }),
            state: WinState::Normal,
            saved: (board_x, y, ww, h),
            owner: crate::aspace::current_id(),
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
        canvas: Some(AppCanvas {
            w: cw,
            h: ch,
            pixels: vec![body; (cw * ch) as usize],
        }),
        state: WinState::Normal,
        saved: (x, y, w, h),
        owner: crate::aspace::current_id(),
    });
    id
}

/// Apply a draw command to an app window's canvas (v0.8 WIN_CMD).
pub fn draw_on_window(id: u32, cmd: &bz_abi::DrawCmd, text: Option<&str>) -> Result<(), &'static str> {
    let mut guard = WM.lock();
    let wm = guard.as_mut().ok_or("wm not initialized")?;
    let win = wm.windows.iter_mut().find(|w| w.id == id).ok_or("no such window")?;
    let canvas = win.canvas.as_mut().ok_or("window has no canvas")?;
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
        _ => return Err("unknown draw op"),
    }
    Ok(())
}

/// Blit a client ARGB pixel buffer into a window's canvas at (x, y), with
/// clipping (v0.16 "Panen" BLIT op — client-side software rendering). The
/// canvas stores 0x00RRGGBB, so alpha is dropped (the client composites first).
///
/// `src_ptr` is a user-space pointer to `bw*bh` ARGB `u32` pixels; it is read
/// per-pixel with `read_volatile` (not turned into a `&[u32]` slice — a slice
/// over user memory would let the optimizer assume the whole region is
/// dereferenceable/immutable, which corrupted the boot).
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
    let canvas = win.canvas.as_mut().ok_or("window has no canvas")?;
    let (cw, ch) = (canvas.w as i32, canvas.h as i32);
    let base = src_ptr as *const u32;
    for row in 0..bh {
        let dy = y + row;
        if dy < 0 || dy >= ch {
            continue;
        }
        for col in 0..bw {
            let dx = x + col;
            if dx < 0 || dx >= cw {
                continue;
            }
            let s = unsafe { core::ptr::read_volatile(base.add((row * bw + col) as usize)) };
            canvas.pixels[(dy * cw + dx) as usize] = s & 0x00FF_FFFF;
        }
    }
    Ok(())
}

/// Reusable compose buffer for [`present_now`] — allocated once and kept, so a
/// present from a shell-launched app never needs a fresh multi-MiB allocation
/// on a full/fragmented heap (that failed and panicked the kernel once).
static PRESENT_BUF: spin::Mutex<alloc::vec::Vec<u32>> = spin::Mutex::new(alloc::vec::Vec::new());

/// Compose + present immediately (v0.8 WIN_PRESENT syscall).
pub fn present_now() {
    if let Some((w, h)) = crate::framebuffer::dimensions() {
        let mut back = PRESENT_BUF.lock();
        if back.len() != w * h {
            back.resize(w * h, 0);
        }
        {
            // Profiler zone: the compositor is the deepest thing a WIN_PRESENT
            // syscall runs, so it is the natural thing to measure. Inert unless
            // profiling is enabled (`prof on` / the boot self-test).
            let _z = crate::profile::Guard::new("wm::compose");
            compose_into(&mut back, w, h);
        }
        let _z = crate::profile::Guard::new("fb::present");
        crate::framebuffer::present(&back);
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

pub fn current_workspace() -> u8 {
    WM.lock().as_ref().map(|w| w.workspace).unwrap_or(0)
}

pub fn set_cursor(x: i32, y: i32) {
    if let Some(wm) = WM.lock().as_mut() {
        wm.cursor = (x.clamp(0, wm.width - 1), y.clamp(0, wm.height - 1));
    }
}

/// Feed one mouse sample (absolute position + left button) into the WM.
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

    /// Handle a click on the taskbar's window buttons (restore/focus).
    fn taskbar_click(&mut self, x: i32) {
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
        crate::wallpaper::paint(canvas, &th, self.workspace);
        self.draw_desktop_icons(canvas, &th);

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
            let active = Some(win.id) == top_id;

            // Drop shadow per theme style.
            match th.shadow_style {
                theme::ShadowStyle::None => {}
                theme::ShadowStyle::Soft => {
                    fill_rounded(canvas, win.x + 4, win.y + 5, win.w, win.h, radius, th.shadow);
                }
                theme::ShadowStyle::HardOffset => {
                    canvas.fill_rect(win.x + 8, win.y + 8, win.w, win.h, th.shadow);
                }
            }

            // Body + border (rounded).
            fill_rounded(canvas, win.x, win.y, win.w, win.h, radius, th.win_body);
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
                // Blit the app canvas into the client area, clipped to both
                // the current window size and the screen.
                let ox = win.x + BORDER;
                let oy = win.y + BORDER + TITLE_H;
                let vis_w = app.w.min(win.w - 2 * BORDER).max(0) as usize;
                let vis_h = app.h.min(win.h - TITLE_H - 2 * BORDER).max(0) as usize;
                for row in 0..vis_h {
                    for col in 0..vis_w {
                        let px = ox + col as i32;
                        let py = oy + row as i32;
                        if px >= 0 && py >= 0 {
                            canvas.put(
                                px as usize,
                                py as usize,
                                app.pixels[row * app.w as usize + col],
                            );
                        }
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

        self.draw_taskbar(canvas, &th);
        self.draw_start_menu(canvas, &th);
        self.draw_ripple(canvas, &th);
        self.draw_cursor(canvas, &th);
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

        // --- tray (right): date, time, workspace dots ------------------------
        let mut rx = self.width - 12;
        for ws in (0..WORKSPACES).rev() {
            let sel = ws == self.workspace;
            let c = if sel { th.accent } else { th.win_border };
            canvas.ellipse(rx - 8, y + 13, 8, 8, c, true);
            rx -= 13;
        }
        rx -= 10;
        let (hh, mm) = read_clock();
        let mut t = [b'0', b'0', b':', b'0', b'0'];
        two_digits(&mut t[0..2], hh);
        two_digits(&mut t[3..5], mm);
        let ts = core::str::from_utf8(&t).unwrap();
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

    fn draw_cursor(&self, canvas: &mut Canvas, th: &theme::Theme) {
        let (cx, cy) = self.cursor;
        let s = self.cursor_scale;
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

pub fn compose_into(back: &mut [u32], width: usize, height: usize) {
    let guard = WM.lock();
    if let Some(wm) = guard.as_ref() {
        let mut canvas = Canvas::new(back, width, height);
        wm.compose(&mut canvas);
    }
}

pub fn render_frame(back: &mut Vec<u32>, width: usize, height: usize) {
    if back.len() != width * height {
        *back = vec![0u32; width * height];
    }
    compose_into(back, width, height);
    crate::framebuffer::present(back);
}
