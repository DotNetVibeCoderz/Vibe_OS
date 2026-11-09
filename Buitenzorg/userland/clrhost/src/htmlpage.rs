//! The browser's page engine in the host (Peramban, MWeb): the same tolerant
//! HTML tokenizer and flow layout as `userland/clr-apps/MWeb/Html.cs`, which
//! stays as the off-device version. Moved here because RustCLR keeps strings
//! as UTF-8, so a character index into a long string is a scan from its start
//! -- tokenizing a 250 KB page in C# took over two minutes. Natives:
//! `HtmlLoad(string) -> id`, `HtmlInfo(id, what) -> string`,
//! `HtmlSetImage(id, index, w, h)`, `HtmlLayout(id, width, int[] metrics,
//! int[] palette) -> items`, `HtmlItems(id, int[])`, `HtmlTexts(id)`,
//! `HtmlFree(id)`. Keep the two versions in step.

use alloc::string::String;
use alloc::vec::Vec;

use rustclr_core::{ArrayStorage, ClrArray, ExecResult, Interpreter, Value};

use crate::bzapi::{int, string};

struct Tag {
    name: String,
    closing: bool,
    attrs: Vec<(String, String)>,
}

impl Tag {
    fn attr(&self, n: &str) -> &str {
        self.attrs.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str()).unwrap_or("")
    }
}

enum Tok {
    Text(String),
    Tag(Tag),
}

struct Img {
    attr_w: i32,
    attr_h: i32,
    nat_w: i32,
    nat_h: i32,
}

struct Item {
    kind: i32, // 0 text, 1 rule, 2 image
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    style: i32,
    color: i32,
    image: i32,
    href: i32,
    text: String,
}

struct Page {
    id: i64,
    title: String,
    toks: Vec<Tok>,
    images: Vec<Img>,
    image_info: Vec<(String, String)>, // src, alt
    links: Vec<String>,
    items: Vec<Item>,
    height: i32,
}

struct Pages {
    next: i64,
    pages: Vec<Page>,
}

static PAGES: crate::netapi::Lock<Pages> = crate::netapi::Lock::new(Pages { next: 1, pages: Vec::new() });

// ---- tokenizer --------------------------------------------------------------

fn ascii_of(c: char, out: &mut String) {
    match c {
        '\n' => out.push('\n'),
        '\t' | '\r' => out.push(' '),
        ' '..='~' => out.push(c),
        '\u{A0}' => out.push(' '),
        '\u{A9}' => out.push_str("(c)"),
        '\u{AE}' => out.push_str("(R)"),
        '\u{2122}' => out.push_str("(TM)"),
        '\u{2014}' => out.push_str("--"),
        '\u{2013}' | '\u{2010}' | '\u{2011}' | '\u{2212}' => out.push('-'),
        '\u{2026}' => out.push_str("..."),
        '\u{AB}' => out.push_str("<<"),
        '\u{BB}' => out.push_str(">>"),
        '\u{2018}' | '\u{2019}' | '\u{2032}' => out.push('\''),
        '\u{201C}' | '\u{201D}' | '\u{2033}' => out.push('"'),
        '\u{2022}' | '\u{B7}' => out.push('*'),
        '\u{D7}' => out.push('x'),
        '\u{B0}' => out.push_str(" deg"),
        '\u{20AC}' => out.push_str("EUR"),
        '\u{C0}'..='\u{C5}' => out.push('A'),
        '\u{E0}'..='\u{E5}' => out.push('a'),
        '\u{C7}' => out.push('C'),
        '\u{E7}' => out.push('c'),
        '\u{C8}'..='\u{CB}' => out.push('E'),
        '\u{E8}'..='\u{EB}' => out.push('e'),
        '\u{CC}'..='\u{CF}' => out.push('I'),
        '\u{EC}'..='\u{EF}' => out.push('i'),
        '\u{D1}' => out.push('N'),
        '\u{F1}' => out.push('n'),
        '\u{D2}'..='\u{D6}' | '\u{D8}' => out.push('O'),
        '\u{F2}'..='\u{F6}' | '\u{F8}' => out.push('o'),
        '\u{D9}'..='\u{DC}' => out.push('U'),
        '\u{F9}'..='\u{FC}' => out.push('u'),
        '\u{DF}' => out.push_str("ss"),
        '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}' | '\u{AD}' => {}
        _ => out.push('?'),
    }
}

fn named(e: &str) -> Option<char> {
    Some(match e {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "copy" => '\u{A9}',
        "reg" => '\u{AE}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "hellip" => '\u{2026}',
        "laquo" => '\u{AB}',
        "raquo" => '\u{BB}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201C}',
        "rdquo" => '\u{201D}',
        "bull" => '\u{2022}',
        "middot" => '\u{B7}',
        "times" => '\u{D7}',
        "deg" => '\u{B0}',
        "euro" => '\u{20AC}',
        "trade" => '\u{2122}',
        "minus" => '\u{2212}',
        "shy" => '\u{AD}',
        _ => return None,
    })
}

/// Character references decoded, characters the bitmap font lacks mapped to ASCII.
fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if c == '&' {
            if let Some(semi) = s[i + 1..].find(';').map(|p| i + 1 + p).filter(|&p| p - i <= 10) {
                let e = &s[i + 1..semi];
                let ch = if let Some(num) = e.strip_prefix('#') {
                    let v = if let Some(h) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
                        u32::from_str_radix(h, 16).ok()
                    } else {
                        num.parse::<u32>().ok()
                    };
                    v.and_then(char::from_u32)
                } else {
                    named(e)
                };
                if let Some(ch) = ch {
                    ascii_of(ch, &mut out);
                    while let Some(&(j, _)) = it.peek() {
                        if j > semi {
                            break;
                        }
                        it.next();
                    }
                    continue;
                }
            }
        }
        ascii_of(c, &mut out);
    }
    out
}

fn find_ci(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

fn parse_tag(body: &str) -> Tag {
    let b = body.as_bytes();
    let mut p = 0;
    let mut closing = false;
    if p < b.len() && b[p] == b'/' {
        closing = true;
        p += 1;
    }
    let start = p;
    while p < b.len() && (b[p].is_ascii_alphanumeric() || b[p] == b'-') {
        p += 1;
    }
    let name = body[start..p].to_ascii_lowercase();
    let mut attrs: Vec<(String, String)> = Vec::new();
    while p < b.len() {
        while p < b.len() && (b[p].is_ascii_whitespace() || b[p] == b'/') {
            p += 1;
        }
        let ns = p;
        while p < b.len() && b[p] != b'=' && !b[p].is_ascii_whitespace() && b[p] != b'/' {
            p += 1;
        }
        if p == ns {
            p += 1;
            continue;
        }
        let an = body[ns..p].to_ascii_lowercase();
        let mut value = String::new();
        while p < b.len() && b[p].is_ascii_whitespace() {
            p += 1;
        }
        if p < b.len() && b[p] == b'=' {
            p += 1;
            while p < b.len() && b[p].is_ascii_whitespace() {
                p += 1;
            }
            if p < b.len() && (b[p] == b'"' || b[p] == b'\'') {
                let q = b[p];
                p += 1;
                let vs = p;
                while p < b.len() && b[p] != q {
                    p += 1;
                }
                value = decode(&body[vs..p.min(b.len())]);
                p += 1;
            } else {
                let vs = p;
                while p < b.len() && !b[p].is_ascii_whitespace() {
                    p += 1;
                }
                value = decode(&body[vs..p]);
            }
        }
        if !attrs.iter().any(|(k, _)| *k == an) {
            attrs.push((an, value));
        }
    }
    Tag { name, closing, attrs }
}

fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c == ' ' || c == '\n' || c == '\r' || c == '\t' {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(c);
    }
    if space && !out.is_empty() {
        out.push(' ');
    }
    out
}

fn parse(html: &str) -> Page {
    let mut page = Page { id: 0, title: String::new(), toks: Vec::new(), images: Vec::new(), image_info: Vec::new(), links: Vec::new(), items: Vec::new(), height: 0 };
    let b = html.as_bytes();
    let n = b.len();
    let mut i = 0;
    let mut in_title = false;
    let mut title = String::new();
    let flush = |text: &str, toks: &mut Vec<Tok>, in_title: bool, title: &mut String| {
        if text.is_empty() {
            return;
        }
        let t = decode(text);
        if in_title {
            title.push_str(&t);
        } else {
            toks.push(Tok::Text(t));
        }
    };
    while i < n {
        if b[i] != b'<' {
            let lt = html[i..].find('<').map(|p| i + p).unwrap_or(n);
            flush(&html[i..lt], &mut page.toks, in_title, &mut title);
            i = lt;
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = html[i + 4..].find("-->").map(|p| i + 4 + p + 3).unwrap_or(n);
            continue;
        }
        if i + 1 < n && (b[i + 1] == b'!' || b[i + 1] == b'?') {
            i = html[i..].find('>').map(|p| i + p + 1).unwrap_or(n);
            continue;
        }
        let Some(close) = html[i..].find('>').map(|p| i + p) else {
            flush(&html[i..], &mut page.toks, in_title, &mut title);
            break;
        };
        if !(i + 1 < n && (b[i + 1].is_ascii_alphabetic() || b[i + 1] == b'/')) {
            flush("<", &mut page.toks, in_title, &mut title);
            i += 1;
            continue;
        }
        let tag = parse_tag(&html[i + 1..close]);
        i = close + 1;
        if tag.name.is_empty() {
            continue;
        }
        if tag.name == "title" {
            in_title = !tag.closing;
            continue;
        }
        if !tag.closing && matches!(tag.name.as_str(), "script" | "style" | "noscript" | "svg" | "template") {
            let end = alloc::format!("</{}", tag.name);
            i = find_ci(b, end.as_bytes(), i).unwrap_or(n);
            continue;
        }
        if tag.name == "img" && !tag.closing {
            let src = tag.attr("src");
            if !src.is_empty() {
                page.images.push(Img {
                    attr_w: tag.attr("width").parse().unwrap_or(0),
                    attr_h: tag.attr("height").parse().unwrap_or(0),
                    nat_w: 0,
                    nat_h: 0,
                });
                page.image_info.push((String::from(src), String::from(tag.attr("alt"))));
            }
        }
        if tag.name == "a" && !tag.closing && !tag.attr("href").is_empty() {
            page.links.push(String::from(tag.attr("href")));
        }
        page.toks.push(Tok::Tag(tag));
    }
    page.title = String::from(collapse(&title).trim());
    page
}

// ---- layout -----------------------------------------------------------------

fn is_block(n: &str) -> bool {
    matches!(
        n,
        "p" | "div" | "section" | "article" | "header" | "footer" | "nav" | "main" | "aside" | "form" | "ul" | "ol" | "li" | "dl" | "dt" | "dd"
            | "table" | "tr" | "blockquote" | "pre" | "figure" | "figcaption" | "center" | "address" | "fieldset" | "details" | "summary"
            | "body" | "html" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "hr" | "caption" | "thead" | "tbody"
    )
}

fn image_size(img: &Img, max_w: i32) -> (i32, i32) {
    let (mut w, mut h) = (img.attr_w, img.attr_h);
    if img.nat_w > 0 {
        let (pw, ph) = (img.nat_w as i64, img.nat_h as i64);
        if w > 0 && h <= 0 {
            h = (ph * w as i64 / pw.max(1)) as i32;
        } else if h > 0 && w <= 0 {
            w = (pw * h as i64 / ph.max(1)) as i32;
        } else if w <= 0 {
            w = pw as i32;
            h = ph as i32;
        }
    } else if w <= 0 || h <= 0 {
        if w <= 0 {
            w = 120;
        }
        if h <= 0 {
            h = 90;
        }
    }
    if w > max_w {
        h = (h as i64 * max_w as i64 / (w as i64).max(1)) as i32;
        w = max_w;
    }
    (w.max(4), h.max(4))
}

struct Lay<'a> {
    items: Vec<Item>,
    line: Vec<Item>,
    width: i32,
    margin: i32,
    indent: i32,
    x: i32,
    y: i32,
    pending_space: bool,
    last_gap: i32,
    bold: i32,
    heading: i32,
    href: i32,
    m: &'a [i32; 6],
    pal: &'a [i32; 5],
}

impl Lay<'_> {
    fn left(&self) -> i32 {
        self.margin + self.indent
    }
    fn right(&self) -> i32 {
        self.width - self.margin
    }
    fn style(&self) -> i32 {
        if self.heading == 1 || self.heading == 2 {
            2
        } else if self.heading > 0 || self.bold > 0 {
            1
        } else {
            0
        }
    }
    fn gw(&self, st: i32) -> i32 {
        self.m[(st * 2) as usize].max(1)
    }
    fn lh(&self, st: i32) -> i32 {
        self.m[(st * 2 + 1) as usize].max(1)
    }
    fn color(&self) -> i32 {
        if self.href >= 0 {
            self.pal[1]
        } else if self.heading > 0 {
            self.pal[2]
        } else {
            self.pal[0]
        }
    }

    fn end_line(&mut self) {
        if self.line.is_empty() {
            return;
        }
        let h = self.line.iter().map(|i| i.h).max().unwrap_or(0);
        for mut it in core::mem::take(&mut self.line) {
            it.y = if it.kind == 0 { self.y + h - it.h } else { self.y };
            self.items.push(it);
        }
        self.y += h + 2;
        self.x = self.left();
        self.pending_space = false;
        self.last_gap = 0;
    }

    fn gap(&mut self, g: i32) {
        self.end_line();
        if g > self.last_gap {
            self.y += g - self.last_gap;
            self.last_gap = g;
        }
        self.x = self.left();
    }

    fn word(&mut self, w: &str) {
        let st = self.style();
        let (gw, lh) = (self.gw(st), self.lh(st));
        let mut chars: Vec<char> = w.chars().collect();
        let need = chars.len() as i32 * gw + if self.pending_space && self.x > self.left() { gw } else { 0 };
        if self.x + need > self.right() && self.x > self.left() {
            self.end_line();
        }
        if self.pending_space && self.x > self.left() {
            self.x += gw;
        }
        self.pending_space = false;
        let color = self.color();
        let mut room = ((self.right() - self.x) / gw).max(1) as usize;
        while chars.len() > room {
            let head: String = chars[..room].iter().collect();
            self.line.push(Item { kind: 0, x: self.x, y: 0, w: room as i32 * gw, h: lh, style: st, color, image: -1, href: self.href, text: head });
            chars.drain(..room);
            self.end_line();
            room = ((self.right() - self.x) / gw).max(1) as usize;
        }
        let text: String = chars.iter().collect();
        let len = chars.len() as i32;
        if let Some(last) = self.line.last_mut() {
            if last.kind == 0 && last.style == st && last.href == self.href && last.color == color && last.x + last.w <= self.x {
                let pad = (self.x - (last.x + last.w)) / gw;
                for _ in 0..pad {
                    last.text.push(' ');
                }
                last.text.push_str(&text);
                last.w = last.text.chars().count() as i32 * gw;
                self.x += len * gw;
                return;
            }
        }
        self.line.push(Item { kind: 0, x: self.x, y: 0, w: len * gw, h: lh, style: st, color, image: -1, href: self.href, text });
        self.x += len * gw;
    }
}

fn layout(page: &mut Page, width: i32, m: &[i32; 6], pal: &[i32; 5]) {
    let mut l = Lay { items: Vec::new(), line: Vec::new(), width, margin: 12, indent: 0, x: 12, y: 8, pending_space: false, last_gap: 0, bold: 0, heading: 0, href: -1, m, pal };
    let mut pre = 0;
    let mut lists: Vec<i32> = Vec::new();
    let mut img_index = 0usize;
    let mut link_index = 0i32;
    for t in &page.toks {
        match t {
            Tok::Text(s) => {
                if pre > 0 {
                    for (li, line) in s.split('\n').enumerate() {
                        if li > 0 {
                            l.end_line();
                        }
                        if !line.is_empty() {
                            l.pending_space = false;
                            l.word(line);
                        }
                    }
                    continue;
                }
                let lead = s.starts_with(' ') || s.starts_with('\n');
                if lead {
                    l.pending_space = true;
                }
                let mut any = false;
                for w in s.split(|c: char| c == ' ' || c == '\n') {
                    if w.is_empty() {
                        continue;
                    }
                    if any {
                        l.pending_space = true;
                    }
                    l.word(w);
                    any = true;
                }
                if any && (s.ends_with(' ') || s.ends_with('\n')) {
                    l.pending_space = true;
                }
            }
            Tok::Tag(tag) => {
                let nm = tag.name.as_str();
                let closing = tag.closing;
                match nm {
                    "br" => {
                        if l.line.is_empty() {
                            l.y += l.lh(0);
                        }
                        l.end_line();
                        continue;
                    }
                    "hr" => {
                        l.gap(8);
                        let (x, w) = (l.left(), l.right() - l.left());
                        l.items.push(Item { kind: 1, x, y: l.y, w, h: 1, style: 0, color: pal[4], image: -1, href: -1, text: String::new() });
                        l.y += 9;
                        l.last_gap = 8;
                        continue;
                    }
                    "img" if !closing => {
                        if img_index >= page.images.len() {
                            continue;
                        }
                        let (w, h) = image_size(&page.images[img_index], l.right() - l.left());
                        if l.x + w > l.right() && l.x > l.left() {
                            l.end_line();
                        }
                        if l.pending_space && l.x > l.left() {
                            l.x += l.gw(0);
                        }
                        let alt = page.image_info[img_index].1.clone();
                        l.line.push(Item { kind: 2, x: l.x, y: 0, w, h, style: 0, color: 0, image: img_index as i32, href: l.href, text: alt });
                        l.x += w;
                        l.pending_space = false;
                        img_index += 1;
                        continue;
                    }
                    "a" => {
                        if closing {
                            l.href = -1;
                        } else if !tag.attr("href").is_empty() {
                            l.href = link_index;
                            link_index += 1;
                        } else {
                            l.href = -1;
                        }
                        continue;
                    }
                    _ => {}
                }
                if nm == "b" || nm == "strong" || nm == "th" {
                    l.bold = (l.bold + if closing { -1 } else { 1 }).max(0);
                    if nm != "th" {
                        continue;
                    }
                }
                if nm == "td" || nm == "th" {
                    if !closing && l.x > l.left() {
                        l.x += l.gw(0) * 2;
                        l.pending_space = false;
                    }
                    continue;
                }
                if !is_block(nm) {
                    continue;
                }
                let nb = nm.as_bytes();
                if nb.len() == 2 && nb[0] == b'h' && (b'1'..=b'6').contains(&nb[1]) {
                    l.gap(if closing { 6 } else { 12 });
                    l.heading = if closing { 0 } else { (nb[1] - b'0') as i32 };
                    continue;
                }
                match nm {
                    "ul" | "ol" => {
                        if !closing {
                            l.gap(if lists.is_empty() { 6 } else { 0 });
                            lists.push(if nm == "ol" { 1 } else { -1 });
                            l.indent += 24;
                        } else if !lists.is_empty() {
                            lists.pop();
                            l.indent = (l.indent - 24).max(0);
                            l.gap(if lists.is_empty() { 6 } else { 0 });
                        }
                    }
                    "li" => {
                        l.gap(if closing { 0 } else { 2 });
                        if !closing {
                            let mut mark = String::from("*");
                            if let Some(n) = lists.last_mut() {
                                if *n > 0 {
                                    mark = alloc::format!("{}.", n);
                                    *n += 1;
                                }
                            }
                            let gw = l.gw(0);
                            let len = mark.len() as i32;
                            l.items.push(Item { kind: 0, x: l.left() - (len + 1) * gw, y: l.y, w: len * gw, h: l.lh(0), style: 0, color: pal[3], image: -1, href: -1, text: mark });
                        }
                    }
                    "pre" => {
                        l.gap(8);
                        if !closing {
                            pre += 1;
                            l.indent += 12;
                        } else {
                            pre = (pre - 1).max(0);
                            l.indent = (l.indent - 12).max(0);
                        }
                    }
                    "blockquote" => {
                        l.gap(8);
                        l.indent = if closing { (l.indent - 20).max(0) } else { l.indent + 20 };
                    }
                    "dd" => {
                        l.gap(0);
                        l.indent = (l.indent + if closing { -20 } else { 20 }).max(0);
                    }
                    _ => l.gap(match nm {
                        "p" | "table" | "figure" => 10,
                        "tr" => 0,
                        _ => 2,
                    }),
                }
            }
        }
    }
    l.end_line();
    page.height = l.y + 16;
    page.items = l.items;
}

// ---- natives ------------------------------------------------------------------

fn ints(i: &Interpreter, a: &[Value], idx: usize) -> Vec<i32> {
    let Some(Value::Obj(h)) = a.get(idx) else { return Vec::new() };
    i.heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::I32(v) => v.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

fn ret_str(i: &mut Interpreter, s: &str) -> ExecResult<Option<Value>> {
    Ok(Some(Value::Obj(i.alloc_string(s))))
}

/// `HtmlLoad(string html)` -> page id.
fn html_load(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let html = string(i, a, 0);
    let mut p = parse(&html);
    let mut t = PAGES.lock();
    p.id = t.next;
    t.next += 1;
    let id = p.id;
    if t.pages.len() >= 8 {
        t.pages.remove(0); // a program that forgets HtmlFree keeps at most 8
    }
    t.pages.push(p);
    Ok(Some(Value::I64(id)))
}

/// `HtmlInfo(id, what)`: 0 title, 1 images ("src\talt\twidth\theight" lines), 2 links (lines).
fn html_info(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (id, what) = (int(a, 0), int(a, 1));
    let s = {
        let t = PAGES.lock();
        match t.pages.iter().find(|p| p.id == id) {
            None => String::new(),
            Some(p) => match what {
                0 => p.title.clone(),
                1 => {
                    let mut s = String::new();
                    for (k, (src, alt)) in p.image_info.iter().enumerate() {
                        let im = &p.images[k];
                        s.push_str(&alloc::format!("{}\t{}\t{}\t{}\n", src.replace(['\t', '\n'], " "), alt.replace(['\t', '\n'], " "), im.attr_w, im.attr_h));
                    }
                    s
                }
                _ => {
                    let mut s = String::new();
                    for l in &p.links {
                        s.push_str(&l.replace('\n', ""));
                        s.push('\n');
                    }
                    s
                }
            },
        }
    };
    ret_str(i, &s)
}

/// `HtmlSetImage(id, index, width, height)`: a loaded image's own size.
fn html_set_image(_: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (id, k) = (int(a, 0), int(a, 1).max(0) as usize);
    let mut t = PAGES.lock();
    if let Some(im) = t.pages.iter_mut().find(|p| p.id == id).and_then(|p| p.images.get_mut(k)) {
        im.nat_w = int(a, 2) as i32;
        im.nat_h = int(a, 3) as i32;
    }
    Ok(Some(Value::I32(0)))
}

/// `HtmlLayout(id, width, int[6] metrics, int[5] palette)` -> number of items
/// (the height: `HtmlInfo(id, 3)` is not needed -- item 0 of HtmlItems' tail).
fn html_layout(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (id, width) = (int(a, 0), int(a, 1) as i32);
    let mv = ints(i, a, 2);
    let pv = ints(i, a, 3);
    let mut m = [8, 16, 8, 16, 12, 24];
    for (k, v) in mv.iter().take(6).enumerate() {
        if *v > 0 {
            m[k] = *v;
        }
    }
    let mut pal = [0x1F2A24, 0x1565C0, 0x1B4D2E, 0x6B7A71, 0xD5DDD8];
    for (k, v) in pv.iter().take(5).enumerate() {
        pal[k] = *v;
    }
    let mut t = PAGES.lock();
    let Some(p) = t.pages.iter_mut().find(|p| p.id == id) else { return Ok(Some(Value::I32(-1))) };
    layout(p, width.max(100), &m, &pal);
    Ok(Some(Value::I32(p.items.len() as i32)))
}

/// `HtmlItems(id, int[] out)`: 9 ints per item (kind, x, y, w, h, style,
/// color, image index or -1, link index or -1), then the page height.
fn html_items(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let id = int(a, 0);
    let data: Vec<i32> = {
        let t = PAGES.lock();
        match t.pages.iter().find(|p| p.id == id) {
            None => Vec::new(),
            Some(p) => {
                let mut v = Vec::with_capacity(p.items.len() * 9 + 1);
                for it in &p.items {
                    v.extend_from_slice(&[it.kind, it.x, it.y, it.w, it.h, it.style, it.color, it.image, it.href]);
                }
                v.push(p.height);
                v
            }
        }
    };
    let Some(Value::Obj(h)) = a.get(1) else { return Ok(Some(Value::I32(-1))) };
    let ok = i
        .heap
        .with_mut::<ClrArray, _>(*h, |arr| match &mut arr.storage {
            ArrayStorage::I32(v) if v.len() >= data.len() => {
                v[..data.len()].copy_from_slice(&data);
                true
            }
            _ => false,
        })
        .unwrap_or(false);
    Ok(Some(Value::I32(if ok { data.len() as i32 } else { -1 })))
}

/// `HtmlTexts(id)`: every item's text, separated by U+0001.
fn html_texts(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let id = int(a, 0);
    let s = {
        let t = PAGES.lock();
        match t.pages.iter().find(|p| p.id == id) {
            None => String::new(),
            Some(p) => {
                let mut s = String::new();
                for (k, it) in p.items.iter().enumerate() {
                    if k > 0 {
                        s.push('\u{1}');
                    }
                    s.push_str(&it.text);
                }
                s
            }
        }
    };
    ret_str(i, &s)
}

fn html_free(_: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let id = int(a, 0);
    PAGES.lock().pages.retain(|p| p.id != id);
    Ok(Some(Value::I32(0)))
}

pub fn register(interp: &mut Interpreter) {
    let natives: [(&str, rustclr_core::interp::NativeFn); 7] = [
        ("HtmlLoad", html_load),
        ("HtmlInfo", html_info),
        ("HtmlSetImage", html_set_image),
        ("HtmlLayout", html_layout),
        ("HtmlItems", html_items),
        ("HtmlTexts", html_texts),
        ("HtmlFree", html_free),
    ];
    for (name, f) in natives {
        interp.register_native(alloc::format!("pinvoke:buitenzorg!{}", name), f);
    }
}
