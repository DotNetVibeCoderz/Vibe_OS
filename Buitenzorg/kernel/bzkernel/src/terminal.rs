//! Terminal emulator bound to a window (v0.7 "Kanopi"). Holds a scrollback of
//! text lines and the current input line; Enter runs the shell (pipelines,
//! redirection, variables: see `shell::run`) and appends its output. The
//! rendered window body is the tail of the scrollback plus the prompt + input
//! line, with a caret at the editing position.
//!
//! Line editing (Developer Workstation phase 1): Left/Right, Home/End,
//! Backspace/Delete, Ctrl+A/E (start/end), Ctrl+U/K (delete to start/end),
//! Ctrl+W (delete the word before), Ctrl+L (clear the screen), Ctrl+C (cancel
//! the line); Up/Down walk the command history (also `history`, kept in
//! `/disk/HISTORY.TXT` across boots); Tab completes command and file names.
//! The view scrolls back through earlier output with the mouse wheel or
//! PageUp/PageDown; typing or Enter return to the bottom; `clear` empties it.

use alloc::{format, string::String, string::ToString, vec::Vec};
use spin::Mutex;

use crate::shell::{self, Effect};
use crate::wm;

const MAX_SCROLLBACK: usize = 1000;
const MAX_HISTORY: usize = 200;
const HISTORY_FILE: &str = "/disk/HISTORY.TXT";

struct Terminal {
    window: u32,
    visible_rows: usize,
    scrollback: Vec<String>,
    /// The line being edited, and the caret position in it (characters).
    input: Vec<char>,
    pos: usize,
    /// Lines the view is scrolled up from the bottom (0 = following output).
    scroll: usize,
    history: Vec<String>,
    /// Where Up/Down is in the history (None = editing a new line), and the
    /// new line kept aside meanwhile.
    hist_at: Option<usize>,
    draft: Vec<char>,
}

static TERMINAL: Mutex<Option<Terminal>> = Mutex::new(None);

/// Attach the terminal to window `id`, showing `visible_rows` body lines.
pub fn attach(window: u32, visible_rows: usize) {
    let mut term = Terminal {
        window,
        visible_rows,
        scrollback: Vec::new(),
        input: Vec::new(),
        pos: 0,
        scroll: 0,
        history: Vec::new(),
        hist_at: None,
        draft: Vec::new(),
    };
    term.scrollback.push("Buitenzorg Terminal - ketik 'help'".to_string());
    *TERMINAL.lock() = Some(term);
    render();
}

/// Begin the interactive session: drop the boot-demo history, adopt the
/// window's new height, load the command history, and greet.
pub fn start_session(visible_rows: usize) {
    let saved: Vec<String> = crate::vfs::read(HISTORY_FILE)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .map(|t| t.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect())
        .unwrap_or_default();
    if let Some(term) = TERMINAL.lock().as_mut() {
        term.visible_rows = visible_rows.max(2);
        term.scrollback.clear();
        term.scroll = 0;
        term.input.clear();
        term.pos = 0;
        term.history = saved;
        term.scrollback.push("Buitenzorg OS - terminal".to_string());
        term.scrollback.push("Ketik 'help' untuk daftar perintah, 'run <app>' untuk membuka app.".to_string());
        term.scrollback.push("Panah atas/bawah = riwayat, Tab = lengkapi, | > >> && ; = pipe & redirect.".to_string());
        term.scrollback.push(String::new());
    }
    render();
}

/// The command history (oldest first), for the shell's `history`.
pub fn history() -> Vec<String> {
    TERMINAL.lock().as_ref().map(|t| t.history.clone()).unwrap_or_default()
}

/// Feed one decoded character. Returns the effect of any command that ran
/// (Clear/Redraw) so the caller can recompose the desktop.
pub fn feed_char(c: char) -> Effect {
    let mut effect = Effect::None;
    {
        let mut guard = TERMINAL.lock();
        let Some(term) = guard.as_mut() else { return effect };
        match c {
            '\u{F707}' => term.scroll_by(term.page() as isize), // PageUp
            '\u{F708}' => term.scroll_by(-(term.page() as isize)), // PageDown
            '\u{F700}' => term.history_step(-1),                // Up
            '\u{F701}' => term.history_step(1),                 // Down
            '\u{F702}' => term.pos = term.pos.saturating_sub(1), // Left
            '\u{F703}' => term.pos = (term.pos + 1).min(term.input.len()), // Right
            '\u{F704}' | '\u{1}' => term.pos = 0,               // Home, Ctrl+A
            '\u{F705}' | '\u{5}' => term.pos = term.input.len(), // End, Ctrl+E
            '\u{F706}' => {
                // Delete
                if term.pos < term.input.len() {
                    term.input.remove(term.pos);
                }
            }
            '\u{15}' => {
                // Ctrl+U: delete to the start
                term.input.drain(..term.pos);
                term.pos = 0;
            }
            '\u{b}' => {
                // Ctrl+K: delete to the end
                term.input.truncate(term.pos);
            }
            '\u{17}' => {
                // Ctrl+W: delete the word before the caret
                let mut i = term.pos;
                while i > 0 && term.input[i - 1] == ' ' {
                    i -= 1;
                }
                while i > 0 && term.input[i - 1] != ' ' {
                    i -= 1;
                }
                term.input.drain(i..term.pos);
                term.pos = i;
            }
            '\u{c}' => {
                // Ctrl+L: clear the screen, keep the line
                term.scrollback.clear();
                term.scroll = 0;
            }
            '\u{3}' => {
                // Ctrl+C: cancel the line
                let line: String = term.input.iter().collect();
                term.scrollback.push(format!("{}{}^C", shell::prompt(), line));
                term.input.clear();
                term.pos = 0;
                term.hist_at = None;
                term.scroll = 0;
                trim(term);
            }
            '\t' => {
                term.scroll = 0;
                drop(guard);
                complete();
                render();
                return effect;
            }
            '\n' | '\r' => {
                term.scroll = 0;
                let line: String = core::mem::take(&mut term.input).into_iter().collect();
                term.pos = 0;
                term.hist_at = None;
                let prompt = shell::prompt();
                term.scrollback.push(format_line(&prompt, &line));
                let save = !line.trim().is_empty() && term.history.last().map(|l| l.as_str()) != Some(line.trim());
                if save {
                    term.history.push(String::from(line.trim()));
                    if term.history.len() > MAX_HISTORY {
                        let excess = term.history.len() - MAX_HISTORY;
                        term.history.drain(..excess);
                    }
                }
                let history = if save { Some(term.history.join("\n")) } else { None };
                // Run outside the terminal lock (the shell may touch wm/theme,
                // and `history` reads this terminal).
                drop(guard);
                if let Some(text) = history {
                    // best effort: /disk may be absent or read-only
                    let _ = crate::vfs::write(HISTORY_FILE, format!("{}\n", text).as_bytes());
                }
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
                if term.pos > 0 {
                    term.pos -= 1;
                    term.input.remove(term.pos);
                }
            }
            // Other navigation keys (U+F700..) are for apps.
            c if ('\u{F700}'..='\u{F7FF}').contains(&c) => {}
            c if !c.is_control() => {
                term.scroll = 0;
                term.input.insert(term.pos, c);
                term.pos += 1;
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
    /// Up (-1) / Down (+1) through the history; past the newest entry the
    /// line being typed before comes back.
    fn history_step(&mut self, dir: isize) {
        if self.history.is_empty() {
            return;
        }
        let next = match (self.hist_at, dir < 0) {
            (None, true) => {
                self.draft = self.input.clone();
                Some(self.history.len() - 1)
            }
            (None, false) => None,
            (Some(i), true) => Some(i.saturating_sub(1)),
            (Some(i), false) if i + 1 < self.history.len() => Some(i + 1),
            (Some(_), false) => {
                self.input = core::mem::take(&mut self.draft);
                self.pos = self.input.len();
                self.hist_at = None;
                return;
            }
        };
        if let Some(i) = next {
            self.input = self.history[i].chars().collect();
            self.pos = self.input.len();
            self.hist_at = Some(i);
        }
        self.scroll = 0;
    }
}

/// Tab: complete the word before the caret -- a command name for the first
/// word of a command, else a file or folder name. One match is filled in;
/// several extend the common prefix and are listed.
fn complete() {
    let (before, after) = {
        let guard = TERMINAL.lock();
        let Some(t) = guard.as_ref() else { return };
        (t.input[..t.pos].iter().collect::<String>(), t.input[t.pos..].iter().collect::<String>())
    };
    let start = before.rfind(|c: char| c == ' ' || c == '|' || c == ';' || c == '&' || c == '>' || c == '<').map_or(0, |i| i + 1);
    let word = &before[start..];
    let head = before[..start].trim_end();
    let first = head.is_empty() || head.ends_with('|') || head.ends_with(';') || head.ends_with('&');
    let (dir, prefix) = match word.rfind('/') {
        Some(i) => (&word[..=i], &word[i + 1..]),
        None => ("", word),
    };
    let mut cands: Vec<String> = if first && dir.is_empty() {
        shell::command_names().into_iter().filter(|n| n.starts_with(prefix)).map(String::from).collect()
    } else {
        let folder = shell::resolve(if dir.is_empty() { None } else { Some(dir) });
        let lower = prefix.to_ascii_lowercase();
        let mut v: Vec<String> = match crate::vfs::list_entries(&folder) {
            Ok(entries) => entries
                .into_iter()
                .filter(|e| e.name != "." && e.name != ".." && e.name.to_ascii_lowercase().starts_with(&lower))
                .map(|e| if e.is_dir { format!("{}/", e.name) } else { e.name })
                .collect(),
            Err(_) => Vec::new(),
        };
        if folder == "/" || dir.is_empty() && word.starts_with('/') {
            v.extend(crate::vfs::mounts().into_iter().map(|m| format!("{}/", m.trim_start_matches('/'))).filter(|m| m.starts_with(prefix)));
        }
        v
    };
    cands.sort();
    cands.dedup();
    if cands.is_empty() {
        return;
    }
    let common = cands.iter().skip(1).fold(cands[0].clone(), |acc, c| {
        acc.chars().zip(c.chars()).take_while(|(a, b)| a.eq_ignore_ascii_case(b)).map(|(a, _)| a).collect()
    });
    let fill = if cands.len() == 1 {
        let c = &cands[0];
        if c.ends_with('/') { c.clone() } else { format!("{} ", c) }
    } else {
        common.clone()
    };
    let mut guard = TERMINAL.lock();
    let Some(t) = guard.as_mut() else { return };
    if fill.len() >= prefix.len() {
        let new_before = format!("{}{}{}", &before[..start], dir, fill);
        let mut input: Vec<char> = new_before.chars().collect();
        t.pos = input.len();
        input.extend(after.chars());
        t.input = input;
    }
    if cands.len() > 1 {
        let line: String = t.input.iter().collect();
        t.scrollback.push(format!("{}{}", shell::prompt(), line));
        // the candidates, several per row
        let width = cands.iter().map(|c| c.len()).max().unwrap_or(1) + 2;
        let per_row = (80 / width).max(1);
        for row in cands.chunks(per_row) {
            t.scrollback.push(row.iter().map(|c| format!("{:<w$}", c, w = width)).collect::<String>().trim_end().to_string());
        }
        trim(t);
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

/// Push the visible tail (scrollback + current input line) and the caret to
/// the window.
pub fn render() {
    let guard = TERMINAL.lock();
    let Some(term) = guard.as_ref() else { return };
    let rows = term.visible_rows.max(1);

    let mut lines: Vec<String> = Vec::with_capacity(rows);
    let prompt = shell::prompt();
    let input: String = term.input.iter().collect();
    let input_line = format_line(&prompt, &input);

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
            *last = format!("-- {} baris lagi di bawah (roda mouse / PageDown, atau mulai mengetik) --", scroll);
        }
    }
    lines.push(input_line);
    let caret = (scroll == 0).then(|| ((lines.len() - 1) as u16, (prompt.chars().count() + term.pos) as u16));
    let window = term.window;
    drop(guard);
    wm::set_window_lines(window, lines);
    wm::set_window_caret(window, caret);
}

/// True if a terminal is attached (keystrokes should be routed to it).
pub fn is_active() -> bool {
    TERMINAL.lock().is_some()
}
