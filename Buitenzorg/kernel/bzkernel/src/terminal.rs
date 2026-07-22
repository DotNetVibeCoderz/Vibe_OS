//! Terminal emulator bound to a window (v0.7 "Kanopi"). Holds a scrollback of
//! text lines and the current input line; keystrokes do line editing and Enter
//! runs the built-in shell, appending its output. The rendered window body is
//! the tail of the scrollback plus the prompt + input line.

use alloc::{string::String, string::ToString, vec::Vec};
use spin::Mutex;

use crate::shell::{self, Effect};
use crate::wm;

const MAX_SCROLLBACK: usize = 200;

struct Terminal {
    window: u32,
    visible_rows: usize,
    scrollback: Vec<String>,
    input: String,
}

static TERMINAL: Mutex<Option<Terminal>> = Mutex::new(None);

/// Attach the terminal to window `id`, showing `visible_rows` body lines.
pub fn attach(window: u32, visible_rows: usize) {
    let mut term = Terminal {
        window,
        visible_rows,
        scrollback: Vec::new(),
        input: String::new(),
    };
    term.scrollback.push("Buitenzorg Terminal — ketik 'help'".to_string());
    *TERMINAL.lock() = Some(term);
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
            '\n' | '\r' => {
                let line = core::mem::take(&mut term.input);
                let prompt = shell::prompt();
                term.scrollback.push(format_line(&prompt, &line));
                // Run outside the terminal lock (shell may touch wm/theme).
                drop(guard);
                let (output, eff) = shell::run(&line);
                effect = eff;
                let mut guard = TERMINAL.lock();
                let term = guard.as_mut().unwrap();
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
            c if !c.is_control() => {
                term.input.push(c);
            }
            _ => {}
        }
    }
    render();
    effect
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
    let start = term.scrollback.len().saturating_sub(body_rows);
    for l in &term.scrollback[start..] {
        lines.push(l.clone());
    }
    lines.push(input_line);
    wm::set_window_lines(term.window, lines);
}

/// True if a terminal is attached (keystrokes should be routed to it).
pub fn is_active() -> bool {
    TERMINAL.lock().is_some()
}
