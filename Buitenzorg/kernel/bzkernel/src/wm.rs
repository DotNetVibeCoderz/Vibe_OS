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
const SM_W: i32 = 244; // start-menu width
const SM_ROW: i32 = 26; // start-menu / icon row height
const ICON_W: i32 = 72; // desktop icon cell width
const ICON_H: i32 = 62; // desktop icon cell height

/// Apps that appear on the desktop / start menu, as (launch-name, label). The
/// launch-name must match `app::app_file`. The first `DESKTOP_ICONS` are also
/// placed as desktop icons.
const LAUNCH: &[(&str, &str)] = &[
    ("files", "Files"),
    ("editor", "Editor"),
    ("calc", "Kalkulator"),
    ("store", "App Store"),
    ("imgview", "Gambar"),
    ("clock", "Jam"),
    ("piano", "Piano"),
    ("2048", "2048"),
    ("taskmgr", "Task Manager"),
    ("paint", "Paint"),
];
const DESKTOP_ICONS: usize = 4;

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
    /// A launch requested by a start-menu / desktop-icon click, drained by the
    /// desktop loop (the WM cannot run a ring-3 app itself).
    pending: Option<String>,
    /// A power action requested from the start menu: 1 = shutdown, 2 = restart.
    pending_power: u8,
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
        pending: None,
        pending_power: 0,
        last_icon: (usize::MAX, 0),
    });
}

/// Drain a pending app-launch requested from the desktop shell (start menu /
/// desktop icon). The desktop loop calls this and runs the app.
pub fn take_pending_launch() -> Option<String> {
    WM.lock().as_mut().and_then(|wm| wm.pending.take())
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
    // Click the first app row ("files"): mirror draw_start_menu's geometry.
    let rows = LAUNCH.len() as i32;
    let menu_h = 40 + rows * SM_ROW + 34;
    let y = height - TASKBAR_H - menu_h - 4;
    let row0 = y + 40 + SM_ROW / 2;
    handle_mouse(30, row0, true);
    handle_mouse(30, row0, false);
    let launched = matches!(take_pending_launch().as_deref(), Some("files"));
    // Reset shell state so the real desktop starts clean.
    if let Some(wm) = WM.lock().as_mut() {
        wm.start_open = false;
        wm.pending = None;
        wm.last_icon = (usize::MAX, 0);
    }
    opened && launched
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
    });
    id
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
            let rows = LAUNCH.len() as i32;
            let h = 40 + rows * SM_ROW + 34;
            let x = 6;
            let y = self.height - TASKBAR_H - h - 4;
            if cx >= x && cx < x + SM_W && cy >= y && cy < y + h {
                let mut ry = y + 40;
                for (name, _) in LAUNCH {
                    if cy >= ry && cy < ry + SM_ROW {
                        self.pending = Some(String::from(*name));
                        self.start_open = false;
                        return true;
                    }
                    ry += SM_ROW;
                }
                let py = y + h - 30;
                if cy >= py && cy < py + 24 {
                    self.pending_power = if cx < x + SM_W / 2 { 1 } else { 2 };
                    self.start_open = false;
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
        for i in 0..DESKTOP_ICONS.min(LAUNCH.len()) {
            let (ix, iy) = icon_rect(i);
            if cx >= ix && cx < ix + ICON_W && cy >= iy && cy < iy + ICON_H {
                let now = crate::interrupts::ticks();
                if self.last_icon.0 == i && now.saturating_sub(self.last_icon.1) < 11 {
                    self.pending = Some(String::from(LAUNCH[i].0));
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
            let title_color = if active { th.title_text } else { th.text };
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
        // Subtle vertical gradient bar with an accent hairline on top.
        gradient_v(canvas, 0, y, self.width, TASKBAR_H, shift(th.taskbar_bg, 14), th.taskbar_bg);
        canvas.fill_rect(0, y, self.width, 1, th.accent);

        // --- Start button (Windows XP-style green pill + a small logo mark) ---
        let sb_hi = self.start_open;
        let base = if sb_hi { shift(th.win_title_active, 30) } else { th.win_title_active };
        fill_rounded_gradient(canvas, 6, y + 4, START_W, TASKBAR_H - 8, 8, shift(base, 40), base);
        // 2x2 logo tile.
        canvas.fill_rect(14, y + 11, 5, 5, th.title_text);
        canvas.fill_rect(20, y + 11, 5, 5, shift(th.title_text, 0));
        canvas.fill_rect(14, y + 17, 5, 5, shift(th.accent, 60));
        canvas.fill_rect(20, y + 17, 5, 5, th.title_text);
        canvas.draw_text(32, y + 9, "Start", th.title_text, 6 + START_W - 4);

        // --- right tray: theme name, clock, workspace pips ---
        let mut rx = self.width - 6;
        // Workspace pips [1..4].
        rx -= (WORKSPACES as i32) * 16 + 8;
        let mut wx = rx + 8;
        for ws in 0..WORKSPACES {
            let sel = ws == self.workspace;
            let c = if sel { th.accent } else { th.win_title_inactive };
            fill_rounded(canvas, wx, y + 13, 10, TASKBAR_H - 26, 2, c);
            wx += 16;
        }
        // Clock HH:MM.
        let (hh, mm) = read_clock();
        let mut cbuf = [0u8; 5];
        cbuf[0] = b'0' + (hh / 10) % 10; cbuf[1] = b'0' + hh % 10; cbuf[2] = b':';
        cbuf[3] = b'0' + (mm / 10) % 10; cbuf[4] = b'0' + mm % 10;
        let cs = core::str::from_utf8(&cbuf).unwrap();
        let cw = 5 * gfx::glyph_width() as i32 + 4;
        rx -= cw + 12;
        canvas.draw_text(rx + 2, y + 9, cs, th.accent, rx + cw);
        // Theme name.
        let tn_w = th.name.len() as i32 * gfx::glyph_width() as i32 + 6;
        rx -= tn_w + 4;
        canvas.draw_text(rx, y + 9, th.name, th.text, rx + tn_w);

        // --- running window buttons (rounded, gradient when active) ---
        let top_id = self
            .windows
            .iter()
            .filter(|w| w.workspace == self.workspace && w.state != WinState::Minimized)
            .next_back()
            .map(|w| w.id);
        let start_x = 6 + START_W + 8;
        let mut bx = start_x;
        for win in self.windows.iter().filter(|w| w.workspace == self.workspace) {
            if bx + 116 > rx - 4 {
                break; // no room before the tray
            }
            let active = Some(win.id) == top_id;
            if active {
                fill_rounded_gradient(canvas, bx, y + 5, 110, TASKBAR_H - 10, 5, shift(th.win_title_active, 26), th.win_title_active);
            } else if win.state == WinState::Minimized {
                fill_rounded(canvas, bx, y + 5, 110, TASKBAR_H - 10, 5, shift(th.taskbar_bg, 22));
            } else {
                fill_rounded(canvas, bx, y + 5, 110, TASKBAR_H - 10, 5, th.win_title_inactive);
            }
            let tc = if active { th.title_text } else { th.text };
            canvas.draw_text(bx + 8, y + 9, &win.title, tc, bx + 108);
            bx += 116;
        }
    }

    /// Draw the start menu (macOS/XP-blended launcher) above the Start button.
    fn draw_start_menu(&self, canvas: &mut Canvas, th: &theme::Theme) {
        if !self.start_open {
            return;
        }
        let rows = LAUNCH.len() as i32;
        let h = 40 + rows * SM_ROW + 34;
        let x = 6;
        let y = self.height - TASKBAR_H - h - 4;
        // Shadow + panel.
        fill_rounded(canvas, x + 3, y + 4, SM_W, h, 10, th.shadow);
        fill_rounded(canvas, x, y, SM_W, h, 10, shift(th.win_body, 6));
        canvas.rect_outline(x, y, SM_W, h, 1, th.win_border);
        // Header band.
        fill_rounded_gradient(canvas, x + 1, y + 1, SM_W - 2, 34, 9, th.win_title_active, darken(th.win_title_active, 20));
        canvas.draw_text(x + 12, y + 7, "Buitenzorg OS", th.title_text, x + SM_W - 8);
        canvas.draw_text(x + 12, y + 20, "Gravicode Studios", darken(th.title_text, 30), x + SM_W - 8);

        // App rows.
        let (mx, my) = self.cursor;
        let mut ry = y + 40;
        for (_, label) in LAUNCH {
            let hot = mx >= x + 4 && mx < x + SM_W - 4 && my >= ry && my < ry + SM_ROW;
            if hot {
                fill_rounded(canvas, x + 4, ry, SM_W - 8, SM_ROW - 2, 4, darken(th.win_title_active, 10));
            }
            // Little app glyph.
            canvas.fill_rect(x + 12, ry + 6, 12, 12, th.accent);
            canvas.fill_rect(x + 14, ry + 8, 8, 8, shift(th.win_body, 10));
            let tc = if hot { th.title_text } else { th.text };
            canvas.draw_text(x + 32, ry + 6, label, tc, x + SM_W - 8);
            ry += SM_ROW;
        }
        // Power row (shutdown / restart).
        let py = y + h - 30;
        canvas.fill_rect(x + 8, py - 4, SM_W - 16, 1, th.win_border);
        let sd_hot = mx >= x + 8 && mx < x + SM_W / 2 && my >= py && my < py + 24;
        let rs_hot = mx >= x + SM_W / 2 && mx < x + SM_W - 8 && my >= py && my < py + 24;
        if sd_hot { fill_rounded(canvas, x + 8, py, SM_W / 2 - 10, 24, 4, gfx::rgb(0xC0, 0x39, 0x2B)); }
        if rs_hot { fill_rounded(canvas, x + SM_W / 2, py, SM_W / 2 - 10, 24, 4, darken(th.win_title_active, 10)); }
        canvas.draw_text(x + 18, py + 6, "Matikan", if sd_hot { gfx::rgb(0xFF, 0xFF, 0xFF) } else { th.text }, x + SM_W / 2);
        canvas.draw_text(x + SM_W / 2 + 10, py + 6, "Restart", if rs_hot { th.title_text } else { th.text }, x + SM_W - 8);
    }

    /// Draw the desktop launcher icons down the top-left (macOS/GNOME feel).
    fn draw_desktop_icons(&self, canvas: &mut Canvas, th: &theme::Theme) {
        let (mx, my) = self.cursor;
        for i in 0..DESKTOP_ICONS.min(LAUNCH.len()) {
            let (ix, iy) = icon_rect(i);
            let hot = mx >= ix && mx < ix + ICON_W && my >= iy && my < iy + ICON_H;
            if hot {
                fill_rounded(canvas, ix, iy, ICON_W, ICON_H, 6, shift(th.desktop_top, 30));
            }
            // Rounded gradient tile with a glyph mark.
            let tx = ix + (ICON_W - 40) / 2;
            fill_rounded_gradient(canvas, tx, iy + 6, 40, 34, 7, shift(th.win_title_active, 30), th.win_title_active);
            canvas.fill_rect(tx + 10, iy + 15, 20, 4, th.title_text);
            canvas.fill_rect(tx + 10, iy + 22, 14, 4, darken(th.title_text, 30));
            // Label centered-ish under the tile.
            let label = LAUNCH[i].1;
            let lw = label.len() as i32 * gfx::glyph_width() as i32;
            canvas.draw_text(ix + (ICON_W - lw) / 2, iy + 46, label, th.title_text, ix + ICON_W);
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
