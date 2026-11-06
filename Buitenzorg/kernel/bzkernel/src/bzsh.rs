//! Shell scripting for the built-in shell (Developer Workstation, phase 1b):
//! aliases, `.SH` scripts with positional parameters and control flow,
//! command lookup through `$PATH`, and the startup profile.
//!
//! * `alias name='command words'`, `alias` (list), `unalias name`: the first
//!   word of every pipeline stage is replaced (repeatedly, at most 8 times).
//! * `sh FILE [args]`, `source FILE [args]`, `. FILE`; a command that names a
//!   script runs it too (`/disk/BUILD.SH`, `./BUILD.SH`, or `build` when
//!   `BUILD.SH` is in a `$PATH` folder). `$0`..`$9`, `$#`, `$@` are the
//!   script's arguments.
//! * Control flow, one statement per line or `;`-separated:
//!   `if CMD; then ...; elif CMD; then ...; else ...; fi`,
//!   `for X in WORDS; do ...; done`, `while CMD; do ...; done`, `exit [N]`,
//!   `#` comments. Conditions are commands (`test` / `[ ]` in coreutils).
//! * `which`/`type NAME`: alias, built-in, script or program.
//! * `/disk/PROFILE.SH` runs when the terminal session starts (aliases,
//!   variables).
//!
//! Scripts run on the desktop's kernel task, so a script has a budget of
//! [`STEP_BUDGET`] statements and loops at most [`LOOP_LIMIT`] rounds:
//! a runaway script ends with a message instead of freezing the desktop.

use alloc::{collections::BTreeMap, format, string::String, vec, vec::Vec};
use spin::Mutex;

use crate::shell::Effect;

/// Statements one top-level script run may execute.
pub const STEP_BUDGET: usize = 20_000;
/// Rounds of one `while` loop.
pub const LOOP_LIMIT: usize = 5_000;
/// Nested script calls.
const MAX_DEPTH: usize = 8;
/// The startup profile.
pub const PROFILE: &str = "/disk/PROFILE.SH";

static ALIASES: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());
/// Positional parameters, one frame per running script (`$0` first).
static FRAMES: Mutex<Vec<Vec<String>>> = Mutex::new(Vec::new());
static STEPS: Mutex<usize> = Mutex::new(0);

// ---- aliases ----------------------------------------------------------------

pub fn alias_of(name: &str) -> Option<String> {
    ALIASES.lock().get(name).cloned()
}

/// `alias`, `alias name=value`, `alias name`.
pub fn alias_cmd(args: &[String]) -> (Vec<String>, bool) {
    if args.is_empty() {
        let a = ALIASES.lock();
        return (a.iter().map(|(k, v)| format!("alias {}='{}'", k, v)).collect(), true);
    }
    let mut out = Vec::new();
    let mut ok = true;
    for a in args {
        match a.split_once('=') {
            Some((k, v)) if valid_name(k) => {
                ALIASES.lock().insert(String::from(k), String::from(v));
            }
            Some((k, _)) => {
                out.push(format!("alias: nama tidak sah: {}", k));
                ok = false;
            }
            None => match alias_of(a) {
                Some(v) => out.push(format!("alias {}='{}'", a, v)),
                None => {
                    out.push(format!("alias: {}: tidak ada", a));
                    ok = false;
                }
            },
        }
    }
    (out, ok)
}

pub fn unalias_cmd(args: &[String]) -> (Vec<String>, bool) {
    if args.is_empty() {
        return (vec![String::from("pakai: unalias NAMA... (atau -a untuk semua)")], false);
    }
    if args[0] == "-a" {
        ALIASES.lock().clear();
        return (Vec::new(), true);
    }
    let mut out = Vec::new();
    for a in args {
        if ALIASES.lock().remove(a.as_str()).is_none() {
            out.push(format!("unalias: {}: tidak ada", a));
        }
    }
    let ok = out.is_empty();
    (out, ok)
}

fn valid_name(k: &str) -> bool {
    !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

// ---- positional parameters ----------------------------------------------------

/// `$0`..`$9` (`n`), or None outside a script.
pub fn positional(n: usize) -> Option<String> {
    FRAMES.lock().last().map(|f| f.get(n).cloned().unwrap_or_default())
}

/// `$#`.
pub fn arg_count() -> usize {
    FRAMES.lock().last().map(|f| f.len().saturating_sub(1)).unwrap_or(0)
}

/// The script's arguments (`$1`...), for `$@` word splitting.
pub fn arg_list() -> Vec<String> {
    FRAMES.lock().last().map(|f| f.iter().skip(1).cloned().collect()).unwrap_or_default()
}

/// `$@`.
pub fn all_args() -> String {
    FRAMES.lock().last().map(|f| f[1.min(f.len())..].join(" ")).unwrap_or_default()
}

// ---- lookup -------------------------------------------------------------------

/// The folders of `$PATH`.
fn path_dirs() -> Vec<String> {
    let p = crate::shell::var_value("PATH").unwrap_or_default();
    p.split(':').filter(|d| !d.is_empty()).map(String::from).collect()
}

fn exists(path: &str) -> bool {
    crate::vfs::stat(path).is_ok_and(|e| !e.is_dir)
}

/// The script a command word names: a path ending in .SH, or NAME / NAME.SH
/// in a `$PATH` folder.
pub fn find_script(word: &str) -> Option<String> {
    let upper = word.to_ascii_uppercase();
    if word.contains('/') {
        let p = crate::shell::resolve(Some(word));
        return (upper.ends_with(".SH") && exists(&p)).then_some(p);
    }
    for d in path_dirs() {
        for cand in [format!("{}/{}.SH", d, upper), format!("{}/{}", d, upper)] {
            if (cand.ends_with(".SH")) && exists(&cand) {
                return Some(cand);
            }
        }
    }
    None
}

/// `which` / `type`.
pub fn which(args: &[String]) -> (Vec<String>, bool) {
    if args.is_empty() {
        return (vec![String::from("pakai: which NAMA...")], false);
    }
    let mut out = Vec::new();
    let mut ok = true;
    for a in args {
        let line = if let Some(v) = alias_of(a) {
            format!("{}: alias untuk '{}'", a, v)
        } else if crate::coreutils::handles(a) || is_builtin(a) {
            format!("{}: perintah bawaan shell", a)
        } else if crate::shell::is_legacy_command(a) {
            format!("{}: perintah bawaan shell", a)
        } else if let Some(p) = crate::bzpkg::find(a) {
            format!("{}: paket {} {} (entry /disk/{})", a, p.name, p.version, p.entry)
        } else if let Some(s) = find_script(a) {
            s
        } else if crate::app::is_app(a) {
            format!("{}: program (jalankan dengan '{}' atau 'run {}')", a, a, a)
        } else {
            ok = false;
            format!("{}: tidak ditemukan", a)
        };
        out.push(line);
    }
    (out, ok)
}

/// Commands this module implements.
pub const NAMES: &[&str] = &["alias", "unalias", "sh", "source", ".", "which", "type", "exit"];

pub fn is_builtin(name: &str) -> bool {
    NAMES.contains(&name)
}

/// Run one of [`NAMES`].
pub fn builtin(argv: &[String]) -> (Vec<String>, Effect, bool) {
    let args = &argv[1..];
    match argv[0].as_str() {
        "alias" => {
            let (o, ok) = alias_cmd(args);
            (o, Effect::None, ok)
        }
        "unalias" => {
            let (o, ok) = unalias_cmd(args);
            (o, Effect::None, ok)
        }
        "which" | "type" => {
            let (o, ok) = which(args);
            (o, Effect::None, ok)
        }
        "exit" => {
            if FRAMES.lock().is_empty() {
                (vec![String::from("exit: hanya berlaku di dalam skrip (tutup jendela Terminal untuk keluar)")], Effect::None, false)
            } else {
                let code = args.first().and_then(|a| a.parse::<i32>().ok()).unwrap_or(0);
                *EXIT.lock() = Some(code);
                (Vec::new(), Effect::None, code == 0)
            }
        }
        _ => {
            // sh / source / .
            let Some(file) = args.first() else {
                return (vec![format!("pakai: {} FILE [ARG...]", argv[0])], Effect::None, false);
            };
            let path = crate::shell::resolve(Some(file));
            run_file(&path, &args[1..])
        }
    }
}

/// `exit N` inside a script: set by the statement, read by the runner.
static EXIT: Mutex<Option<i32>> = Mutex::new(None);

// ---- scripts --------------------------------------------------------------------

/// Run a script file with arguments.
pub fn run_file(path: &str, args: &[String]) -> (Vec<String>, Effect, bool) {
    let text = match crate::vfs::read(path) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) => return (vec![format!("sh: {}: {}", path, e)], Effect::None, false),
    };
    let mut frame = vec![String::from(path)];
    frame.extend(args.iter().cloned());
    run_text(&text, frame)
}

/// Run script text with positional parameters `frame` (`$0` first).
pub fn run_text(text: &str, frame: Vec<String>) -> (Vec<String>, Effect, bool) {
    let depth = FRAMES.lock().len();
    if depth >= MAX_DEPTH {
        return (vec![String::from("sh: skrip bersarang terlalu dalam")], Effect::None, false);
    }
    if depth == 0 {
        *STEPS.lock() = 0;
    }
    let stmts = statements(text);
    let mut pos = 0;
    let nodes = match parse_block(&stmts, &mut pos, &[]) {
        Ok((n, _)) => n,
        Err(e) => return (vec![format!("sh: {}: {}", frame[0], e)], Effect::None, false),
    };
    FRAMES.lock().push(frame);
    let mut cx = Cx { out: Vec::new(), effect: Effect::None, ok: true, stop: None };
    exec_block(&nodes, &mut cx);
    FRAMES.lock().pop();
    let mut exit = EXIT.lock();
    let ok = match exit.take() {
        Some(code) => code == 0,
        None => cx.ok,
    };
    if depth > 0 {
        // an inner script's exit ends only that script
    }
    drop(exit);
    if let Some(msg) = cx.stop {
        cx.out.push(msg);
        return (cx.out, cx.effect, false);
    }
    (cx.out, cx.effect, ok)
}

/// Split script text into statements: lines, then `;` outside quotes;
/// comments removed; `then X` / `do X` / `else X` become two statements.
fn statements(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let mut cur = String::new();
        let mut quote: Option<char> = None;
        let mut prev_space = true;
        let chars: Vec<char> = raw.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            match quote {
                Some(q) => {
                    if c == q {
                        quote = None;
                    }
                    cur.push(c);
                }
                None => match c {
                    '\'' | '"' => {
                        quote = Some(c);
                        cur.push(c);
                    }
                    '#' if prev_space => break,
                    ';' if chars.get(i + 1) != Some(&';') => {
                        push_stmt(&mut out, &cur);
                        cur.clear();
                    }
                    '\\' => {
                        cur.push(c);
                        if let Some(&n) = chars.get(i + 1) {
                            cur.push(n);
                            i += 1;
                        }
                    }
                    _ => cur.push(c),
                },
            }
            prev_space = c.is_whitespace();
            i += 1;
        }
        push_stmt(&mut out, &cur);
    }
    out
}

fn push_stmt(out: &mut Vec<String>, s: &str) {
    let t = s.trim();
    if t.is_empty() {
        return;
    }
    for kw in ["then", "do", "else"] {
        if let Some(rest) = t.strip_prefix(kw) {
            if rest.starts_with(char::is_whitespace) {
                out.push(String::from(kw));
                push_stmt(out, rest);
                return;
            }
        }
    }
    out.push(String::from(t));
}

enum Node {
    Cmd(String),
    /// (condition, body) pairs, then the else body.
    If(Vec<(String, Vec<Node>)>, Vec<Node>),
    For(String, String, Vec<Node>),
    While(String, Vec<Node>),
}

fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

fn after_word(s: &str) -> &str {
    let t = s.trim_start();
    match t.find(char::is_whitespace) {
        Some(i) => t[i..].trim(),
        None => "",
    }
}

fn expect(stmts: &[String], pos: &mut usize, kw: &str) -> Result<(), String> {
    match stmts.get(*pos) {
        Some(s) if s == kw => {
            *pos += 1;
            Ok(())
        }
        Some(s) => Err(format!("'{}' diharapkan, ditemukan '{}'", kw, s)),
        None => Err(format!("'{}' diharapkan sebelum akhir skrip", kw)),
    }
}

/// Parse statements until one of `ends` (returned) or the end (None).
fn parse_block(stmts: &[String], pos: &mut usize, ends: &[&str]) -> Result<(Vec<Node>, Option<String>), String> {
    let mut nodes = Vec::new();
    while *pos < stmts.len() {
        let s = &stmts[*pos];
        let w = first_word(s);
        if ends.contains(&w) {
            *pos += 1;
            return Ok((nodes, Some(s.clone())));
        }
        *pos += 1;
        match w {
            "if" => {
                let mut arms = Vec::new();
                let mut cond = String::from(after_word(s));
                let mut else_body = Vec::new();
                loop {
                    expect(stmts, pos, "then")?;
                    let (body, end) = parse_block(stmts, pos, &["elif", "else", "fi"])?;
                    arms.push((cond.clone(), body));
                    match end.as_deref().map(first_word) {
                        Some("elif") => cond = String::from(after_word(end.as_deref().unwrap())),
                        Some("else") => {
                            let (b, e) = parse_block(stmts, pos, &["fi"])?;
                            if e.is_none() {
                                return Err(String::from("'fi' hilang"));
                            }
                            else_body = b;
                            break;
                        }
                        Some(_) => break,
                        None => return Err(String::from("'fi' hilang")),
                    }
                }
                nodes.push(Node::If(arms, else_body));
            }
            "for" => {
                let rest = after_word(s);
                let var = String::from(first_word(rest));
                let tail = after_word(rest);
                let words = if first_word(tail) == "in" { String::from(after_word(tail)) } else { String::from("$@") };
                if var.is_empty() {
                    return Err(String::from("for: nama variabel hilang"));
                }
                expect(stmts, pos, "do")?;
                let (body, end) = parse_block(stmts, pos, &["done"])?;
                if end.is_none() {
                    return Err(String::from("'done' hilang"));
                }
                nodes.push(Node::For(var, words, body));
            }
            "while" => {
                let cond = String::from(after_word(s));
                expect(stmts, pos, "do")?;
                let (body, end) = parse_block(stmts, pos, &["done"])?;
                if end.is_none() {
                    return Err(String::from("'done' hilang"));
                }
                nodes.push(Node::While(cond, body));
            }
            "then" | "do" | "fi" | "done" | "elif" | "else" => return Err(format!("'{}' tidak pada tempatnya", w)),
            _ => nodes.push(Node::Cmd(s.clone())),
        }
    }
    if ends.is_empty() {
        Ok((nodes, None))
    } else {
        Err(format!("'{}' hilang", ends[ends.len() - 1]))
    }
}

struct Cx {
    out: Vec<String>,
    effect: Effect,
    ok: bool,
    /// Why the run stopped (budget), if it did.
    stop: Option<String>,
}

impl Cx {
    fn halted(&self) -> bool {
        self.stop.is_some() || EXIT.lock().is_some()
    }
}

/// Run one command line, charging the step budget.
fn step(line: &str, cx: &mut Cx) -> bool {
    {
        let mut s = STEPS.lock();
        *s += 1;
        if *s > STEP_BUDGET {
            cx.stop = Some(format!("sh: dihentikan setelah {} perintah (batas skrip)", STEP_BUDGET));
            return false;
        }
    }
    let (lines, effect) = crate::shell::run(line);
    cx.out.extend(lines);
    if cx.out.len() > 4000 {
        let drop = cx.out.len() - 4000;
        cx.out.drain(..drop);
    }
    if effect == Effect::Clear || cx.effect == Effect::None {
        cx.effect = effect;
    }
    cx.ok = crate::shell::last_ok();
    cx.ok
}

fn exec_block(nodes: &[Node], cx: &mut Cx) {
    for n in nodes {
        if cx.halted() {
            return;
        }
        match n {
            Node::Cmd(line) => {
                step(line, cx);
            }
            Node::If(arms, else_body) => {
                let mut taken = false;
                for (cond, body) in arms {
                    if step(cond, cx) {
                        exec_block(body, cx);
                        taken = true;
                        break;
                    }
                    if cx.halted() {
                        return;
                    }
                }
                if !taken {
                    cx.ok = true;
                    exec_block(else_body, cx);
                }
            }
            Node::For(var, words, body) => {
                let values = crate::shell::expand_words(words).unwrap_or_default();
                for v in values {
                    if cx.halted() {
                        return;
                    }
                    crate::shell::set_var(var, &v);
                    exec_block(body, cx);
                }
            }
            Node::While(cond, body) => {
                let mut rounds = 0;
                while step(cond, cx) {
                    rounds += 1;
                    if rounds > LOOP_LIMIT {
                        cx.stop = Some(format!("sh: while dihentikan setelah {} putaran", LOOP_LIMIT));
                        return;
                    }
                    exec_block(body, cx);
                    if cx.halted() {
                        return;
                    }
                }
                if !cx.halted() {
                    cx.ok = true;
                }
            }
        }
    }
}

/// Run `/disk/PROFILE.SH` (terminal session start); its output, if any.
pub fn run_profile() -> Vec<String> {
    if !exists(PROFILE) {
        return Vec::new();
    }
    let (out, _, ok) = run_file(PROFILE, &[]);
    let mut out = out;
    if !ok {
        out.push(format!("({} berakhir dengan galat)", PROFILE));
    }
    out
}

/// For `help`.
pub fn describe() -> Vec<String> {
    vec![
        String::from("Skrip shell: sh FILE.SH [arg..] / source FILE / . FILE; if/elif/else/fi, for X in ..; do/done,"),
        String::from("  while ..; do/done, exit N, $1..$9 $# $@; test / [ -f X ] [ A = B ] [ N -lt M ]; alias, unalias, which"),
        String::from("  /disk/PROFILE.SH berjalan saat Terminal dibuka (taruh alias di sana)"),
    ]
}
