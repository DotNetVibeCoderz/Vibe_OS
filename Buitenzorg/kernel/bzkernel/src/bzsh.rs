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
//!   variables, functions).
//! * Functions: `name() { ...; }` or `function name { ...; }` (one line or
//!   several); called like a command, with their own `$1`..`$9`, `$#`, `$@`;
//!   `return [N]` leaves one. A function runs in a script frame, so it nests
//!   at most [`MAX_DEPTH`] deep (recursion included).
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
/// Defined functions: name -> body statements.
static FUNCS: Mutex<BTreeMap<String, Vec<String>>> = Mutex::new(BTreeMap::new());
/// `local` variables, one list per running frame: (name, value before).
static LOCALS: Mutex<Vec<Vec<(String, Option<String>)>>> = Mutex::new(Vec::new());
/// Here-document bodies by id: (lines, expand `$`). A statement refers to one
/// as `<< \u{1}H<id>`; the newest [`MAX_HEREDOCS`] are kept.
static HEREDOCS: Mutex<BTreeMap<u32, (Vec<String>, bool)>> = Mutex::new(BTreeMap::new());
static NEXT_HEREDOC: Mutex<u32> = Mutex::new(0);
const MAX_HEREDOCS: usize = 256;
/// The word a statement uses for here-document `id`.
const HEREDOC_MARK: char = '\u{1}';

/// The lines of here-document `word` (a `\u{1}H<id>` marker), `$` expanded
/// unless its delimiter was quoted. None: not a here-document.
pub fn heredoc(word: &str) -> Option<Vec<String>> {
    let id: u32 = word.strip_prefix(HEREDOC_MARK)?.strip_prefix('H')?.parse().ok()?;
    let (lines, expand) = HEREDOCS.lock().get(&id).cloned()?;
    if !expand {
        return Some(lines);
    }
    Some(lines.iter().map(|l| crate::shell::expand_string(l)).collect())
}

fn store_heredoc(lines: Vec<String>, expand: bool) -> u32 {
    let mut n = NEXT_HEREDOC.lock();
    let id = *n;
    *n = n.wrapping_add(1);
    let mut h = HEREDOCS.lock();
    h.insert(id, (lines, expand));
    while h.len() > MAX_HEREDOCS {
        let oldest = *h.keys().next().unwrap();
        h.remove(&oldest);
    }
    id
}

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
        } else if is_function(a) {
            format!("{}: fungsi shell ('functions' menampilkan isinya)", a)
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
pub const NAMES: &[&str] = &["alias", "unalias", "sh", "source", ".", "which", "type", "exit", "return", "functions", "local"];

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
        "functions" => {
            let f = FUNCS.lock();
            let lines: Vec<String> = if f.is_empty() {
                vec![String::from("(belum ada fungsi -- buat dengan: nama() { perintah; })")]
            } else {
                f.iter().map(|(n, body)| format!("{}() {{ {}; }}", n, body.join("; "))).collect()
            };
            (lines, Effect::None, true)
        }
        "local" => {
            let mut locals = LOCALS.lock();
            let Some(frame) = locals.last_mut() else {
                return (vec![String::from("local: hanya di dalam fungsi atau skrip")], Effect::None, false);
            };
            for a in args {
                let (name, value) = match a.split_once('=') {
                    Some((n, v)) => (n, v),
                    None => (a.as_str(), ""),
                };
                if !frame.iter().any(|(n, _)| n == name) {
                    frame.push((String::from(name), crate::shell::var_value(name)));
                }
                if !crate::shell::set_var(name, value) {
                    return (vec![format!("local: '{}' bukan nama variabel", name)], Effect::None, false);
                }
            }
            (Vec::new(), Effect::None, true)
        }
        "exit" | "return" => {
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
    run_stmts(&statements(text), frame)
}

/// Run parsed statements with positional parameters `frame` (`$0` first).
fn run_stmts(stmts: &[String], frame: Vec<String>) -> (Vec<String>, Effect, bool) {
    let depth = FRAMES.lock().len();
    if depth >= MAX_DEPTH {
        return (vec![String::from("sh: skrip / fungsi bersarang terlalu dalam")], Effect::None, false);
    }
    if depth == 0 {
        *STEPS.lock() = 0;
    }
    let mut pos = 0;
    let nodes = match parse_block(stmts, &mut pos, &[]) {
        Ok((n, _)) => n,
        Err(e) => return (vec![format!("sh: {}: {}", frame[0], e)], Effect::None, false),
    };
    FRAMES.lock().push(frame);
    LOCALS.lock().push(Vec::new());
    let mut cx = Cx { out: Vec::new(), effect: Effect::None, ok: true, stop: None };
    exec_block(&nodes, &mut cx);
    // `local` variables get their earlier values back (or vanish)
    let saved = LOCALS.lock().pop().unwrap_or_default();
    for (name, old) in saved.into_iter().rev() {
        match old {
            Some(v) => {
                crate::shell::set_var(&name, &v);
            }
            None => crate::shell::unset_var(&name),
        }
    }
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
    let all: Vec<&str> = text.lines().collect();
    let mut li = 0;
    while li < all.len() {
        let raw = all[li];
        li += 1;
        let line_start = out.len();
        // here-documents opened on this line: (delimiter, strip tabs, expand, id slot)
        let mut docs: Vec<(String, bool, bool)> = Vec::new();
        let mut cur = String::new();
        let mut quote: Option<char> = None;
        let mut prev_space = true;
        let chars: Vec<char> = raw.chars().collect();
        let mut paren = 0usize; // inside $( ... ): ';' belongs to the inner line
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if quote.is_none() {
                if c == '$' && chars.get(i + 1) == Some(&'(') {
                    paren += 1;
                    cur.push(c);
                    cur.push('(');
                    i += 2;
                    prev_space = false;
                    continue;
                }
                if paren > 0 && c == '(' {
                    paren += 1;
                } else if paren > 0 && c == ')' {
                    paren -= 1;
                }
            }
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
                    '<' if paren == 0 && chars.get(i + 1) == Some(&'<') && chars.get(i + 2) != Some(&'<') => {
                        // <<WORD / <<-WORD / <<'WORD': the body is the next lines
                        let mut k = i + 2;
                        let strip = chars.get(k) == Some(&'-');
                        if strip {
                            k += 1;
                        }
                        while chars.get(k).is_some_and(|c| *c == ' ') {
                            k += 1;
                        }
                        let mut delim = String::new();
                        let mut expand = true;
                        if let Some(&q @ ('\'' | '"')) = chars.get(k) {
                            expand = false;
                            k += 1;
                            while let Some(&c) = chars.get(k) {
                                k += 1;
                                if c == q {
                                    break;
                                }
                                delim.push(c);
                            }
                        } else {
                            while let Some(&c) = chars.get(k) {
                                if c.is_whitespace() || c == ';' || c == '|' || c == '&' || c == '>' || c == '<' {
                                    break;
                                }
                                delim.push(c);
                                k += 1;
                            }
                        }
                        if delim.is_empty() {
                            cur.push_str("<<");
                            i += 2;
                            continue;
                        }
                        cur.push_str(&format!("<< {}H{{{}}} ", HEREDOC_MARK, docs.len()));
                        docs.push((delim, strip, expand));
                        prev_space = true;
                        i = k;
                        continue;
                    }
                    ';' if paren == 0 && chars.get(i + 1) == Some(&';') => {
                        // `;;` ends a case item: a statement of its own
                        push_stmt(&mut out, &cur);
                        cur.clear();
                        out.push(String::from(";;"));
                        prev_space = true;
                        i += 2;
                        continue;
                    }
                    ';' if paren == 0 => {
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
        // the bodies: following lines up to each delimiter, in order
        for (n, (delim, strip, expand)) in docs.iter().enumerate() {
            let mut body = Vec::new();
            while li < all.len() {
                let line = all[li];
                li += 1;
                let l = if *strip { line.trim_start_matches('\t') } else { line };
                if l.trim_end() == delim {
                    break;
                }
                body.push(String::from(l));
            }
            let id = store_heredoc(body, *expand);
            let (from, to) = (format!("{}H{{{}}}", HEREDOC_MARK, n), format!("{}H{}", HEREDOC_MARK, id));
            cur = cur.replace(&from, &to);
            // statements of this line already pushed (before a ';') too
            for s in out[line_start..].iter_mut() {
                if s.contains(&from) {
                    *s = s.replace(&from, &to);
                }
            }
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
    /// `case WORD in` items: (patterns, body).
    Case(String, Vec<(Vec<String>, Vec<Node>)>),
    /// A function definition: name, body statements.
    Func(String, Vec<String>),
}

/// `name() { rest` / `name () {` / `function name {`: (name, what follows `{`).
fn function_head(s: &str) -> Option<(String, String)> {
    let t = s.trim();
    let (name, after) = if let Some(rest) = t.strip_prefix("function ") {
        let rest = rest.trim_start();
        let end = rest.find(|c: char| c.is_whitespace() || c == '(' || c == '{').unwrap_or(rest.len());
        let name = &rest[..end];
        let mut after = rest[end..].trim_start();
        if let Some(a) = after.strip_prefix("()") {
            after = a.trim_start();
        }
        (name, after)
    } else {
        let open = t.find('(')?;
        let name = t[..open].trim();
        let after = t[open..].strip_prefix("()")?.trim_start();
        (name, after)
    };
    if !valid_name(name) {
        return None;
    }
    let body = after.strip_prefix('{')?;
    Some((String::from(name), String::from(body.trim())))
}

/// Whether a whole command line defines a function (the shell routes it here).
pub fn defines_function(line: &str) -> bool {
    function_head(line).is_some()
}

pub fn is_function(name: &str) -> bool {
    FUNCS.lock().contains_key(name)
}

/// Call a defined function with arguments.
pub fn call_function(name: &str, args: &[String]) -> (Vec<String>, Effect, bool) {
    let Some(body) = FUNCS.lock().get(name).cloned() else {
        return (vec![format!("{}: bukan fungsi", name)], Effect::None, false);
    };
    let mut frame = vec![String::from(name)];
    frame.extend(args.iter().cloned());
    run_stmts(&body, frame)
}

/// Defined function names (tab completion).
pub fn function_names() -> Vec<String> {
    FUNCS.lock().keys().cloned().collect()
}

/// Forget a function (`unset -f NAME`); false when there was none.
pub fn remove_function(name: &str) -> bool {
    FUNCS.lock().remove(name).is_some()
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
        if let Some((name, first)) = function_head(s) {
            // the body runs to a statement "}" (or one ending in " }")
            let mut body = Vec::new();
            let mut rest = first;
            loop {
                let r = String::from(rest.trim());
                if r == "}" {
                    break;
                }
                if let Some(head) = r.strip_suffix('}').filter(|h| h.ends_with(char::is_whitespace) || h.ends_with(';')) {
                    let h = head.trim().trim_end_matches(';').trim();
                    if !h.is_empty() {
                        body.push(String::from(h));
                    }
                    break;
                }
                if !r.is_empty() {
                    body.push(r);
                }
                match stmts.get(*pos) {
                    Some(next) => {
                        rest = next.clone();
                        *pos += 1;
                    }
                    None => return Err(format!("fungsi {}: '}}' hilang", name)),
                }
            }
            nodes.push(Node::Func(name, body));
            continue;
        }
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
            "case" => {
                // case WORD in [first item]
                let rest = after_word(s);
                let word = String::from(first_word(rest));
                let tail = after_word(rest);
                if word.is_empty() || first_word(tail) != "in" {
                    return Err(String::from("case: pakai 'case KATA in'"));
                }
                let mut pending: Option<String> = Some(String::from(after_word(tail))).filter(|t| !t.is_empty());
                let mut items = Vec::new();
                loop {
                    let st = match pending.take() {
                        Some(p) => p,
                        None => {
                            let x = stmts.get(*pos).ok_or_else(|| String::from("'esac' hilang"))?.clone();
                            *pos += 1;
                            x
                        }
                    };
                    if st == "esac" {
                        break;
                    }
                    if st == ";;" {
                        continue;
                    }
                    let Some(close) = pattern_end(&st) else {
                        return Err(format!("case: pola tanpa ')': {}", st));
                    };
                    let pats: Vec<String> = st[..close]
                        .trim()
                        .trim_start_matches('(')
                        .split('|')
                        .map(|p| String::from(p.trim()))
                        .filter(|p| !p.is_empty())
                        .collect();
                    let mut body = Vec::new();
                    let first = st[close + 1..].trim();
                    if !first.is_empty() {
                        body.push(String::from(first));
                    }
                    // the item's statements up to ';;' or 'esac' (nested cases keep theirs)
                    let mut depth = usize::from(first_word(first) == "case");
                    loop {
                        let Some(x) = stmts.get(*pos) else { return Err(String::from("'esac' hilang")) };
                        if depth == 0 && x == ";;" {
                            *pos += 1;
                            break;
                        }
                        if depth == 0 && x == "esac" {
                            break;
                        }
                        if first_word(x) == "case" {
                            depth += 1;
                        } else if x == "esac" {
                            depth -= 1;
                        }
                        body.push(x.clone());
                        *pos += 1;
                    }
                    let mut p = 0;
                    let (nodes_in, _) = parse_block(&body, &mut p, &[])?;
                    items.push((pats, nodes_in));
                }
                nodes.push(Node::Case(word, items));
            }
            "then" | "do" | "fi" | "done" | "elif" | "else" | "esac" | ";;" => return Err(format!("'{}' tidak pada tempatnya", w)),
            _ => nodes.push(Node::Cmd(s.clone())),
        }
    }
    if ends.is_empty() {
        Ok((nodes, None))
    } else {
        Err(format!("'{}' hilang", ends[ends.len() - 1]))
    }
}

/// Where a case item's pattern ends: the first ')' outside quotes.
fn pattern_end(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c == ')' => return Some(i),
            None => {}
        }
    }
    None
}

/// Whether a case pattern matches: quoted = compared literally, else `$`
/// expanded and matched with `*` / `?`.
fn case_matches(p: &str, value: &str) -> bool {
    let b = p.as_bytes();
    if b.len() >= 2 && (b[0] == b'\'' || b[0] == b'"') && b[b.len() - 1] == b[0] {
        return &p[1..p.len() - 1] == value;
    }
    let pat = if p.contains('$') { crate::shell::expand_string(p) } else { String::from(p) };
    crate::coreutils::wildcard(&pat, value)
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
            Node::Func(name, body) => {
                FUNCS.lock().insert(name.clone(), body.clone());
                cx.ok = true;
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
            Node::Case(word, items) => {
                let value = crate::shell::expand_words(word).unwrap_or_default().join(" ");
                cx.ok = true;
                for (pats, body) in items {
                    if pats.iter().any(|p| case_matches(p, &value)) {
                        exec_block(body, cx);
                        break;
                    }
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
