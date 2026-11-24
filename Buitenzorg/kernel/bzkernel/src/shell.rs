//! Built-in shell (v0.7 "Kanopi"): popular cross-ecosystem commands (Unix +
//! Windows aliases) over the VFS, the theme, and the window manager. Pure over
//! its input line: `run` returns output lines plus an [`Effect`].
//!
//! Supported: help, ls/dir, cat/type, echo, pwd, cd, clear/cls, ver, uname,
//! theme, ws, mounts, bz.

use alloc::{format, string::String, string::ToString, vec, vec::Vec};
use spin::Mutex;

use crate::{ai, app, model, pkg, power, screensaver, script, theme, vfs, vmm, vmx, wallpaper, wm};

pub struct ShellState {
    pub cwd: String,
    /// Shell variables (`NAME=value`, `set`, `export`; `$NAME` expands them).
    pub vars: alloc::collections::BTreeMap<String, String>,
    /// Whether the last command succeeded (`$?`, `&&`, `||`).
    pub ok: bool,
}

static STATE: Mutex<ShellState> = Mutex::new(ShellState { cwd: String::new(), vars: alloc::collections::BTreeMap::new(), ok: true });

/// A fresh shell state (a new terminal tab).
pub fn new_state() -> ShellState {
    ShellState { cwd: String::new(), vars: alloc::collections::BTreeMap::new(), ok: true }
}

/// Exchange the shell's state with `s`: a terminal tab swaps its own state in
/// while its command line runs, and back out afterwards.
pub fn swap_state(s: &mut ShellState) {
    core::mem::swap(&mut *STATE.lock(), s);
}

/// The shell variables (set ones, then the built-in PWD/HOME/USER/SHELL).
pub fn vars() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = STATE.lock().vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for k in ["PWD", "HOME", "USER", "SHELL", "PATH"] {
        if !v.iter().any(|(n, _)| n == k) {
            v.push((String::from(k), var(k).unwrap_or_default()));
        }
    }
    v.sort();
    v
}

fn var(name: &str) -> Option<String> {
    if let Some(v) = STATE.lock().vars.get(name) {
        return Some(v.clone());
    }
    match name {
        "PWD" => Some(cwd()),
        "HOME" => Some(String::from("/disk")),
        "USER" => Some(String::from("administrator")),
        "SHELL" => Some(String::from("bzsh")),
        "PATH" => Some(String::from("/disk:/ram")),
        _ => None,
    }
}

/// A variable's value (set, else built-in), for other modules.
pub fn var_value(name: &str) -> Option<String> {
    var(name)
}

/// Whether the last command succeeded (`$?` = 0).
pub fn last_ok() -> bool {
    STATE.lock().ok
}

/// Whether `name` is one of the shell's own (one-line) commands.
pub fn is_legacy_command(name: &str) -> bool {
    LEGACY_COMMANDS.contains(&name)
}

/// Expand a word list (variables, quotes, globs) the way a command line's
/// arguments are expanded; operators are an error. Used by `for X in ...`.
pub fn expand_words(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for t in tokenize(text)? {
        match t {
            Tok::Word(w) => out.push(w),
            Tok::Op(op) => return Err(format!("{} tidak boleh di sini", op)),
        }
    }
    Ok(out)
}

/// Expand `$` in text the way double quotes do (here-documents, case
/// patterns): one string, no word splitting or globbing; backslashes stay
/// except before `$`.
pub fn expand_string(text: &str) -> String {
    let mut q = String::from("\"");
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '"' => q.push_str("\\\""),
            '\\' if chars.get(i + 1) != Some(&'$') => q.push_str("\\\\"),
            _ => q.push(c),
        }
    }
    q.push('"');
    match tokenize(&q) {
        Ok(toks) => toks
            .into_iter()
            .filter_map(|t| match t {
                Tok::Word(w) => Some(w),
                Tok::Op(_) => None,
            })
            .collect::<Vec<_>>()
            .join(" "),
        Err(_) => String::from(text),
    }
}

/// Set a variable; false when `name` is not a valid name.
pub fn set_var(name: &str, value: &str) -> bool {
    let valid = !name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if valid {
        STATE.lock().vars.insert(String::from(name), String::from(value));
    }
    valid
}

pub fn unset_var(name: &str) {
    STATE.lock().vars.remove(name);
}

/// Every command name (the shell's own and the core utilities), for Tab.
pub fn command_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = LEGACY_COMMANDS.to_vec();
    v.extend_from_slice(crate::coreutils::NAMES);
    v.extend(crate::bzsh::NAMES.iter().filter(|n| **n != "."));
    v.sort();
    v.dedup();
    v
}

const LEGACY_COMMANDS: &[&str] = &[
    "help", "ls", "dir", "echo", "rm", "del", "df", "pwd", "cd", "clear", "cls", "ver", "uname", "about", "mounts",
    "dmesg", "lspci", "lsusb", "wifi", "pi", "gpu", "theme", "ws", "run", "jobs", "fg", "tab", "ps", "crash", "dbg", "gdbserver", "kgdb", "logs", "mixer", "tutup", "tile", "cpu", "kill", "deploy", "ssh", "ipconfig",
    "netup", "ping", "ping6", "arp", "route", "netstat", "hostname", "nslookup", "free", "uptime", "top", "layar", "screenshot", "font", "fon", "bg", "saver", "cursor", "anim", "tone",
    "settings", "notify", "icons", "cari", "search", "ask", "vm", "script", "js", "ts", "py", "csx", "prof", "gfx3d", "compute", "llm", "shutdown", "restart", "sleep", "bz",
];

// ---- command lines: quoting, variables, globs, pipes, redirection ----------
//
// A line is commands joined by `;`, `&&` (run if the previous one succeeded)
// and `||` (run if it failed); a command is a pipeline `a | b | c` whose
// stages pass their output lines on as the next one's input; any stage may
// redirect: `> FILE` (write), `>> FILE` (append), `< FILE` (input). Words may
// be quoted ('...' literal, "..." with $ expansion) or escaped with \;
// `$NAME`, `${NAME}` and `$?` expand; an unquoted word with * or ? expands to
// the matching file names. A command that is only `NAME=value` sets a
// variable. The core utilities (coreutils.rs) read their input from the
// pipe; the shell's other commands ignore it.

#[derive(Clone, PartialEq)]
enum Tok {
    Word(String),
    Op(&'static str),
}

/// Expand `$...` at `chars[i]` (a '$') into `out`; returns the index after it.
fn expand_var(chars: &[char], i: usize, out: &mut String) -> usize {
    let mut j = i + 1;
    // $((expr)) arithmetic, $(command) substitution
    if j < chars.len() && chars[j] == '(' {
        let Some(close) = find_close(chars, j) else {
            out.push('$');
            return i + 1;
        };
        let inner: String = chars[j + 1..close].iter().collect();
        if inner.starts_with('(') {
            // $1, $X, $(cmd) inside the expression first; bare names stay for arith
            let ichars: Vec<char> = inner.chars().collect();
            let mut expanded = String::new();
            let mut k = 0;
            while k < ichars.len() {
                if ichars[k] == '$' {
                    k = expand_var(&ichars, k, &mut expanded);
                } else {
                    expanded.push(ichars[k]);
                    k += 1;
                }
            }
            match arith(&expanded) {
                Ok(v) => out.push_str(&v.to_string()),
                Err(e) => {
                    out.push_str("0");
                    ARITH_ERROR.lock().get_or_insert(e);
                }
            }
        } else {
            out.push_str(&command_subst(&inner));
        }
        return close + 1;
    }
    if j < chars.len() && chars[j] == '?' {
        out.push(if STATE.lock().ok { '0' } else { '1' });
        return j + 1;
    }
    // script parameters: $0..$9, $#, $@
    if j < chars.len() && chars[j].is_ascii_digit() {
        let n = chars[j] as usize - '0' as usize;
        out.push_str(&crate::bzsh::positional(n).unwrap_or_default());
        return j + 1;
    }
    if j < chars.len() && chars[j] == '#' {
        out.push_str(&crate::bzsh::arg_count().to_string());
        return j + 1;
    }
    if j < chars.len() && chars[j] == '@' {
        out.push_str(&crate::bzsh::all_args());
        return j + 1;
    }
    let braced = j < chars.len() && chars[j] == '{';
    if braced {
        j += 1;
    }
    let start = j;
    while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
        j += 1;
    }
    if j == start {
        out.push('$');
        return i + 1;
    }
    let name: String = chars[start..j].iter().collect();
    if braced && j < chars.len() && chars[j] == '}' {
        j += 1;
    }
    out.push_str(&var(&name).unwrap_or_default());
    j
}

/// An arithmetic error inside `$((...))` (expansion cannot fail; `run`
/// reports it after tokenizing).
static ARITH_ERROR: Mutex<Option<String>> = Mutex::new(None);
/// Nesting of `$(...)` (a substitution runs a whole command line).
static SUBST_DEPTH: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Index of the `)` matching the `(` at `open`, skipping quotes and nested parentheses.
fn find_close(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    let mut i = open;
    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some(q) => {
                if c == '\\' && q == '"' {
                    i += 1;
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '\'' | '"' => quote = Some(c),
                '\\' => i += 1,
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    None
}

/// `$(command)`: run the line and take its output (lines joined by newlines,
/// trailing ones dropped). Nested at most 8 deep.
fn command_subst(line: &str) -> String {
    use core::sync::atomic::Ordering;
    if SUBST_DEPTH.fetch_add(1, Ordering::SeqCst) >= 8 {
        SUBST_DEPTH.fetch_sub(1, Ordering::SeqCst);
        return String::new();
    }
    let (lines, _) = run(line);
    SUBST_DEPTH.fetch_sub(1, Ordering::SeqCst);
    let mut s = lines.join("\n");
    while s.ends_with('\n') {
        s.pop();
    }
    s
}

/// `$((expr))`: integer arithmetic with `+ - * / %`, comparisons
/// (`< <= > >= == !=` give 1 or 0), `&&` `||` `!`, parentheses, numbers and
/// variables (`$X` or a bare `X`; unset = 0).
pub fn arith(expr: &str) -> Result<i64, String> {
    struct P<'a> {
        s: &'a [u8],
        i: usize,
    }
    impl P<'_> {
        fn ws(&mut self) {
            while self.i < self.s.len() && (self.s[self.i] as char).is_whitespace() {
                self.i += 1;
            }
        }
        fn eat(&mut self, t: &str) -> bool {
            self.ws();
            if self.s[self.i..].starts_with(t.as_bytes()) {
                self.i += t.len();
                true
            } else {
                false
            }
        }
        fn or(&mut self) -> Result<i64, String> {
            let mut v = self.and()?;
            while self.eat("||") {
                let r = self.and()?;
                v = (v != 0 || r != 0) as i64;
            }
            Ok(v)
        }
        fn and(&mut self) -> Result<i64, String> {
            let mut v = self.cmp()?;
            while self.eat("&&") {
                let r = self.cmp()?;
                v = (v != 0 && r != 0) as i64;
            }
            Ok(v)
        }
        fn cmp(&mut self) -> Result<i64, String> {
            let v = self.sum()?;
            for op in ["<=", ">=", "==", "!=", "<", ">"] {
                if self.eat(op) {
                    let r = self.sum()?;
                    return Ok(match op {
                        "<=" => v <= r,
                        ">=" => v >= r,
                        "==" => v == r,
                        "!=" => v != r,
                        "<" => v < r,
                        _ => v > r,
                    } as i64);
                }
            }
            Ok(v)
        }
        fn sum(&mut self) -> Result<i64, String> {
            let mut v = self.term()?;
            loop {
                if self.eat("+") {
                    v = v.wrapping_add(self.term()?);
                } else if self.eat("-") {
                    v = v.wrapping_sub(self.term()?);
                } else {
                    return Ok(v);
                }
            }
        }
        fn term(&mut self) -> Result<i64, String> {
            let mut v = self.unary()?;
            loop {
                let op = if self.eat("*") {
                    '*'
                } else if self.eat("/") {
                    '/'
                } else if self.eat("%") {
                    '%'
                } else {
                    return Ok(v);
                };
                let r = self.unary()?;
                v = match op {
                    '*' => v.wrapping_mul(r),
                    _ if r == 0 => return Err(String::from("pembagian dengan nol")),
                    '/' => v.wrapping_div(r),
                    _ => v.wrapping_rem(r),
                };
            }
        }
        fn unary(&mut self) -> Result<i64, String> {
            if self.eat("-") {
                return Ok(self.unary()?.wrapping_neg());
            }
            if self.eat("+") {
                return self.unary();
            }
            if self.eat("!") {
                return Ok((self.unary()? == 0) as i64);
            }
            if self.eat("(") {
                let v = self.or()?;
                if !self.eat(")") {
                    return Err(String::from("')' hilang"));
                }
                return Ok(v);
            }
            self.ws();
            let start = self.i;
            let _ = self.eat("$");
            let name_start = self.i;
            while self.i < self.s.len() && ((self.s[self.i] as char).is_ascii_alphanumeric() || self.s[self.i] == b'_') {
                self.i += 1;
            }
            let word = core::str::from_utf8(&self.s[name_start..self.i]).unwrap_or("");
            if word.is_empty() {
                return Err(format!("angka diharapkan di '{}'", core::str::from_utf8(&self.s[start..]).unwrap_or("")));
            }
            if word.as_bytes()[0].is_ascii_digit() {
                return word.parse::<i64>().map_err(|_| format!("bukan angka: {}", word));
            }
            Ok(var(word).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0))
        }
    }
    let mut p = P { s: expr.as_bytes(), i: 0 };
    let v = p.or()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(format!("sisa tak terbaca: '{}'", &expr[p.i..]));
    }
    Ok(v)
}

/// Expand a glob word against its folder; the word itself when nothing matches.
fn expand_glob(word: &str) -> Vec<String> {
    let (dir, pattern) = match word.rfind('/') {
        Some(i) => (&word[..=i], &word[i + 1..]),
        None => ("", word),
    };
    let folder = resolve(if dir.is_empty() { None } else { Some(dir) });
    let mut hits: Vec<String> = vfs::list_entries(&folder)
        .map(|es| {
            es.into_iter()
                .filter(|e| e.name != "." && e.name != ".." && crate::coreutils::wildcard(pattern, &e.name))
                .map(|e| format!("{}{}", dir, e.name))
                .collect()
        })
        .unwrap_or_default();
    hits.sort();
    if hits.is_empty() { vec![String::from(word)] } else { hits }
}

fn tokenize(line: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = line.chars().collect();
    let mut toks = Vec::new();
    let mut cur = String::new();
    let (mut has, mut glob) = (false, false);
    fn flush(toks: &mut Vec<Tok>, cur: &mut String, has: &mut bool, glob: &mut bool) {
        if *has {
            if *glob {
                toks.extend(expand_glob(cur).into_iter().map(Tok::Word));
            } else {
                toks.push(Tok::Word(core::mem::take(cur)));
            }
        }
        cur.clear();
        *has = false;
        *glob = false;
    }
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            ' ' | '\t' => flush(&mut toks, &mut cur, &mut has, &mut glob),
            '\'' => {
                has = true;
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    cur.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err(String::from("tanda kutip ' tidak ditutup"));
                }
            }
            '"' => {
                has = true;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    // "$@": one word per script argument
                    if chars[i] == '$' && chars.get(i + 1) == Some(&'@') {
                        for (k, a) in crate::bzsh::arg_list().iter().enumerate() {
                            if k > 0 {
                                flush(&mut toks, &mut cur, &mut has, &mut glob);
                                has = true;
                            }
                            cur.push_str(a);
                        }
                        i += 2;
                        continue;
                    }
                    if chars[i] == '$' {
                        i = expand_var(&chars, i, &mut cur);
                        continue;
                    }
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                    }
                    cur.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err(String::from("tanda kutip \" tidak ditutup"));
                }
            }
            '\\' => {
                if let Some(n) = next {
                    cur.push(n);
                    has = true;
                    i += 1;
                }
            }
            '$' if next == Some('@') => {
                // $@ unquoted: the script's arguments as separate words
                for (k, a) in crate::bzsh::arg_list().iter().enumerate() {
                    if k > 0 {
                        flush(&mut toks, &mut cur, &mut has, &mut glob);
                    }
                    cur.push_str(a);
                    has = true;
                }
                i += 2;
                continue;
            }
            // unquoted $(command): its output splits into words (`for f in $(ls)`)
            '$' if next == Some('(') && chars.get(i + 2) != Some(&'(') => {
                let mut sub = String::new();
                i = expand_var(&chars, i, &mut sub);
                for (k, w) in sub.split_whitespace().enumerate() {
                    if k > 0 {
                        flush(&mut toks, &mut cur, &mut has, &mut glob);
                    }
                    cur.push_str(w);
                    has = true;
                }
                continue;
            }
            '$' => {
                has = true;
                i = expand_var(&chars, i, &mut cur);
                continue;
            }
            ';' => {
                flush(&mut toks, &mut cur, &mut has, &mut glob);
                toks.push(Tok::Op(";"));
            }
            '&' if next == Some('&') => {
                flush(&mut toks, &mut cur, &mut has, &mut glob);
                toks.push(Tok::Op("&&"));
                i += 1;
            }
            '|' => {
                flush(&mut toks, &mut cur, &mut has, &mut glob);
                if next == Some('|') {
                    toks.push(Tok::Op("||"));
                    i += 1;
                } else {
                    toks.push(Tok::Op("|"));
                }
            }
            '>' => {
                flush(&mut toks, &mut cur, &mut has, &mut glob);
                if next == Some('>') {
                    toks.push(Tok::Op(">>"));
                    i += 1;
                } else {
                    toks.push(Tok::Op(">"));
                }
            }
            '<' => {
                flush(&mut toks, &mut cur, &mut has, &mut glob);
                if chars.get(i + 1) == Some(&'<') {
                    i += 1;
                    toks.push(Tok::Op("<<"));
                } else {
                    toks.push(Tok::Op("<"));
                }
            }
            '*' | '?' => {
                glob = true;
                has = true;
                cur.push(c);
            }
            _ => {
                has = true;
                cur.push(c);
            }
        }
        i += 1;
    }
    flush(&mut toks, &mut cur, &mut has, &mut glob);
    Ok(toks)
}

/// Split a line at `;`, `&&` and `||` outside quotes: (command, the
/// connector before it). Each command is tokenized only when its turn comes,
/// so `X=1; echo $X` and `false || echo $?` see the values of the moment.
fn split_commands(line: &str) -> Result<Vec<(String, &'static str)>, String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut connector = ";";
    let mut quote: Option<char> = None;
    // inside $( ... ): its ; && || belong to the inner command line
    let mut depth = 0usize;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if quote.is_none() && depth > 0 {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                '\\' => {
                    cur.push(c);
                    if let Some(n) = next {
                        cur.push(n);
                        i += 1;
                    }
                    i += 1;
                    continue;
                }
                '\'' | '"' => quote = Some(c),
                _ => {}
            }
            cur.push(c);
            i += 1;
            continue;
        }
        if quote.is_none() && c == '$' && next == Some('(') {
            depth = 1;
            cur.push(c);
            cur.push('(');
            i += 2;
            continue;
        }
        match quote {
            Some(q) => {
                if c == '\\' && q == '"' && next.is_some() {
                    cur.push(c);
                    cur.push(next.unwrap());
                    i += 2;
                    continue;
                }
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
                '\\' => {
                    cur.push(c);
                    if let Some(n) = next {
                        cur.push(n);
                        i += 1;
                    }
                }
                ';' => {
                    out.push((core::mem::take(&mut cur), connector));
                    connector = ";";
                }
                '&' if next == Some('&') => {
                    out.push((core::mem::take(&mut cur), connector));
                    connector = "&&";
                    i += 1;
                }
                '|' if next == Some('|') => {
                    out.push((core::mem::take(&mut cur), connector));
                    connector = "||";
                    i += 1;
                }
                _ => cur.push(c),
            },
        }
        i += 1;
    }
    if let Some(q) = quote {
        return Err(format!("tanda kutip {} tidak ditutup", q));
    }
    out.push((cur, connector));
    Ok(out)
}

/// Run one command line; returns (output lines, effect).
pub fn run(line: &str) -> (Vec<String>, Effect) {
    // `name() { ...; }` (and whatever follows it): the script engine parses it
    if crate::bzsh::defines_function(line) {
        let (out, effect, ok) = crate::bzsh::run_text(line, vec![String::from("sh")]);
        STATE.lock().ok = ok;
        return (out, effect);
    }
    let commands = match split_commands(line) {
        Ok(c) => c,
        Err(e) => {
            STATE.lock().ok = false;
            return (vec![format!("sh: {}", e)], Effect::None);
        }
    };
    let mut out = Vec::new();
    let mut effect = Effect::None;
    let mut ok = true;
    for (text, connector) in commands {
        if text.trim().is_empty() {
            continue;
        }
        let run_it = match connector {
            "&&" => ok,
            "||" => !ok,
            _ => true,
        };
        if !run_it {
            continue;
        }
        let toks = match tokenize(&text) {
            Ok(t) => t,
            Err(e) => {
                out.push(format!("sh: {}", e));
                ok = false;
                STATE.lock().ok = false;
                continue;
            }
        };
        if let Some(e) = ARITH_ERROR.lock().take() {
            out.push(format!("sh: $((...)): {}", e));
            ok = false;
            STATE.lock().ok = false;
            continue;
        }
        if toks.is_empty() {
            continue;
        }
        let (o, e, good) = run_pipeline(&toks);
        out.extend(o);
        if e == Effect::Clear || effect == Effect::None {
            effect = e;
        }
        ok = good;
        STATE.lock().ok = ok;
    }
    (out, effect)
}

/// One pipeline: `a | b | c`, each stage with its redirections.
fn run_pipeline(toks: &[Tok]) -> (Vec<String>, Effect, bool) {
    let mut stdin: Option<Vec<String>> = None;
    let mut effect = Effect::None;
    let mut ok = true;
    let stages: Vec<&[Tok]> = toks.split(|t| *t == Tok::Op("|")).collect();
    let last = stages.len() - 1;
    for (k, stage) in stages.iter().enumerate() {
        let mut argv: Vec<String> = Vec::new();
        let mut out_file: Option<(String, bool)> = None;
        let mut j = 0;
        while j < stage.len() {
            match &stage[j] {
                Tok::Word(w) => argv.push(w.clone()),
                Tok::Op(op) => {
                    let Some(Tok::Word(target)) = stage.get(j + 1) else {
                        return (vec![format!("sh: butuh nama file setelah {}", op)], effect, false);
                    };
                    match *op {
                        ">" | ">>" => out_file = Some((target.clone(), *op == ">>")),
                        "<<" => match crate::bzsh::heredoc(target) {
                            Some(lines) => stdin = Some(lines),
                            None => return (vec![String::from("sh: here-document (<<) hanya di skrip")], effect, false),
                        },
                        "<" => match crate::coreutils::run(&[String::from("cat"), target.clone()], None) {
                            (lines, true) => stdin = Some(lines),
                            (msg, false) => return (msg, effect, false),
                        },
                        _ => {}
                    }
                    j += 1;
                }
            }
            j += 1;
        }
        // aliases: the first word of a stage (repeatedly, a few times)
        for _ in 0..8 {
            let Some(expansion) = argv.first().and_then(|w| crate::bzsh::alias_of(w)) else { break };
            match expand_words(&expansion) {
                Ok(mut words) if !words.is_empty() && words[0] != argv[0] => {
                    words.extend(argv.drain(1..));
                    argv = words;
                }
                Ok(mut words) if !words.is_empty() => {
                    words.extend(argv.drain(1..));
                    argv = words;
                    break;
                }
                _ => break,
            }
        }
        if argv.is_empty() && stage.is_empty() {
            return (vec![String::from("sh: perintah kosong di pipe")], effect, false);
        }
        // only NAME=value words: set variables
        if !argv.is_empty() && argv.iter().all(|a| a.split_once('=').is_some_and(|(k, _)| !k.is_empty() && !k.contains('/'))) {
            for a in &argv {
                let (k, v) = a.split_once('=').unwrap();
                if !set_var(k, v) {
                    return (vec![format!("sh: nama variabel tidak sah: {}", k)], effect, false);
                }
            }
            stdin = Some(Vec::new());
            continue;
        }
        let (mut lines, e, good) = exec(&argv, stdin.as_deref());
        if e != Effect::None {
            effect = e;
        }
        ok = good;
        if let Some((file, append)) = out_file {
            if let Err(err) = crate::coreutils::write_lines(&file, &lines, append) {
                return (vec![format!("sh: {}", err)], effect, false);
            }
            lines = Vec::new();
        }
        if k == last {
            return (lines, effect, ok);
        }
        stdin = Some(lines);
    }
    (Vec::new(), effect, ok)
}

/// Run one command with its arguments (and the pipe's input, if any).
fn exec(argv: &[String], stdin: Option<&[String]>) -> (Vec<String>, Effect, bool) {
    let Some(name) = argv.first() else { return (Vec::new(), Effect::None, true) };
    // shell functions come first, like in other shells
    if crate::bzsh::is_function(name) {
        return crate::bzsh::call_function(name, &argv[1..]);
    }
    if crate::coreutils::handles(name) {
        let (lines, ok) = crate::coreutils::run(argv, stdin);
        return (lines, Effect::None, ok);
    }
    if crate::bzsh::is_builtin(name) {
        return crate::bzsh::builtin(argv);
    }
    if !LEGACY_COMMANDS.contains(&name.as_str()) {
        // a package whose entry is a script, a script (path to a .SH, or
        // NAME.SH in $PATH), else a program
        if let Some(entry) = crate::bzpkg::entry_for(name).filter(|e| e.ends_with(".SH")) {
            return crate::bzsh::run_file(&format!("/disk/{}", entry), &argv[1..]);
        }
        if let Some(script) = crate::bzsh::find_script(name) {
            return crate::bzsh::run_file(&script, &argv[1..]);
        }
        if app::is_app(name) {
            let mut line = String::from("run");
            for a in argv {
                line.push(' ');
                line.push_str(a);
            }
            let (lines, effect) = run_legacy(&line);
            let failed = lines.first().is_some_and(|l| l.starts_with("run: "));
            return (lines, effect, !failed);
        }
    }
    let (lines, effect) = run_legacy(&argv.join(" "));
    // the shell's own commands report errors as "<command>: ..."
    let failed = lines.first().is_some_and(|l| l.contains("command not found") || l.starts_with(&format!("{}: ", name)) && !l.contains(": ok"));
    (lines, effect, !failed)
}

/// Side effect a command asks the terminal/desktop to perform.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    Clear,
    Redraw, // theme/workspace changed; recompose the desktop
}

// ---- network commands -------------------------------------------------------

fn ipconfig() -> Vec<String> {
    use crate::net::{fmt_ip, fmt_mac};
    let Some(i) = crate::net::info() else { return vec![String::from("jaringan: tidak aktif")] };
    let prefix: u32 = i.mask.iter().map(|b| b.count_ones()).sum();
    let mut out = vec![
        format!("{}  {}", if i.attached { "eth0" } else { "lo  " }, if i.attached { "(kartu jaringan)" } else { "(loopback -- tidak ada kartu jaringan)" }),
        format!("      MAC          {}", fmt_mac(i.mac)),
        format!("      IPv4         {}/{}", fmt_ip(i.ip), prefix),
        format!("      netmask      {}", fmt_ip(i.mask)),
        format!("      gateway      {}", if i.gateway == [0; 4] { String::from("-") } else { fmt_ip(i.gateway) }),
        String::from("      DNS          - (belum ada resolver)"),
        format!("      frame        {} terkirim, {} diterima", i.frames_tx, i.frames_rx),
        format!(
            "      IPv6         {}",
            crate::ipv6::addresses().iter().map(|(a, l)| format!("{}/{}", crate::ipv6::fmt(a), l)).collect::<Vec<_>>().join(", ")
        ),
        format!("      router IPv6  {}", crate::ipv6::router().map(|r| crate::ipv6::fmt(&r)).unwrap_or_else(|| String::from("- (ketik 'ping6 -r' untuk meminta)"))),
        format!("      UDP          {} terkirim, {} diterima; ICMP balasan {}", i.udp_tx, i.udp_rx, i.icmp_replies),
    ];
    if !i.attached {
        if let Some(s) = crate::net::status() {
            for (k, part) in s.split("; ").enumerate() {
                out.push(format!("      {}  {}", if k == 0 { "status     " } else { "           " }, part));
            }
        }
        out.push(String::from("      (colok kabel lalu ketik 'netup' untuk mencoba lagi; rincian: dmesg e1000e)"));
    }
    let (ssh_on, _, _) = crate::ssh::status();
    let (dep_on, code, _) = crate::deploy::status();
    out.push(format!(
        "      layanan      ssh {}, deploy {}",
        if ssh_on { "aktif (tcp/22)" } else { "mati" },
        if dep_on { format!("aktif (udp/{}, kode {:04})", crate::deploy::PORT, code) } else { String::from("mati") }
    ));
    out
}

fn ping6(args: &[&str]) -> Vec<String> {
    let Some(target) = args.first() else {
        return vec![String::from("pakai: ping6 <alamat-ipv6> [jumlah]   (mis. ping6 fe80::1); ping6 -r meminta iklan router (SLAAC)")];
    };
    if *target == "-r" {
        return vec![if crate::ipv6::autoconfigure(3000) {
            format!("alamat global: {}", crate::ipv6::addresses().last().map(|(a, l)| format!("{}/{}", crate::ipv6::fmt(a), l)).unwrap_or_default())
        } else {
            String::from("ping6: tidak ada router IPv6 yang menjawab")
        }];
    }
    let dest = match crate::dns::resolve6(target) {
        Ok(a) => a,
        Err(e) => return vec![format!("ping6: {}", e)],
    };
    let n = args.get(1).and_then(|c| c.parse::<u16>().ok()).unwrap_or(4).clamp(1, 20);
    let mut out = vec![format!("PING6 {} ({} kali)", crate::ipv6::fmt(&dest), n)];
    let mut ok = 0;
    for seq in 1..=n {
        match crate::ipv6::ping(&dest, seq, 1000) {
            Some(us) => {
                ok += 1;
                out.push(format!("balasan dari {}: seq={} waktu={}.{:03} ms", crate::ipv6::fmt(&dest), seq, us / 1000, us % 1000));
            }
            None => out.push(format!("seq={}: tidak ada balasan", seq)),
        }
    }
    out.push(format!("{} terkirim, {} diterima", n, ok));
    out
}

fn ping(args: &[&str]) -> Vec<String> {
    let Some(target) = args.first() else {
        return vec![String::from("pakai: ping <alamat-ip|nama> [jumlah]   (mis. ping 10.0.2.2)")];
    };
    let dest = match crate::dns::resolve(target) {
        Ok(ip) => ip,
        Err(e) => return vec![format!("ping: {}", e)],
    };
    let n = args.get(1).and_then(|c| c.parse::<u16>().ok()).unwrap_or(4).clamp(1, 20);
    let ip = crate::net::fmt_ip(dest);
    let mut out = vec![format!("PING {} ({} kali)", ip, n)];
    let (mut ok, mut sum, mut min, mut max) = (0u32, 0u64, u64::MAX, 0u64);
    for seq in 1..=n {
        match crate::net::ping_once(dest, seq, 1000) {
            Some(us) => {
                ok += 1;
                sum += us;
                min = min.min(us);
                max = max.max(us);
                out.push(format!("balasan dari {}: seq={} waktu={}.{:03} ms", ip, seq, us / 1000, us % 1000));
            }
            None => out.push(format!("seq={}: tidak ada balasan (1 detik)", seq)),
        }
    }
    let loss = (n as u32 - ok) * 100 / n as u32;
    out.push(format!("--- {}: {} terkirim, {} diterima, {}% hilang", ip, n, ok, loss));
    if ok > 0 {
        out.push(format!("waktu min/rata/maks = {}.{:03} / {}.{:03} / {}.{:03} ms",
            min / 1000, min % 1000, sum / ok as u64 / 1000, sum / ok as u64 % 1000, max / 1000, max % 1000));
    }
    out
}

fn arp() -> Vec<String> {
    let Some(i) = crate::net::info() else { return vec![String::from("jaringan: tidak aktif")] };
    let mut out = vec![String::from("alamat IP          alamat MAC")];
    for (ip, mac) in i.arp {
        out.push(format!("{:<18} {}{}", crate::net::fmt_ip(ip), crate::net::fmt_mac(mac), if ip == i.ip { "  (perangkat ini)" } else { "" }));
    }
    let nb = crate::ipv6::neighbours();
    if !nb.is_empty() {
        out.push(String::from("tetangga IPv6"));
        for (a, mac) in nb {
            out.push(format!("{:<28} {}", crate::ipv6::fmt(&a), crate::net::fmt_mac(mac)));
        }
    }
    out
}

fn route() -> Vec<String> {
    let Some(i) = crate::net::info() else { return vec![String::from("jaringan: tidak aktif")] };
    let net: [u8; 4] = core::array::from_fn(|k| i.ip[k] & i.mask[k]);
    let mut out = vec![
        String::from("tujuan             netmask            gateway            antarmuka"),
        format!("{:<18} {:<18} {:<18} {}", crate::net::fmt_ip(net), crate::net::fmt_ip(i.mask), "langsung", if i.attached { "eth0" } else { "lo" }),
    ];
    if i.gateway != [0; 4] {
        out.push(format!("{:<18} {:<18} {:<18} {}", "0.0.0.0", "0.0.0.0", crate::net::fmt_ip(i.gateway), if i.attached { "eth0" } else { "lo" }));
    }
    out
}

fn netstat() -> Vec<String> {
    let (conns, listeners) = crate::tcp::snapshot();
    let mut out = vec![String::from("proto  lokal            jarak jauh           status")];
    for p in listeners {
        out.push(format!("tcp    *:{:<14} *:*                  LISTEN", p));
    }
    for (lp, rip, rp, st) in conns {
        let remote = format!("{}:{}", rip, rp);
        out.push(format!("tcp    *:{:<14} {:<20} {:?}", lp, remote, st));
    }
    if let Some(i) = crate::net::info() {
        for p in i.udp_ports {
            out.push(format!("udp    *:{:<14} *:*", p));
        }
    }
    out
}

/// `ssh [on | off]`: the SSH server (port 22, accounts from Pengaturan > Pengguna).
fn ssh_cmd(arg: Option<&str>) -> Vec<String> {
    match arg {
        Some("on") => {
            if let Err(e) = crate::ssh::start(true) {
                return vec![format!("ssh: {}", e)];
            }
        }
        Some("off") => crate::ssh::stop(),
        None | Some("status") => {}
        Some(_) => return vec![String::from("pakai: ssh [on | off]")],
    }
    let (on, sessions, logins) = crate::ssh::status();
    let ip = crate::net::local_ip().map_or(String::from("(tanpa jaringan)"), crate::net::fmt_ip);
    if on {
        vec![
            format!("ssh: AKTIF di {}:{} -- {} sesi, {} login berhasil", ip, crate::ssh::PORT, sessions, logins),
            format!("     kunci host {}", crate::ssh::fingerprint()),
            String::from("     masuk: ssh administrator@<ip> (ganti kata sandi bawaan di Pengaturan > Pengguna)"),
        ]
    } else {
        vec![String::from("ssh: mati ('ssh on' untuk menyalakan server SSH di port 22)")]
    }
}

/// `crash [PID|last]`: list the crash reports of ring-3 programs, or show one.
fn crash_cmd(arg: Option<&str>) -> Vec<String> {
    let dir = crate::crash::dir();
    match arg {
        None => {
            let mut out = Vec::new();
            match crate::vfs::list(dir) {
                Ok(names) if !names.is_empty() => {
                    out.push(format!("laporan crash di {}:", dir));
                    for n in names {
                        out.push(format!("  {}", n));
                    }
                    out.push(String::from("'crash PID' atau 'crash last' untuk melihat isinya"));
                }
                _ => out.push(String::from("belum ada laporan crash")),
            }
            out
        }
        Some("last") => match crate::crash::last() {
            Some((_, t)) => t.lines().map(String::from).collect(),
            None => vec![String::from("belum ada laporan crash")],
        },
        Some(pid) => match crate::vfs::read(&format!("{}/P{}.TXT", dir, pid.trim_start_matches('P').trim_end_matches(".TXT"))) {
            Ok(b) => String::from_utf8_lossy(&b).lines().map(String::from).collect(),
            Err(_) => vec![format!("crash: tidak ada laporan untuk {}", pid)],
        },
    }
}

/// `deploy [on [code] | off]`: the remote deploy service for the Magic App
/// Generator (BZDP over UDP 7070, `deploy.rs`).
fn deploy_cmd(args: &[&str]) -> Vec<String> {
    match args.first().copied() {
        Some("on") => {
            let code = args.get(1).and_then(|c| c.parse::<u16>().ok()).filter(|c| *c < 10_000).unwrap_or_else(crate::deploy::random_code);
            if !crate::deploy::enable(code) {
                return vec![String::from("deploy: port UDP 7070 tidak bisa dibuka")];
            }
            crate::deploy::remember(true, code); // stays on after a reboot until 'deploy off'
        }
        Some("off") => {
            crate::deploy::disable();
            crate::deploy::remember(false, 0);
        }
        None | Some("status") => {}
        Some("log") => {
            let n = args.get(1).and_then(|n| n.parse::<usize>().ok()).unwrap_or(30);
            let lines = crate::deploy::audit_tail(n);
            return if lines.is_empty() { vec![String::from("deploy log: kosong (/disk/DEPLOY.LOG)")] } else { lines };
        }
        Some(_) => return vec![String::from("pakai: deploy [on [kode] | off | log [N]]")],
    }
    let (on, code, served) = crate::deploy::status();
    let ip = crate::net::local_ip().map_or(String::from("(tanpa jaringan)"), |ip| format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]));
    if on {
        vec![
            format!(
                "deploy: AKTIF di {}:{} (UDP + TCP{}), kode pairing {:04}",
                ip,
                crate::deploy::PORT,
                if crate::serial::aux_available() { " + serial COM2" } else { "" },
                code
            ),
            format!("         {} permintaan dilayani; dari PC: 'bz device connect {}:{} {:04}'", served, ip, crate::deploy::PORT, code),
            String::from("         tetap aktif setelah restart sampai 'deploy off'"),
        ]
    } else {
        vec![String::from("deploy: mati ('deploy on' untuk menerima app dari Magic App Generator)")]
    }
}

pub fn cwd() -> String {
    let s = STATE.lock();
    if s.cwd.is_empty() { String::from("/disk") } else { s.cwd.clone() }
}

pub fn prompt() -> String {
    format!("buitenzorg:{}$ ", cwd())
}

/// Run one of the shell's own commands (a plain, already expanded line).
fn run_legacy(line: &str) -> (Vec<String>, Effect) {
    let line = line.trim();
    if line.is_empty() {
        return (Vec::new(), Effect::None);
    }
    let mut parts = line.split_whitespace();
    let cmd = parts.next().unwrap();
    let args: Vec<&str> = parts.collect();

    match cmd {
        "help" => (help(), Effect::None),
        "ls" | "dir" if args.len() > 1 => {
            let mut out = Vec::new();
            for a in &args {
                let lines = list_dir(Some(a));
                if !(lines.len() == 1 && lines[0] == *a) {
                    out.push(format!("{}:", a));
                }
                out.extend(lines);
            }
            (out, Effect::None)
        }
        "ls" | "dir" => (list_dir(args.first().copied()), Effect::None),
        "cat" | "type" => (cat(args.first().copied()), Effect::None),
        "echo" => (echo(&args), Effect::None),
        "rm" | "del" => (rm(args.first().copied()), Effect::None),
        "df" => (df(), Effect::None),
        "pwd" => (vec![cwd()], Effect::None),
        "cd" => (cd(args.first().copied()), Effect::None),
        "clear" | "cls" => (Vec::new(), Effect::Clear),
        "ver" | "uname" => (vec![version()], Effect::None),
        "about" | "credits" => (about(), Effect::None),
        "mounts" => (vfs::mounts(), Effect::None),
        "dmesg" | "log" => (dmesg(args.first().copied()), Effect::None),
        "lspci" => (lspci(), Effect::None),
        "lsusb" => (crate::usb::describe(), Effect::None),
        "wifi" => (crate::wifi::command(&args), Effect::None),
        "notify" | "notif" => (crate::notify::command(&args), Effect::None),
        "cari" | "search" => (crate::search::command(&args), Effect::None),
        #[cfg(target_arch = "aarch64")]
        "pi" => (crate::platform::virt::tryboot::command(&args), Effect::None),
        "gpu" => (gpu_cmd(args.first().copied()), Effect::None),
        "theme" => theme_cmd(args.first().copied()),
        "ws" => ws_cmd(args.first().copied()),
        "run" => run_cmd(args.first().copied(), &args[args.len().min(1)..]),
        "llm" => {
            // llm [MODEL.GGU] [prompt...] [--steps N] [--temp T] [--cpu|--gpu]:
            // the local language model (CLRHOST.ELF llm.rs), default the tiny
            // TinyStories model on every image
            let (model, rest) = match args.first() {
                Some(m) if m.to_ascii_lowercase().ends_with(".ggu") || m.to_ascii_lowercase().ends_with(".gguf") => (*m, &args[1..]),
                _ => ("/disk/STORIES.GGU", &args[..]),
            };
            run_cmd(Some(model), rest)
        }
        "ps" => (ps_cmd(), Effect::None),
        "free" => (free_cmd(), Effect::None),
        "uptime" => (vec![uptime_line()], Effect::None),
        "layar" | "screenshot" => match crate::screenshot::save_screen() {
            Ok(p) => (vec![format!("layar disimpan: {}", p)], Effect::None),
            Err(e) => (vec![format!("layar: {}", e)], Effect::None),
        },
        "top" => (top_cmd(), Effect::None),
        "font" | "fon" => (crate::font::command(&args), Effect::None),
        "crash" | "crashes" => (crash_cmd(args.first().copied()), Effect::None),
        "dbg" | "debug" => (crate::debug::command(&args), Effect::None),
        "gdbserver" => (crate::gdbstub::command(&args), Effect::None),
        "kgdb" => (crate::gdbstub::kgdb_command(&args), Effect::None),
        "logs" => (crate::applog::command(&args), Effect::None),
        "mixer" => (mixer_cmd(&args), Effect::None),
        "tutup" | "closeall" => {
            let n = crate::wm::close_workspace_windows();
            (vec![format!("{} jendela ditutup", n)], Effect::Redraw)
        }
        "tile" | "susun" => {
            let n = crate::wm::tile();
            (vec![format!("{} jendela disusun", n)], Effect::Redraw)
        }
        "cpu" | "cpus" => (cpu_cmd(args.first().copied()), Effect::None),
        "kill" => (kill_cmd(args.first().copied()), Effect::Redraw),
        "jobs" => (crate::terminal::jobs_cmd(), Effect::None),
        "fg" => (crate::terminal::fg_cmd(args.first().copied()), Effect::Redraw),
        "tab" => (crate::terminal::tab_cmd(args.first().copied()), Effect::Redraw),
        "deploy" => (deploy_cmd(&args), Effect::None),
        "ssh" | "sshd" => (ssh_cmd(args.first().copied()), Effect::None),
        "ipconfig" | "ifconfig" | "ip" => (ipconfig(), Effect::None),
        "netup" | "dhcp" => (
            vec![String::from(if crate::net::retry() {
                "jaringan: mencoba lagi di latar belakang (10-15 detik), lalu 'ipconfig'"
            } else {
                "jaringan: tidak ada yang bisa dicoba di platform ini"
            })],
            Effect::None,
        ),
        "ping" => (ping(&args), Effect::None),
        "ping6" => (ping6(&args), Effect::None),
        "arp" => (arp(), Effect::None),
        "route" => (route(), Effect::None),
        "netstat" => (netstat(), Effect::None),
        "hostname" => (vec![String::from("buitenzorg")], Effect::None),
        "nslookup" | "dig" | "host" => {
            let lines = match args.first() {
                None => {
                    let mut l = vec![format!(
                        "server DNS: {}",
                        crate::dns::server().map(crate::net::fmt_ip).unwrap_or_else(|| String::from("(tidak ada)"))
                    )];
                    for (n, ip) in crate::dns::cached() {
                        l.push(format!("  {}  {}", crate::net::fmt_ip(ip), n));
                    }
                    l.push(String::from("pakai: nslookup NAMA (IPv4) | nslookup -6 NAMA (IPv6, AAAA)"));
                    l
                }
                Some(&"-6") => match args.get(1) {
                    None => vec![String::from("pakai: nslookup -6 NAMA")],
                    Some(name) => match crate::dns::resolve6(name) {
                        Ok(a) => vec![format!("{}: {} (AAAA)", name, crate::ipv6::fmt(&a))],
                        Err(e) => vec![format!("nslookup: {}", e)],
                    },
                },
                Some(name) => match crate::dns::resolve(name) {
                    Ok(ip) => vec![format!("{}: {}", name, crate::net::fmt_ip(ip))],
                    Err(e) => vec![format!("nslookup: {}", e)],
                },
            };
            (lines, Effect::None)
        }
        "bg" | "wallpaper" => bg_cmd(args.first().copied()),
        "icons" | "ikon" => icons_cmd(args.first().copied()),
        "saver" | "screensaver" => saver_cmd(args.first().copied()),
        "cursor" => cursor_cmd(args.first().copied()),
        "anim" => anim_cmd(args.first().copied()),
        "tone" | "nada" => {
            // tone <Hz> <ms>: a sine tone on every audio output (sound card / jack / HDMI)
            let hz = args.first().and_then(|a| a.parse::<u32>().ok()).unwrap_or(440);
            let ms = args.get(1).and_then(|a| a.parse::<u32>().ok()).unwrap_or(1000).min(10_000);
            let ok = crate::audio::play_tone(hz, ms);
            (vec![if ok { format!("nada {} Hz, {} ms", hz, ms) } else { String::from("tidak ada perangkat suara") }], Effect::None)
        }
        "settings" | "personalize" => (settings_show(), Effect::None),
        "ask" | "ai" => ai_ask(&args),
        "vm" => vm_cmd(&args),
        "script" | "js" | "ts" | "py" => script_cmd(cmd, &args),
        "csx" | "csharp" => csx_cmd(&args),
        "prof" | "profile" if matches!(args.first().copied(), Some("app" | "pid" | "show" | "stop" | "last")) => {
            (crate::appprof::command(&args), Effect::None)
        }
        "prof" | "profile" => prof_cmd(args.first().copied()),
        "compute" => {
            // compute [gpu|ref|test]: the GPU compute backend (GEMV for AI)
            match args.first().copied() {
                Some("gpu") => crate::compute::set_enabled(true),
                Some("ref") | Some("cpu") => crate::compute::set_enabled(false),
                Some("test") => {
                    crate::compute::self_test();
                }
                _ => {}
            }
            (crate::compute::report(), Effect::None)
        }
        "gfx3d" | "3d" => {
            // gfx3d [test]: the 3D API's resources, or its self-test
            let mut out = Vec::new();
            if args.first() == Some(&"test") {
                let ok = crate::gfx3d::self_test();
                out.push(String::from(if ok { "self-test 3D: OK (lihat log serial)" } else { "self-test 3D GAGAL (lihat log serial)" }));
            }
            match args.first().copied() {
                Some("time") => {
                    out.extend(crate::gfx3d::timing_lines());
                    if args.get(1) == Some(&"reset") {
                        crate::gfx3d::reset_timing();
                        out.push(String::from("waktu direset"));
                    }
                }
                Some("sw") => {
                    crate::virtio::virgl::set_enabled(false);
                    crate::gfx3d::set_native(false);
                }
                Some("gpu") => {
                    crate::virtio::virgl::set_enabled(true);
                    crate::gfx3d::set_native(true);
                }
                _ => {}
            }
            let gpu = crate::virtio::virgl::enabled() && crate::virtio::virgl::available();
            if let Some((name, on)) = crate::gfx3d::native_state() {
                out.push(format!(
                    "backend native: {} ({}) -- 'gfx3d sw' / 'gfx3d gpu' untuk konteks baru",
                    name,
                    if on { "aktif" } else { "mati" }
                ));
            }
            out.push(String::from(if gpu {
                "backend: GPU (virgl, virtio-gpu 3D) untuk konteks baru -- 'gfx3d sw' untuk software"
            } else {
                "backend: rasterizer software -- 'gfx3d gpu' memakai GPU bila virtio-gpu-gl ada"
            }));
            #[cfg(target_arch = "aarch64")]
            {
                let v = crate::platform::virt::v3d::version();
                if v != 0 {
                    out.push(format!("GPU Raspberry Pi: V3D {}.{} terdeteksi (3D tetap rasterizer software; uji job GPU: gpu v3d)", v / 10, v % 10));
                }
            }
            let (c, m, t, used, draws, tris, px) = crate::gfx3d::summary();
            out.push(format!("3D: {} konteks, {} mesh, {} tekstur, {} KiB dari {} KiB", c, m, t, used / 1024, crate::gfx3d::BUDGET / 1024));
            out.push(format!("   {} draw, {} segitiga, {} piksel sejak boot", draws, tris, px));
            (out, Effect::None)
        }
        "shutdown" | "poweroff" => power::shutdown(),
        "restart" | "reboot" => power::restart(),
        "sleep" | "suspend" => {
            power::sleep();
            (vec![String::from("bangun dari sleep")], Effect::Redraw)
        }
        "bz" => bz_cmd(&args),
        other => (vec![format!("{}: command not found (try 'help')", other)], Effect::None),
    }
}

/// The OS version: one place, shown by `ver`, `about`, `bz version` and the
/// boot log. A release candidate until v1.0 has booted on a physical PC.
pub const VERSION: &str = "v1.0 'Buitenzorg'";

/// The kernel log (the last 16 KiB), optionally only lines containing `filter`
/// -- on a PC without a serial cable, the way to see what the boot did.
fn dmesg(filter: Option<&str>) -> Vec<String> {
    let mut buf = vec![0u8; 16 * 1024];
    let (n, _) = crate::logring::read(0, &mut buf);
    let text = String::from_utf8_lossy(&buf[..n]);
    let lines: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty() && filter.map_or(true, |f| l.to_lowercase().contains(&f.to_lowercase())))
        .map(String::from)
        .collect();
    let skip = lines.len().saturating_sub(40);
    let mut out: Vec<String> = lines.into_iter().skip(skip).collect();
    if out.is_empty() {
        out.push(String::from("dmesg: no matching lines"));
    }
    out
}

/// What the GPU is and how the firmware set it up (Intel, PCs).
fn gpu_info() -> Vec<String> {
    let mut out = gpu_info_device();
    out.push(format!(
        "display: vsync from {} at {}.{:03} Hz ({} wait(s) served); full screen: {}",
        crate::vsync::source_name(),
        crate::vsync::refresh_mhz() / 1000,
        crate::vsync::refresh_mhz() % 1000,
        crate::vsync::waits(),
        match crate::wm::fullscreen_window() {
            Some(id) => format!("window #{}", id),
            None => String::from("off"),
        }
    ));
    out
}

fn gpu_info_device() -> Vec<String> {
    #[cfg(target_arch = "x86_64")]
    {
        crate::intel_gpu::info()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        #[cfg(target_arch = "aarch64")]
        {
            let v = crate::platform::virt::v3d::version();
            if v != 0 {
                let mut out = vec![format!("gpu: V3D {}.{} (Raspberry Pi 4) -- 'gpu v3d' runs the self-test, 'gpu on' / 'gpu cpu' switch presenting", v / 10, v % 10)];
                out.extend(crate::platform::virt::v3d_draw::report());
                return out;
            }
        }
        vec![String::from("gpu: no GPU driver here (Intel graphics on PCs, V3D on a Raspberry Pi 4)")]
    }
}

/// `gpu blit`: the Intel blitter self-test (step 2 of GPU support).
/// `gpu on` / `gpu cpu`: present through the blitter or by CPU (step 2b).
fn gpu_cmd(arg: Option<&str>) -> Vec<String> {
    #[cfg(target_arch = "x86_64")]
    {
        use crate::platform::pc::intel_blt;
        match arg {
            Some("blit") => intel_blt::self_test(),
            Some("render") => crate::platform::pc::intel_render::self_test(),
            Some("on") => vec![format!("gpu: {}", intel_blt::enable_present())],
            Some("soft") => vec![format!("gpu: {}", intel_blt::soft_present())],
            Some("soft-stats") => vec![format!("gpu soft: {} rectangles copied", intel_blt::soft_rects())],
            Some("cpu") | Some("off") => {
                intel_blt::disable_present();
                vec![String::from("gpu: the desktop is presented by the CPU")]
            }
            _ => gpu_info(),
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        match arg {
            None => gpu_info(),
            #[cfg(target_arch = "aarch64")]
            Some("v3d") => crate::platform::virt::v3d::self_test(),
            #[cfg(target_arch = "aarch64")]
            Some("on") => vec![format!("gpu: {}", crate::platform::virt::v3d_draw::enable_present())],
            #[cfg(target_arch = "aarch64")]
            Some("cpu") | Some("off") => {
                crate::platform::virt::v3d_draw::disable_present();
                vec![String::from("gpu: the desktop is presented by the CPU")]
            }
            Some(_) => vec![String::from("gpu blit/render/on/cpu: PCs with Intel graphics; 'gpu v3d' / 'gpu on' / 'gpu cpu' on a Raspberry Pi 4")],
        }
    }
}

/// PCI devices: which controllers a PC has (and so which drivers it needs).
fn lspci() -> Vec<String> {
    #[cfg(target_arch = "x86_64")]
    {
        crate::pci::scan()
            .iter()
            .map(|d| {
                format!(
                    "{:02x}:{:02x}.{} {:04x}:{:04x} {:02x}{:02x}{:02x} {}",
                    d.bus, d.slot, d.function, d.vendor_id, d.device_id, d.class, d.subclass, d.prog_if, d.class_name()
                )
            })
            .collect()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        vec![String::from("lspci: PCs only")]
    }
}

fn version() -> String {
    format!("Buitenzorg OS {} -- Gravicode Studios (Kang Fadhil)", VERSION)
}

fn about() -> Vec<String> {
    let mut lines: Vec<String> = ["Buitenzorg OS",
     "  Sistem operasi hibrida & AI-native (kernel Rust, userland C#).",
     "  \"zonder zorg\" - tanpa kekhawatiran. Kebun Raya Bogor edition.",
     "",
     "  Dibuat oleh : Gravicode Studios",
     "  Dipimpin oleh: Kang Fadhil"]
        .iter()
        .map(|s| String::from(*s))
        .collect();
    lines.push(format!("  Versi       : {}", VERSION));
    lines
}

fn help() -> Vec<String> {
    let mut v = help_lines();
    v.extend(crate::bzsh::describe());
    v
}

/// "  izin: network, audio" for an installed package (`bz install`).
fn perm_line(name: &str) -> String {
    format!("  izin: {} (ubah: bz perm {} +izin / -izin)", crate::perm::names(crate::bzpkg::find(name).map_or(0, |p| p.perms)), name)
}

fn help_lines() -> Vec<String> {
    ["Perintah:",
     "  ls|dir [path]     daftar isi direktori (VFS)",
     "  cat|type <file>   tampilkan isi file",
     "  cd <path>         pindah direktori",
     "  pwd               direktori sekarang",
     "  dmesg [kata]      log kernel (40 baris terakhir, opsional disaring)",
     "  lspci             perangkat PCI (kontroler disk/USB/jaringan)",
     "  lsusb             perangkat USB (colok/cabut terdeteksi otomatis)",
     "  layar             simpan tangkapan layar ke /disk/LAYAR (juga tombol Print Screen)",
     "  pi [tryboot|promote]  Raspberry Pi: coba kernel baru (/disk/KERNTRY.IMG) sekali",
     "  gpu [blit|on|cpu] GPU; blit = uji blitter Intel, on/cpu = tampilkan lewat GPU/CPU",
     "  gpu render        uji render engine Intel (3D, tahap 3a)",
     "  netup             coba lagi menyalakan jaringan (DHCP)",
     "  echo <teks>       cetak teks",
     "  mounts            daftar mount VFS",
     "  theme [nama|cycle|list|reload]  ganti tema (bawaan + paket /disk/THEMES/*.THM)",
     "  icons [nama|list|bawaan]    paket ikon dari /disk/ICONS/*.BIN",
     "  notify <judul> [isi]        kirim notifikasi (toast + pusat notifikasi)",
     "  cari <kata>                 cari aplikasi, pengaturan, dan berkas",
     "  bg [nama|list|/path.bmp]    ganti wallpaper (bawaan / gambar user)",
     "  saver [nama|list|off]       screensaver (starfield, mystify, pipes, ...)",
     "  cursor [normal|besar]       ukuran kursor",
     "  anim [on|off]               micro-interaction on/off",
     "  settings                    tampilkan pengaturan (personalization)",
     "  ws [1-4]          pindah virtual desktop",
     "  tile|susun        susun jendela desktop ini berdampingan",
     "  tutup|closeall    tutup semua jendela desktop ini (kecuali terminal)",
     "  run <app> [arg..] jalankan app sebagai proses (xox, paint, taskmgr)",
     "  run <app>.dll     app C# managed (RustCLR), mis. 'run hellogui'",
     "  ps                daftar proses (task kernel + app)",
     "  top               proses menurut waktu CPU + memori + uptime",
     "  free / uptime     memori (RAM, frame, heap kernel) / lama menyala",
     "  cpu [on|off]      daftar CPU; on/off = proses ring-3 boleh jalan di AP",
     "  echo <teks> > f   tulis teks ke file (>> = tambahkan); /disk persisten",
     "  rm <file>         hapus file",
     "  df                ruang kosong tiap mount",
     "  kill <pid>        hentikan proses app",
     "  cmd &  | jobs | fg [%N] | kill %N  job: latar belakang / daftar / tunggu lagi / hentikan",
     "  tab [new|N|close]  tab terminal (Ctrl+T baru, Ctrl+N/P pindah, Ctrl+D tutup)",
     "  mixer [PID VOL]   aliran audio per program dan volumenya",
     "  logs [APP|PID]    keluaran per program (8 KiB terakhir, juga setelah selesai)",
     "  kgdb PORT         baca memori kernel dari GDB di PC (tanpa kode: jaringan tepercaya)",
     "  gdbserver PORT APP  debug APP dari GDB di PC (target remote IP:PORT)",
     "  crash [PID|last]  laporan crash app (backtrace bersimbol)",
     "  dbg run <app>     debugger: break/cont/step/regs/mem/bt ('dbg help')",
     "  deploy on|off     terima app dari Magic App Generator (UDP 7070)",
     "  ssh on|off        server SSH (port 22) untuk akses jarak jauh",
     "  ipconfig          alamat IP, MAC, gateway, statistik jaringan",
     "  ping <ip> [n]     uji koneksi (ICMP echo)",
     "  arp, route, netstat, hostname   tabel ARP, rute, koneksi",
     "  ask <prompt>      AI: lengkapi teks (LLM lokal)",
     "  vm <sub>          virtualisasi (list/create/start/snapshot/restore)",
     "  script <lang> [f] polyglot: js|ts|py (jalankan demo/file)",
     "  csx [FILE] [arg]  C# script (.csx) lewat engine bytecode; UI: run FILE.CSX",
     "  prof [self|on|off|reset|report]  profiler zona (TSC)",
     "  prof app NAMA | pid N | stop | last  profil per aplikasi (sampel timer per fungsi)",
     "  shutdown|restart|sleep      power management",
     "  bz <sub>          CLI (install/remove/list, model, power, vm, virt, script)",
     "  clear|cls         bersihkan layar",
     "  ver|uname         versi OS",
     "  about|credits     tentang & pembuat"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Resolve `arg` (optional) against the cwd into an absolute VFS path, with
/// `.` and `..` folded (never above the mount: `/disk/..` stays `/disk`).
pub fn resolve(arg: Option<&str>) -> String {
    let raw = match arg {
        Some(p) if p.starts_with('/') => p.to_string(),
        Some(p) => format!("{}/{}", cwd().trim_end_matches('/'), p),
        None => cwd(),
    };
    let mut parts: Vec<&str> = Vec::new();
    for c in raw.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                if parts.len() > 1 {
                    parts.pop();
                }
            }
            x => parts.push(x),
        }
    }
    format!("/{}", parts.join("/"))
}

/// `echo text`, `echo text > FILE` (create/overwrite) and `echo text >> FILE`
/// (append). Only echo redirects for now; general pipes/redirection are on the
/// Developer Workstation roadmap.
/// `echo -e`: backslash escapes -- \n \t \\ and \e / \033 (ANSI colours).
fn echo_escapes(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push_str("    "),
            Some('e') => out.push('\u{1b}'),
            Some('0') if it.peek() == Some(&'3') => {
                it.next();
                if it.peek() == Some(&'3') {
                    it.next();
                }
                out.push('\u{1b}');
            }
            Some('\\') => out.push('\\'),
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn echo(args: &[&str]) -> Vec<String> {
    if args.first() == Some(&"-e") {
        return echo_escapes(&args[1..].join(" ")).split('\n').map(String::from).collect();
    }
    let pos = args.iter().position(|a| *a == ">" || *a == ">>");
    let Some(pos) = pos else {
        return vec![args.join(" ")];
    };
    let append = args[pos] == ">>";
    let Some(target) = args.get(pos + 1).copied() else {
        return vec![String::from("echo: butuh nama file setelah >")];
    };
    let path = resolve(Some(target));
    let mut data = if append { vfs::read(&path).unwrap_or_default() } else { Vec::new() };
    data.extend_from_slice(args[..pos].join(" ").as_bytes());
    data.push(b'\n');
    match vfs::write(&path, &data) {
        Ok(()) => vec![format!("{} byte ditulis ke {}", data.len(), path)],
        Err(e) => vec![format!("echo: {}: {}", path, e)],
    }
}

fn rm(arg: Option<&str>) -> Vec<String> {
    let Some(_) = arg else {
        return vec![String::from("rm: butuh nama file")];
    };
    let path = resolve(arg);
    match vfs::remove(&path) {
        Ok(()) => vec![format!("{} dihapus", path)],
        Err(e) => vec![format!("rm: {}: {}", path, e)],
    }
}

fn df() -> Vec<String> {
    let mut out = vec![String::from("MOUNT    BEBAS      MODE")];
    for m in vfs::mounts() {
        match vfs::free_space(&m) {
            Ok((free, ro)) => out.push(format!("{:<8} {:>6} KiB  {}", m, free / 1024, if ro { "ro" } else { "rw" })),
            Err(e) => out.push(format!("{:<8} ?  ({})", m, e)),
        }
    }
    out
}

fn list_dir(arg: Option<&str>) -> Vec<String> {
    let path = resolve(arg);
    // a file operand (e.g. from a glob: `ls /disk/H*.ELF`) lists itself
    if let Some(a) = arg {
        if let Ok(e) = vfs::stat(&path) {
            if !e.is_dir {
                return vec![String::from(a)];
            }
        }
    }
    match vfs::list(&path) {
        Ok(names) if names.is_empty() => vec![format!("{}: (kosong)", path)],
        Ok(names) => names,
        Err(e) => vec![format!("ls: {}: {}", path, e)],
    }
}

fn cat(arg: Option<&str>) -> Vec<String> {
    let Some(_) = arg else {
        return vec![String::from("cat: butuh nama file")];
    };
    let path = resolve(arg);
    match vfs::read(&path) {
        Ok(bytes) => match core::str::from_utf8(&bytes) {
            Ok(text) => text.lines().map(String::from).collect(),
            Err(_) => vec![format!("cat: {}: bukan teks UTF-8 ({} byte)", path, bytes.len())],
        },
        Err(e) => vec![format!("cat: {}: {}", path, e)],
    }
}

fn cd(arg: Option<&str>) -> Vec<String> {
    let path = resolve(arg);
    // Accept a path whose mount exists.
    if vfs::mounts().iter().any(|m| path == *m || path.starts_with(&format!("{}/", m))) {
        STATE.lock().cwd = path;
        Vec::new()
    } else {
        vec![format!("cd: {}: tidak ada", path)]
    }
}

/// `icons [list | NAME | default]`: choose an icon pack from /disk/ICONS.
fn icons_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    match arg {
        None | Some("list") => {
            let cur = crate::icons::pack_name();
            let mut out = vec![format!("{}bawaan (ikon asli)", if cur.is_none() { "* " } else { "  " })];
            for p in crate::icons::packs() {
                out.push(format!("{}{}", if cur.as_deref() == Some(p.as_str()) { "* " } else { "  " }, p.to_ascii_lowercase()));
            }
            (out, Effect::None)
        }
        Some("default") | Some("bawaan") => {
            let _ = crate::icons::set_pack(None);
            (vec![String::from("ikon: bawaan")], Effect::Redraw)
        }
        Some(name) => match crate::icons::set_pack(Some(name)) {
            Ok(()) => (vec![format!("ikon: {}", name)], Effect::Redraw),
            Err(e) => (vec![format!("icons: {}", e)], Effect::None),
        },
    }
}

fn theme_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    match arg {
        None | Some("toggle") => {
            (vec![format!("tema: {}", theme::toggle())], Effect::Redraw)
        }
        Some("cycle") | Some("next") => {
            (vec![format!("tema: {}", theme::cycle())], Effect::Redraw)
        }
        Some("reload") => {
            let n = theme::load_packages();
            (vec![format!("tema: {} paket tema dimuat dari /disk/THEMES", n)], Effect::Redraw)
        }
        Some("list") => {
            let mut out = vec![String::from("tema tersedia:")];
            for t in theme::names().iter() {
                let mark = if t.name == theme::name() { "* " } else { "  " };
                let pkg = if theme::is_package(t.name) { "  (paket /disk/THEMES)" } else { "" };
                out.push(format!("{}{}{}", mark, t.name, pkg));
            }
            (out, Effect::None)
        }
        Some(name) => {
            if theme::set_by_name(name) {
                (vec![format!("tema: {}", name)], Effect::Redraw)
            } else {
                (
                    vec![format!("theme: '{}' tidak dikenal (coba 'theme list')", name)],
                    Effect::None,
                )
            }
        }
    }
}

fn ws_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    match arg {
        None => (vec![format!("workspace: {}", wm::current_workspace() + 1)], Effect::None),
        Some(n) => match n.parse::<u8>() {
            Ok(v) if (1..=wm::WORKSPACES).contains(&v) => {
                wm::switch_workspace(v - 1);
                (vec![format!("pindah ke desktop {}", v)], Effect::Redraw)
            }
            _ => (vec![format!("ws: nomor 1..{}", wm::WORKSPACES)], Effect::None),
        },
    }
}

fn run_cmd(arg: Option<&str>, app_args: &[&str]) -> (Vec<String>, Effect) {
    let Some(name) = arg else {
        return (vec![String::from("run: butuh nama app (mis. 'run xox')")], Effect::None);
    };
    // a UI script by any path: `run MAIN.JS`, `run /disk/PROYEK/X/MAIN.PY`
    let lower = name.to_ascii_lowercase();
    let resolved;
    let name = if lower.ends_with(".js") || lower.ends_with(".ts") || lower.ends_with(".py") || lower.ends_with(".csx") || lower.ends_with(".ggu") || lower.ends_with(".gguf") {
        resolved = resolve(Some(name));
        resolved.as_str()
    } else {
        name
    };
    if !app::is_app(name) {
        return (vec![format!("run: '{}' tidak dikenal (lihat 'bz list')", name)], Effect::None);
    }
    // Apps in the registry must be installed first; others (svc) run directly.
    if pkg::find(name).is_some() && !pkg::is_installed(name) {
        return (
            vec![format!("run: '{}' belum terpasang — jalankan 'bz install {}'", name, name)],
            Effect::None,
        );
    }
    // Start it as its own process: the desktop and shell stay responsive while
    // it runs, and several apps can run side by side.
    match app::spawn_with_args(name, &app_args.join(" ")) {
        Ok(pid) => {
            crate::terminal::spawned(pid); // a job of the terminal tab that ran this
            (vec![format!("app '{}' berjalan (pid {})", name, pid)], Effect::Redraw)
        }
        Err(e) => (vec![format!("run: {}: {}", name, e)], Effect::None),
    }
}

fn cpu_cmd(arg: Option<&str>) -> Vec<String> {
    match arg {
        Some("on") => {
            crate::task::set_ap_scheduling(true);
            vec![String::from("proses ring-3 kini dibagi ke semua CPU")]
        }
        Some("threads-on") => {
            crate::task::set_parallel_threads(true);
            vec![String::from("thread dalam satu proses boleh jalan paralel")]
        }
        Some("threads-off") => {
            crate::task::set_parallel_threads(false);
            vec![String::from("thread dalam satu proses kini bergiliran")]
        }
        Some("off") => {
            crate::task::set_ap_scheduling(false);
            vec![String::from("proses ring-3 kini hanya di CPU #0 (BSP)")]
        }
        _ => crate::smp::report(),
    }
}

fn ps_cmd() -> Vec<String> {
    let mut buf: Vec<bz_abi::ProcInfo> = Vec::new();
    buf.resize_with(64, || unsafe { core::mem::zeroed() });
    let n = crate::process::list(&mut buf);
    let mut out = vec![String::from("  PID  JENIS   CPU  NAMA")];
    for p in &buf[..n] {
        let len = p.name.iter().position(|&b| b == 0).unwrap_or(p.name.len());
        let name = core::str::from_utf8(&p.name[..len]).unwrap_or("?");
        let kind = if p.kind == 1 { "app" } else { "kernel" };
        out.push(format!("{:>5}  {:<6} {:>5}  {}", p.pid, kind, p.cpu_ticks, name));
    }
    out
}

/// Processes as `ps` sees them: (pid, kind, cpu ticks, state, name).
fn processes() -> Vec<(u64, u64, u64, u64, String)> {
    let mut buf: Vec<bz_abi::ProcInfo> = Vec::new();
    buf.resize_with(64, || unsafe { core::mem::zeroed() });
    let n = crate::process::list(&mut buf);
    buf[..n]
        .iter()
        .map(|p| {
            let len = p.name.iter().position(|&b| b == 0).unwrap_or(p.name.len());
            (p.pid, p.kind, p.cpu_ticks, p.state, String::from(core::str::from_utf8(&p.name[..len]).unwrap_or("?")))
        })
        .collect()
}

/// `uptime`: how long the system has run, how many processes, CPUs.
fn uptime_line() -> String {
    let s = crate::arch::cpu::nanos() / 1_000_000_000;
    let procs = processes();
    let apps = procs.iter().filter(|p| p.1 == 1).count();
    format!(
        "menyala {}:{:02}:{:02}, {} task kernel + {} app, {} CPU",
        s / 3600,
        s / 60 % 60,
        s % 60,
        procs.len() - apps,
        apps,
        crate::smp::cpus_online().max(1)
    )
}

/// `free`: physical memory, frames in use, the kernel heap.
fn free_cmd() -> Vec<String> {
    let total_mib = crate::memory::total_usable_mib();
    let frames = crate::memory::frames_in_use();
    let used_mib = frames * 4096 / (1024 * 1024);
    let (heap_used, heap_total) = crate::allocator::stats();
    vec![
        String::from("               total        dipakai      bebas"),
        format!("RAM (MiB)  {:>9}    {:>9}  {:>9}   ({} frame 4 KiB dipakai program + DMA)", total_mib, used_mib, total_mib.saturating_sub(used_mib), frames),
        format!("heap (KiB) {:>9}    {:>9}  {:>9}   (heap kernel)", heap_total / 1024, heap_used / 1024, (heap_total - heap_used) / 1024),
    ]
}

/// `top`: one snapshot -- uptime, memory, and processes by CPU time used.
fn top_cmd() -> Vec<String> {
    let mut out = vec![uptime_line()];
    out.extend(free_cmd().into_iter().skip(1));
    let mut procs = processes();
    let total: u64 = procs.iter().map(|p| p.2).sum::<u64>().max(1);
    procs.sort_by(|a, b| b.2.cmp(&a.2));
    out.push(String::from("  PID  JENIS    CPU%   CPU   NAMA"));
    for (pid, kind, cpu, _state, name) in procs.iter().take(20) {
        out.push(format!(
            "{:>5}  {:<6} {:>4}.{}  {:>5}   {}",
            pid,
            if *kind == 1 { "app" } else { "kernel" },
            cpu * 100 / total,
            cpu * 1000 / total % 10,
            cpu,
            name
        ));
    }
    out.push(String::from("(CPU% = bagian waktu CPU sejak boot; 'kill PID' menghentikan app)"));
    out
}

/// `mixer` (the streams), `mixer PID VOLUME`.
fn mixer_cmd(args: &[&str]) -> Vec<String> {
    if let [pid, vol] = args {
        let (Ok(p), Ok(v)) = (pid.parse::<u64>(), vol.parse::<u32>()) else { return vec![String::from("mixer PID VOLUME (0-100)")] };
        return vec![format!("mixer: pid {} volume {}", p, crate::mixer::set_volume(p, v))];
    }
    let streams = crate::mixer::list();
    let mut out = vec![format!(
        "mixer: {} -- {} stream(s), master {}%",
        if crate::audio::is_streaming() { "playing" } else if crate::audio::can_stream() { "idle" } else { "no streaming sound card" },
        streams.len(),
        crate::audio::volume()
    )];
    for (pid, ms, vol) in streams {
        out.push(format!("  pid {:<6} {:<14} {:>5} ms queued  volume {}%", pid, crate::process::name_of(pid).unwrap_or_else(|| String::from(if pid == 0 { "system" } else { "-" })), ms, vol));
    }
    out
}

fn kill_cmd(arg: Option<&str>) -> Vec<String> {
    // `kill %N`: a job of this terminal tab
    let job = arg.filter(|a| a.starts_with('%')).map(|a| crate::terminal::job_pid(a));
    if let Some(found) = job {
        let Some(pid) = found else { return vec![String::from("kill: tidak ada job seperti itu ('jobs')")] };
        return if crate::process::kill(pid) { vec![format!("job {} (pid {}) dihentikan", arg.unwrap_or(""), pid)] } else { vec![format!("kill: pid {} sudah berakhir", pid)] };
    }
    let Some(pid) = arg.and_then(|a| a.parse::<u64>().ok()) else {
        return vec![String::from("kill: butuh pid (lihat 'ps')")];
    };
    if crate::process::kill(pid) {
        vec![format!("proses {} dihentikan", pid)]
    } else {
        vec![format!("kill: {}: tidak bisa dihentikan (tidak ada, app sinkron, atau task inti)", pid)]
    }
}

fn bg_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    match arg {
        None => (vec![format!("wallpaper: {}", wallpaper::label())], Effect::None),
        Some("list") => {
            let mut out = vec![String::from("wallpaper resmi Buitenzorg OS:")];
            for (n, _, desc) in wallpaper::OFFICIAL {
                out.push(format!("  {:<11} {}", n, desc));
            }
            out.push(String::from("pola bawaan:"));
            for n in wallpaper::BUILTINS {
                out.push(format!("  {}", n));
            }
            out.push(String::from("  /disk/<file>.bmp (gambar user 24-bit)"));
            (out, Effect::None)
        }
        Some(path) if path.starts_with('/') => match vfs::read(path) {
            Ok(bytes) => match wallpaper::load_bmp(&bytes, path) {
                Ok((w, h)) => (vec![format!("wallpaper: {} ({}x{})", path, w, h)], Effect::Redraw),
                Err(e) => (vec![format!("bg: {}: {}", path, e)], Effect::None),
            },
            Err(e) => (vec![format!("bg: {}: {}", path, e)], Effect::None),
        },
        Some(name) if wallpaper::OFFICIAL.iter().any(|(n, _, _)| *n == name) => {
            match wallpaper::set_official(name) {
                Ok(_) => (vec![format!("wallpaper: {} (resmi)", name)], Effect::Redraw),
                Err(e) => (vec![format!("bg: {}: {}", name, e)], Effect::None),
            }
        }
        Some(name) => {
            if wallpaper::set_builtin(name) {
                (vec![format!("wallpaper: {}", name)], Effect::Redraw)
            } else {
                (vec![format!("bg: '{}' tidak dikenal (coba 'bg list')", name)], Effect::None)
            }
        }
    }
}

fn saver_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    match arg {
        None => (vec![format!("screensaver: {}", screensaver::name())], Effect::None),
        Some("preview") | Some("pratinjau") => {
            screensaver::request_preview();
            (vec![format!("screensaver: pratinjau '{}' (gerakkan mouse atau tekan tombol untuk berhenti)", screensaver::name())], Effect::None)
        }
        Some("list") => {
            let mut out = vec![String::from("screensaver:")];
            for n in screensaver::NAMES {
                out.push(format!("  {}", n));
            }
            out.push(String::from("  off"));
            (out, Effect::None)
        }
        Some(name) => {
            if screensaver::set(name) {
                (vec![format!("screensaver: {}", screensaver::name())], Effect::None)
            } else {
                (vec![format!("saver: '{}' tidak dikenal (coba 'saver list')", name)], Effect::None)
            }
        }
    }
}

fn cursor_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    match arg {
        Some("besar") | Some("large") | Some("2") => {
            wm::set_cursor_scale(2);
            (vec![String::from("kursor: besar")], Effect::Redraw)
        }
        Some("normal") | Some("1") => {
            wm::set_cursor_scale(1);
            (vec![String::from("kursor: normal")], Effect::Redraw)
        }
        _ => (vec![String::from("cursor: normal | besar")], Effect::None),
    }
}

fn anim_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    let (_, rounded) = wm::options();
    match arg {
        Some("off") => {
            wm::set_options(false, rounded);
            (vec![String::from("animasi: off")], Effect::Redraw)
        }
        Some("on") => {
            wm::set_options(true, rounded);
            (vec![String::from("animasi: on")], Effect::Redraw)
        }
        _ => (vec![String::from("anim: on | off")], Effect::None),
    }
}

fn settings_show() -> Vec<String> {
    let (anim, rounded) = wm::options();
    vec![
        String::from("== Personalization =="),
        format!("  tema      : {}", theme::name()),
        format!("  wallpaper : {}", wallpaper::label()),
        format!("  saver     : {}", screensaver::name()),
        format!("  animasi   : {}", if anim { "on" } else { "off" }),
        format!("  rounded   : {}", if rounded { "on" } else { "off" }),
        String::from("  ubah: theme/bg/saver/cursor/anim <nilai>"),
    ]
}

/// The local language model `ask` uses (the GGUF model on every image).
pub const ASK_MODEL: &str = "/disk/STORIES.GGU";

/// `ask [--bigram] [--steps N] [--temp T] [--cpu|--gpu] PROMPT`: the local
/// GGUF language model (CLRHOST.ELF llm.rs) continues the prompt. Inside a
/// Terminal tab the answer streams into the tab as a foreground job (Ctrl+C
/// stops it); elsewhere (SHELL_RUN, bz-link, ssh) the shell waits and returns
/// the answer. Without the model on the medium, or with `--bigram`, the old
/// kernel bigram model (`ai.rs`) answers.
fn ai_ask(args: &[&str]) -> (Vec<String>, Effect) {
    let bigram = args.iter().any(|a| *a == "--bigram");
    let rest: Vec<&str> = args.iter().copied().filter(|a| *a != "--bigram").collect();
    let mut words: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--steps" | "--temp" => i += 1,
            "--cpu" | "--gpu" | "--gpu-force" => {}
            w => words.push(w),
        }
        i += 1;
    }
    if words.is_empty() {
        return (vec![String::from("ask: butuh prompt (mis. 'ask Once upon a time', 'ask --bigram kernel')")], Effect::None);
    }
    if bigram || crate::vfs::stat(ASK_MODEL).is_err() {
        let out = ai::llm_complete(&format!("{} ", words.join(" ")), 48);
        let mut lines = vec![String::from("[nalar/LLM lokal - buitenzorg/nalar-bigram]")];
        if !bigram {
            lines.push(format!("({} tidak ada di media ini: model bigram kernel yang menjawab)", ASK_MODEL));
        }
        lines.push(out);
        return (lines, Effect::None);
    }
    let mut llm_args: Vec<&str> = rest.clone();
    if !rest.contains(&"--steps") {
        llm_args.extend(["--steps", "48"]);
    }
    if crate::terminal::in_tab() {
        let (mut lines, fx) = run_cmd(Some(ASK_MODEL), &llm_args);
        lines.insert(0, String::from("[ask] model GGUF lokal (STORIES.GGU); jawaban mengalir di bawah, Ctrl+C menghentikan"));
        return (lines, fx);
    }
    match ask_capture(&llm_args.join(" ")) {
        Ok(lines) => (lines, Effect::None),
        Err(e) => (vec![format!("ask: {}", e)], Effect::None),
    }
}

/// Run the GGUF model with `args` and wait for it: its output without the
/// model-description line. `ask` outside a Terminal tab, and the boot check.
pub fn ask_capture(args: &str) -> Result<Vec<String>, String> {
    let pid = app::spawn_with_args(ASK_MODEL, args).map_err(String::from)?;
    let code = app::wait(pid);
    let text = crate::applog::text_of(pid).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().filter(|l| !l.starts_with("[llm] /")).map(String::from).collect();
    if code != 0 {
        lines.push(format!("ask: model selesai dengan kode {:#x}", code));
    }
    Ok(lines)
}

fn model_cmd(args: &[&str]) -> (Vec<String>, Effect) {
    match args.first().copied() {
        None | Some("list") => {
            let mut out = vec![String::from("galeri model (Hugging Face-style):")];
            for m in model::GALLERY {
                let mark = if model::is_pulled(m.id) { "[v]" } else { "[ ]" };
                out.push(format!(
                    "{} {:<28} {:<15} {}MB {}",
                    mark, m.id, m.task, m.size_mb, m.format
                ));
            }
            (out, Effect::None)
        }
        Some("pull") | Some("download") => {
            let Some(id) = args.get(1) else {
                return (vec![String::from("bz model pull <id>")], Effect::None);
            };
            match model::pull(id) {
                Ok(m) => (
                    vec![format!("model diunduh: {} ({} MB, {})", m.id, m.size_mb, m.license)],
                    Effect::None,
                ),
                Err(e) => (vec![format!("bz model: {}: {}", id, e)], Effect::None),
            }
        }
        Some("info") => {
            let Some(id) = args.get(1) else {
                return (vec![String::from("bz model info <id>")], Effect::None);
            };
            match model::find(id) {
                Some(m) => (
                    vec![
                        format!("id      : {}", m.id),
                        format!("task    : {}", m.task),
                        format!("size    : {} MB (VRAM ~{} MB)", m.size_mb, m.vram_mb),
                        format!("license : {}   format: {}", m.license, m.format),
                        format!("status  : {}", if model::is_pulled(m.id) { "tersedia offline" } else { "belum diunduh" }),
                    ],
                    Effect::None,
                ),
                None => (vec![format!("bz model: '{}' tidak ada di galeri", id)], Effect::None),
            }
        }
        Some(sub) => (vec![format!("bz model {}: list|pull|info", sub)], Effect::None),
    }
}

fn power_cmd(args: &[&str]) -> (Vec<String>, Effect) {
    match args.first().copied() {
        Some("off") | Some("shutdown") => power::shutdown(),
        Some("restart") | Some("reboot") => power::restart(),
        Some("sleep") => {
            power::sleep();
            (vec![String::from("bangun dari sleep")], Effect::Redraw)
        }
        _ => {
            let (acpi, pm1a, slp, reset) = power::summary();
            (
                vec![
                    format!("power: acpi={} pm1a_cnt={:#x} slp_typ_s5={} reset_port={:#x}", acpi, pm1a, slp, reset),
                    String::from("bz power off | restart | sleep"),
                ],
                Effect::None,
            )
        }
    }
}

fn vm_cmd(args: &[&str]) -> (Vec<String>, Effect) {
    match args.first().copied() {
        None | Some("list") | Some("ls") => {
            let vms = vmm::list();
            if vms.is_empty() {
                return (vec![String::from("belum ada VM (buat: 'vm create <nama>')")], Effect::None);
            }
            let mut out = vec![String::from("virtual machines:")];
            for v in vms {
                out.push(format!(
                    "  #{} {:<10} {:>4}KiB {}vcpu disk={}KiB {:<8} steps={}{}",
                    v.id, v.name, v.mem_kib, v.vcpus, v.disk_kib, v.state.name(), v.steps,
                    if v.has_snapshot { " [snap]" } else { "" }
                ));
            }
            (out, Effect::None)
        }
        Some("create") | Some("new") => {
            let name = args.get(1).copied().unwrap_or("nanovm");
            let id = vmm::create(name, 64, 1);
            (vec![format!("VM dibuat: #{} '{}' (guest: NanoOS, 64KiB, 1 vcpu)", id, name)], Effect::None)
        }
        Some("start") | Some("run") | Some("boot") => {
            let Some(sel) = args.get(1).copied() else {
                return (vec![String::from("vm start <nama|id>")], Effect::None);
            };
            match vmm::start(sel) {
                Ok(r) => {
                    let mut out = vec![format!("== VM '{}' -> {} ({} instruksi ==", r.name, r.state.name(), r.steps)];
                    for l in r.console.lines() {
                        out.push(format!("  | {}", l));
                    }
                    (out, Effect::Redraw)
                }
                Err(e) => (vec![format!("vm start: {}", e)], Effect::None),
            }
        }
        Some("snapshot") | Some("snap") => {
            let Some(sel) = args.get(1).copied() else {
                return (vec![String::from("vm snapshot <nama|id>")], Effect::None);
            };
            match vmm::snapshot(sel) {
                Ok(()) => (vec![format!("snapshot dibuat untuk '{}'", sel)], Effect::None),
                Err(e) => (vec![format!("vm snapshot: {}", e)], Effect::None),
            }
        }
        Some("restore") => {
            let Some(sel) = args.get(1).copied() else {
                return (vec![String::from("vm restore <nama|id>")], Effect::None);
            };
            match vmm::restore(sel) {
                Ok(()) => (vec![format!("'{}' dipulihkan dari snapshot", sel)], Effect::None),
                Err(e) => (vec![format!("vm restore: {}", e)], Effect::None),
            }
        }
        Some("remove") | Some("rm") | Some("delete") => {
            let Some(sel) = args.get(1).copied() else {
                return (vec![String::from("vm remove <nama|id>")], Effect::None);
            };
            match vmm::remove(sel) {
                Ok(()) => (vec![format!("VM '{}' dihapus", sel)], Effect::None),
                Err(e) => (vec![format!("vm remove: {}", e)], Effect::None),
            }
        }
        Some(sub) => (
            vec![format!("vm {}: list|create|start|snapshot|restore|remove", sub)],
            Effect::None,
        ),
    }
}

fn lang_of(s: &str) -> Option<script::Lang> {
    match s {
        "js" | "javascript" => Some(script::Lang::Js),
        "ts" | "typescript" => Some(script::Lang::Ts),
        "py" | "python" => Some(script::Lang::Python),
        _ => None,
    }
}

/// The script debugger's run: the output as `run_script` prints it, then one
/// record per stop for Buitenzorg Studio:
/// `@dbg LINE<TAB>OUTPUT_LINES<TAB>STACK<TAB>LOCALS<TAB>GLOBALS`, the stack as
/// `fn:line` innermost first separated by `,`, variables as `name=value`
/// separated by U+001F.
fn debug_script(lang: script::Lang, src: &str, breaks: Vec<u32>, every: bool) -> Vec<String> {
    let out = script::run_debug(lang, src, breaks, every, 400);
    let snaps = out.snaps.iter().map(|s| (s.line, s.out_lines, &s.stack, &s.locals, &s.globals));
    debug_lines(lang.name(), &out.lines, &out.error, out.steps, out.snaps.len(), snaps)
}

/// The output of a debugged run, then the `@dbg` records (see debug_script).
fn debug_lines<'a>(
    lang: &str,
    out: &[String],
    error: &Option<String>,
    steps: u64,
    count: usize,
    snaps: impl Iterator<Item = (u32, usize, &'a Vec<(String, u32)>, &'a script::VarList, &'a script::VarList)>,
) -> Vec<String> {
    script::debug_report(lang, out, error, steps, count, snaps)
}

/// UI scripts debugged so far (names their report files).
static UI_DEBUGS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// `script debug` on a UI script: start it as an app under the debugger
/// (CLRHOST.ELF `--bzdebug`); it records stops while it runs in its window
/// and writes the report to a file when it ends. The caller polls
/// `script debug status PID FILE` -- never wait here: the shell may run on
/// the desktop loop, which the app needs for its input.
fn debug_ui_script(path: &str, spec: &str) -> Vec<String> {
    let n = UI_DEBUGS.fetch_add(1, core::sync::atomic::Ordering::Relaxed) % 1000;
    let dir = if vfs::list("/ram").is_ok() { "/ram" } else { "/disk" };
    let out = format!("{}/UIDBG{}.TXT", dir, n);
    let _ = vfs::remove(&out);
    match crate::app::spawn_with_args(path, &format!("--bzdebug {} {}", spec, out)) {
        Ok(pid) => vec![
            format!("@dbgui {} {}", pid, out),
            format!("aplikasi UI pid {} berjalan di jendelanya; henti direkam sampai ia berakhir ('script debug status {} {}')", pid, pid, out),
        ],
        Err(e) => vec![format!("script: {}: {}", path, e)],
    }
}

/// `script debug status PID FILE`: `@dbgui running`, or the finished report.
fn debug_ui_status(pid: &str, file: &str) -> Vec<String> {
    let Ok(pid) = pid.parse::<u64>() else { return vec![String::from("script debug status PID FILE")] };
    if crate::process::is_alive(pid) {
        return vec![format!("@dbgui running {}", pid)];
    }
    match vfs::read(file) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).lines().map(String::from).collect(),
        Err(_) => vec![format!(
            "script: pid {} berakhir tanpa rekaman (kode keluar {:#x})",
            pid,
            crate::process::exit_code(pid).unwrap_or(0)
        )],
    }
}

fn run_script(lang: script::Lang, src: &str) -> Vec<String> {
    let out = script::run(lang, src);
    let mut lines = vec![format!("[{}] ({} langkah)", lang.name(), out.steps)];
    for l in &out.lines {
        lines.push(format!("  {}", l));
    }
    if let Some(e) = out.error {
        // a UI script (window/button/...) needs its own process and window
        let ui = ["window", "button", "present", "running", "clear", "rect", "text"]
            .iter()
            .any(|f| e.contains(&format!("fungsi '{}' tak dikenal", f)));
        lines.push(format!("  error: {}", e));
        if ui {
            lines.push(String::from("  (skrip UI: jalankan sebagai aplikasi dengan 'run FILE', mis. 'run MAIN.JS')"));
        }
    }
    lines
}

/// `prof [on|off|reset|report|self]` — drive the instrumented profiler.
/// With no argument, run a quick self-profile (compose a desktop frame under
/// profiling) and show where the cycles went. Profiling is off by default so it
/// never perturbs normal timing.
fn prof_cmd(arg: Option<&str>) -> (Vec<String>, Effect) {
    match arg {
        Some("on") => {
            crate::profile::enable();
            (vec![String::from("profiler: on")], Effect::None)
        }
        Some("off") => {
            crate::profile::disable();
            (vec![String::from("profiler: off")], Effect::None)
        }
        Some("hw") | Some("sw") => {
            let hw = arg == Some("hw");
            crate::wm::set_hw_cursor(hw);
            (vec![format!("cursor: {}", if hw { "hardware where an output has one" } else { "software everywhere" })], Effect::None)
        }
        Some("seq") | Some("par") => {
            let par = arg == Some("par");
            crate::wm::set_parallel_compose(par);
            (vec![format!("compose: {}", if par { "two bands in parallel" } else { "one CPU" })], Effect::None)
        }
        Some("reset") => {
            crate::profile::reset();
            (vec![String::from("profiler: cleared")], Effect::None)
        }
        Some("report") => {
            crate::profile::report();
            (
                vec![format!(
                    "profiler: {} zone(s) — full report on the serial log",
                    crate::profile::zone_count()
                )],
                Effect::None,
            )
        }
        None | Some("self") => {
            // Profile a real, representative workload: recompose the desktop a
            // few times (the WIN_PRESENT hot path) and report.
            let was = crate::profile::is_enabled();
            crate::profile::reset();
            crate::profile::enable();
            for _ in 0..8 {
                let _z = crate::profile::Guard::new("shell:present");
                crate::wm::present_now();
            }
            if !was {
                crate::profile::disable();
            }
            crate::profile::report();
            let (wr, sk) = crate::framebuffer::present_stats();
            crate::println!("[profile] present tiles since boot: {} written, {} unchanged and skipped", wr, sk);
            let (hits, misses) = crate::wm::layer_stats();
            crate::println!("[profile] shell layers (icons, taskbar) since boot: {} reused, {} redrawn", hits, misses);
            crate::println!("[profile] cursor-only frames since boot: {}", crate::wm::cursor_only_frames());
            crate::println!("[profile] frames composed in two bands (parallel): {}", crate::wm::parallel_frames());
            let (gpu_hw, fb_hw) = crate::wm::hw_cursor_outputs();
            let (moves, bands) = crate::virtio::gpu::stats();
            crate::println!("[profile] hardware cursor: virtio-gpu {}, board framebuffer {}; gpu cursor moves {}, gpu damage presents {}", gpu_hw, fb_hw, moves, bands);
            let (df, dr) = crate::wm::damage_stats();
            crate::println!("[profile] damage-presented frames since boot: {} ({} rows in all)", df, dr);
            (
                vec![
                    format!("profiler: sampled {} zone(s)", crate::profile::zone_count()),
                    String::from("full report on the serial log; `prof on|off|reset|report`"),
                ],
                Effect::None,
            )
        }
        Some(other) => (
            vec![format!("prof: unknown option '{}' (on|off|reset|report|self)", other)],
            Effect::None,
        ),
    }
}

/// `csx [FILE] [args]`: a C# script through the bytecode engine (`csx.rs`).
fn csx_cmd(args: &[&str]) -> (Vec<String>, Effect) {
    let (src, label) = match args.first() {
        None => (String::from(crate::csx::DEMO), String::from("demo")),
        Some(path) => match vfs::read(&resolve(Some(path))) {
            Ok(bytes) => (String::from_utf8_lossy(&bytes).into_owned(), String::from(*path)),
            Err(e) => return (vec![format!("csx: {}: {}", path, e)], Effect::None),
        },
    };
    let script_args: Vec<String> = args.iter().skip(1).map(|a| String::from(*a)).collect();
    let t0 = crate::arch::cpu::nanos();
    let out = crate::csx::run(&src, &script_args);
    let ms = (crate::arch::cpu::nanos().saturating_sub(t0)) / 1_000_000;
    let mut lines = vec![format!("[C#] {} ({} instruksi, {} ms)", label, out.steps, ms)];
    for l in &out.lines {
        lines.push(format!("  {}", l));
    }
    if let Some(e) = out.error {
        let ui = e.contains("'Ui.");
        lines.push(format!("  error: {}", e));
        if ui {
            lines.push(String::from("  (skrip UI: jalankan sebagai aplikasi dengan 'run FILE', mis. 'run MAIN.CSX')"));
        }
    }
    (lines, Effect::None)
}

fn script_cmd(cmd: &str, args: &[&str]) -> (Vec<String>, Effect) {
    // `script cs FILE` / `script csx FILE`: C# script
    if cmd == "script" && matches!(args.first().copied(), Some("cs") | Some("csx") | Some("csharp")) {
        return csx_cmd(&args[1..]);
    }
    // Direct form: `js|ts|py [path]`
    if let Some(lang) = lang_of(cmd) {
        return match args.first().copied() {
            None => (run_script(lang, script::demo_source(lang)), Effect::Redraw),
            Some(path) => match vfs::read(&resolve(Some(path))) {
                Ok(bytes) => match core::str::from_utf8(&bytes) {
                    Ok(text) => (run_script(lang, text), Effect::Redraw),
                    Err(_) => (vec![format!("script: {}: bukan teks UTF-8", path)], Effect::None),
                },
                Err(e) => (vec![format!("script: {}: {}", path, e)], Effect::None),
            },
        };
    }
    // `script <sub>`
    match args.first().copied() {
        None | Some("list") | Some("help") => (
            vec![
                String::from("runtime polyglot (v0.14 'Babel') -- interpreter bersama:"),
                String::from("  js | javascript   JavaScript"),
                String::from("  ts | typescript   TypeScript (transpile: strip tipe -> JS)"),
                String::from("  py | python       Python (subset, indentasi)"),
                String::from("pakai: script <lang> [file]   |   js|ts|py [file]"),
                String::from("  tanpa file -> jalankan demo bawaan (fib) untuk bahasa itu"),
            ],
            Effect::None,
        ),
        Some(l) if lang_of(l).is_some() => {
            let lang = lang_of(l).unwrap();
            match args.get(1).copied() {
                None => (run_script(lang, script::demo_source(lang)), Effect::Redraw),
                Some(path) => match vfs::read(&resolve(Some(path))) {
                    Ok(bytes) => match core::str::from_utf8(&bytes) {
                        Ok(text) => (run_script(lang, text), Effect::Redraw),
                        Err(_) => (vec![format!("script: {}: bukan teks UTF-8", path)], Effect::None),
                    },
                    Err(e) => (vec![format!("script: {}: {}", path, e)], Effect::None),
                },
            }
        }
        Some("run") => {
            let Some(l) = args.get(1).copied().and_then(lang_of) else {
                return (vec![String::from("script run <js|ts|py> <file>")], Effect::None);
            };
            let Some(path) = args.get(2).copied() else {
                return (vec![String::from("script run <js|ts|py> <file>")], Effect::None);
            };
            match vfs::read(&resolve(Some(path))) {
                Ok(bytes) => match core::str::from_utf8(&bytes) {
                    Ok(text) => (run_script(l, text), Effect::Redraw),
                    Err(_) => (vec![format!("script: {}: bukan teks UTF-8", path)], Effect::None),
                },
                Err(e) => (vec![format!("script: {}: {}", path, e)], Effect::None),
            }
        }
        Some("debug") => {
            // script debug <js|ts|py|csx> <file> <line,line,...|all>: run under
            // the script debugger; after the output, one `@dbg` record per stop.
            // A UI script runs as an app instead (`@dbgui PID FILE`, then
            // `script debug status PID FILE` until the report is there).
            if let (Some("status"), Some(pid), Some(file)) = (args.get(1).copied(), args.get(2).copied(), args.get(3).copied()) {
                return (debug_ui_status(pid, &resolve(Some(file))), Effect::None);
            }
            let csx = matches!(args.get(1).copied(), Some("cs") | Some("csx"));
            let lang = args.get(1).copied().and_then(lang_of);
            let (true, Some(path)) = (csx || lang.is_some(), args.get(2).copied()) else {
                return (vec![String::from("script debug <js|ts|py|csx> <file> <baris,baris,...|all>")], Effect::None);
            };
            let spec = args.get(3).copied().unwrap_or("all");
            let every = spec == "all";
            let breaks: Vec<u32> = spec.split(',').filter_map(|n| n.trim().parse().ok()).collect();
            match vfs::read(&resolve(Some(path))) {
                Ok(bytes) => match core::str::from_utf8(&bytes) {
                    Ok(text) if text.contains("window(") || text.contains("Ui.Window(") => {
                        (debug_ui_script(&resolve(Some(path)), spec), Effect::None)
                    }
                    Ok(text) if csx => {
                        let out = crate::csx::run_debug(text, breaks, every, 400);
                        let snaps = out.snaps.iter().map(|s| (s.line, s.out_lines, &s.stack, &s.locals, &s.globals));
                        (debug_lines("cs", &out.lines, &out.error, out.steps, out.snaps.len(), snaps), Effect::None)
                    }
                    Ok(text) => (debug_script(lang.unwrap(), text, breaks, every), Effect::None),
                    Err(_) => (vec![format!("script: {}: bukan teks UTF-8", path)], Effect::None),
                },
                Err(e) => (vec![format!("script: {}: {}", path, e)], Effect::None),
            }
        }
        Some(sub) => (vec![format!("script {}: list|js|ts|py|run|debug", sub)], Effect::None),
    }
}

fn bz_cmd(args: &[&str]) -> (Vec<String>, Effect) {
    match args.first().copied() {
        Some("theme") => theme_cmd(args.get(1).copied()),
        Some("ws") => ws_cmd(args.get(1).copied()),
        Some("version") | None => (vec![format!("bz {} - Buitenzorg CLI", VERSION)], Effect::None),
        Some("list") | Some("search") => {
            let mut out = pkg_list(args.get(1).copied());
            out.extend(crate::repo::command(&["search", args.get(1).copied().unwrap_or("")]));
            (out, Effect::None)
        }
        Some("repo") | Some("update") | Some("upgrade") => (crate::repo::command(args), Effect::None),
        Some("buildsrv") => (crate::rbuild::command(args), Effect::None),
        Some("trust") => (crate::bzpkg::trust_command(&args[1..]), Effect::None),
        Some("perm") | Some("izin") => (crate::bzpkg::perm_command(&args[1..]), Effect::None),
        Some("rollback") => (
            match args.get(1) {
                Some(n) => match crate::bzpkg::rollback(n) {
                    Ok((from, to)) => vec![format!("dikembalikan: {} {} -> {} ('bz rollback {}' lagi = maju kembali)", n, from, to, n)],
                    Err(e) => vec![format!("bz rollback: {}", e)],
                },
                None => vec![String::from("bz rollback NAMA   (pasang lagi versi sebelumnya)")],
            },
            Effect::None,
        ),
        Some("build") => {
            // bz build DIR [--run]: the project folder, resolved like any path
            let dir = resolve(args.get(1).copied().filter(|a| !a.starts_with("--")));
            let mut a: Vec<&str> = vec!["build", dir.as_str()];
            a.extend(args.iter().skip(1).filter(|x| x.starts_with("--")));
            (crate::rbuild::command(&a), Effect::Redraw)
        }
        Some("install") | Some("add") => (pkg_install(args.get(1).copied()), Effect::None),
        Some("remove") | Some("uninstall") => (pkg_remove(args.get(1).copied()), Effect::None),
        Some("info") => (pkg_info(args.get(1).copied()), Effect::None),
        Some("run") => run_cmd(args.get(1).copied(), &args[args.len().min(2)..]),
        Some("model") => model_cmd(&args[1..]),
        Some("power") => power_cmd(&args[1..]),
        Some("vm") => vm_cmd(&args[1..]),
        Some("virt") => (vmx::summary(), Effect::None),
        Some("script") => script_cmd("script", &args[1..]),
        Some(sub) => (
            vec![format!("bz {}: belum tersedia (lihat roadmap requirements.md §16)", sub)],
            Effect::None,
        ),
    }
}

fn pkg_list(filter: Option<&str>) -> Vec<String> {
    let mut out = vec![String::from("registry paket:")];
    for p in pkg::REGISTRY {
        if let Some(f) = filter {
            if !p.name.contains(f) && !p.description.contains(f) {
                continue;
            }
        }
        let mark = if pkg::is_installed(p.name) { "[x]" } else { "[ ]" };
        out.push(format!("{} {:<10} {:<7} {} ({})", mark, p.name, p.version, p.description, p.kind));
    }
    let files = crate::bzpkg::installed();
    if !files.is_empty() {
        out.push(String::from("terpasang dari berkas .bzpkg:"));
        for p in files {
            if filter.is_some_and(|f| !p.name.contains(f) && !p.title.contains(f)) {
                continue;
            }
            out.push(format!("[x] {:<10} {:<7} {} ({} berkas, entry {})", p.name, p.version, p.title, p.files.len(), p.entry));
        }
    }
    out
}

/// A package-file argument: a path, or a name with a .BZP/.BZPKG extension.
fn is_package_file(arg: &str) -> bool {
    let u = arg.to_ascii_uppercase();
    arg.contains('/') || u.ends_with(".BZP") || u.ends_with(".BZPKG")
}

fn pkg_info(arg: Option<&str>) -> Vec<String> {
    match arg {
        Some(a) if is_package_file(a) => match crate::bzpkg::info_file(&resolve(Some(a))) {
            Ok(lines) => lines,
            Err(e) => vec![format!("bz info: {}", e)],
        },
        Some(a) => match crate::bzpkg::find(a) {
            Some(p) => vec![
                format!("{} {} - {}", p.name, p.version, p.title),
                format!("  entry {}, berkas: {}", p.entry, p.files.join(" ")),
            ],
            None => match pkg::find(a) {
                Some(p) => vec![format!("{} {} - {} (paket bawaan, {})", p.name, p.version, p.description, if pkg::is_installed(a) { "terpasang" } else { "belum terpasang" })],
                None => vec![format!("bz info: {}: tidak ada", a)],
            },
        },
        None => vec![String::from("pakai: bz info NAMA | bz info ./PAKET.BZP")],
    }
}

fn pkg_install(name: Option<&str>) -> Vec<String> {
    let Some(name) = name else {
        return vec![String::from("bz install: butuh nama paket (lihat 'bz list') atau berkas ./PAKET.BZP")];
    };
    if is_package_file(name) {
        return match crate::bzpkg::install_file(&resolve(Some(name))) {
            Ok((n, v, Some(old))) => vec![format!("diperbarui: {} {} -> {}", n, old, v), format!("  {}", crate::bzpkg::last_signer()), perm_line(&n)],
            Ok((n, v, None)) => vec![format!("terpasang: {} v{} (dari berkas; jalankan '{}')", n, v, n), format!("  {}", crate::bzpkg::last_signer()), perm_line(&n)],
            Err(e) => vec![format!("bz install: {}", e)],
        };
    }
    // not a built-in package: the remote repository (with its dependencies)
    if pkg::find(name).is_none() && crate::repo::knows(name) {
        return crate::repo::command(&["install", name]);
    }
    match pkg::install(name) {
        Ok(ver) => vec![format!("terpasang: {} v{}", name, ver)],
        Err(e) => vec![format!("bz install: {}: {}", name, e)],
    }
}

fn pkg_remove(name: Option<&str>) -> Vec<String> {
    let Some(name) = name else {
        return vec![String::from("bz remove: butuh nama paket")];
    };
    if crate::bzpkg::find(name).is_some() {
        return match crate::bzpkg::remove(name) {
            Ok(n) => vec![format!("dihapus: {} ({} berkas)", name, n)],
            Err(e) => vec![format!("bz remove: {}", e)],
        };
    }
    match pkg::remove(name) {
        Ok(()) => vec![format!("dihapus: {}", name)],
        Err(e) => vec![format!("bz remove: {}: {}", name, e)],
    }
}

/// Shell language self-test (self-test boot, every kernel): `$((...))`,
/// `$(...)` with word splitting, functions with arguments, recursion through
/// substitution, `return`, an arithmetic error, `free` / `uptime` / `top`.
pub fn script_self_test() -> bool {
    let cases: [(&str, &str); 9] = [
        ("echo $((2+3*4))", "14"),
        ("echo $(( (7 - 1) / 4 % 5 ))", "1"),
        ("BZT_X=5; echo $((BZT_X * BZT_X + $BZT_X)) $((BZT_X > 3 && 2 != 2))", "30 0"),
        ("echo hasil:$(echo dalam)", "hasil:dalam"),
        ("bzt_kuadrat() { echo $(( $1 * $1 )); }; bzt_kuadrat 7", "49"),
        ("bzt_fakt() { if [ $1 -le 1 ]; then echo 1; return 0; fi; echo $(( $1 * $(bzt_fakt $(( $1 - 1 ))) )); }; bzt_fakt 5", "120"),
        ("bzt_salam() { echo halo $1 dari $0 ($# argumen); }; bzt_salam dunia", "halo dunia dari bzt_salam (1 argumen)"),
        ("echo $(echo a; echo b) | wc -w", "2"),
        ("for w in $(echo x y z); do echo -$w-; done", "-x-|-y-|-z-"),
    ];
    let mut failed = 0;
    for (line, want) in cases {
        let got = if line.starts_with("for ") {
            crate::bzsh::run_text(line, vec![String::from("sh")]).0.join("|")
        } else {
            run(line).0.join("|")
        };
        let got = String::from(got.trim());
        if got != want {
            failed += 1;
            crate::println!("[shell] self-test FAILED: {} -> '{}' (want '{}')", line, got, want);
        }
    }
    // script constructs: case, local, here-documents
    let scripts: [(&str, &str); 6] = [
        ("BZT_X=apel\ncase $BZT_X in\n  jeruk|apel) echo buah;;\n  *) echo lain;;\nesac", "buah"),
        ("case zz in a*) echo a;; 'z?') echo kutip;; z?) echo pola;; *) echo default;; esac", "pola"),
        ("BZT_X=luar\nbzt_lok() {\n  local BZT_X=dalam BZT_BARU=1\n  echo $BZT_X $BZT_BARU\n}\nbzt_lok\necho $BZT_X [$BZT_BARU]", "dalam 1|luar []"),
        ("BZT_X=dunia\ncat <<EOF\nhalo $BZT_X\n  baris \"dua\"\nEOF\necho sesudah", "halo dunia|  baris \"dua\"|sesudah"),
        ("cat <<'X'; echo dan\n$BZT_X tetap\nX", "$BZT_X tetap|dan"),
        ("grep b <<-E | wc -l\n\ta\n\tb1\n\tb2\n\tE", "2"),
    ];
    for (text, want) in scripts {
        let got = crate::bzsh::run_text(text, vec![String::from("sh")]).0.join("|");
        if got.trim() != want {
            failed += 1;
            crate::println!("[shell] self-test FAILED: {:?} -> '{}' (want '{}')", text, got.trim(), want);
        }
    }
    crate::bzsh::remove_function("bzt_lok");
    // an arithmetic error is reported and fails the command
    let (err, _) = run("echo $((1/0))");
    let err_ok = err.first().is_some_and(|l| l.contains("pembagian dengan nol")) && !last_ok();
    if !err_ok {
        failed += 1;
        crate::println!("[shell] self-test FAILED: $((1/0)) -> {:?}", err);
    }
    let sys_ok = run("uptime").0.first().is_some_and(|l| l.starts_with("menyala"))
        && run("free").0.len() == 3
        && run("top").0.iter().any(|l| l.contains("PID"));
    if !sys_ok {
        failed += 1;
        crate::println!("[shell] self-test FAILED: free / uptime / top");
    }
    // echo -e: escapes, an ANSI colour code survives for the terminal to draw
    if run("echo -e 'a\\tb\\e[31mc'").0.join("|") != "a    b\u{1b}[31mc" {
        failed += 1;
        crate::println!("[shell] self-test FAILED: echo -e");
    }
    // the clipboard through a pipe and back (the user's clipboard is restored)
    let saved = crate::clipboard::get();
    let _ = run("echo baris-klip | clip");
    let clip_ok = run("paste").0.join("|") == "baris-klip" && run("echo [$(paste)]").0.join("|") == "[baris-klip]";
    crate::clipboard::set(&saved);
    if !clip_ok {
        failed += 1;
        crate::println!("[shell] self-test FAILED: clip / paste");
    }
    for f in ["bzt_kuadrat", "bzt_fakt", "bzt_salam"] {
        crate::bzsh::remove_function(f);
    }
    let _ = run("unset BZT_X");
    if failed == 0 {
        crate::println!("[kernel] MILESTONE: SHELL SCRIPT OK ($((arithmetic)), $(command) substitution + word splitting, functions + recursion + return, case, local, here-documents, clip/paste, echo -e + ANSI colours, free/uptime/top)");
    }
    failed == 0
}
