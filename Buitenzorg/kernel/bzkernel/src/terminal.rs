//! Terminal emulator bound to a window (v0.7 "Kanopi"). Holds a scrollback of
//! text lines and the current input line; keystrokes do line editing and Enter
//! runs the built-in shell, appending its output. The rendered window body is
//! the tail of the scrollback plus the prompt + input line. The view can be
//! scrolled back through earlier output (mouse wheel over the window,
//! PageUp/PageDown; End, typing or Enter return to the bottom); `clear`
//! empties the history.

use alloc::{string::String, string::ToString, vec::Vec};
use spin::Mutex;

use crate::shell::{self, Effect};
use crate::wm;

const MAX_SCROLLBACK: usize = 1000;

struct Terminal {
    window: u32,
    visible_rows: usize,
    scrollback: Vec<String>,
    input: String,
    /// Lines the view is scrolled up from the bottom (0 = following output).
    scroll: usize,
}

static TERMINAL: Mutex<Option<Terminal>> = Mutex::new(None);

/// Attach the terminal to window `id`, showing `visible_rows` body lines.
pub fn attach(window: u32, visible_rows: usize) {
    let mut term = Terminal {
        window,
        visible_rows,
        scrollback: Vec::new(),
        input: String::new(),
        scroll: 0,
    };
    term.scrollback.push("Buitenzorg Terminal - ketik 'help'".to_string());
    *TERMINAL.lock() = Some(term);
    render();
}

/// Begin the interactive session: drop the boot-demo history, adopt the
/// window's new height, and greet with where to start.
pub fn start_session(visible_rows: usize) {
    if let Some(term) = TERMINAL.lock().as_mut() {
        term.visible_rows = visible_rows.max(2);
        term.scrollback.clear();
        term.scroll = 0;
        term.scrollback.push("Buitenzorg OS - terminal".to_string());
        term.scrollback.push("Ketik 'help' untuk daftar perintah, 'run <app>' untuk membuka app.".to_string());
        term.scrollback.push(String::new());
    }
    render();
}

/// Feed one decoded character. Returns the effect of any command that ran
/// (Clear/Redraw) so the caller can recompose the desktop.
pub fn feed_char(c: char) -> Effect {
    let mut effect = Effect::None;
    {
        let mut guard = TERMINAL.lock();
        let Some(term) = guard.as_mut() else { return effect };
        match c {
            // PageUp / PageDown: a screen of history; Home / End: oldest / newest
            '\u{F707}' => term.scroll_by(term.page() as isize),
            '\u{F708}' => term.scroll_by(-(term.page() as isize)),
            '\u{F704}' => term.scroll_by(isize::MAX / 2),
            '\u{F705}' => term.scroll = 0,
            '\n' | '\r' => {
                term.scroll = 0;
                let line = core::mem::take(&mut term.input);
                let prompt = shell::prompt();
                term.scrollback.push(format_line(&prompt, &line));
                // Run outside the terminal lock (shell may touch wm/theme).
                drop(guard);
                let (output, eff) = shell::run(&line);
                effect = eff;
                let mut guard = TERMINAL.lock();
                let term = guard.as_mut().unwrap();
                term.scroll = 0;
                if eff == Effect::Clear {
                    term.scrollback.clear();
                } else {
                    for l in output {
                        term.scrollback.push(l);
                    }
                }
                trim(term);
            }
            '\u{8}' | '\u{7f}' => {
                term.input.pop();
            }
            // Navigation keys (U+F700.., keyboard::nav_char) are for apps.
            c if ('\u{F700}'..='\u{F7FF}').contains(&c) => {}
            c if !c.is_control() => {
                term.scroll = 0;
                term.input.push(c);
            }
            _ => {}
        }
    }
    render();
    effect
}

impl Terminal {
    /// Body rows (the last row is the prompt).
    fn body_rows(&self) -> usize {
        self.visible_rows.max(1).saturating_sub(1)
    }
    /// How far a PageUp/PageDown moves: a screen, keeping one line of context.
    fn page(&self) -> usize {
        self.body_rows().saturating_sub(1).max(1)
    }
    /// Scroll up by `lines` (negative = down), within the history.
    fn scroll_by(&mut self, lines: isize) {
        let max = self.scrollback.len().saturating_sub(self.body_rows());
        let s = (self.scroll as isize).saturating_add(lines).clamp(0, max as isize);
        self.scroll = s as usize;
    }
}

/// Mouse wheel over the terminal window: `notches` > 0 = away from the user
/// = back in history (3 lines per notch). False when the pointer is not over
/// the terminal.
pub fn wheel(window: u32, notches: i64) -> bool {
    {
        let mut guard = TERMINAL.lock();
        let Some(term) = guard.as_mut() else { return false };
        if term.window != window {
            return false;
        }
        term.scroll_by((notches.clamp(-100, 100) * 3) as isize);
    }
    render();
    true
}

fn format_line(prompt: &str, line: &str) -> String {
    let mut s = String::from(prompt);
    s.push_str(line);
    s
}

fn trim(term: &mut Terminal) {
    if term.scrollback.len() > MAX_SCROLLBACK {
        let excess = term.scrollback.len() - MAX_SCROLLBACK;
        term.scrollback.drain(0..excess);
    }
}

/// Push the visible tail (scrollback + current input line) to the window.
pub fn render() {
    let guard = TERMINAL.lock();
    let Some(term) = guard.as_ref() else { return };
    let rows = term.visible_rows.max(1);

    let mut lines: Vec<String> = Vec::with_capacity(rows);
    let input_line = format_line(&shell::prompt(), &term.input);

    // Reserve the last row for the live prompt+input.
    let body_rows = rows.saturating_sub(1);
    let len = term.scrollback.len();
    let scroll = term.scroll.min(len.saturating_sub(body_rows));
    let end = len - scroll;
    let start = end.saturating_sub(body_rows);
    for l in &term.scrollback[start..end] {
        lines.push(l.clone());
    }
    if scroll > 0 {
        // the last history row says where the view is
        if let Some(last) = lines.last_mut() {
            *last = alloc::format!(
                "-- {} baris lagi di bawah (roda mouse / PageDown / End untuk kembali) --",
                scroll
            );
        }
    }
    lines.push(input_line);
    wm::set_window_lines(term.window, lines);
}

/// True if a terminal is attached (keystrokes should be routed to it).
pub fn is_active() -> bool {
    TERMINAL.lock().is_some()
}
