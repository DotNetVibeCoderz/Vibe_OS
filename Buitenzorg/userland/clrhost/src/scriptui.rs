//! UI programs in JavaScript, TypeScript and Python: `CLRHOST.ELF <file.js|
//! .ts|.py> [args]` runs the script with the kernel's own interpreter
//! (`kernel/bzkernel/src/script.rs`, the same source) as a ring-3 process,
//! with no step budget and a window API in **immediate mode** -- every frame
//! draws everything again, and `button(...)` is true in the frame after it
//! was clicked:
//!
//! ```js
//! window("Penghitung", 320, 200);
//! let n = 0;
//! while (running()) {
//!   clear(0x14201A);
//!   text(20, 20, "Nilai: " + n, 0xFFFFFF, 2);
//!   if (button(20, 70, 140, 34, "Tambah")) { n = n + 1; }
//!   present();
//! }
//! ```
//!
//! Functions (colors are 0xRRGGBB, coordinates in the client area):
//! `window(title, w, h)`, `width()`, `height()`, `clear(c)`,
//! `rect(x, y, w, h, c [, radius])`, `frame(x, y, w, h, c [, radius])`,
//! `gradient(x, y, w, h, top, bottom)`, `line(x1, y1, x2, y2, c)`,
//! `circle(cx, cy, r, c)`, `ring(cx, cy, r, c)`, `text(x, y, s, c [, style
//! 0 body / 1 bold / 2 heading])`, `textwidth(s [, style])`, `lineheight([style])`,
//! `button(x, y, w, h, label)`, `present()`, `running()`, `quit()`,
//! `mousex()`, `mousey()`, `mousedown()`, `clicked()`, `clickx()`, `clicky()`,
//! `key()` (next key code, 0 = none; `chr(key)` = the character; 27 Esc, 10
//! Enter, 8 Backspace, 63232.. arrows), `ticks()` (ms), `wait(ms)`,
//! `random(n)`, `rgb(r, g, b)`, `arg(i)`, `argcount()`,
//! `input(x, y, w, id [, placeholder])` (a text field: click to focus, type,
//! Backspace; Enter / Tab / Esc leave it -- Enter is also handed on to
//! `key()`; returns the text), `setinput(id, text)`, `focus(id)`.
//!
//! During the headless self-test boot (`IS_INTERACTIVE` = 0) no window is
//! created (drawing is a no-op) and `running()` is true for three frames
//! only, so a UI program can never block the boot.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::script::{self, Host, Lang, Value};
use crate::sys;

const IS_INTERACTIVE: u64 = 31;
const WIN_CREATE: u64 = 6;
const WIN_CMD: u64 = 7;
const WIN_PRESENT: u64 = 8;
const KEY_READ: u64 = 9;
const FUTEX_WAIT_TIMEOUT: u64 = 43;
const CLOCK_NANOS: u64 = 44;
const WIN_EVENT: u64 = 45;
const TEXT_METRICS: u64 = 46;
const CLIENT_SIZE: u64 = 1 << 63;

// bz_abi::draw_op
const FILL_RECT: u64 = 0;
const DRAW_TEXT: u64 = 1;
const CLEAR: u64 = 2;
const LINE: u64 = 3;
const ELLIPSE: u64 = 4;
const FILL_ELLIPSE: u64 = 5;
const RECT: u64 = 6;
const FILL_ROUND_RECT: u64 = 8;
const ROUND_RECT: u64 = 9;
const GRADIENT_V: u64 = 10;
const FILL_ROUND_GRADIENT_V: u64 = 11;
const DRAW_TEXT_STYLED: u64 = 12;

// bz_abi::pointer_event kinds
const EV_MOVE: u64 = 1;
const EV_DOWN: u64 = 2;
const EV_UP: u64 = 3;

/// `bz_abi::DrawCmd`.
#[repr(C)]
struct DrawCmd {
    op: u64,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: u32,
    pad: u32,
    text_ptr: u64,
    text_len: u64,
}

struct Ui {
    win: u64,
    w: i64,
    h: i64,
    interactive: bool,
    closed: bool,
    frames: u64,
    t0: u64,
    last_frame: u64,
    mx: i64,
    my: i64,
    down: bool,
    press: (i64, i64),
    click: Option<(i64, i64, i64, i64)>, // press x/y, release x/y (this frame)
    keys: VecDeque<i64>,
    rng: u64,
    args: Vec<String>,
    /// Text fields: their text by id, and the one that has the keyboard.
    fields: BTreeMap<String, String>,
    focus: Option<String>,
}

fn nanos() -> u64 {
    sys::syscall(CLOCK_NANOS, 0, 0, 0)
}

fn sleep_ms(ms: u64) {
    let word: u32 = 0;
    sys::syscall(FUTEX_WAIT_TIMEOUT, &word as *const u32 as u64, 0, ms.max(1) * 1_000_000);
}

fn metrics(style: u64) -> (i64, i64) {
    let m = sys::syscall(TEXT_METRICS, style, 0, 0);
    let (w, h) = ((m >> 32) as i64, (m & 0xFFFF_FFFF) as i64);
    if w <= 0 { (8, 16) } else { (w, h) }
}

fn int(a: &[Value], i: usize) -> i64 {
    a.get(i).map(|v| v.as_int()).unwrap_or(0)
}

fn text_of(a: &[Value], i: usize) -> String {
    a.get(i).map(|v| v.display()).unwrap_or_default()
}

impl Ui {
    fn draw(&self, op: u64, x: i64, y: i64, w: i64, h: i64, color: i64, pad: u64, text: &str, text_len: u64) {
        if self.win == 0 {
            return;
        }
        let cmd = DrawCmd {
            op,
            x: x as i32,
            y: y as i32,
            w: w as i32,
            h: h as i32,
            color: (color as u32) & 0x00FF_FFFF,
            pad: pad as u32,
            text_ptr: if text.is_empty() { 0 } else { text.as_ptr() as u64 },
            text_len: if text.is_empty() { text_len } else { text.len() as u64 },
        };
        sys::syscall(WIN_CMD, self.win, &cmd as *const DrawCmd as u64, 0);
    }

    fn inside(&self, x: i64, y: i64, bx: i64, by: i64, bw: i64, bh: i64) -> bool {
        x >= bx && x < bx + bw && y >= by && y < by + bh
    }

    /// Show the frame, keep ~30 frames a second, then collect input.
    fn present(&mut self) {
        if self.win != 0 {
            sys::syscall(WIN_PRESENT, self.win, 0, 0);
        }
        self.frames += 1;
        let now = nanos();
        let spent = now.saturating_sub(self.last_frame) / 1_000_000;
        if spent < 33 {
            sleep_ms(33 - spent);
        }
        self.last_frame = nanos();
        self.click = None;
        if self.win == 0 {
            return;
        }
        loop {
            let ev = sys::syscall(WIN_EVENT, self.win, 0, 0);
            if ev == 0 || sys::is_err(ev) {
                break;
            }
            let kind = ev >> 56;
            let x = (ev & 0xFFFF) as u16 as i16 as i64;
            let y = ((ev >> 16) & 0xFFFF) as u16 as i16 as i64;
            match kind {
                EV_MOVE => {
                    self.mx = x;
                    self.my = y;
                }
                EV_DOWN => {
                    self.mx = x;
                    self.my = y;
                    self.down = true;
                    self.press = (x, y);
                }
                EV_UP => {
                    self.mx = x;
                    self.my = y;
                    if self.down {
                        self.click = Some((self.press.0, self.press.1, x, y));
                    }
                    self.down = false;
                }
                _ => {}
            }
        }
        for _ in 0..32 {
            let k = sys::syscall(KEY_READ, 0, 0, 0);
            if k == 0 || sys::is_err(k) {
                break;
            }
            let k = if k == 13 { 10 } else if k == 127 { 0xF706 } else { k as i64 as u64 };
            self.keys.push_back(k as i64);
            if self.keys.len() > 64 {
                self.keys.pop_front();
            }
        }
    }

    fn button(&mut self, x: i64, y: i64, w: i64, h: i64, label: &str) -> bool {
        let hover = self.inside(self.mx, self.my, x, y, w, h);
        let pressed = hover && self.down;
        let (top, bottom) = if pressed {
            (0x2E6B4C, 0x24573D)
        } else if hover {
            (0x5DAA80, 0x3F8A62)
        } else {
            (0x4E9A72, 0x357A55)
        };
        self.draw(FILL_ROUND_GRADIENT_V, x, y, w, h, top, bottom, "", 8);
        self.draw(ROUND_RECT, x, y, w, h, 0x7CC4A4, 8, "", 0);
        let (gw, lh) = metrics(1);
        let tw = label.chars().count() as i64 * gw;
        self.draw(DRAW_TEXT_STYLED, x + (w - tw) / 2, y + (h - lh) / 2, 0, 0, 0xFFFFFF, 1, label, 0);
        match self.click {
            Some((px, py, rx, ry)) => self.inside(px, py, x, y, w, h) && self.inside(rx, ry, x, y, w, h),
            None => false,
        }
    }
}

impl Ui {
    /// An immediate-mode text field.
    fn input(&mut self, x: i64, y: i64, w: i64, id: &str, placeholder: &str) -> String {
        let (gw, lh) = metrics(0);
        let h = lh + 12;
        if let Some((px, py, rx, ry)) = self.click {
            let inside = self.inside(px, py, x, y, w, h) && self.inside(rx, ry, x, y, w, h);
            if inside {
                self.focus = Some(String::from(id));
            } else if self.focus.as_deref() == Some(id) {
                self.focus = None;
            }
        }
        let focused = self.focus.as_deref() == Some(id);
        let mut text = self.fields.get(id).cloned().unwrap_or_default();
        if focused {
            let mut rest = VecDeque::new();
            while let Some(k) = self.keys.pop_front() {
                match k {
                    8 => {
                        text.pop();
                    }
                    10 | 9 | 27 => {
                        self.focus = None;
                        if k == 10 {
                            rest.push_back(k); // the program may submit on Enter
                        }
                        rest.extend(self.keys.drain(..));
                        break;
                    }
                    k if k >= 32 && k < 0xF700 && k != 127 => {
                        if text.chars().count() < 80 {
                            if let Some(c) = char::from_u32(k as u32) {
                                text.push(c);
                            }
                        }
                    }
                    _ => rest.push_back(k), // arrows etc. stay for key()
                }
            }
            self.keys = rest;
            self.fields.insert(String::from(id), text.clone());
        }
        let focused = self.focus.as_deref() == Some(id);
        self.draw(FILL_ROUND_RECT, x, y, w, h, 0x0B1610, 6, "", 0);
        self.draw(ROUND_RECT, x, y, w, h, if focused { 0x7CC4A4 } else { 0x3F6B52 }, 6, "", 0);
        let fit = ((w - 16) / gw).max(1) as usize;
        let n = text.chars().count();
        if n == 0 && !focused && !placeholder.is_empty() {
            let shown: String = placeholder.chars().take(fit).collect();
            self.draw(DRAW_TEXT, x + 8, y + 6, 0, 0, 0x6F8F7A, 0, &shown, 0);
        } else {
            // the end of a long text stays visible
            let shown: String = text.chars().skip(n.saturating_sub(fit)).collect();
            self.draw(DRAW_TEXT, x + 8, y + 6, 0, 0, 0xFFFFFF, 0, &shown, 0);
            if focused && (nanos() / 500_000_000) % 2 == 0 {
                let cx = x + 8 + shown.chars().count() as i64 * gw;
                self.draw(FILL_RECT, cx, y + 5, 2, lh, 0x7CC4A4, 0, "", 0);
            }
        }
        text
    }
}

impl Host for Ui {
    fn print(&mut self, line: &str) -> bool {
        sys::print(line);
        sys::print("\n");
        true
    }

    fn call(&mut self, name: &str, a: &[Value]) -> Option<Result<Value, String>> {
        let nil = Value::Nil;
        let r = match name {
            "window" => {
                // headless (boot self-test): no window -- every call still
                // runs, drawing is a no-op; windows of finished processes are
                // only cleaned up by the live desktop, so test runs would pile
                // their canvases up in the kernel heap
                if !self.interactive {
                    self.w = int(a, 1).clamp(80, 2000);
                    self.h = int(a, 2).clamp(60, 1400);
                    return Some(Ok(Value::Bool(true)));
                }
                if self.win == 0 {
                    let title = text_of(a, 0);
                    let (w, h) = (int(a, 1).clamp(80, 2000), int(a, 2).clamp(60, 1400));
                    let id = sys::syscall(WIN_CREATE, title.as_ptr() as u64, title.len() as u64, CLIENT_SIZE | (w as u64) << 32 | h as u64);
                    if !sys::is_err(id) && id != 0 {
                        self.win = id;
                        self.w = w;
                        self.h = h;
                    }
                }
                Value::Bool(self.win != 0)
            }
            "width" => Value::Int(self.w),
            "height" => Value::Int(self.h),
            "clear" => {
                self.draw(CLEAR, 0, 0, 0, 0, int(a, 0), 0, "", 0);
                nil
            }
            "rect" => {
                let r = int(a, 5);
                let op = if r > 0 { FILL_ROUND_RECT } else { FILL_RECT };
                self.draw(op, int(a, 0), int(a, 1), int(a, 2), int(a, 3), int(a, 4), r as u64, "", 0);
                nil
            }
            "frame" => {
                let r = int(a, 5);
                let op = if r > 0 { ROUND_RECT } else { RECT };
                self.draw(op, int(a, 0), int(a, 1), int(a, 2), int(a, 3), int(a, 4), r as u64, "", 0);
                nil
            }
            "gradient" => {
                self.draw(GRADIENT_V, int(a, 0), int(a, 1), int(a, 2), int(a, 3), int(a, 4), int(a, 5) as u32 as u64, "", 0);
                nil
            }
            "line" => {
                let (x1, y1) = (int(a, 0), int(a, 1));
                self.draw(LINE, x1, y1, int(a, 2) - x1, int(a, 3) - y1, int(a, 4), 0, "", 0);
                nil
            }
            "circle" | "ring" => {
                let (cx, cy, r) = (int(a, 0), int(a, 1), int(a, 2).max(1));
                let op = if name == "circle" { FILL_ELLIPSE } else { ELLIPSE };
                self.draw(op, cx - r, cy - r, 2 * r, 2 * r, int(a, 3), 0, "", 0);
                nil
            }
            "text" => {
                let s = text_of(a, 2);
                if a.len() > 4 {
                    self.draw(DRAW_TEXT_STYLED, int(a, 0), int(a, 1), 0, 0, int(a, 3), int(a, 4).clamp(0, 2) as u64, &s, 0);
                } else {
                    self.draw(DRAW_TEXT, int(a, 0), int(a, 1), 0, 0, int(a, 3), 0, &s, 0);
                }
                nil
            }
            "textwidth" => Value::Int(text_of(a, 0).chars().count() as i64 * metrics(int(a, 1).clamp(0, 2) as u64).0),
            "lineheight" => Value::Int(metrics(int(a, 0).clamp(0, 2) as u64).1),
            "button" => {
                let label = text_of(a, 4);
                Value::Bool(self.button(int(a, 0), int(a, 1), int(a, 2), int(a, 3), &label))
            }
            "present" => {
                self.present();
                nil
            }
            "running" => Value::Bool(!self.closed && (self.interactive || self.frames < 3)),
            "quit" => {
                self.closed = true;
                nil
            }
            "mousex" => Value::Int(self.mx),
            "mousey" => Value::Int(self.my),
            "mousedown" => Value::Bool(self.down),
            "clicked" => Value::Bool(self.click.is_some()),
            "clickx" => Value::Int(self.click.map(|c| c.2).unwrap_or(-1)),
            "clicky" => Value::Int(self.click.map(|c| c.3).unwrap_or(-1)),
            "key" => Value::Int(self.keys.pop_front().unwrap_or(0)),
            "ticks" => Value::Int((nanos().saturating_sub(self.t0) / 1_000_000) as i64),
            "wait" => {
                sleep_ms(int(a, 0).clamp(0, 60_000) as u64);
                nil
            }
            "random" => {
                // xorshift64*
                self.rng ^= self.rng >> 12;
                self.rng ^= self.rng << 25;
                self.rng ^= self.rng >> 27;
                let v = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33;
                let n = int(a, 0).max(1) as u64;
                Value::Int((v % n) as i64)
            }
            "rgb" => Value::Int((int(a, 0).clamp(0, 255) << 16) | (int(a, 1).clamp(0, 255) << 8) | int(a, 2).clamp(0, 255)),
            "input" => {
                let (id, placeholder) = (text_of(a, 3), text_of(a, 4));
                Value::Str(self.input(int(a, 0), int(a, 1), int(a, 2).max(30), &id, &placeholder))
            }
            "setinput" => {
                self.fields.insert(text_of(a, 0), text_of(a, 1));
                nil
            }
            "focus" => {
                let id = text_of(a, 0);
                self.focus = if id.is_empty() { None } else { Some(id) };
                nil
            }
            "arg" => Value::Str(self.args.get(int(a, 0).max(0) as usize).cloned().unwrap_or_default()),
            "argcount" => Value::Int(self.args.len() as i64),
            _ => return None,
        };
        Some(Ok(r))
    }
}

/// Is `path` a script this host runs?
pub fn is_script(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.ends_with(".js") || p.ends_with(".ts") || p.ends_with(".py") || p.ends_with(".csx")
}

/// C# script (`.csx`, the bytecode engine `csx.rs`): `Ui.Window(...)`,
/// `Ui.Button(...)`, ... map to the same functions as the JS/TS/Python API
/// (`Ui.TextWidth` -> `textwidth`), `Console.Write(Line)` to the system log.
struct CsUi<'a>(&'a mut Ui);

fn to_script(v: &crate::csx::Value) -> Value {
    use crate::csx::Value as C;
    match v {
        C::Int(n) => Value::Int(*n),
        C::Double(d) => Value::Int(*d as i64),
        C::Bool(b) => Value::Bool(*b),
        C::Char(c) => Value::Int(*c as i64),
        C::Null => Value::Nil,
        other => Value::Str(other.display()),
    }
}

fn from_script(v: Value) -> crate::csx::Value {
    use crate::csx::Value as C;
    match v {
        Value::Int(n) => C::Int(n),
        Value::Bool(b) => C::Bool(b),
        Value::Str(s) => C::str(&s),
        Value::Nil => C::Null,
    }
}

impl crate::csx::Host for CsUi<'_> {
    fn call(&mut self, name: &str, args: &[crate::csx::Value]) -> Option<Result<crate::csx::Value, String>> {
        let short = if let Some(m) = name.strip_prefix("Ui.") {
            m.to_ascii_lowercase()
        } else if name == "Environment.TickCount" {
            String::from("ticks")
        } else {
            name.to_ascii_lowercase()
        };
        let sargs: Vec<Value> = args.iter().map(to_script).collect();
        self.0.call(&short, &sargs).map(|r| r.map(from_script))
    }
    fn print(&mut self, text: &str) -> bool {
        sys::print(text);
        true
    }
}

/// Run a script file as a UI program; exits the process.
pub fn run(path: &str, args: Vec<String>) -> ! {
    let Some(bytes) = sys::read_file(path) else {
        sys::print(&format!("[script] {}: file not found\n", path));
        sys::exit(3);
    };
    let src = String::from_utf8_lossy(&bytes).into_owned();
    let p = path.to_ascii_lowercase();
    let lang = if p.ends_with(".py") { Lang::Python } else if p.ends_with(".ts") { Lang::Ts } else { Lang::Js };
    let t0 = nanos();
    let mut ui = Ui {
        win: 0,
        w: 0,
        h: 0,
        interactive: sys::syscall(IS_INTERACTIVE, 0, 0, 0) == 1,
        closed: false,
        frames: 0,
        t0,
        last_frame: t0,
        mx: -1,
        my: -1,
        down: false,
        press: (-1, -1),
        click: None,
        keys: VecDeque::new(),
        rng: t0 | 1,
        args,
        fields: BTreeMap::new(),
        focus: None,
    };
    let (lines, error, steps) = if p.ends_with(".csx") {
        let out = crate::csx::run_with(&src, &ui.args.clone(), Some(&mut CsUi(&mut ui)), u64::MAX);
        (out.lines, out.error, out.steps)
    } else {
        let out = script::run_with(lang, &src, Some(&mut ui), u64::MAX);
        (out.lines, out.error, out.steps)
    };
    for l in &lines {
        sys::print(l);
        sys::print("\n");
    }
    match error {
        Some(e) => {
            sys::print(&format!("[script] {}: error: {}\n", path, e));
            sys::exit(1)
        }
        None => {
            sys::print(&format!("[script] {}: selesai ({} frame, {} langkah)\n", path, ui.frames, steps));
            sys::exit(0)
        }
    }
}
