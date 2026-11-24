//! The page's DOM for scripts (Peramban): `document`, `window`, `location`
//! and elements over the browser's token stream (`htmlpage.rs`), driven by
//! the JavaScript engine in `js.rs`.
//!
//! The page is a flat list of tokens; an element is its opening tag (found
//! by a stable `uid`) up to the matching closing tag. Scripts can find
//! elements (`getElementById`, `querySelector[All]` with tag / #id / .class /
//! [attr] / descendant selectors, `getElementsBy*`), read and change them
//! (`textContent`, `innerHTML`, attributes, `style.*`, `classList`, `value`),
//! build and move them (`createElement`, `appendChild`, `insertBefore`,
//! `remove`, ...), listen to events (`onclick`, `addEventListener`, inline
//! `onclick="..."` attributes; `click`, `input`, `change`, `load`,
//! `DOMContentLoaded`; events bubble), and use timers (`setTimeout`,
//! `setInterval`, `requestAnimationFrame`) and `document.write` while the
//! page loads. Every change re-lays the page out. Not modelled: computed
//! layout (`offsetWidth` & co are 0), forms submission, `fetch`/XHR,
//! canvas, focus.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::htmlpage::{Page, Tag, Tok};
use crate::js::{self, HostOut, Interp, Value};

const DOCUMENT: u64 = 0;
const WINDOW: u64 = 1;
const LOCATION: u64 = 2;
const STYLE: u64 = 1 << 40;
const CLASSES: u64 = 2 << 40;
const MASK: u64 = (1 << 40) - 1;

/// Run in front of every page's scripts.
const PRELUDE: &str = r#"
var self = window;
var navigator = { userAgent: "Peramban/1.0 (Buitenzorg OS)", language: "id-ID", platform: "Buitenzorg" };
function requestAnimationFrame(f) { return setTimeout(function () { f(Date.now()); }, 16); }
function cancelAnimationFrame(id) { clearTimeout(id); }
function __bzEvent(type, target) {
  return { type: type, target: target, currentTarget: target, defaultPrevented: false, __stop: false,
    preventDefault: function () { this.defaultPrevented = true; },
    stopPropagation: function () { this.__stop = true; } };
}
var localStorage = {
  __d: {},
  getItem: function (k) { return Object.prototype.hasOwnProperty.call(this.__d, k) ? this.__d[k] : null; },
  setItem: function (k, v) { this.__d[k] = String(v); },
  removeItem: function (k) { delete this.__d[k]; },
  clear: function () { this.__d = {}; }
};
var sessionStorage = localStorage;
function Date(a, b, c, d, e, f) {
  if (a === undefined) this.__t = document.__now;
  else if (b === undefined) this.__t = typeof a === "number" ? a : Date.parse(a);
  else this.__t = Date.UTC(a, b, c === undefined ? 1 : c, d || 0, e || 0, f || 0);
}
Date.now = function () { return document.__now; };
Date.UTC = function (y, mo, d, h, mi, s) {
  y += Math.floor(mo / 12); mo = ((mo % 12) + 12) % 12;
  var yy = mo <= 1 ? y - 1 : y, era = Math.floor(yy / 400), yoe = yy - era * 400;
  var mp = (mo + 9) % 12, doy = Math.floor((153 * mp + 2) / 5) + (d === undefined ? 1 : d) - 1;
  var doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
  var days = era * 146097 + doe - 719468;
  return ((days * 24 + (h || 0)) * 60 + (mi || 0)) * 60000 + (s || 0) * 1000;
};
Date.parse = function (s) {
  var m = String(s).split(/*no regex*/"T")[0].split("-");
  return m.length >= 3 ? Date.UTC(+m[0], +m[1] - 1, +m[2]) : NaN;
};
Date.prototype.__civil = function () {
  var z = Math.floor(this.__t / 86400000) + 719468, era = Math.floor(z / 146097), doe = z - era * 146097;
  var yoe = Math.floor((doe - Math.floor(doe / 1460) + Math.floor(doe / 36524) - Math.floor(doe / 146096)) / 365);
  var doy = doe - (365 * yoe + Math.floor(yoe / 4) - Math.floor(yoe / 100)), mp = Math.floor((5 * doy + 2) / 153);
  var d = doy - Math.floor((153 * mp + 2) / 5) + 1, m = mp < 10 ? mp + 3 : mp - 9;
  return [yoe + era * 400 + (m <= 2 ? 1 : 0), m - 1, d];
};
Date.prototype.getTime = function () { return this.__t; };
Date.prototype.valueOf = Date.prototype.getTime;
Date.prototype.getFullYear = function () { return this.__civil()[0]; };
Date.prototype.getMonth = function () { return this.__civil()[1]; };
Date.prototype.getDate = function () { return this.__civil()[2]; };
Date.prototype.getDay = function () { return ((Math.floor(this.__t / 86400000) % 7) + 11) % 7; };
Date.prototype.getHours = function () { return Math.floor(this.__t / 3600000) % 24; };
Date.prototype.getMinutes = function () { return Math.floor(this.__t / 60000) % 60; };
Date.prototype.getSeconds = function () { return Math.floor(this.__t / 1000) % 60; };
Date.prototype.getMilliseconds = function () { return this.__t % 1000; };
Date.prototype.getTimezoneOffset = function () { return 0; };
Date.prototype.__p = function (n) { return n < 10 ? "0" + n : "" + n; };
Date.prototype.toLocaleTimeString = function () { return this.__p(this.getHours()) + "." + this.__p(this.getMinutes()) + "." + this.__p(this.getSeconds()); };
Date.prototype.toLocaleDateString = function () { return this.getDate() + "/" + (this.getMonth() + 1) + "/" + this.getFullYear(); };
Date.prototype.toISOString = function () { return this.getFullYear() + "-" + this.__p(this.getMonth() + 1) + "-" + this.__p(this.getDate()) + "T" + this.__p(this.getHours()) + ":" + this.__p(this.getMinutes()) + ":" + this.__p(this.getSeconds()) + "Z"; };
Date.prototype.toString = function () { return this.toLocaleDateString() + " " + this.toLocaleTimeString(); };
Date.prototype.toLocaleString = Date.prototype.toString;
"#;

struct Timer {
    id: u32,
    due: u64,
    every: Option<u64>,
    f: Value,
}

pub struct JsState {
    interp: Interp,
    st: DomState,
}

// The page table is behind a lock and a page's script state is only ever
// touched by the thread holding it, so the Rc-based values never cross threads
// while in use.
unsafe impl Send for JsState {}

#[derive(Default)]
struct DomState {
    /// (target handle, event type, function); `on:<type>` = the `onX` property
    handlers: Vec<(u64, String, Value)>,
    timers: Vec<Timer>,
    next_timer: u32,
    now: u64,
    url: String,
    width: i32,
    nav: Option<String>,
    /// insertion point of `document.write` (token index)
    write_at: Option<usize>,
    changed: bool,
    detached: Vec<Vec<Tok>>,
    clicks: Vec<u32>,
}

/// The host: the page's tokens + the script state.
struct Dom<'a> {
    toks: &'a mut Vec<Tok>,
    title: &'a mut String,
    next_uid: &'a mut u32,
    clickable: &'a mut alloc::collections::BTreeSet<u32>,
    st: &'a mut DomState,
}

fn is_void(n: &str) -> bool {
    matches!(n, "br" | "hr" | "img" | "input" | "meta" | "link" | "area" | "base" | "col" | "embed" | "source" | "wbr" | "param" | "track")
}

/// Index of the token that ends the element opened at `start` (its closing
/// tag, or where an unclosed element implicitly ends); `start` for a void one.
fn span_end(v: &[Tok], start: usize) -> usize {
    let Tok::Tag(t) = &v[start] else { return start };
    if is_void(&t.name) {
        return start;
    }
    let mut open: Vec<&str> = Vec::new();
    for (i, tok) in v.iter().enumerate().skip(start + 1) {
        let Tok::Tag(x) = tok else { continue };
        if x.closing {
            if open.is_empty() {
                return i; // its own close, or a parent's: ends here
            }
            if let Some(at) = open.iter().rposition(|n| *n == x.name) {
                open.truncate(at);
            }
        } else if !is_void(&x.name) {
            if open.is_empty() && x.name == t.name && matches!(t.name.as_str(), "p" | "li" | "option" | "dt" | "dd" | "tr" | "td" | "th") {
                return i; // <p>a<p>b
            }
            open.push(&x.name);
        }
    }
    v.len()
}

fn has_close(v: &[Tok], start: usize, end: usize) -> bool {
    match (&v[start], v.get(end)) {
        (Tok::Tag(a), Some(Tok::Tag(b))) => b.closing && b.name == a.name,
        _ => false,
    }
}

fn camel_to_kebab(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            o.push('-');
            o.push(c.to_ascii_lowercase());
        } else {
            o.push(c);
        }
    }
    o
}

fn style_get(style: &str, key: &str) -> String {
    for decl in style.split(';') {
        if let Some((k, v)) = decl.split_once(':') {
            if k.trim().eq_ignore_ascii_case(key) {
                return String::from(v.trim());
            }
        }
    }
    String::new()
}

fn style_set(style: &str, key: &str, val: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for decl in style.split(';') {
        if let Some((k, v)) = decl.split_once(':') {
            if !k.trim().eq_ignore_ascii_case(key) {
                out.push(format!("{}: {}", k.trim(), v.trim()));
            }
        }
    }
    if !val.is_empty() {
        out.push(format!("{}: {}", key, val));
    }
    out.join("; ")
}

fn escape_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            c => out.push(c),
        }
    }
}

fn serialize(v: &[Tok], out: &mut String) {
    for t in v {
        match t {
            Tok::Text(s) => escape_text(s, out),
            Tok::Script(_) => {}
            Tok::Tag(tag) => {
                if tag.name == "#text" {
                    continue;
                }
                out.push('<');
                if tag.closing {
                    out.push('/');
                }
                out.push_str(&tag.name);
                for (k, val) in &tag.attrs {
                    out.push(' ');
                    out.push_str(k);
                    out.push_str("=\"");
                    out.push_str(&val.replace('"', "&quot;"));
                    out.push('"');
                }
                out.push('>');
            }
        }
    }
}

/// A selector compound: `tag#id.class[attr=value]`.
#[derive(Default)]
struct Compound {
    tag: String,
    id: String,
    classes: Vec<String>,
    attrs: Vec<(String, Option<String>)>,
}

fn parse_compound(s: &str) -> Compound {
    let mut c = Compound::default();
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    let word = |i: &mut usize| -> String {
        let st = *i;
        while *i < b.len() && (b[*i].is_alphanumeric() || b[*i] == '-' || b[*i] == '_') {
            *i += 1;
        }
        b[st..*i].iter().collect()
    };
    if i < b.len() && b[i] == '*' {
        i += 1;
    } else {
        c.tag = word(&mut i).to_ascii_lowercase();
    }
    while i < b.len() {
        match b[i] {
            '#' => {
                i += 1;
                c.id = word(&mut i);
            }
            '.' => {
                i += 1;
                c.classes.push(word(&mut i));
            }
            '[' => {
                i += 1;
                let end = b[i..].iter().position(|&x| x == ']').map(|p| i + p).unwrap_or(b.len());
                let inner: String = b[i..end].iter().collect();
                match inner.split_once('=') {
                    Some((k, v)) => c.attrs.push((String::from(k.trim()), Some(String::from(v.trim().trim_matches(|x| x == '"' || x == '\''))))),
                    None => c.attrs.push((String::from(inner.trim()), None)),
                }
                i = end + 1;
            }
            _ => i += 1,
        }
    }
    c
}

fn matches_compound(t: &Tag, c: &Compound) -> bool {
    if !c.tag.is_empty() && t.name != c.tag {
        return false;
    }
    if !c.id.is_empty() && t.attr("id") != c.id {
        return false;
    }
    let cls = t.attr("class");
    for want in &c.classes {
        if !cls.split_whitespace().any(|x| x == want) {
            return false;
        }
    }
    for (k, v) in &c.attrs {
        match v {
            None if !t.attrs.iter().any(|(a, _)| a == k) => return false,
            Some(v) if t.attr(k) != v => return false,
            _ => {}
        }
    }
    true
}

impl Dom<'_> {
    /// Where an element lives: (None = the page, Some(i) = a detached tree), index.
    fn find(&self, uid: u32) -> Option<(Option<usize>, usize)> {
        let pos = |v: &[Tok]| v.iter().position(|t| matches!(t, Tok::Tag(x) if !x.closing && x.uid == uid));
        if let Some(i) = pos(self.toks) {
            return Some((None, i));
        }
        for (k, d) in self.st.detached.iter().enumerate() {
            if let Some(i) = pos(d) {
                return Some((Some(k), i));
            }
        }
        None
    }

    fn vec(&mut self, at: Option<usize>) -> &mut Vec<Tok> {
        match at {
            None => self.toks,
            Some(k) => &mut self.st.detached[k],
        }
    }

    fn tag(&mut self, uid: u32) -> Option<&mut Tag> {
        let (at, i) = self.find(uid)?;
        match &mut self.vec(at)[i] {
            Tok::Tag(t) => Some(t),
            _ => None,
        }
    }

    fn tag_ref(&self, uid: u32) -> Option<Tag> {
        let (at, i) = self.find(uid)?;
        let v = match at {
            None => &*self.toks,
            Some(k) => &self.st.detached[k],
        };
        match &v[i] {
            Tok::Tag(t) => Some(t.clone()),
            _ => None,
        }
    }

    fn new_uid(&mut self) -> u32 {
        let u = *self.next_uid;
        *self.next_uid += 1;
        u
    }

    /// Parse an HTML fragment into tokens with fresh uids (scripts dropped).
    fn fragment(&mut self, html: &str) -> Vec<Tok> {
        let p = crate::htmlpage::parse_fragment(html);
        let mut out = Vec::with_capacity(p.len());
        for t in p {
            match t {
                Tok::Tag(mut tag) => {
                    if !tag.closing {
                        tag.uid = self.new_uid();
                        if tag.attrs.iter().any(|(k, _)| k == "onclick") {
                            self.clickable.insert(tag.uid);
                        }
                    }
                    out.push(Tok::Tag(tag));
                }
                Tok::Script(_) => {}
                t => out.push(t),
            }
        }
        out
    }

    fn text_of(&self, uid: u32) -> String {
        let Some((at, i)) = self.find(uid) else { return String::new() };
        let v = match at {
            None => &*self.toks,
            Some(k) => &self.st.detached[k],
        };
        let end = span_end(v, i);
        let mut s = String::new();
        for t in &v[i + 1..end.max(i + 1).min(v.len())] {
            if let Tok::Text(x) = t {
                s.push_str(x);
            }
        }
        s
    }

    /// Replace an element's content.
    fn set_content(&mut self, uid: u32, content: Vec<Tok>) {
        let Some((at, i)) = self.find(uid) else { return };
        let v = self.vec(at);
        let Tok::Tag(t) = &v[i] else { return };
        if is_void(&t.name) {
            return;
        }
        let name = t.name.clone();
        let end = span_end(v, i);
        if has_close(v, i, end) || end > i + 1 {
            let stop = end.min(v.len());
            v.splice(i + 1..stop, content);
        } else {
            let mut ins = content;
            ins.push(Tok::Tag(Tag { name, closing: true, attrs: Vec::new(), uid: 0 }));
            v.splice(i + 1..i + 1, ins);
        }
        self.st.changed = true;
    }

    /// Cut an element (with its content) out of wherever it is.
    fn take(&mut self, uid: u32) -> Option<Vec<Tok>> {
        let (at, i) = self.find(uid)?;
        let v = self.vec(at);
        let end = span_end(v, i);
        let stop = if has_close(v, i, end) { end + 1 } else { end.max(i + 1) };
        let stop = stop.min(v.len());
        let cut: Vec<Tok> = v.drain(i..stop).collect();
        if let Some(k) = at {
            if self.st.detached[k].is_empty() {
                self.st.detached.remove(k);
            }
        }
        self.st.changed = true;
        Some(cut)
    }

    /// Close an element that has no closing tag, so content can go in.
    fn ensure_closed(&mut self, uid: u32) -> Option<()> {
        let (at, i) = self.find(uid)?;
        let v = self.vec(at);
        let end = span_end(v, i);
        if has_close(v, i, end) {
            return Some(());
        }
        let Tok::Tag(t) = &v[i] else { return None };
        if is_void(&t.name) {
            return None;
        }
        let close = Tok::Tag(Tag { name: t.name.clone(), closing: true, attrs: Vec::new(), uid: 0 });
        v.insert(end.min(v.len()), close);
        Some(())
    }

    /// Insert `nodes` into `parent`, before `before` (None = at the end).
    fn insert(&mut self, parent: u32, nodes: Vec<Tok>, before: Option<u32>, at_start: bool) -> bool {
        if self.ensure_closed(parent).is_none() {
            return false;
        }
        let Some((at, i)) = self.find(parent) else { return false };
        let pos = match before {
            Some(b) => match self.find(b) {
                Some((bat, bi)) if bat == at => bi,
                _ => return false,
            },
            None if at_start => i + 1,
            None => span_end(self.vec(at), i),
        };
        let v = self.vec(at);
        v.splice(pos..pos, nodes);
        self.st.changed = true;
        true
    }

    fn parent_of(&self, uid: u32) -> Option<u32> {
        let (at, i) = self.find(uid)?;
        let v = match at {
            None => &*self.toks,
            Some(k) => &self.st.detached[k],
        };
        let mut stack: Vec<(String, u32)> = Vec::new();
        for t in &v[..i] {
            let Tok::Tag(x) = t else { continue };
            if x.closing {
                if let Some(p) = stack.iter().rposition(|(n, _)| *n == x.name) {
                    stack.truncate(p);
                }
            } else if !is_void(&x.name) {
                stack.push((x.name.clone(), x.uid));
            }
        }
        // elements an unclosed <p> would "contain" are siblings: drop
        // implicitly closed ones
        while let Some((_, u)) = stack.last() {
            let u = *u;
            let pi = v.iter().position(|t| matches!(t, Tok::Tag(x) if !x.closing && x.uid == u)).unwrap();
            if span_end(v, pi) > i {
                return Some(u);
            }
            stack.pop();
        }
        None
    }

    /// Direct child elements.
    fn children(&self, uid: u32) -> Vec<u32> {
        let Some((at, i)) = self.find(uid) else { return Vec::new() };
        let v = match at {
            None => &*self.toks,
            Some(k) => &self.st.detached[k],
        };
        let end = span_end(v, i).min(v.len());
        let mut out = Vec::new();
        let mut k = i + 1;
        while k < end {
            if let Tok::Tag(x) = &v[k] {
                if !x.closing {
                    if x.name != "#text" {
                        out.push(x.uid);
                    }
                    let e = span_end(v, k);
                    k = if has_close(v, k, e) { e + 1 } else { e.max(k + 1) };
                    continue;
                }
            }
            k += 1;
        }
        out
    }

    /// Elements matching a selector list, inside `scope` (None = the page).
    fn select(&self, scope: Option<u32>, sel: &str, first: bool) -> Vec<u32> {
        let groups: Vec<Vec<Compound>> = sel.split(',').map(|g| g.split_whitespace().filter(|c| *c != ">").map(parse_compound).collect()).collect();
        let (v, lo, hi): (&[Tok], usize, usize) = match scope {
            None => (self.toks, 0, self.toks.len()),
            Some(u) => match self.find(u) {
                None => return Vec::new(),
                Some((at, i)) => {
                    let v = match at {
                        None => &*self.toks,
                        Some(k) => &self.st.detached[k],
                    };
                    (v, i + 1, span_end(v, i).min(v.len()))
                }
            },
        };
        let mut out = Vec::new();
        let mut stack: Vec<&Tag> = Vec::new();
        for t in &v[lo..hi] {
            let Tok::Tag(x) = t else { continue };
            if x.closing {
                if let Some(p) = stack.iter().rposition(|s| s.name == x.name) {
                    stack.truncate(p);
                }
                continue;
            }
            let hit = groups.iter().any(|g| {
                let Some((last, rest)) = g.split_last() else { return false };
                if !matches_compound(x, last) {
                    return false;
                }
                // the other compounds must match ancestors, in order
                let mut need = rest.len();
                for anc in stack.iter().rev() {
                    if need == 0 {
                        break;
                    }
                    if matches_compound(anc, &rest[need - 1]) {
                        need -= 1;
                    }
                }
                need == 0
            });
            if hit && x.name != "#text" {
                out.push(x.uid);
                if first {
                    break;
                }
            }
            if !is_void(&x.name) {
                stack.push(x);
            }
        }
        out
    }

    fn uid_of(v: &Value) -> Option<u32> {
        match v {
            Value::Obj(o) => match o.borrow().kind {
                js::Kind::Host(h) if h & !MASK == 0 && h >= 16 => Some(h as u32),
                _ => None,
            },
            _ => None,
        }
    }

    /// A value appended to a node: an element (moved), else text.
    fn node_toks(&mut self, v: &Value) -> Vec<Tok> {
        match Self::uid_of(v) {
            Some(u) => self.take(u).unwrap_or_default(),
            None => alloc::vec![Tok::Text(v.to_str())],
        }
    }

    fn body(&self) -> Option<u32> {
        self.select(None, "body", true).first().copied().or_else(|| {
            self.toks.iter().find_map(|t| match t {
                Tok::Tag(x) if !x.closing && !matches!(x.name.as_str(), "html" | "head" | "meta" | "link" | "title") => Some(x.uid),
                _ => None,
            })
        })
    }

    fn timer(&mut self, args: &[Value], every: bool) -> HostOut {
        let f = args.first().cloned().unwrap_or(Value::Undef);
        let ms = args.get(1).map(|v| v.to_num()).filter(|n| n.is_finite()).unwrap_or(0.0).max(0.0) as u64;
        self.st.next_timer += 1;
        let id = self.st.next_timer;
        let ms = if every { ms.max(10) } else { ms };
        self.st.timers.push(Timer { id, due: self.st.now + ms, every: if every { Some(ms) } else { None }, f });
        HostOut::Val(Value::Num(id as f64))
    }

    fn element_get(&mut self, uid: u32, name: &str) -> Option<HostOut> {
        let t = self.tag_ref(uid)?;
        let s = |x: &str| Some(HostOut::Val(Value::str(x)));
        match name {
            "tagName" | "nodeName" => s(&t.name.to_ascii_uppercase()),
            "id" => s(t.attr("id")),
            "className" => s(t.attr("class")),
            "textContent" | "innerText" => {
                let x = self.text_of(uid);
                s(&x)
            }
            "innerHTML" => {
                let (at, i) = self.find(uid)?;
                let v = match at {
                    None => &*self.toks,
                    Some(k) => &self.st.detached[k],
                };
                let end = span_end(v, i).min(v.len());
                let mut out = String::new();
                serialize(&v[(i + 1).min(end)..end], &mut out);
                s(&out)
            }
            "outerHTML" => {
                let mut out = String::new();
                let (at, i) = self.find(uid)?;
                let v = match at {
                    None => &*self.toks,
                    Some(k) => &self.st.detached[k],
                };
                let end = span_end(v, i);
                let stop = if has_close(v, i, end) { end + 1 } else { end.max(i + 1) };
                serialize(&v[i..stop.min(v.len())], &mut out);
                s(&out)
            }
            "value" => {
                if t.name == "textarea" {
                    let x = self.text_of(uid);
                    return s(&x);
                }
                s(t.attr("value"))
            }
            "checked" | "disabled" | "hidden" | "selected" | "readOnly" | "required" => {
                let key = name.to_ascii_lowercase();
                Some(HostOut::Val(Value::Bool(t.attrs.iter().any(|(k, _)| *k == key))))
            }
            "href" | "src" | "alt" | "title" | "type" | "name" | "placeholder" | "lang" | "dir" | "rel" | "target" => s(t.attr(name)),
            "style" => Some(HostOut::Elem(uid as u64 | STYLE)),
            "classList" => Some(HostOut::Elem(uid as u64 | CLASSES)),
            "children" | "childNodes" => Some(HostOut::Elems(self.children(uid).into_iter().map(|u| u as u64).collect())),
            "childElementCount" => Some(HostOut::Val(Value::Num(self.children(uid).len() as f64))),
            "firstElementChild" | "firstChild" => Some(match self.children(uid).first() {
                Some(c) => HostOut::Elem(*c as u64),
                None => HostOut::Val(Value::Null),
            }),
            "lastElementChild" | "lastChild" => Some(match self.children(uid).last() {
                Some(c) => HostOut::Elem(*c as u64),
                None => HostOut::Val(Value::Null),
            }),
            "parentNode" | "parentElement" => Some(match self.parent_of(uid) {
                Some(p) => HostOut::Elem(p as u64),
                None => HostOut::Val(Value::Null),
            }),
            "nextElementSibling" | "previousElementSibling" | "nextSibling" | "previousSibling" => {
                let sibs = match self.parent_of(uid) {
                    Some(p) => self.children(p),
                    None => return Some(HostOut::Val(Value::Null)),
                };
                let k = sibs.iter().position(|x| *x == uid)?;
                let next = name.starts_with("next");
                let o = if next { sibs.get(k + 1) } else { k.checked_sub(1).and_then(|j| sibs.get(j)) };
                Some(match o {
                    Some(x) => HostOut::Elem(*x as u64),
                    None => HostOut::Val(Value::Null),
                })
            }
            "nodeType" => Some(HostOut::Val(Value::Num(1.0))),
            "isConnected" => Some(HostOut::Val(Value::Bool(matches!(self.find(uid), Some((None, _)))))),
            "offsetWidth" | "offsetHeight" | "clientWidth" | "clientHeight" | "scrollTop" | "scrollLeft" | "offsetTop" | "offsetLeft" => Some(HostOut::Val(Value::Num(0.0))),
            "dataset" => None,
            n if n.starts_with("on") => {
                let ev = format!("on:{}", &n[2..]);
                Some(HostOut::Val(self.st.handlers.iter().find(|(h, e, _)| *h == uid as u64 && *e == ev).map(|(_, _, f)| f.clone()).unwrap_or(Value::Null)))
            }
            _ => None,
        }
    }

    fn element_set(&mut self, uid: u32, name: &str, v: &Value) -> bool {
        let text = v.to_str();
        match name {
            "textContent" | "innerText" => {
                let c = if text.is_empty() { Vec::new() } else { alloc::vec![Tok::Text(text)] };
                self.set_content(uid, c);
            }
            "innerHTML" => {
                let f = self.fragment(&text);
                self.set_content(uid, f);
            }
            "value" => {
                let is_area = self.tag_ref(uid).map(|t| t.name == "textarea").unwrap_or(false);
                if is_area {
                    self.set_content(uid, alloc::vec![Tok::Text(text)]);
                } else if let Some(t) = self.tag(uid) {
                    t.set_attr("value", &text);
                }
                self.st.changed = true;
            }
            "id" | "href" | "src" | "alt" | "title" | "type" | "name" | "placeholder" | "lang" | "target" => {
                if let Some(t) = self.tag(uid) {
                    t.set_attr(name, &text);
                }
                self.st.changed = true;
            }
            "className" => {
                if let Some(t) = self.tag(uid) {
                    t.set_attr("class", &text);
                }
                self.st.changed = true;
            }
            "checked" | "disabled" | "hidden" | "selected" => {
                if let Some(t) = self.tag(uid) {
                    t.attrs.retain(|(k, _)| k != name);
                    if v.truthy() {
                        t.attrs.push((String::from(name), String::new()));
                    }
                }
                self.st.changed = true;
            }
            "style" => {
                if let Some(t) = self.tag(uid) {
                    t.set_attr("style", &text);
                }
                self.st.changed = true;
            }
            n if n.starts_with("on") => {
                let ev = format!("on:{}", &n[2..]);
                self.st.handlers.retain(|(h, e, _)| !(*h == uid as u64 && *e == ev));
                if matches!(v, Value::Obj(_)) {
                    self.st.handlers.push((uid as u64, ev, v.clone()));
                    if n == "onclick" {
                        self.clickable.insert(uid);
                        self.st.changed = true;
                    }
                }
            }
            _ => return false,
        }
        true
    }

    fn element_call(&mut self, uid: u32, name: &str, a: &[Value]) -> Option<Result<HostOut, String>> {
        let arg = |i: usize| a.get(i).cloned().unwrap_or(Value::Undef);
        let ok = |v: Value| Some(Ok(HostOut::Val(v)));
        match name {
            "getAttribute" => {
                let k = arg(0).to_str().to_ascii_lowercase();
                let t = self.tag_ref(uid)?;
                match t.attrs.iter().find(|(x, _)| *x == k) {
                    Some((_, v)) => ok(Value::str(v)),
                    None => ok(Value::Null),
                }
            }
            "hasAttribute" => {
                let k = arg(0).to_str().to_ascii_lowercase();
                let t = self.tag_ref(uid)?;
                ok(Value::Bool(t.attrs.iter().any(|(x, _)| *x == k)))
            }
            "setAttribute" => {
                let k = arg(0).to_str().to_ascii_lowercase();
                let v = arg(1).to_str();
                if k == "onclick" {
                    self.clickable.insert(uid);
                }
                if let Some(t) = self.tag(uid) {
                    t.set_attr(&k, &v);
                }
                self.st.changed = true;
                ok(Value::Undef)
            }
            "removeAttribute" => {
                let k = arg(0).to_str().to_ascii_lowercase();
                if let Some(t) = self.tag(uid) {
                    t.attrs.retain(|(x, _)| *x != k);
                }
                self.st.changed = true;
                ok(Value::Undef)
            }
            "appendChild" | "append" | "prepend" => {
                let mut nodes = Vec::new();
                for v in a {
                    nodes.extend(self.node_toks(v));
                }
                self.insert(uid, nodes, None, name == "prepend");
                ok(arg(0))
            }
            "insertBefore" => {
                let nodes = self.node_toks(&arg(0));
                let before = Self::uid_of(&arg(1));
                if !self.insert(uid, nodes.clone(), before, false) {
                    self.insert(uid, nodes, None, false);
                }
                ok(arg(0))
            }
            "removeChild" => {
                if let Some(c) = Self::uid_of(&arg(0)) {
                    if let Some(cut) = self.take(c) {
                        self.st.detached.push(cut);
                    }
                }
                ok(arg(0))
            }
            "replaceChild" => {
                let new = arg(0);
                let old = Self::uid_of(&arg(1));
                let nodes = self.node_toks(&new);
                if let Some(o) = old {
                    if self.insert(uid, nodes, Some(o), false) {
                        if let Some(cut) = self.take(o) {
                            self.st.detached.push(cut);
                        }
                    }
                }
                ok(arg(1))
            }
            "remove" => {
                if let Some(cut) = self.take(uid) {
                    self.st.detached.push(cut);
                }
                ok(Value::Undef)
            }
            "replaceChildren" => {
                let mut nodes = Vec::new();
                for v in a {
                    nodes.extend(self.node_toks(v));
                }
                self.set_content(uid, nodes);
                ok(Value::Undef)
            }
            "addEventListener" => {
                let ev = arg(0).to_str();
                if ev == "click" {
                    self.clickable.insert(uid);
                    self.st.changed = true;
                }
                self.st.handlers.push((uid as u64, ev, arg(1)));
                ok(Value::Undef)
            }
            "removeEventListener" => {
                let ev = arg(0).to_str();
                let f = arg(1);
                self.st.handlers.retain(|(h, e, g)| !(*h == uid as u64 && *e == ev && js::strict_eq(g, &f)));
                ok(Value::Undef)
            }
            "querySelector" => Some(Ok(match self.select(Some(uid), &arg(0).to_str(), true).first() {
                Some(u) => HostOut::Elem(*u as u64),
                None => HostOut::Val(Value::Null),
            })),
            "querySelectorAll" => Some(Ok(HostOut::Elems(self.select(Some(uid), &arg(0).to_str(), false).into_iter().map(|u| u as u64).collect()))),
            "getElementsByTagName" => Some(Ok(HostOut::Elems(self.select(Some(uid), &arg(0).to_str(), false).into_iter().map(|u| u as u64).collect()))),
            "getElementsByClassName" => {
                let sel: String = arg(0).to_str().split_whitespace().map(|c| format!(".{}", c)).collect();
                Some(Ok(HostOut::Elems(self.select(Some(uid), &sel, false).into_iter().map(|u| u as u64).collect())))
            }
            "matches" => {
                let t = self.tag_ref(uid)?;
                let sel = arg(0).to_str();
                ok(Value::Bool(sel.split(',').any(|g| g.split_whitespace().last().map(|c| matches_compound(&t, &parse_compound(c))).unwrap_or(false))))
            }
            "closest" => {
                let sel = arg(0).to_str();
                let mut cur = Some(uid);
                while let Some(u) = cur {
                    if let Some(t) = self.tag_ref(u) {
                        if sel.split(',').any(|g| g.split_whitespace().last().map(|c| matches_compound(&t, &parse_compound(c))).unwrap_or(false)) {
                            return Some(Ok(HostOut::Elem(u as u64)));
                        }
                    }
                    cur = self.parent_of(u);
                }
                ok(Value::Null)
            }
            "contains" => {
                let Some(mut c) = Self::uid_of(&arg(0)) else { return ok(Value::Bool(false)) };
                loop {
                    if c == uid {
                        return ok(Value::Bool(true));
                    }
                    match self.parent_of(c) {
                        Some(p) => c = p,
                        None => return ok(Value::Bool(false)),
                    }
                }
            }
            "cloneNode" => {
                let deep = arg(0).truthy();
                let (at, i) = self.find(uid)?;
                let v = match at {
                    None => &*self.toks,
                    Some(k) => &self.st.detached[k],
                };
                let end = span_end(v, i);
                let stop = if has_close(v, i, end) { end + 1 } else { end.max(i + 1) };
                let mut copy: Vec<Tok> = if deep { v[i..stop.min(v.len())].to_vec() } else { alloc::vec![v[i].clone()] };
                if !deep {
                    if let Tok::Tag(t) = &v[i] {
                        if !is_void(&t.name) {
                            copy.push(Tok::Tag(Tag { name: t.name.clone(), closing: true, attrs: Vec::new(), uid: 0 }));
                        }
                    }
                }
                let mut first = 0;
                for (k, t) in copy.iter_mut().enumerate() {
                    if let Tok::Tag(x) = t {
                        if !x.closing {
                            x.uid = *self.next_uid;
                            *self.next_uid += 1;
                            if k == 0 {
                                first = x.uid;
                            }
                        }
                    }
                }
                self.st.detached.push(copy);
                Some(Ok(HostOut::Elem(first as u64)))
            }
            "click" => {
                self.st.clicks.push(uid);
                ok(Value::Undef)
            }
            "focus" | "blur" | "scrollIntoView" | "select" => ok(Value::Undef),
            "getBoundingClientRect" => ok(Value::Undef),
            _ => None,
        }
    }

    fn style_get_prop(&mut self, uid: u32, name: &str) -> Option<HostOut> {
        let t = self.tag_ref(uid)?;
        if name == "cssText" {
            return Some(HostOut::Val(Value::str(t.attr("style"))));
        }
        Some(HostOut::Val(Value::str(&style_get(t.attr("style"), &camel_to_kebab(name)))))
    }

    fn class_call(&mut self, uid: u32, name: &str, a: &[Value]) -> Option<Result<HostOut, String>> {
        let t = self.tag_ref(uid)?;
        let mut cls: Vec<String> = t.attr("class").split_whitespace().map(String::from).collect();
        let c = a.first().map(|v| v.to_str()).unwrap_or_default();
        let r = match name {
            "contains" => return Some(Ok(HostOut::Val(Value::Bool(cls.iter().any(|x| *x == c))))),
            "add" => {
                for v in a {
                    let s = v.to_str();
                    if !cls.contains(&s) {
                        cls.push(s);
                    }
                }
                Value::Undef
            }
            "remove" => {
                for v in a {
                    let s = v.to_str();
                    cls.retain(|x| *x != s);
                }
                Value::Undef
            }
            "toggle" => {
                let force = a.get(1).map(|v| v.truthy());
                let has = cls.contains(&c);
                let want = force.unwrap_or(!has);
                cls.retain(|x| *x != c);
                if want {
                    cls.push(c);
                }
                Value::Bool(want)
            }
            _ => return None,
        };
        if let Some(t) = self.tag(uid) {
            t.set_attr("class", &cls.join(" "));
        }
        self.st.changed = true;
        Some(Ok(HostOut::Val(r)))
    }
}

impl js::Host for Dom<'_> {
    fn get(&mut self, h: u64, name: &str) -> Option<HostOut> {
        let s = |x: &str| Some(HostOut::Val(Value::str(x)));
        match h {
            DOCUMENT => match name {
                "title" => {
                    let t = self.title.clone();
                    s(&t)
                }
                "body" => Some(match self.body() {
                    Some(b) => HostOut::Elem(b as u64),
                    None => HostOut::Val(Value::Null),
                }),
                "documentElement" | "head" => {
                    let want = if name == "head" { "head" } else { "html" };
                    Some(match self.select(None, want, true).first().copied().or_else(|| self.body()) {
                        Some(b) => HostOut::Elem(b as u64),
                        None => HostOut::Val(Value::Null),
                    })
                }
                "readyState" => s("complete"),
                "URL" => {
                    let u = self.st.url.clone();
                    s(&u)
                }
                "location" => Some(HostOut::Elem(LOCATION)),
                "defaultView" => Some(HostOut::Elem(WINDOW)),
                "cookie" | "referrer" => s(""),
                "__now" => Some(HostOut::Val(Value::Num(self.st.now as f64))),
                _ => None,
            },
            WINDOW => match name {
                "innerWidth" | "outerWidth" => Some(HostOut::Val(Value::Num(self.st.width as f64))),
                "innerHeight" | "outerHeight" => Some(HostOut::Val(Value::Num(480.0))),
                "devicePixelRatio" => Some(HostOut::Val(Value::Num(1.0))),
                "location" => Some(HostOut::Elem(LOCATION)),
                "document" => Some(HostOut::Elem(DOCUMENT)),
                "window" | "self" | "top" | "parent" => Some(HostOut::Elem(WINDOW)),
                "scrollY" | "scrollX" | "pageYOffset" => Some(HostOut::Val(Value::Num(0.0))),
                _ => None,
            },
            LOCATION => {
                let url = self.st.url.clone();
                let (proto, rest) = url.split_once("://").map(|(a, b)| (format!("{}:", a), String::from(b))).unwrap_or((String::new(), url.clone()));
                let (host, path) = match rest.find('/') {
                    Some(i) if !proto.is_empty() => (String::from(&rest[..i]), String::from(&rest[i..])),
                    _ if !proto.is_empty() => (rest.clone(), String::from("/")),
                    _ => (String::new(), rest.clone()),
                };
                let (path, hash) = path.split_once('#').map(|(a, b)| (String::from(a), format!("#{}", b))).unwrap_or((path, String::new()));
                let (path, search) = path.split_once('?').map(|(a, b)| (String::from(a), format!("?{}", b))).unwrap_or((path, String::new()));
                match name {
                    "href" => s(&url),
                    "protocol" => s(&proto),
                    "host" => s(&host),
                    "hostname" => s(host.split(':').next().unwrap_or("")),
                    "port" => s(host.split(':').nth(1).unwrap_or("")),
                    "pathname" => s(&path),
                    "search" => s(&search),
                    "hash" => s(&hash),
                    "origin" => s(&format!("{}//{}", proto, host)),
                    _ => None,
                }
            }
            h if h & CLASSES != 0 && h & STYLE == 0 => match name {
                "length" => {
                    let t = self.tag_ref((h & MASK) as u32)?;
                    Some(HostOut::Val(Value::Num(t.attr("class").split_whitespace().count() as f64)))
                }
                "value" => {
                    let t = self.tag_ref((h & MASK) as u32)?;
                    s(t.attr("class"))
                }
                _ => None,
            },
            h if h & STYLE != 0 => self.style_get_prop((h & MASK) as u32, name),
            h => self.element_get(h as u32, name),
        }
    }

    fn set(&mut self, h: u64, name: &str, v: &Value) -> bool {
        match h {
            DOCUMENT => match name {
                "title" => {
                    *self.title = v.to_str();
                    self.st.changed = true;
                    true
                }
                "cookie" => true,
                n if n.starts_with("on") => {
                    let ev = format!("on:{}", &n[2..]);
                    self.st.handlers.retain(|(hh, e, _)| !(*hh == DOCUMENT && *e == ev));
                    self.st.handlers.push((DOCUMENT, ev, v.clone()));
                    true
                }
                _ => false,
            },
            WINDOW => match name {
                n if n.starts_with("on") => {
                    let ev = format!("on:{}", &n[2..]);
                    self.st.handlers.retain(|(hh, e, _)| !(*hh == WINDOW && *e == ev));
                    self.st.handlers.push((WINDOW, ev, v.clone()));
                    true
                }
                "location" => {
                    self.st.nav = Some(v.to_str());
                    true
                }
                _ => false,
            },
            LOCATION => {
                if name == "href" || name == "hash" || name == "search" || name == "pathname" {
                    let t = v.to_str();
                    self.st.nav = Some(if name == "hash" { format!("{}#{}", self.st.url.split('#').next().unwrap_or(""), t.trim_start_matches('#')) } else { t });
                }
                true
            }
            h if h & STYLE != 0 => {
                let uid = (h & MASK) as u32;
                if let Some(t) = self.tag(uid) {
                    let old = String::from(t.attr("style"));
                    let new = if name == "cssText" { v.to_str() } else { style_set(&old, &camel_to_kebab(name), &v.to_str()) };
                    t.set_attr("style", &new);
                }
                self.st.changed = true;
                true
            }
            h if h & CLASSES != 0 => false,
            h => self.element_set(h as u32, name, v),
        }
    }

    fn call(&mut self, h: u64, name: &str, a: &[Value]) -> Option<Result<HostOut, String>> {
        let arg = |i: usize| a.get(i).cloned().unwrap_or(Value::Undef);
        match name {
            "setTimeout" if h <= WINDOW => return Some(Ok(self.timer(a, false))),
            "setInterval" if h <= WINDOW => return Some(Ok(self.timer(a, true))),
            "clearTimeout" | "clearInterval" if h <= WINDOW => {
                let id = arg(0).to_num() as u32;
                self.st.timers.retain(|t| t.id != id);
                return Some(Ok(HostOut::Val(Value::Undef)));
            }
            _ => {}
        }
        match h {
            DOCUMENT => match name {
                "getElementById" => {
                    let id = arg(0).to_str();
                    let hit = self.toks.iter().find_map(|t| match t {
                        Tok::Tag(x) if !x.closing && x.attr("id") == id => Some(x.uid),
                        _ => None,
                    });
                    Some(Ok(match hit {
                        Some(u) => HostOut::Elem(u as u64),
                        None => HostOut::Val(Value::Null),
                    }))
                }
                "querySelector" => Some(Ok(match self.select(None, &arg(0).to_str(), true).first() {
                    Some(u) => HostOut::Elem(*u as u64),
                    None => HostOut::Val(Value::Null),
                })),
                "querySelectorAll" | "getElementsByTagName" => Some(Ok(HostOut::Elems(self.select(None, &arg(0).to_str(), false).into_iter().map(|u| u as u64).collect()))),
                "getElementsByClassName" => {
                    let sel: String = arg(0).to_str().split_whitespace().map(|c| format!(".{}", c)).collect();
                    Some(Ok(HostOut::Elems(self.select(None, &sel, false).into_iter().map(|u| u as u64).collect())))
                }
                "getElementsByName" => Some(Ok(HostOut::Elems(self.select(None, &format!("[name={}]", arg(0).to_str()), false).into_iter().map(|u| u as u64).collect()))),
                "createElement" => {
                    let name = arg(0).to_str().to_ascii_lowercase();
                    let uid = self.new_uid();
                    let mut v = alloc::vec![Tok::Tag(Tag { name: name.clone(), closing: false, attrs: Vec::new(), uid })];
                    if !is_void(&name) {
                        v.push(Tok::Tag(Tag { name, closing: true, attrs: Vec::new(), uid: 0 }));
                    }
                    self.st.detached.push(v);
                    Some(Ok(HostOut::Elem(uid as u64)))
                }
                "createTextNode" => {
                    let uid = self.new_uid();
                    self.st.detached.push(alloc::vec![
                        Tok::Tag(Tag { name: String::from("#text"), closing: false, attrs: Vec::new(), uid }),
                        Tok::Text(arg(0).to_str()),
                        Tok::Tag(Tag { name: String::from("#text"), closing: true, attrs: Vec::new(), uid: 0 }),
                    ]);
                    Some(Ok(HostOut::Elem(uid as u64)))
                }
                "write" | "writeln" => {
                    let mut html: String = a.iter().map(|v| v.to_str()).collect();
                    if name == "writeln" {
                        html.push('\n');
                    }
                    let f = self.fragment(&html);
                    let n = f.len();
                    match self.st.write_at {
                        Some(at) => {
                            let at = at.min(self.toks.len());
                            self.toks.splice(at..at, f);
                            self.st.write_at = Some(at + n);
                        }
                        None => {
                            // after loading, document.write replaces the page
                            *self.toks = f;
                        }
                    }
                    self.st.changed = true;
                    Some(Ok(HostOut::Val(Value::Undef)))
                }
                "addEventListener" => {
                    self.st.handlers.push((DOCUMENT, arg(0).to_str(), arg(1)));
                    Some(Ok(HostOut::Val(Value::Undef)))
                }
                "removeEventListener" => {
                    let (ev, f) = (arg(0).to_str(), arg(1));
                    self.st.handlers.retain(|(hh, e, g)| !(*hh == DOCUMENT && *e == ev && js::strict_eq(g, &f)));
                    Some(Ok(HostOut::Val(Value::Undef)))
                }
                "hasFocus" => Some(Ok(HostOut::Val(Value::Bool(true)))),
                _ => None,
            },
            WINDOW => match name {
                "addEventListener" => {
                    self.st.handlers.push((WINDOW, arg(0).to_str(), arg(1)));
                    Some(Ok(HostOut::Val(Value::Undef)))
                }
                "removeEventListener" => Some(Ok(HostOut::Val(Value::Undef))),
                "alert" | "confirm" | "prompt" => Some(Ok(HostOut::Val(Value::Undef))),
                "scrollTo" | "scrollBy" | "focus" => Some(Ok(HostOut::Val(Value::Undef))),
                _ => None,
            },
            LOCATION => match name {
                "assign" | "replace" => {
                    self.st.nav = Some(arg(0).to_str());
                    Some(Ok(HostOut::Val(Value::Undef)))
                }
                "reload" => {
                    self.st.nav = Some(self.st.url.clone());
                    Some(Ok(HostOut::Val(Value::Undef)))
                }
                "toString" => Some(Ok(HostOut::Val(Value::str(&self.st.url.clone())))),
                _ => None,
            },
            h if h & STYLE != 0 => match name {
                "setProperty" => {
                    let uid = (h & MASK) as u32;
                    if let Some(t) = self.tag(uid) {
                        let old = String::from(t.attr("style"));
                        t.set_attr("style", &style_set(&old, &arg(0).to_str(), &arg(1).to_str()));
                    }
                    self.st.changed = true;
                    Some(Ok(HostOut::Val(Value::Undef)))
                }
                "getPropertyValue" => self.style_get_prop((h & MASK) as u32, &arg(0).to_str()).map(Ok),
                "removeProperty" => {
                    let uid = (h & MASK) as u32;
                    if let Some(t) = self.tag(uid) {
                        let old = String::from(t.attr("style"));
                        t.set_attr("style", &style_set(&old, &arg(0).to_str(), ""));
                    }
                    self.st.changed = true;
                    Some(Ok(HostOut::Val(Value::Undef)))
                }
                _ => None,
            },
            h if h & CLASSES != 0 => self.class_call((h & MASK) as u32, name, a),
            h => self.element_call(h as u32, name, a),
        }
    }
}

// ---- the page's script state ----------------------------------------------------

/// What a script run changed (returned to the browser as bits).
pub const CHANGED: i32 = 1;
pub const TIMERS: i32 = 2;
pub const NAVIGATE: i32 = 4;
pub const PREVENTED: i32 = 8;

fn flags(page: &Page, js: &JsState) -> i32 {
    let mut f = 0;
    if !js.st.timers.is_empty() {
        f |= TIMERS;
    }
    if js.st.nav.is_some() {
        f |= NAVIGATE;
    }
    let _ = page;
    f
}

fn note(js: &mut JsState, r: Result<Value, String>, log: &mut Vec<String>) {
    log.append(&mut js.interp.output);
    if let Err(e) = r {
        log.push(format!("[galat] {}", e));
    }
}

/// Run a page's scripts in document order, then its load handlers.
/// `external[k]` is the fetched code of script k (when it has a `src`).
pub fn run_scripts(page: &mut Page, url: &str, now: u64, width: i32, log: &mut Vec<String>) -> i32 {
    if page.scripts.is_empty() {
        return 0;
    }
    let mut js = Box::new(JsState { interp: Interp::new(), st: DomState::default() });
    js.interp.limit = 3_000_000;
    js.st.url = String::from(url);
    js.st.now = now;
    js.st.width = width;
    js::set_seed(now ^ 0x5DEE_CE66_D1CE_4E5B);
    let document = js.interp.host_value(DOCUMENT);
    let window = js.interp.host_value(WINDOW);
    let location = js.interp.host_value(LOCATION);
    js.interp.set_global("document", document);
    js.interp.set_global("window", window);
    js.interp.set_global("location", location);
    {
        let JsState { interp, st } = &mut *js;
        let mut d = Dom { toks: &mut page.toks, title: &mut page.title, next_uid: &mut page.next_uid, clickable: &mut page.clickable, st };
        let r = interp.run(PRELUDE, &mut d);
        if let Err(e) = r {
            log.push(format!("[galat prelude] {}", e));
        }
    }
    let n = page.scripts.len();
    for k in 0..n {
        let code = core::mem::take(&mut page.scripts[k].code);
        // document.write goes right after the script's own position
        let at = page.toks.iter().position(|t| matches!(t, Tok::Script(i) if *i == k)).map(|p| p + 1);
        js.st.write_at = at;
        let JsState { interp, st } = &mut *js;
        let mut d = Dom { toks: &mut page.toks, title: &mut page.title, next_uid: &mut page.next_uid, clickable: &mut page.clickable, st };
        let r = interp.run(&code, &mut d);
        page.scripts[k].code = code;
        note(&mut js, r, log);
        process_clicks(page, &mut js, log);
    }
    js.st.write_at = None;
    // DOMContentLoaded, then load
    for (target, ev) in [(DOCUMENT, "DOMContentLoaded"), (WINDOW, "load"), (DOCUMENT, "load")] {
        dispatch_on(page, &mut js, target, ev, log);
    }
    let mut f = CHANGED; // document.write and handlers may have changed it
    f |= flags(page, &js);
    page.js = Some(js);
    f
}

/// Call the handlers of one target (no bubbling): `on<ev>` and listeners.
fn dispatch_on(page: &mut Page, js: &mut JsState, target: u64, ev: &str, log: &mut Vec<String>) -> bool {
    let on = format!("on:{}", ev);
    let fs: Vec<Value> = js.st.handlers.iter().filter(|(h, e, _)| *h == target && (*e == ev || *e == on)).map(|(_, _, f)| f.clone()).collect();
    if fs.is_empty() {
        return false;
    }
    let this = js.interp.host_value(target);
    let JsState { interp, st } = js;
    let mut d = Dom { toks: &mut page.toks, title: &mut page.title, next_uid: &mut page.next_uid, clickable: &mut page.clickable, st };
    let event = make_event(interp, &mut d, ev, &this);
    for f in fs {
        let r = interp.call_value(&f, &this, &[event.clone()], &mut d);
        note_split(interp, r, log);
    }
    is_prevented(interp, &event, &mut d)
}

fn make_event(interp: &mut Interp, d: &mut Dom, ty: &str, target: &Value) -> Value {
    let f = interp.get_global("__bzEvent").unwrap_or(Value::Undef);
    interp.call_value(&f, &Value::Undef, &[Value::str(ty), target.clone()], d).unwrap_or(Value::Undef)
}

fn is_prevented(interp: &mut Interp, ev: &Value, d: &mut Dom) -> bool {
    interp.get(ev, "defaultPrevented", d).map(|v| v.truthy()).unwrap_or(false)
}

fn is_stopped(interp: &mut Interp, ev: &Value, d: &mut Dom) -> bool {
    interp.get(ev, "__stop", d).map(|v| v.truthy()).unwrap_or(false)
}

fn note_split(interp: &mut Interp, r: Result<Value, String>, log: &mut Vec<String>) {
    log.append(&mut interp.output);
    if let Err(e) = r {
        log.push(format!("[galat] {}", e));
    }
}

/// Dispatch a bubbling event at element `uid`. Returns true when a handler
/// called `preventDefault()`.
fn dispatch(page: &mut Page, js: &mut JsState, uid: u32, ev: &str, log: &mut Vec<String>) -> bool {
    // the chain: the element, its ancestors, the document
    let mut chain: Vec<u64> = Vec::new();
    {
        let JsState { st, .. } = js;
        let d = Dom { toks: &mut page.toks, title: &mut page.title, next_uid: &mut page.next_uid, clickable: &mut page.clickable, st };
        let mut cur = Some(uid);
        while let Some(u) = cur {
            chain.push(u as u64);
            cur = d.parent_of(u);
            if chain.len() > 64 {
                break;
            }
        }
    }
    chain.push(DOCUMENT);
    let target = js.interp.host_value(uid as u64);
    let JsState { interp, st } = js;
    let mut d = Dom { toks: &mut page.toks, title: &mut page.title, next_uid: &mut page.next_uid, clickable: &mut page.clickable, st };
    let event = make_event(interp, &mut d, ev, &target);
    for h in chain {
        let node = interp.host_value(h);
        let _ = interp.put(&event, "currentTarget", node.clone(), &mut d);
        // an inline attribute handler (onclick="...")
        if h != DOCUMENT {
            let code = d.tag_ref(h as u32).map(|t| String::from(t.attr(&format!("on{}", ev)))).unwrap_or_default();
            if !code.is_empty() {
                interp.set_global("__bz_this", node.clone());
                interp.set_global("__bz_event", event.clone());
                let src = format!("(function (event) {{ {} \n}}).call(__bz_this, __bz_event);", code);
                let r = interp.run(&src, &mut d);
                note_split(interp, r, log);
            }
        }
        let fs: Vec<Value> = d
            .st
            .handlers
            .iter()
            .filter(|(t, e, _)| *t == h && (*e == ev || *e == format!("on:{}", ev)))
            .map(|(_, _, f)| f.clone())
            .collect();
        for f in fs {
            let r = interp.call_value(&f, &node, &[event.clone()], &mut d);
            note_split(interp, r, log);
        }
        if is_stopped(interp, &event, &mut d) {
            break;
        }
    }
    is_prevented(interp, &event, &mut d)
}

/// `el.click()` calls queued while a script ran.
fn process_clicks(page: &mut Page, js: &mut JsState, log: &mut Vec<String>) {
    for _ in 0..32 {
        let Some(u) = js.st.clicks.pop() else { break };
        dispatch(page, js, u, "click", log);
    }
}

/// A click on link `k`: returns flags (PREVENTED = do not follow the address).
pub fn click(page: &mut Page, k: usize, log: &mut Vec<String>) -> i32 {
    let Some(&uid) = page.link_uids.get(k) else { return 0 };
    let href = page.links.get(k).cloned().unwrap_or_default();
    let Some(mut js) = page.js.take() else {
        // a page without scripts: only javascript: links are inert
        return if href.starts_with("javascript:") || href == "#" { PREVENTED } else { 0 };
    };
    js.st.changed = false;
    let mut prevented = dispatch(page, &mut js, uid, "click", log);
    // a checkbox toggles itself
    {
        let JsState { st, .. } = &mut *js;
        let mut d = Dom { toks: &mut page.toks, title: &mut page.title, next_uid: &mut page.next_uid, clickable: &mut page.clickable, st };
        if let Some(t) = d.tag(uid) {
            if t.name == "input" && matches!(t.attr("type"), "checkbox" | "radio") {
                if t.attrs.iter().any(|(k, _)| k == "checked") {
                    if t.attr("type") == "checkbox" {
                        t.attrs.retain(|(k, _)| k != "checked");
                    }
                } else {
                    t.attrs.push((String::from("checked"), String::new()));
                }
                d.st.changed = true;
            }
        }
    }
    if let Some(code) = href.strip_prefix("javascript:") {
        let JsState { interp, st } = &mut *js;
        let mut d = Dom { toks: &mut page.toks, title: &mut page.title, next_uid: &mut page.next_uid, clickable: &mut page.clickable, st };
        let r = interp.run(code, &mut d);
        note_split(interp, r, log);
        prevented = true;
    }
    if href == "#" || href.starts_with("js:") {
        prevented = true;
    }
    process_clicks(page, &mut js, log);
    let f = flags(page, &js) | if js.st.changed { CHANGED } else { 0 } | if prevented { PREVENTED } else { 0 };
    page.js = Some(js);
    f
}

/// Text typed into a field (link `k`): sets its value, fires input + change.
pub fn input(page: &mut Page, k: usize, text: &str, log: &mut Vec<String>) -> i32 {
    let Some(&uid) = page.link_uids.get(k) else { return 0 };
    let mut js = page.js.take().unwrap_or_else(|| Box::new(JsState { interp: Interp::new(), st: DomState::default() }));
    {
        let JsState { st, .. } = &mut *js;
        let mut d = Dom { toks: &mut page.toks, title: &mut page.title, next_uid: &mut page.next_uid, clickable: &mut page.clickable, st };
        d.element_set(uid, "value", &Value::str(text));
    }
    js.st.changed = true;
    if page.scripts.is_empty() {
        page.js = Some(js);
        return CHANGED;
    }
    dispatch(page, &mut js, uid, "input", log);
    dispatch(page, &mut js, uid, "change", log);
    process_clicks(page, &mut js, log);
    let f = flags(page, &js) | CHANGED;
    page.js = Some(js);
    f
}

/// Run the timers that are due at `now` (ms).
pub fn tick(page: &mut Page, now: u64, log: &mut Vec<String>) -> i32 {
    let Some(mut js) = page.js.take() else { return 0 };
    js.st.now = now;
    js.st.changed = false;
    let mut rounds = 0;
    loop {
        rounds += 1;
        if rounds > 64 {
            break;
        }
        let Some(pos) = js.st.timers.iter().enumerate().filter(|(_, t)| t.due <= now).min_by_key(|(_, t)| t.due).map(|(i, _)| i) else { break };
        let (f, id, every) = {
            let t = &mut js.st.timers[pos];
            (t.f.clone(), t.id, t.every)
        };
        match every {
            Some(ms) => {
                let t = &mut js.st.timers[pos];
                t.due = (t.due + ms).max(now + 1);
            }
            None => {
                js.st.timers.remove(pos);
            }
        }
        let _ = id;
        let JsState { interp, st } = &mut *js;
        let mut d = Dom { toks: &mut page.toks, title: &mut page.title, next_uid: &mut page.next_uid, clickable: &mut page.clickable, st };
        let r = if let Value::Str(code) = &f {
            interp.run(code, &mut d)
        } else {
            interp.call_value(&f, &Value::Undef, &[], &mut d)
        };
        note_split(interp, r, log);
        process_clicks(page, &mut js, log);
    }
    let f = flags(page, &js) | if js.st.changed { CHANGED } else { 0 };
    page.js = Some(js);
    f
}

/// The address a script asked to go to (`location.href = ...`), once.
pub fn take_navigation(page: &mut Page) -> Option<String> {
    page.js.as_mut().and_then(|j| j.st.nav.take())
}

/// Milliseconds until the next timer (None = no timers).
pub fn next_timer(page: &Page) -> Option<u64> {
    let js = page.js.as_ref()?;
    js.st.timers.iter().map(|t| t.due.saturating_sub(js.st.now)).min()
}

