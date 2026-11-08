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
//!
//! **Tabs** (2026-11-08): several sessions in one window, each with its own
//! scrollback, input line, working directory and variables (a
//! `shell::ShellState` swapped in while that tab's command runs) and jobs;
//! the history is shared. Ctrl+T opens a tab, Ctrl+N / Ctrl+P switch, Ctrl+D
//! on an empty line closes one; `tab [new|N|close]` does the same. With more
//! than one tab the first row is the tab bar.
//!
//! **Jobs**: a program started from a tab (`run X`, or a program name) is a
//! job of that tab, and what it prints (DEBUG_WRITE) appears in the tab. A
//! plain command runs it in the *foreground*: the prompt waits until it
//! ends, Ctrl+C ends it, Ctrl+Z lets it go on in the background. `cmd &`
//! starts it in the background at once. `jobs` lists them, `fg [%N]` waits
//! for one again, `kill %N` ends one; the tab reports a background job that
//! finished.

use alloc::collections::BTreeMap;
use alloc::{format, string::String, string::ToString, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

use crate::shell::{self, Effect, ShellState};
use crate::wm;

const MAX_SCROLLBACK: usize = 1000;
const MAX_HISTORY: usize = 200;
const MAX_TABS: usize = 8;
const HISTORY_FILE: &str = "/disk/HISTORY.TXT";

struct Job {
    id: u32,
    pid: u64,
    line: String,
    /// Exit code once it ended.
    done: Option<u64>,
}

struct Tab {
    /// Stable id (jobs and spawns refer to it; the index moves when tabs close).
    id: u32,
    scrollback: Vec<String>,
    /// The line being edited, and the caret position in it (characters).
    input: Vec<char>,
    pos: usize,
    /// Lines the view is scrolled up from the bottom (0 = following output).
    scroll: usize,
    /// Where Up/Down is in the history (None = editing a new line), and the
    /// new line kept aside meanwhile.
    hist_at: Option<usize>,
    draft: Vec<char>,
    /// This tab's working directory, variables and `$?`.
    state: ShellState,
    jobs: Vec<Job>,
    next_job: u32,
    /// The job the prompt waits for.
    fg: Option<u64>,
    /// Program output not yet ended by a newline, per pid.
    partial: BTreeMap<u64, String>,
}

struct Terminal {
    window: u32,
    visible_rows: usize,
    history: Vec<String>,
    tabs: Vec<Tab>,
    cur: usize,
    next_tab: u32,
}

static TERMINAL: Mutex<Option<Terminal>> = Mutex::new(None);
/// The tab whose command line is running right now (0 = none): programs it
/// starts become its jobs.
static RUNNING_TAB: AtomicU32 = AtomicU32::new(0);
/// That command line ended with `&`.
static BACKGROUND: AtomicBool = AtomicBool::new(false);
/// Programs started by the running command line, and what they printed
/// before the terminal adopted them.
static SPAWNED: Mutex<Vec<u64>> = Mutex::new(Vec::new());
static EARLY: Mutex<BTreeMap<u64, String>> = Mutex::new(BTreeMap::new());
/// A job of some tab printed something or may have ended: the desktop loop
/// re-renders ([`poll`]).
static DIRTY: AtomicBool = AtomicBool::new(false);
static JOBS_ALIVE: AtomicU32 = AtomicU32::new(0);
/// Tab commands from inside a command line (`tab new`, ...), applied after it.
static TAB_REQUEST: Mutex<Option<TabRequest>> = Mutex::new(None);

#[derive(Clone, Copy)]
enum TabRequest {
    New,
    Close,
    Go(usize),
}

impl Tab {
    fn new(id: u32) -> Tab {
        Tab {
            id,
            scrollback: Vec::new(),
            input: Vec::new(),
            pos: 0,
            scroll: 0,
            hist_at: None,
            draft: Vec::new(),
            state: shell::new_state(),
            jobs: Vec::new(),
            next_job: 1,
            fg: None,
            partial: BTreeMap::new(),
        }
    }
}

/// Attach the terminal to window `id`, showing `visible_rows` body lines.
pub fn attach(window: u32, visible_rows: usize) {
    let mut tab = Tab::new(1);
    tab.scrollback.push("Buitenzorg Terminal - ketik 'help'".to_string());
    *TERMINAL.lock() = Some(Terminal { window, visible_rows, history: Vec::new(), tabs: alloc::vec![tab], cur: 0, next_tab: 2 });
    render();
}

const GREETING: [&str; 4] = [
    "Buitenzorg OS - terminal",
    "Ketik 'help' untuk daftar perintah, 'run <app>' untuk membuka app.",
    "Panah atas/bawah = riwayat, Tab = lengkapi, | > >> && ; & = pipe, redirect, latar belakang.",
    "Ctrl+T tab baru, Ctrl+N/P pindah tab, Ctrl+D tutup tab; 'jobs', 'fg', Ctrl+C/Ctrl+Z.",
];

/// Begin the interactive session: drop the boot-demo history, adopt the
/// window's new height, load the command history, and greet.
pub fn start_session(visible_rows: usize) {
    let saved: Vec<String> = crate::vfs::read(HISTORY_FILE)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .map(|t| t.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect())
        .unwrap_or_default();
    let tab_id = {
        let mut guard = TERMINAL.lock();
        let Some(term) = guard.as_mut() else { return };
        term.visible_rows = visible_rows.max(2);
        term.history = saved;
        term.tabs.truncate(1);
        term.cur = 0;
        let tab = &mut term.tabs[0];
        tab.scrollback.clear();
        tab.scroll = 0;
        tab.input.clear();
        tab.pos = 0;
        tab.scrollback.extend(GREETING.iter().map(|s| s.to_string()));
        tab.scrollback.push(String::new());
        tab.id
    };
    run_profile_in(tab_id);
    render();
}

/// The startup profile (aliases, variables) in a tab; runs without the
/// terminal lock, with the tab's shell state swapped in.
fn run_profile_in(tab_id: u32) {
    let mut state = take_state(tab_id);
    shell::swap_state(&mut state);
    let profile = crate::bzsh::run_profile();
    shell::swap_state(&mut state);
    put_state(tab_id, state);
    if !profile.is_empty() {
        with_tab(tab_id, |t| t.scrollback.extend(profile));
    }
}

/// The command history (oldest first), for the shell's `history`.
pub fn history() -> Vec<String> {
    TERMINAL.lock().as_ref().map(|t| t.history.clone()).unwrap_or_default()
}

fn take_state(tab_id: u32) -> ShellState {
    let mut guard = TERMINAL.lock();
    guard
        .as_mut()
        .and_then(|t| t.tabs.iter_mut().find(|x| x.id == tab_id))
        .map(|t| core::mem::replace(&mut t.state, shell::new_state()))
        .unwrap_or_else(shell::new_state)
}

fn put_state(tab_id: u32, state: ShellState) {
    with_tab(tab_id, |t| t.state = state);
}

fn with_tab<R>(tab_id: u32, f: impl FnOnce(&mut Tab) -> R) -> Option<R> {
    let mut guard = TERMINAL.lock();
    guard.as_mut().and_then(|t| t.tabs.iter_mut().find(|x| x.id == tab_id)).map(f)
}

/// Feed one decoded character. Returns the effect of any command that ran
/// (Clear/Redraw) so the caller can recompose the desktop.
pub fn feed_char(c: char) -> Effect {
    let mut effect = Effect::None;
    {
        let mut guard = TERMINAL.lock();
        let Some(term) = guard.as_mut() else { return effect };
        // tab keys work whatever the tab is doing
        match c {
            '\u{14}' => {
                // Ctrl+T
                drop(guard);
                open_tab();
                render();
                return Effect::Redraw;
            }
            '\u{e}' | '\u{10}' if term.tabs.len() > 1 => {
                // Ctrl+N / Ctrl+P
                let n = term.tabs.len();
                term.cur = if c == '\u{e}' { (term.cur + 1) % n } else { (term.cur + n - 1) % n };
                drop(guard);
                render();
                return effect;
            }
            _ => {}
        }
        let rows = term.visible_rows;
        let ntabs = term.tabs.len();
        let tab_bar = ntabs > 1;
        let history_len = term.history.len();
        let cur = term.cur;
        let tab = &mut term.tabs[cur];
        if let Some(pid) = tab.fg {
            // a foreground job: the line waits for it
            match c {
                '\u{3}' => {
                    // Ctrl+C: end it
                    crate::process::kill(pid);
                    tab.scrollback.push(String::from("^C"));
                    tab.fg = None;
                }
                '\u{1a}' => {
                    // Ctrl+Z: let it go on in the background
                    if let Some(j) = tab.jobs.iter().find(|j| j.pid == pid) {
                        let msg = format!("^Z\n[{}]+ berjalan di latar belakang   {}", j.id, j.line);
                        tab.scrollback.extend(msg.lines().map(String::from));
                    }
                    tab.fg = None;
                }
                '\u{F707}' => tab.scroll_by(tab.page(rows, tab_bar) as isize, rows, tab_bar),
                '\u{F708}' => tab.scroll_by(-(tab.page(rows, tab_bar) as isize), rows, tab_bar),
                _ => {}
            }
            trim(tab);
            drop(guard);
            render();
            return effect;
        }
        match c {
            '\u{F707}' => tab.scroll_by(tab.page(rows, tab_bar) as isize, rows, tab_bar), // PageUp
            '\u{F708}' => tab.scroll_by(-(tab.page(rows, tab_bar) as isize), rows, tab_bar), // PageDown
            '\u{F700}' | '\u{F701}' => {
                // Up / Down: history
                let dir = if c == '\u{F700}' { -1 } else { 1 };
                let next = history_step(tab, history_len, dir);
                if let Some(i) = next {
                    tab.input = term.history[i].chars().collect();
                    let tab = &mut term.tabs[cur];
                    tab.pos = tab.input.len();
                }
            }
            '\u{F702}' => tab.pos = tab.pos.saturating_sub(1), // Left
            '\u{F703}' => tab.pos = (tab.pos + 1).min(tab.input.len()), // Right
            '\u{F704}' | '\u{1}' => tab.pos = 0,               // Home, Ctrl+A
            '\u{F705}' | '\u{5}' => tab.pos = tab.input.len(), // End, Ctrl+E
            '\u{F706}' => {
                // Delete
                if tab.pos < tab.input.len() {
                    tab.input.remove(tab.pos);
                }
            }
            '\u{15}' => {
                // Ctrl+U: delete to the start
                tab.input.drain(..tab.pos);
                tab.pos = 0;
            }
            '\u{b}' => {
                // Ctrl+K: delete to the end
                tab.input.truncate(tab.pos);
            }
            '\u{17}' => {
                // Ctrl+W: delete the word before the caret
                let mut i = tab.pos;
                while i > 0 && tab.input[i - 1] == ' ' {
                    i -= 1;
                }
                while i > 0 && tab.input[i - 1] != ' ' {
                    i -= 1;
                }
                tab.input.drain(i..tab.pos);
                tab.pos = i;
            }
            '\u{c}' => {
                // Ctrl+L: clear the screen, keep the line
                tab.scrollback.clear();
                tab.scroll = 0;
            }
            '\u{4}' if tab.input.is_empty() && ntabs > 1 => {
                // Ctrl+D on an empty line: close the tab
                drop(guard);
                close_tab();
                render();
                return Effect::Redraw;
            }
            '\u{3}' => {
                // Ctrl+C: cancel the line
                let line: String = tab.input.iter().collect();
                let p = prompt_of(&tab.state);
                tab.scrollback.push(format!("{}{}^C", p, line));
                tab.input.clear();
                tab.pos = 0;
                tab.hist_at = None;
                tab.scroll = 0;
                trim(tab);
            }
            '\t' => {
                tab.scroll = 0;
                let id = tab.id;
                drop(guard);
                let mut state = take_state(id);
                shell::swap_state(&mut state);
                complete(id);
                shell::swap_state(&mut state);
                put_state(id, state);
                render();
                return effect;
            }
            '\n' | '\r' => {
                tab.scroll = 0;
                let line: String = core::mem::take(&mut tab.input).into_iter().collect();
                tab.pos = 0;
                tab.hist_at = None;
                let p = prompt_of(&tab.state);
                tab.scrollback.push(format_line(&p, &line));
                let tab_id = tab.id;
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
                // and `history` / `jobs` read this terminal).
                drop(guard);
                if let Some(text) = history {
                    // best effort: /disk may be absent or read-only
                    let _ = crate::vfs::write(HISTORY_FILE, format!("{}\n", text).as_bytes());
                }
                effect = run_line(tab_id, &line);
            }
            '\u{8}' | '\u{7f}' => {
                if tab.pos > 0 {
                    tab.pos -= 1;
                    tab.input.remove(tab.pos);
                }
            }
            // Other navigation keys (U+F700..) are for apps.
            c if ('\u{F700}'..='\u{F7FF}').contains(&c) => {}
            c if !c.is_control() => {
                tab.scroll = 0;
                tab.input.insert(tab.pos, c);
                tab.pos += 1;
            }
            _ => {}
        }
    }
    render();
    effect
}

/// Run one command line in a tab: its shell state swapped in, the programs it
/// starts adopted as jobs, `&` = background.
fn run_line(tab_id: u32, line: &str) -> Effect {
    let trimmed = line.trim_end();
    let background = trimmed.ends_with('&') && !trimmed.ends_with("&&");
    let cmd = if background { &trimmed[..trimmed.len() - 1] } else { line };
    let mut state = take_state(tab_id);
    shell::swap_state(&mut state);
    SPAWNED.lock().clear();
    EARLY.lock().clear();
    BACKGROUND.store(background, Ordering::SeqCst);
    RUNNING_TAB.store(tab_id, Ordering::SeqCst);
    let (output, eff) = shell::run(cmd);
    RUNNING_TAB.store(0, Ordering::SeqCst);
    shell::swap_state(&mut state);
    let spawned = core::mem::take(&mut *SPAWNED.lock());
    let early = core::mem::take(&mut *EARLY.lock());
    {
        let mut guard = TERMINAL.lock();
        if let Some(term) = guard.as_mut() {
            if let Some(tab) = term.tabs.iter_mut().find(|t| t.id == tab_id) {
                tab.state = state;
                tab.scroll = 0;
                if eff == Effect::Clear {
                    tab.scrollback.clear();
                } else {
                    tab.scrollback.extend(output);
                }
                let fg = !background && spawned.len() == 1;
                for pid in spawned {
                    let id = tab.next_job;
                    tab.next_job += 1;
                    tab.jobs.push(Job { id, pid, line: String::from(cmd.trim()), done: None });
                    JOBS_ALIVE.fetch_add(1, Ordering::Relaxed);
                    if fg {
                        tab.fg = Some(pid);
                        tab.scrollback.push(String::from("(Ctrl+C hentikan, Ctrl+Z biarkan jalan di latar belakang)"));
                    } else {
                        tab.scrollback.push(format!("[{}] {}", id, pid));
                    }
                    if let Some(text) = early.get(&pid) {
                        append_output(tab, pid, text);
                    }
                }
                trim(tab);
            }
        }
    }
    apply_tab_request();
    DIRTY.store(true, Ordering::Relaxed);
    eff
}

/// `run` / a program name started `pid`: a job of the running tab, if a tab
/// command line is running (called by the shell).
pub fn spawned(pid: u64) {
    if RUNNING_TAB.load(Ordering::SeqCst) != 0 {
        SPAWNED.lock().push(pid);
    }
}

/// A program printed `text` (DEBUG_WRITE, any CPU). True when a terminal tab
/// shows it.
pub fn program_output(pid: u64, text: &str) -> bool {
    if pid == 0 {
        return false;
    }
    // a program started by the running command line, not adopted yet
    if RUNNING_TAB.load(Ordering::SeqCst) != 0 && SPAWNED.lock().contains(&pid) {
        let mut early = EARLY.lock();
        let e = early.entry(pid).or_default();
        if e.len() < 16 * 1024 {
            e.push_str(text);
        }
        return true;
    }
    if JOBS_ALIVE.load(Ordering::Relaxed) == 0 {
        return false;
    }
    let mut guard = TERMINAL.lock();
    let Some(term) = guard.as_mut() else { return false };
    let Some(tab) = term.tabs.iter_mut().find(|t| t.jobs.iter().any(|j| j.pid == pid && j.done.is_none())) else {
        return false;
    };
    append_output(tab, pid, text);
    trim(tab);
    DIRTY.store(true, Ordering::Relaxed);
    true
}

fn append_output(tab: &mut Tab, pid: u64, text: &str) {
    let part = tab.partial.entry(pid).or_default();
    part.push_str(text);
    while let Some(i) = part.find('\n') {
        let line: String = part[..i].trim_end_matches('\r').to_string();
        part.drain(..=i);
        tab.scrollback.push(line);
    }
    if part.len() > 512 {
        let line = core::mem::take(part);
        tab.scrollback.push(line);
    }
}

/// Desktop loop: notice jobs that ended and new program output; re-render.
pub fn poll() {
    let mut changed = DIRTY.swap(false, Ordering::Relaxed);
    if JOBS_ALIVE.load(Ordering::Relaxed) > 0 {
        let mut guard = TERMINAL.lock();
        if let Some(term) = guard.as_mut() {
            for tab in term.tabs.iter_mut() {
                let mut finished = Vec::new();
                for j in tab.jobs.iter_mut().filter(|j| j.done.is_none()) {
                    if !crate::process::is_alive(j.pid) {
                        let code = crate::process::exit_code(j.pid).unwrap_or(0);
                        j.done = Some(code);
                        finished.push((j.id, j.pid, code, j.line.clone()));
                    }
                }
                for (id, pid, code, line) in finished {
                    JOBS_ALIVE.fetch_sub(1, Ordering::Relaxed);
                    if let Some(rest) = tab.partial.remove(&pid).filter(|r| !r.is_empty()) {
                        tab.scrollback.push(rest);
                    }
                    if tab.fg == Some(pid) {
                        tab.fg = None;
                        if code != 0 {
                            tab.scrollback.push(format!("(kode keluar {:#x})", code));
                        }
                    } else {
                        tab.scrollback.push(format!("[{}]  Selesai (kode {})   {}", id, code, line));
                    }
                    changed = true;
                }
                trim(tab);
            }
        }
    }
    if changed {
        render();
    }
}

/// `jobs`: the running tab's jobs.
pub fn jobs_cmd() -> Vec<String> {
    let tab_id = RUNNING_TAB.load(Ordering::SeqCst);
    let rows = with_tab(tab_id, |t| {
        t.jobs
            .iter()
            .map(|j| match j.done {
                None => format!("[{}]  Berjalan       pid {:<5} {}", j.id, j.pid, j.line),
                Some(c) => format!("[{}]  Selesai (kode {}) pid {:<5} {}", j.id, c, j.pid, j.line),
            })
            .collect::<Vec<_>>()
    });
    match rows {
        None => alloc::vec![String::from("jobs: hanya di Terminal")],
        Some(r) if r.is_empty() => alloc::vec![String::from("jobs: tidak ada (mulai dengan 'run APP &')")],
        Some(r) => r,
    }
}

/// The pid of job `%N` (or `N`) of the running tab.
pub fn job_pid(spec: &str) -> Option<u64> {
    let n: u32 = spec.trim_start_matches('%').parse().ok()?;
    with_tab(RUNNING_TAB.load(Ordering::SeqCst), |t| t.jobs.iter().find(|j| j.id == n).map(|j| j.pid)).flatten()
}

/// `fg [%N]`: wait for a job again (the newest running one by default).
pub fn fg_cmd(spec: Option<&str>) -> Vec<String> {
    let r = with_tab(RUNNING_TAB.load(Ordering::SeqCst), |t| {
        let job = match spec {
            Some(s) => {
                let n: u32 = s.trim_start_matches('%').parse().unwrap_or(0);
                t.jobs.iter().find(|j| j.id == n)
            }
            None => t.jobs.iter().rev().find(|j| j.done.is_none()),
        };
        match job {
            Some(j) if j.done.is_none() => {
                t.fg = Some(j.pid);
                format!("{}   (Ctrl+C hentikan, Ctrl+Z ke latar belakang)", j.line)
            }
            Some(j) => format!("fg: job {} sudah selesai", j.id),
            None => String::from("fg: tidak ada job seperti itu ('jobs')"),
        }
    });
    alloc::vec![r.unwrap_or_else(|| String::from("fg: hanya di Terminal"))]
}

/// `tab`, `tab new`, `tab N`, `tab close`.
pub fn tab_cmd(arg: Option<&str>) -> Vec<String> {
    if RUNNING_TAB.load(Ordering::SeqCst) == 0 {
        return alloc::vec![String::from("tab: hanya di Terminal")];
    }
    let req = match arg {
        Some("new" | "baru") => Some(TabRequest::New),
        Some("close" | "tutup") => Some(TabRequest::Close),
        Some(n) => match n.parse::<usize>() {
            Ok(n) if n >= 1 => Some(TabRequest::Go(n - 1)),
            _ => return alloc::vec![String::from("tab [new | close | N]")],
        },
        None => None,
    };
    if let Some(r) = req {
        *TAB_REQUEST.lock() = Some(r);
        return Vec::new();
    }
    let guard = TERMINAL.lock();
    let Some(term) = guard.as_ref() else { return Vec::new() };
    term.tabs
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let running = t.jobs.iter().filter(|j| j.done.is_none()).count();
            format!("{} tab {}  {}  ({} job berjalan)", if i == term.cur { "*" } else { " " }, i + 1, cwd_of(&t.state), running)
        })
        .collect()
}

fn apply_tab_request() {
    let Some(r) = TAB_REQUEST.lock().take() else { return };
    match r {
        TabRequest::New => open_tab(),
        TabRequest::Close => close_tab(),
        TabRequest::Go(n) => {
            if let Some(t) = TERMINAL.lock().as_mut() {
                if n < t.tabs.len() {
                    t.cur = n;
                }
            }
        }
    }
}

fn open_tab() {
    let id = {
        let mut guard = TERMINAL.lock();
        let Some(term) = guard.as_mut() else { return };
        if term.tabs.len() >= MAX_TABS {
            let cur = term.cur;
            term.tabs[cur].scrollback.push(format!("tab: paling banyak {} tab", MAX_TABS));
            return;
        }
        let id = term.next_tab;
        term.next_tab += 1;
        let mut tab = Tab::new(id);
        tab.scrollback.push(format!("tab {} -- Ctrl+N/P pindah tab, Ctrl+D (baris kosong) menutupnya", term.tabs.len() + 1));
        term.tabs.push(tab);
        term.cur = term.tabs.len() - 1;
        id
    };
    run_profile_in(id);
}

/// Close the current tab; its running jobs go on (detached).
fn close_tab() {
    let mut guard = TERMINAL.lock();
    let Some(term) = guard.as_mut() else { return };
    if term.tabs.len() <= 1 {
        return;
    }
    let tab = term.tabs.remove(term.cur);
    let running = tab.jobs.iter().filter(|j| j.done.is_none()).count() as u32;
    JOBS_ALIVE.fetch_sub(running.min(JOBS_ALIVE.load(Ordering::Relaxed)), Ordering::Relaxed);
    term.cur = term.cur.min(term.tabs.len() - 1);
}

/// Number of tabs (tests, `tab`).
#[allow(dead_code)]
pub fn tab_count() -> usize {
    TERMINAL.lock().as_ref().map_or(0, |t| t.tabs.len())
}

impl Tab {
    /// Body rows (the last row is the prompt, the first the tab bar).
    fn body_rows(&self, rows: usize, tab_bar: bool) -> usize {
        rows.max(1).saturating_sub(1 + tab_bar as usize)
    }
    /// How far a PageUp/PageDown moves: a screen, keeping one line of context.
    fn page(&self, rows: usize, tab_bar: bool) -> usize {
        self.body_rows(rows, tab_bar).saturating_sub(1).max(1)
    }
    /// Scroll up by `lines` (negative = down), within the history.
    fn scroll_by(&mut self, lines: isize, rows: usize, tab_bar: bool) {
        let max = self.scrollback.len().saturating_sub(self.body_rows(rows, tab_bar));
        let s = (self.scroll as isize).saturating_add(lines).clamp(0, max as isize);
        self.scroll = s as usize;
    }
}

/// Up (-1) / Down (+1) through the history: the entry to show, if any; past
/// the newest entry the line being typed before comes back.
fn history_step(tab: &mut Tab, len: usize, dir: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    tab.scroll = 0;
    let next = match (tab.hist_at, dir < 0) {
        (None, true) => {
            tab.draft = tab.input.clone();
            Some(len - 1)
        }
        (None, false) => None,
        (Some(i), true) => Some(i.saturating_sub(1)),
        (Some(i), false) if i + 1 < len => Some(i + 1),
        (Some(_), false) => {
            tab.input = core::mem::take(&mut tab.draft);
            tab.pos = tab.input.len();
            tab.hist_at = None;
            return None;
        }
    };
    tab.hist_at = next;
    next
}

fn cwd_of(s: &ShellState) -> String {
    if s.cwd.is_empty() { String::from("/disk") } else { s.cwd.clone() }
}

fn prompt_of(s: &ShellState) -> String {
    format!("buitenzorg:{}$ ", cwd_of(s))
}

/// Tab: complete the word before the caret -- a command name for the first
/// word of a command, else a file or folder name. One match is filled in;
/// several extend the common prefix and are listed. (The tab's shell state
/// is swapped in by the caller.)
fn complete(tab_id: u32) {
    let Some((before, after)) = with_tab(tab_id, |t| {
        (t.input[..t.pos].iter().collect::<String>(), t.input[t.pos..].iter().collect::<String>())
    }) else {
        return;
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
    let prompt = shell::prompt();
    with_tab(tab_id, |t| {
        if fill.len() >= prefix.len() {
            let new_before = format!("{}{}{}", &before[..start], dir, fill);
            let mut input: Vec<char> = new_before.chars().collect();
            t.pos = input.len();
            input.extend(after.chars());
            t.input = input;
        }
        if cands.len() > 1 {
            let line: String = t.input.iter().collect();
            t.scrollback.push(format!("{}{}", prompt, line));
            // the candidates, several per row
            let width = cands.iter().map(|c| c.len()).max().unwrap_or(1) + 2;
            let per_row = (80 / width).max(1);
            for row in cands.chunks(per_row) {
                t.scrollback.push(row.iter().map(|c| format!("{:<w$}", c, w = width)).collect::<String>().trim_end().to_string());
            }
            trim(t);
        }
    });
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
        let (rows, bar, cur) = (term.visible_rows, term.tabs.len() > 1, term.cur);
        term.tabs[cur].scroll_by((notches.clamp(-100, 100) * 3) as isize, rows, bar);
    }
    render();
    true
}

fn format_line(prompt: &str, line: &str) -> String {
    let mut s = String::from(prompt);
    s.push_str(line);
    s
}

fn trim(tab: &mut Tab) {
    if tab.scrollback.len() > MAX_SCROLLBACK {
        let excess = tab.scrollback.len() - MAX_SCROLLBACK;
        tab.scrollback.drain(0..excess);
    }
}

/// Push the visible tail (tab bar + scrollback + current input line) and the
/// caret to the window.
pub fn render() {
    let guard = TERMINAL.lock();
    let Some(term) = guard.as_ref() else { return };
    let rows = term.visible_rows.max(1);
    let tab_bar = term.tabs.len() > 1;
    let tab = &term.tabs[term.cur];

    let mut lines: Vec<String> = Vec::with_capacity(rows);
    if tab_bar {
        let mut bar = String::new();
        for (i, t) in term.tabs.iter().enumerate() {
            let busy = if t.fg.is_some() || t.jobs.iter().any(|j| j.done.is_none()) { "*" } else { "" };
            let label = format!("{}:{}{}", i + 1, cwd_of(&t.state), busy);
            if i == term.cur {
                bar.push_str(&format!("[{}] ", label));
            } else {
                bar.push_str(&format!(" {}  ", label));
            }
        }
        lines.push(bar);
    }
    let (prompt, input_line) = match tab.fg {
        Some(pid) => (String::new(), format!("(pid {} berjalan -- Ctrl+C hentikan, Ctrl+Z ke latar belakang)", pid)),
        None => {
            let p = prompt_of(&tab.state);
            let input: String = tab.input.iter().collect();
            let l = format_line(&p, &input);
            (p, l)
        }
    };

    // Reserve the last row for the live prompt+input.
    let body_rows = tab.body_rows(rows, tab_bar);
    let len = tab.scrollback.len();
    let scroll = tab.scroll.min(len.saturating_sub(body_rows));
    let end = len - scroll;
    let start = end.saturating_sub(body_rows);
    for l in &tab.scrollback[start..end] {
        lines.push(l.clone());
    }
    if scroll > 0 {
        // the last history row says where the view is
        if let Some(last) = lines.last_mut() {
            *last = format!("-- {} baris lagi di bawah (roda mouse / PageDown, atau mulai mengetik) --", scroll);
        }
    }
    lines.push(input_line);
    let caret = (scroll == 0 && tab.fg.is_none()).then(|| ((lines.len() - 1) as u16, (prompt.chars().count() + tab.pos) as u16));
    let window = term.window;
    drop(guard);
    wm::set_window_lines(window, lines);
    wm::set_window_caret(window, caret);
}

/// True if a terminal is attached (keystrokes should be routed to it).
pub fn is_active() -> bool {
    TERMINAL.lock().is_some()
}

/// The scrollback of tab `index` (tests).
fn scrollback_of(index: usize) -> Vec<String> {
    TERMINAL.lock().as_ref().and_then(|t| t.tabs.get(index)).map(|t| t.scrollback.clone()).unwrap_or_default()
}

fn type_line(text: &str) {
    for c in text.chars() {
        feed_char(c);
    }
}

/// Wait (up to `ms`) until `done()` holds, polling the jobs like the desktop loop.
fn wait_for(ms: u64, done: impl Fn() -> bool) -> bool {
    let t0 = crate::arch::cpu::nanos();
    while crate::arch::cpu::nanos() - t0 < ms * 1_000_000 {
        poll();
        if done() {
            return true;
        }
        crate::task::yield_now();
    }
    false
}

/// Self-test boot: tabs with their own variables, a background job whose
/// output lands in its tab, a foreground job ended by Ctrl+C, closing a tab
/// -- all through the keys a user would type. `program` prints `expect`.
pub fn self_test(program: &str, expect: &str) -> bool {
    if !is_active() {
        return false;
    }
    let mut ok = Vec::new();
    feed_char('\u{14}'); // Ctrl+T
    ok.push(("Ctrl+T opened tab 2", tab_count() == 2));
    type_line("X=dua\n");
    feed_char('\u{10}'); // Ctrl+P: back to tab 1
    type_line("echo [$X]\n");
    feed_char('\u{e}'); // Ctrl+N: tab 2
    type_line("echo [$X]\n");
    let (t1, t2) = (scrollback_of(0), scrollback_of(1));
    ok.push(("variables are per tab", t1.iter().any(|l| l == "[]") && t2.iter().any(|l| l == "[dua]")));
    type_line(&format!("run {} &\n", program));
    let started = scrollback_of(1).iter().any(|l| l.starts_with("[1] "));
    let ended = wait_for(120_000, || scrollback_of(1).iter().any(|l| l.starts_with("[1]  Selesai")));
    let sb = scrollback_of(1);
    ok.push(("background job: started, its output in the tab, reported done", started && ended && sb.iter().any(|l| l.contains(expect))));
    type_line(&format!("run {}\n", program));
    let fg = TERMINAL.lock().as_ref().is_some_and(|t| t.tabs[1].fg.is_some());
    feed_char('\u{3}'); // Ctrl+C
    let stopped = TERMINAL.lock().as_ref().is_some_and(|t| t.tabs[1].fg.is_none());
    let killed = wait_for(10_000, || scrollback_of(1).iter().any(|l| l.starts_with("[2]  Selesai")));
    ok.push(("foreground job waits; Ctrl+C ends it", fg && stopped && killed));
    type_line("jobs\n");
    ok.push(("jobs lists both", scrollback_of(1).iter().filter(|l| l.contains("Selesai (kode")).count() >= 4));
    feed_char('\u{4}'); // Ctrl+D on an empty line
    ok.push(("Ctrl+D closed the tab", tab_count() == 1));
    let failed: Vec<&str> = ok.iter().filter(|(_, v)| !v).map(|(n, _)| *n).collect();
    for (n, v) in &ok {
        crate::println!("[terminal] self-test: {} {}", if *v { "ok  " } else { "FAIL" }, n);
    }
    if failed.is_empty() {
        crate::println!("[kernel] MILESTONE: TERMINAL TABS OK (2 tabs, per-tab variables, background + foreground jobs, Ctrl+C, Ctrl+D)");
    }
    failed.is_empty()
}
