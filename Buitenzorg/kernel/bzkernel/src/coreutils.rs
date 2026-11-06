//! Core utilities for the shell's pipelines (Developer Workstation phase 1).
//!
//! Each utility takes its arguments and, inside a pipeline, the previous
//! command's output as `stdin` (lines), and returns its output lines plus a
//! success flag (for `&&`, `||` and `$?`). Files go through the VFS; paths
//! are resolved against the shell's working directory (`shell::resolve`).
//! Text is handled as lines of UTF-8; binary files are refused except by
//! `hexdump`, `cp` and `mv`.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::shell;
use crate::vfs;

/// What a utility returns: output lines and whether it succeeded.
pub type Outcome = (Vec<String>, bool);

/// The utilities this module provides (the shell asks before running one).
pub const NAMES: &[&str] = &[
    "cat", "grep", "head", "tail", "wc", "sort", "uniq", "tee", "cp", "mv", "mkdir", "rmdir", "touch", "find",
    "hexdump", "xxd", "seq", "basename", "dirname", "true", "false", "env", "set", "export", "unset", "history",
];

pub fn handles(name: &str) -> bool {
    NAMES.contains(&name)
}

fn fail(msg: String) -> Outcome {
    (vec![msg], false)
}

/// `-x` style flags first, then the rest; flags may take a value (`-n 5`, `-n5`).
struct Args<'a> {
    flags: BTreeMap<char, Option<&'a str>>,
    rest: Vec<&'a str>,
}

fn parse<'a>(argv: &'a [String], with_value: &[char]) -> Args<'a> {
    let mut flags = BTreeMap::new();
    let mut rest = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let a = argv[i].as_str();
        if a.len() > 1 && a.starts_with('-') && rest.is_empty() && a != "--" && a.parse::<i64>().is_err() {
            let mut chars = a[1..].char_indices();
            while let Some((k, c)) = chars.next() {
                if with_value.contains(&c) {
                    let tail = &a[1 + k + c.len_utf8()..];
                    if !tail.is_empty() {
                        flags.insert(c, Some(tail));
                    } else {
                        i += 1;
                        flags.insert(c, argv.get(i).map(|s| s.as_str()));
                    }
                    break;
                }
                flags.insert(c, None);
            }
        } else if a == "--" && rest.is_empty() {
            rest.extend(argv[i + 1..].iter().map(|s| s.as_str()));
            break;
        } else {
            rest.push(a);
        }
        i += 1;
    }
    Args { flags, rest }
}

/// The lines of a text file.
fn read_lines(name: &str) -> Result<Vec<String>, String> {
    let path = shell::resolve(Some(name));
    let bytes = vfs::read(&path).map_err(|e| format!("{}: {}", path, e))?;
    let text = core::str::from_utf8(&bytes).map_err(|_| format!("{}: bukan teks UTF-8 ({} byte)", path, bytes.len()))?;
    Ok(text.lines().map(String::from).collect())
}

/// Input of a filter: the named files, else the pipeline's stdin.
fn input(cmd: &str, files: &[&str], stdin: Option<&[String]>) -> Result<Vec<String>, Outcome> {
    if files.is_empty() {
        return match stdin {
            Some(lines) => Ok(lines.to_vec()),
            None => Err(fail(format!("{}: butuh nama file atau input dari pipe (|)", cmd))),
        };
    }
    let mut out = Vec::new();
    for f in files {
        match read_lines(f) {
            Ok(mut l) => out.append(&mut l),
            Err(e) => return Err(fail(format!("{}: {}", cmd, e))),
        }
    }
    Ok(out)
}

fn count(flag: Option<&Option<&str>>, default: usize) -> usize {
    flag.and_then(|v| *v).and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// `*` and `?` against a name, case-insensitively (FAT names are upper case).
pub fn wildcard(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], n) || (!n.is_empty() && go(p, &n[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &n[1..]),
            (Some(a), Some(b)) => a.to_ascii_lowercase() == b.to_ascii_lowercase() && go(&p[1..], &n[1..]),
            _ => false,
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    go(&p, &n)
}

/// Run utility `argv[0]`.
pub fn run(argv: &[String], stdin: Option<&[String]>) -> Outcome {
    let name = argv[0].as_str();
    let args = &argv[1..];
    match name {
        "cat" => {
            let a = parse(args, &[]);
            match input("cat", &a.rest, stdin) {
                Ok(lines) => {
                    if a.flags.contains_key(&'n') {
                        (lines.iter().enumerate().map(|(i, l)| format!("{:>5}  {}", i + 1, l)).collect(), true)
                    } else {
                        (lines, true)
                    }
                }
                Err(o) => o,
            }
        }
        "grep" => {
            let a = parse(args, &[]);
            let Some((pat, files)) = a.rest.split_first() else {
                return fail(String::from("pakai: grep [-i] [-v] [-n] [-c] POLA [FILE...]"));
            };
            let lines = match input("grep", files, stdin) {
                Ok(l) => l,
                Err(o) => return o,
            };
            let icase = a.flags.contains_key(&'i');
            let invert = a.flags.contains_key(&'v');
            let pat_l = pat.to_lowercase();
            let hit = |l: &str| {
                let m = if icase { l.to_lowercase().contains(&pat_l) } else { l.contains(pat) };
                m != invert
            };
            let matched: Vec<(usize, &String)> = lines.iter().enumerate().filter(|(_, l)| hit(l)).collect();
            let any = !matched.is_empty();
            if a.flags.contains_key(&'c') {
                return (vec![matched.len().to_string()], any);
            }
            let numbered = a.flags.contains_key(&'n');
            (matched.iter().map(|(i, l)| if numbered { format!("{}:{}", i + 1, l) } else { (*l).clone() }).collect(), any)
        }
        "head" | "tail" => {
            let a = parse(args, &['n']);
            let n = count(a.flags.get(&'n'), 10);
            match input(name, &a.rest, stdin) {
                Ok(lines) => {
                    let out = if name == "head" {
                        lines.into_iter().take(n).collect()
                    } else {
                        let skip = lines.len().saturating_sub(n);
                        lines.into_iter().skip(skip).collect()
                    };
                    (out, true)
                }
                Err(o) => o,
            }
        }
        "wc" => {
            let a = parse(args, &[]);
            let lines = match input("wc", &a.rest, stdin) {
                Ok(l) => l,
                Err(o) => return o,
            };
            let words: usize = lines.iter().map(|l| l.split_whitespace().count()).sum();
            let bytes: usize = lines.iter().map(|l| l.len() + 1).sum();
            let only = |c| a.flags.contains_key(&c);
            let out = if only('l') {
                lines.len().to_string()
            } else if only('w') {
                words.to_string()
            } else if only('c') {
                bytes.to_string()
            } else {
                format!("{:>7} {:>7} {:>7}", lines.len(), words, bytes)
            };
            (vec![out], true)
        }
        "sort" => {
            let a = parse(args, &[]);
            let mut lines = match input("sort", &a.rest, stdin) {
                Ok(l) => l,
                Err(o) => return o,
            };
            if a.flags.contains_key(&'n') {
                let key = |l: &String| l.trim().split_whitespace().next().and_then(|w| w.parse::<i64>().ok()).unwrap_or(0);
                lines.sort_by_key(key);
            } else {
                lines.sort();
            }
            if a.flags.contains_key(&'r') {
                lines.reverse();
            }
            if a.flags.contains_key(&'u') {
                lines.dedup();
            }
            (lines, true)
        }
        "uniq" => {
            let a = parse(args, &[]);
            let lines = match input("uniq", &a.rest, stdin) {
                Ok(l) => l,
                Err(o) => return o,
            };
            let mut out: Vec<(usize, String)> = Vec::new();
            for l in lines {
                match out.last_mut() {
                    Some((n, prev)) if *prev == l => *n += 1,
                    _ => out.push((1, l)),
                }
            }
            if a.flags.contains_key(&'c') {
                (out.into_iter().map(|(n, l)| format!("{:>7} {}", n, l)).collect(), true)
            } else {
                (out.into_iter().map(|(_, l)| l).collect(), true)
            }
        }
        "tee" => {
            let a = parse(args, &[]);
            let lines = stdin.map(|s| s.to_vec()).unwrap_or_default();
            let append = a.flags.contains_key(&'a');
            for f in &a.rest {
                if let Err(e) = write_lines(f, &lines, append) {
                    return fail(format!("tee: {}", e));
                }
            }
            (lines, true)
        }
        "cp" | "mv" => {
            let a = parse(args, &[]);
            let [src, dst] = a.rest[..] else {
                return fail(format!("pakai: {} SUMBER TUJUAN", name));
            };
            let from = shell::resolve(Some(src));
            let mut to = shell::resolve(Some(dst));
            if vfs::stat(&to).is_ok_and(|e| e.is_dir) || vfs::mounts().iter().any(|m| *m == to) {
                let base = from.rsplit('/').next().unwrap_or(&from);
                to = format!("{}/{}", to.trim_end_matches('/'), base);
            }
            if name == "mv" {
                return match vfs::rename(&from, &to) {
                    Ok(()) => (Vec::new(), true),
                    Err(e) => fail(format!("mv: {}: {}", from, e)),
                };
            }
            match vfs::read(&from) {
                Ok(bytes) => match vfs::write(&to, &bytes) {
                    Ok(()) => (Vec::new(), true),
                    Err(e) => fail(format!("cp: {}: {}", to, e)),
                },
                Err(e) => fail(format!("cp: {}: {}", from, e)),
            }
        }
        "mkdir" => {
            let a = parse(args, &[]);
            if a.rest.is_empty() {
                return fail(String::from("pakai: mkdir FOLDER..."));
            }
            for d in &a.rest {
                let path = shell::resolve(Some(d));
                if let Err(e) = vfs::mkdir(&path) {
                    return fail(format!("mkdir: {}: {}", path, e));
                }
            }
            (Vec::new(), true)
        }
        "rmdir" => {
            let a = parse(args, &[]);
            for d in &a.rest {
                let path = shell::resolve(Some(d));
                match vfs::stat(&path) {
                    Ok(e) if e.is_dir => {
                        if let Err(e) = vfs::remove(&path) {
                            return fail(format!("rmdir: {}: {}", path, e));
                        }
                    }
                    Ok(_) => return fail(format!("rmdir: {}: bukan folder", path)),
                    Err(e) => return fail(format!("rmdir: {}: {}", path, e)),
                }
            }
            (Vec::new(), !a.rest.is_empty())
        }
        "touch" => {
            let a = parse(args, &[]);
            for f in &a.rest {
                let path = shell::resolve(Some(f));
                let data = vfs::read(&path).unwrap_or_default();
                if let Err(e) = vfs::write(&path, &data) {
                    return fail(format!("touch: {}: {}", path, e));
                }
            }
            (Vec::new(), !a.rest.is_empty())
        }
        "find" => {
            let a = parse(args, &[]);
            // find [DIR] [-name POLA]: the flag parser stops at the first
            // word, so -name is picked out by hand
            let mut dir = None;
            let mut pattern = None;
            let mut i = 0;
            while i < args.len() {
                if args[i] == "-name" {
                    pattern = args.get(i + 1).map(|s| s.as_str());
                    i += 2;
                } else {
                    dir.get_or_insert(args[i].as_str());
                    i += 1;
                }
            }
            let _ = a;
            let root = shell::resolve(dir);
            let mut out = Vec::new();
            find(&root, pattern, &mut out, 0);
            let any = !out.is_empty();
            (out, any)
        }
        "hexdump" | "xxd" => {
            let a = parse(args, &['n']);
            let Some(f) = a.rest.first() else {
                return fail(format!("pakai: {} [-n BYTE] FILE", name));
            };
            let path = shell::resolve(Some(f));
            let limit = count(a.flags.get(&'n'), 512);
            match vfs::read(&path) {
                Ok(bytes) => {
                    let mut out = Vec::new();
                    for (row, chunk) in bytes.iter().take(limit).collect::<Vec<_>>().chunks(16).enumerate() {
                        let hex: String = chunk.iter().map(|b| format!("{:02x} ", b)).collect();
                        let asc: String = chunk.iter().map(|&&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' }).collect();
                        out.push(format!("{:08x}  {:<48} {}", row * 16, hex, asc));
                    }
                    if bytes.len() > limit {
                        out.push(format!("... ({} byte lagi; -n untuk lebih)", bytes.len() - limit));
                    }
                    (out, true)
                }
                Err(e) => fail(format!("{}: {}: {}", name, path, e)),
            }
        }
        "seq" => {
            let nums: Vec<i64> = args.iter().filter_map(|a| a.parse().ok()).collect();
            let (from, to) = match nums[..] {
                [n] => (1, n),
                [a, b, ..] => (a, b),
                _ => return fail(String::from("pakai: seq [AWAL] AKHIR")),
            };
            if to - from > 10_000 {
                return fail(String::from("seq: paling banyak 10000 angka"));
            }
            ((from..=to).map(|n| n.to_string()).collect(), true)
        }
        "basename" | "dirname" => {
            let Some(p) = args.first() else { return fail(format!("pakai: {} PATH", name)) };
            let p = p.trim_end_matches('/');
            let (dir, base) = match p.rfind('/') {
                Some(i) => (if i == 0 { "/" } else { &p[..i] }, &p[i + 1..]),
                None => (".", p),
            };
            (vec![String::from(if name == "basename" { base } else { dir })], true)
        }
        "true" => (Vec::new(), true),
        "false" => (Vec::new(), false),
        "env" | "set" | "export" => {
            // set/export NAME=value ...; without arguments: list the variables
            let assigns: Vec<&String> = args.iter().filter(|a| a.contains('=')).collect();
            if assigns.is_empty() {
                return (shell::vars().into_iter().map(|(k, v)| format!("{}={}", k, v)).collect(), true);
            }
            for a in assigns {
                let (k, v) = a.split_once('=').unwrap();
                if !shell::set_var(k, v) {
                    return fail(format!("{}: nama variabel tidak sah: {}", name, k));
                }
            }
            (Vec::new(), true)
        }
        "unset" => {
            for a in args {
                shell::unset_var(a);
            }
            (Vec::new(), true)
        }
        "history" => {
            let h = crate::terminal::history();
            (h.iter().enumerate().map(|(i, l)| format!("{:>4}  {}", i + 1, l)).collect(), true)
        }
        _ => fail(format!("{}: bukan utilitas", name)),
    }
}

/// Write `lines` to a file (appending when asked).
pub fn write_lines(name: &str, lines: &[String], append: bool) -> Result<(), String> {
    let path = shell::resolve(Some(name));
    let mut data = if append { vfs::read(&path).unwrap_or_default() } else { Vec::new() };
    for l in lines {
        data.extend_from_slice(l.as_bytes());
        data.push(b'\n');
    }
    vfs::write(&path, &data).map_err(|e| format!("{}: {}", path, e))
}

/// Depth-first listing of `dir` (paths), filtered by a name pattern.
fn find(dir: &str, pattern: Option<&str>, out: &mut Vec<String>, depth: usize) {
    if depth > 8 || out.len() > 2000 {
        return;
    }
    let Ok(entries) = vfs::list_entries(dir) else { return };
    for e in entries {
        if e.name == "." || e.name == ".." {
            continue;
        }
        let path = format!("{}/{}", dir.trim_end_matches('/'), e.name);
        if pattern.is_none_or(|p| wildcard(p, &e.name)) {
            out.push(if e.is_dir { format!("{}/", path) } else { path.clone() });
        }
        if e.is_dir {
            find(&path, pattern, out, depth + 1);
        }
    }
}
