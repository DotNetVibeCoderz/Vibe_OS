//! A small CSS engine for Peramban's page layout (`htmlpage.rs`; the C# copy
//! is `userland/clr-apps/MWeb/Css.cs` -- keep them in step).
//!
//! Style sheets come from `<style>` blocks, `<link rel=stylesheet>` files the
//! app fetches (`HtmlAddCss`) and `style=""` attributes. Selectors: `*`,
//! `tag`, `.class`, `#id`, compounds (`p.note#x`), descendant and child
//! combinators (both treated as "descendant"), comma lists; a selector with a
//! pseudo-class, attribute test or sibling combinator is ignored. The cascade
//! orders by specificity, then source order; `style=""` wins.
//!
//! Properties: `color`, `background(-color)`, `border(-*)` (a color = a
//! frame around the block), `font-weight`, `font-size` / `font` (mapped onto
//! the three kernel fonts: body, bold, heading), `display` (none / block /
//! inline), `text-align` (left / center / right), `margin(-left)` and
//! `padding(-left)` (the left indent of a block). Everything else is ignored.

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Default)]
struct Compound {
    tag: String,
    id: String,
    classes: Vec<String>,
}

pub struct Rule {
    /// compounds, the subject last
    sel: Vec<Compound>,
    spec: u32,
    order: u32,
    decls: Vec<(String, String)>,
}

/// What an element carries for selector matching.
pub struct Elem<'a> {
    pub tag: &'a str,
    pub id: &'a str,
    pub class: &'a str,
}

/// One element's computed style: inherited parts and its own box.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Style {
    // inherited
    pub color: Option<i32>,
    /// 0 none set, 1 normal, 2 bold, 3 heading-sized
    pub font: u8,
    /// 0 left, 1 center, 2 right
    pub align: u8,
    // the element's own
    pub display: u8, // 0 default, 1 none, 2 block, 3 inline
    pub background: Option<i32>,
    pub border: Option<i32>,
    pub indent: i32,
    /// `color` set on this element itself (a link keeps its link color otherwise)
    pub own_color: bool,
}

impl Style {
    /// The inherited part of a parent's style, as a child starts.
    pub fn inherit(&self) -> Style {
        Style { color: self.color, font: self.font, align: self.align, ..Style::default() }
    }
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find("/*") {
        out.push_str(&rest[..p]);
        rest = match rest[p + 2..].find("*/") {
            Some(q) => &rest[p + 2 + q + 2..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

fn compound(s: &str) -> Option<Compound> {
    let mut c = Compound::default();
    let b = s.as_bytes();
    let mut i = 0;
    let word = |i: &mut usize| {
        let st = *i;
        while *i < b.len() && (b[*i].is_ascii_alphanumeric() || b[*i] == b'-' || b[*i] == b'_') {
            *i += 1;
        }
        s[st..*i].to_ascii_lowercase()
    };
    if i < b.len() && b[i] == b'*' {
        i += 1;
    } else if i < b.len() && b[i].is_ascii_alphabetic() {
        c.tag = word(&mut i);
    }
    while i < b.len() {
        match b[i] {
            b'.' => {
                i += 1;
                let w = word(&mut i);
                if w.is_empty() {
                    return None;
                }
                c.classes.push(w);
            }
            b'#' => {
                i += 1;
                let w = word(&mut i);
                if w.is_empty() {
                    return None;
                }
                c.id = w;
            }
            _ => return None, // :hover, [attr], ...: not supported
        }
    }
    Some(c)
}

fn selector(s: &str) -> Option<(Vec<Compound>, u32)> {
    if s.contains('+') || s.contains('~') {
        return None;
    }
    let mut parts = Vec::new();
    for p in s.replace('>', " ").split_ascii_whitespace() {
        parts.push(compound(p)?);
    }
    if parts.is_empty() {
        return None;
    }
    let spec = parts.iter().map(|c| (!c.id.is_empty()) as u32 * 10000 + c.classes.len() as u32 * 100 + (!c.tag.is_empty()) as u32).sum();
    Some((parts, spec))
}

/// Declarations of a block (`a: b; c: d`), names lower case, `!important` dropped.
pub fn declarations(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for d in s.split(';') {
        let Some(c) = d.find(':') else { continue };
        let name = d[..c].trim().to_ascii_lowercase();
        let mut v = d[c + 1..].trim();
        if let Some(p) = v.to_ascii_lowercase().find("!important") {
            v = v[..p].trim();
        }
        if !name.is_empty() && !v.is_empty() {
            out.push((name, String::from(v)));
        }
    }
    out
}

/// Add every rule of a style sheet to `rules` (`order` continues across sheets).
pub fn parse_sheet(src: &str, order: &mut u32, rules: &mut Vec<Rule>) {
    let s = strip_comments(src);
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        if b[i] == b'@' {
            // @media / @font-face / @import ...: skip the statement or its whole block
            let semi = s[i..].find(';').map(|p| i + p);
            let brace = s[i..].find('{').map(|p| i + p);
            match (semi, brace) {
                (Some(sc), Some(br)) if sc < br => i = sc + 1,
                (_, Some(br)) => {
                    let mut depth = 0;
                    let mut j = br;
                    while j < b.len() {
                        if b[j] == b'{' {
                            depth += 1;
                        } else if b[j] == b'}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        j += 1;
                    }
                    i = j + 1;
                }
                (Some(sc), None) => i = sc + 1,
                _ => break,
            }
            continue;
        }
        let Some(open) = s[i..].find('{').map(|p| i + p) else { break };
        let close = s[open..].find('}').map(|p| open + p).unwrap_or(b.len());
        let decls = declarations(&s[open + 1..close]);
        if !decls.is_empty() {
            for sel in s[i..open].split(',') {
                if let Some((parts, spec)) = selector(sel.trim()) {
                    *order += 1;
                    rules.push(Rule { sel: parts, spec, order: *order, decls: decls.clone() });
                }
            }
        }
        i = close + 1;
    }
}

fn matches(c: &Compound, e: &Elem) -> bool {
    (c.tag.is_empty() || c.tag == e.tag)
        && (c.id.is_empty() || e.id.eq_ignore_ascii_case(&c.id))
        && c.classes.iter().all(|k| e.class.split_ascii_whitespace().any(|x| x.eq_ignore_ascii_case(k)))
}

fn rule_matches(r: &Rule, e: &Elem, ancestors: &[Elem]) -> bool {
    let (last, rest) = r.sel.split_last().unwrap();
    if !matches(last, e) {
        return false;
    }
    // the remaining compounds, right to left, each on some further-out ancestor
    let mut k = ancestors.len();
    for c in rest.iter().rev() {
        loop {
            if k == 0 {
                return false;
            }
            k -= 1;
            if matches(c, &ancestors[k]) {
                break;
            }
        }
    }
    true
}

const NAMES: [(&str, i32); 26] = [
    ("black", 0x000000), ("white", 0xFFFFFF), ("red", 0xFF0000), ("green", 0x008000), ("blue", 0x0000FF), ("yellow", 0xFFFF00),
    ("gray", 0x808080), ("grey", 0x808080), ("silver", 0xC0C0C0), ("maroon", 0x800000), ("navy", 0x000080), ("teal", 0x008080),
    ("olive", 0x808000), ("purple", 0x800080), ("orange", 0xFFA500), ("lime", 0x00FF00), ("aqua", 0x00FFFF), ("fuchsia", 0xFF00FF),
    ("brown", 0xA52A2A), ("pink", 0xFFC0CB), ("darkgreen", 0x006400), ("darkblue", 0x00008B), ("darkred", 0x8B0000), ("lightgray", 0xD3D3D3),
    ("lightgrey", 0xD3D3D3), ("gold", 0xFFD700),
];

fn hexv(s: &str) -> Option<i32> {
    i32::from_str_radix(s, 16).ok()
}

/// A CSS color: `#rgb`, `#rrggbb` (`#rrggbbaa` without the alpha), `rgb()` / `rgba()`, a few names.
pub fn color(v: &str) -> Option<i32> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(h) = v.strip_prefix('#') {
        return match h.len() {
            3 | 4 => {
                let d: Vec<i32> = (0..3).map(|k| hexv(&h[k..k + 1]).unwrap_or(0) * 17).collect();
                Some(d[0] << 16 | d[1] << 8 | d[2])
            }
            6 | 8 => hexv(&h[..6]),
            _ => None,
        };
    }
    if v.starts_with("rgb") {
        let inner = v.split('(').nth(1)?.trim_end_matches(')');
        let n: Vec<i32> = inner
            .split(|c: char| c == ',' || c == ' ' || c == '/')
            .filter(|x| !x.is_empty())
            .take(3)
            .map(|x| if let Some(p) = x.strip_suffix('%') { p.parse::<i32>().unwrap_or(0) * 255 / 100 } else { x.parse::<f32>().unwrap_or(0.0) as i32 })
            .collect();
        if n.len() == 3 {
            return Some(n[0].clamp(0, 255) << 16 | n[1].clamp(0, 255) << 8 | n[2].clamp(0, 255));
        }
        return None;
    }
    NAMES.iter().find(|(n, _)| *n == v).map(|(_, c)| *c)
}

/// A length in pixels (`px`, `em`/`rem` = 16 px, `pt`, `%` of 16 px); None for `auto` and friends.
pub fn length(v: &str) -> Option<i32> {
    let v = v.trim().to_ascii_lowercase();
    let end = v.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).unwrap_or(v.len());
    let n: f32 = v[..end].parse().ok()?;
    let unit = &v[end..];
    Some(match unit {
        "" | "px" => n,
        "em" | "rem" => n * 16.0,
        "pt" => n * 4.0 / 3.0,
        "%" => n * 16.0 / 100.0,
        _ => return None,
    } as i32)
}

fn font_class(px: i32) -> u8 {
    if px >= 22 {
        3
    } else if px >= 17 {
        2
    } else {
        1
    }
}

/// Apply one declaration to `st`.
pub fn apply(st: &mut Style, name: &str, value: &str) {
    let lv = value.trim().to_ascii_lowercase();
    match name {
        "color" => {
            if let Some(c) = color(&lv) {
                st.color = Some(c);
                st.own_color = true;
            }
        }
        "background" | "background-color" => {
            if lv == "none" || lv == "transparent" {
                st.background = None;
            } else if let Some(c) = lv.split_ascii_whitespace().find_map(color).or_else(|| color(&lv)) {
                st.background = Some(c);
            }
        }
        "border" | "border-top" | "border-bottom" | "border-left" | "border-right" | "border-color" => {
            if lv.contains("none") || lv == "0" || lv.starts_with("0 ") {
                st.border = None;
            } else if let Some(c) = lv.split_ascii_whitespace().find_map(color) {
                st.border = Some(c);
            } else if name == "border" && lv.contains("solid") {
                st.border = Some(0x000000);
            }
        }
        "font-weight" => {
            let bold = matches!(lv.as_str(), "bold" | "bolder") || lv.parse::<i32>().map(|n| n >= 600).unwrap_or(false);
            if st.font != 3 {
                st.font = if bold { 2 } else { 1 };
            }
        }
        "font-size" => {
            let px = match lv.as_str() {
                "xx-large" | "xxx-large" => Some(32),
                "x-large" | "larger" => Some(24),
                "large" => Some(18),
                "medium" | "small" | "x-small" | "xx-small" | "smaller" => Some(14),
                _ => length(&lv),
            };
            if let Some(px) = px {
                st.font = font_class(px).max(if st.font == 2 { 2 } else { 0 });
            }
        }
        "font" => {
            let mut f = 1;
            for t in lv.split_ascii_whitespace() {
                if t == "bold" || t == "bolder" || t.parse::<i32>().map(|n| n >= 600).unwrap_or(false) {
                    f = f.max(2);
                }
                if let Some(px) = t.split('/').next().and_then(length) {
                    if t.contains("px") || t.contains("em") || t.contains("pt") {
                        f = f.max(font_class(px));
                    }
                }
            }
            st.font = f;
        }
        "display" => {
            st.display = match lv.as_str() {
                "none" => 1,
                "block" | "flex" | "grid" | "list-item" | "table" | "flow-root" => 2,
                "inline" | "inline-block" | "inline-flex" | "contents" => 3,
                _ => st.display,
            }
        }
        "text-align" => {
            st.align = match lv.as_str() {
                "center" => 1,
                "right" | "end" => 2,
                _ => 0,
            }
        }
        "margin-left" | "padding-left" => {
            if let Some(n) = length(&lv) {
                st.indent += n.clamp(0, 200);
            }
        }
        "margin" | "padding" => {
            let p: Vec<&str> = lv.split_ascii_whitespace().collect();
            let left = match p.len() {
                1 => p[0],
                2 | 3 => p[1],
                n if n >= 4 => p[3],
                _ => "0",
            };
            if let Some(n) = length(left) {
                st.indent += n.clamp(0, 200);
            }
        }
        _ => {}
    }
}

/// The computed style of an element: the parent's inherited style, then the
/// matching rules (by specificity, then order), then its `style` attribute.
pub fn compute(parent: &Style, e: &Elem, ancestors: &[Elem], rules: &[Rule], inline: &str) -> Style {
    let mut st = parent.inherit();
    let mut hits: Vec<&Rule> = rules.iter().filter(|r| rule_matches(r, e, ancestors)).collect();
    hits.sort_by_key(|r| (r.spec, r.order));
    for r in hits {
        for (n, v) in &r.decls {
            apply(&mut st, n, v);
        }
    }
    if !inline.is_empty() {
        for (n, v) in declarations(inline) {
            apply(&mut st, &n, &v);
        }
    }
    st
}
