//! A JavaScript engine for the browser (Peramban): no_std + alloc, a
//! tree-walking interpreter over a parsed AST. It covers what page scripts
//! commonly use -- ES5 plus `let`/`const`, arrow functions, template
//! literals, `for...of`, classes are not supported -- with objects, arrays,
//! closures, prototypes (`new F()`, `F.prototype.m`), exceptions
//! (`try/catch/finally`, runtime errors are catchable `Error` objects), and
//! the usual String / Array / Math / JSON built-ins.
//!
//! The embedder supplies host objects (the page's `document` and elements)
//! through [`Host`]: property reads/writes and method calls on a handle.
//! A step budget stops runaway scripts (`while (true) {}`) without hanging
//! the browser. Not supported: regular expressions, getters/setters,
//! generators/async, labelled statements, `with`, destructuring.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

// ===== values ================================================================

#[derive(Clone)]
pub enum Value {
    Undef,
    Null,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    Obj(ObjRef),
}

pub type ObjRef = Rc<RefCell<Obj>>;

pub struct Obj {
    pub props: Vec<(Rc<str>, Value)>,
    pub proto: Option<ObjRef>,
    pub kind: Kind,
}

pub enum Kind {
    Plain,
    Array(Vec<Value>),
    Func(Func),
    Host(u64),
    Error,
}

#[derive(Clone)]
pub enum Func {
    Script(Rc<FnDef>, Env, Option<Value>), // arrow functions keep `this`
    Native(Rc<str>, Option<Value>),        // built-in, bound receiver
    HostMethod(u64, Rc<str>),
}

/// What a host hands back: a plain value, an element, or a list of them.
pub enum HostOut {
    Val(Value),
    Elem(u64),
    Elems(Vec<u64>),
}

pub trait Host {
    fn get(&mut self, handle: u64, name: &str) -> Option<HostOut>;
    /// true when the host took the write
    fn set(&mut self, handle: u64, name: &str, v: &Value) -> bool;
    fn call(&mut self, handle: u64, name: &str, args: &[Value]) -> Option<Result<HostOut, String>>;
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }
    pub fn truthy(&self) -> bool {
        match self {
            Value::Undef | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0 && !n.is_nan(),
            Value::Str(s) => !s.is_empty(),
            Value::Obj(_) => true,
        }
    }
    pub fn to_num(&self) -> f64 {
        match self {
            Value::Undef => f64::NAN,
            Value::Null => 0.0,
            Value::Bool(b) => *b as i32 as f64,
            Value::Num(n) => *n,
            Value::Str(s) => str_to_num(s),
            Value::Obj(o) => {
                let o = o.borrow();
                match &o.kind {
                    Kind::Array(a) if a.is_empty() => 0.0,
                    Kind::Array(a) if a.len() == 1 => a[0].to_num(),
                    _ => f64::NAN,
                }
            }
        }
    }
    pub fn to_str(&self) -> String {
        match self {
            Value::Undef => "undefined".into(),
            Value::Null => "null".into(),
            Value::Bool(b) => if *b { "true".into() } else { "false".into() },
            Value::Num(n) => num_to_str(*n),
            Value::Str(s) => s.to_string(),
            Value::Obj(o) => {
                let o = o.borrow();
                match &o.kind {
                    Kind::Array(a) => {
                        let parts: Vec<String> = a
                            .iter()
                            .map(|v| if matches!(v, Value::Undef | Value::Null) { String::new() } else { v.to_str() })
                            .collect();
                        parts.join(",")
                    }
                    Kind::Func(_) => "function () { [code] }".into(),
                    Kind::Error => {
                        let name = get_own(&o, "name").map(|v| v.to_str()).unwrap_or_else(|| "Error".into());
                        let msg = get_own(&o, "message").map(|v| v.to_str()).unwrap_or_default();
                        if msg.is_empty() { name } else { format!("{}: {}", name, msg) }
                    }
                    Kind::Host(_) => "[object HTMLElement]".into(),
                    Kind::Plain => "[object Object]".into(),
                }
            }
        }
    }
    fn type_of(&self) -> &'static str {
        match self {
            Value::Undef => "undefined",
            Value::Null => "object",
            Value::Bool(_) => "boolean",
            Value::Num(_) => "number",
            Value::Str(_) => "string",
            Value::Obj(o) => match o.borrow().kind {
                Kind::Func(_) => "function",
                _ => "object",
            },
        }
    }
    pub fn as_obj(&self) -> Option<&ObjRef> {
        match self {
            Value::Obj(o) => Some(o),
            _ => None,
        }
    }
}

fn get_own(o: &Obj, name: &str) -> Option<Value> {
    o.props.iter().find(|(k, _)| &**k == name).map(|(_, v)| v.clone())
}

fn set_own(o: &mut Obj, name: &str, v: Value) {
    if let Some(slot) = o.props.iter_mut().find(|(k, _)| &**k == name) {
        slot.1 = v;
    } else {
        o.props.push((Rc::from(name), v));
    }
}

pub fn num_to_str(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if n == 0.0 {
        return "0".into();
    }
    if libm::floor(n) == n && n.abs() < 1e21 {
        return format!("{}", n as i64);
    }
    // the shortest fixed form that reads back as the same number
    // (0.1 + 0.2 shows like JS: 0.30000000000000004)
    for p in 1..=17 {
        let c = format!("{:.*}", p, n);
        if c.parse::<f64>().ok() == Some(n) {
            return String::from(c.trim_end_matches('0').trim_end_matches('.'));
        }
    }
    format!("{}", n)
}

fn str_to_num(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(h, 16).map(|v| v as f64).unwrap_or(f64::NAN);
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if t.bytes().any(|b| b.is_ascii_alphabetic() && b != b'e' && b != b'E') {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

pub fn new_obj(kind: Kind, proto: Option<ObjRef>) -> ObjRef {
    Rc::new(RefCell::new(Obj { props: Vec::new(), proto, kind }))
}

// ===== lexer =================================================================

#[derive(Clone, PartialEq)]
enum T {
    Num(f64),
    Str(Rc<str>),
    Tpl(Vec<(bool, String)>), // (is expression, text)
    Id(Rc<str>),
    P(&'static str), // punctuator or keyword-ish operator
    Eof,
}

#[derive(Clone)]
struct Tok {
    t: T,
    line: u32,
    nl: bool, // a line break before it
}

const PUNCT: &[&str] = &[
    ">>>=", "===", "!==", "**=", "...", "<<=", ">>=", ">>>", "=>", "==", "!=", "<=", ">=", "&&", "||", "??", "?.", "++",
    "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "**", "<<", ">>", "{", "}", "(", ")", "[", "]", ";", ",",
    "<", ">", "+", "-", "*", "/", "%", "&", "|", "^", "!", "~", "?", ":", "=", ".",
];

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line, mut nl) = (0usize, 1u32, false);
    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            line += 1;
            nl = true;
            i += 1;
            continue;
        }
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if ch == '/' && i + 1 < c.len() && c[i + 1] == '/' {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == '/' && i + 1 < c.len() && c[i + 1] == '*' {
            i += 2;
            while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                if c[i] == '\n' {
                    line += 1;
                    nl = true;
                }
                i += 1;
            }
            i += 2;
            continue;
        }
        // HTML comment markers some pages wrap scripts in
        if ch == '<' && c[i..].starts_with(&['<', '!', '-', '-']) || ch == '-' && c[i..].starts_with(&['-', '-', '>']) && (nl || out.is_empty()) {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        let tline = line;
        let tnl = nl;
        nl = false;
        if ch.is_ascii_digit() || (ch == '.' && i + 1 < c.len() && c[i + 1].is_ascii_digit()) {
            let st = i;
            if ch == '0' && i + 1 < c.len() && (c[i + 1] == 'x' || c[i + 1] == 'X') {
                i += 2;
                while i < c.len() && c[i].is_ascii_hexdigit() {
                    i += 1;
                }
                let s: String = c[st + 2..i].iter().collect();
                out.push(Tok { t: T::Num(i64::from_str_radix(&s, 16).unwrap_or(0) as f64), line: tline, nl: tnl });
                continue;
            }
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                i += 1;
            }
            if i < c.len() && (c[i] == 'e' || c[i] == 'E') {
                i += 1;
                if i < c.len() && (c[i] == '+' || c[i] == '-') {
                    i += 1;
                }
                while i < c.len() && c[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let s: String = c[st..i].iter().collect();
            out.push(Tok { t: T::Num(s.parse().map_err(|_| format!("baris {}: angka salah '{}'", tline, s))?), line: tline, nl: tnl });
            continue;
        }
        if ch.is_alphabetic() || ch == '_' || ch == '$' {
            let st = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_' || c[i] == '$') {
                i += 1;
            }
            let s: String = c[st..i].iter().collect();
            out.push(Tok { t: T::Id(Rc::from(s.as_str())), line: tline, nl: tnl });
            continue;
        }
        if ch == '"' || ch == '\'' {
            i += 1;
            let mut s = String::new();
            while i < c.len() && c[i] != ch {
                if c[i] == '\n' {
                    return Err(format!("baris {}: string tidak ditutup", tline));
                }
                if c[i] == '\\' && i + 1 < c.len() {
                    i += 1;
                    i = escape(&c, i, &mut s);
                    continue;
                }
                s.push(c[i]);
                i += 1;
            }
            i += 1;
            out.push(Tok { t: T::Str(Rc::from(s.as_str())), line: tline, nl: tnl });
            continue;
        }
        if ch == '`' {
            i += 1;
            let mut parts = Vec::new();
            let mut s = String::new();
            while i < c.len() && c[i] != '`' {
                if c[i] == '\\' && i + 1 < c.len() {
                    i += 1;
                    i = escape(&c, i, &mut s);
                    continue;
                }
                if c[i] == '$' && i + 1 < c.len() && c[i + 1] == '{' {
                    parts.push((false, core::mem::take(&mut s)));
                    i += 2;
                    let mut depth = 1;
                    let mut e = String::new();
                    while i < c.len() {
                        if c[i] == '{' {
                            depth += 1;
                        } else if c[i] == '}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        if c[i] == '\n' {
                            line += 1;
                        }
                        e.push(c[i]);
                        i += 1;
                    }
                    i += 1;
                    parts.push((true, e));
                    continue;
                }
                if c[i] == '\n' {
                    line += 1;
                }
                s.push(c[i]);
                i += 1;
            }
            i += 1;
            parts.push((false, s));
            out.push(Tok { t: T::Tpl(parts), line: tline, nl: tnl });
            continue;
        }
        let rest: String = c[i..(i + 4).min(c.len())].iter().collect();
        match PUNCT.iter().find(|p| rest.starts_with(**p)) {
            Some(p) => {
                // `a?.5:b` is a conditional, not optional chaining
                let p = if *p == "?." && i + 2 < c.len() && c[i + 2].is_ascii_digit() { "?" } else { *p };
                i += p.len();
                out.push(Tok { t: T::P(p), line: tline, nl: tnl });
            }
            None => return Err(format!("baris {}: karakter tak dikenal '{}'", tline, ch)),
        }
    }
    out.push(Tok { t: T::Eof, line, nl: true });
    Ok(out)
}

fn escape(c: &[char], mut i: usize, s: &mut String) -> usize {
    let e = c[i];
    i += 1;
    match e {
        'n' => s.push('\n'),
        't' => s.push('\t'),
        'r' => s.push('\r'),
        'b' => s.push('\u{8}'),
        '0' => s.push('\0'),
        'x' if i + 1 < c.len() => {
            let h: String = c[i..i + 2].iter().collect();
            if let Some(ch) = u32::from_str_radix(&h, 16).ok().and_then(char::from_u32) {
                s.push(ch);
            }
            i += 2;
        }
        'u' => {
            let (h, n) = if i < c.len() && c[i] == '{' {
                let end = c[i..].iter().position(|&x| x == '}').map(|p| i + p).unwrap_or(c.len());
                (c[i + 1..end].iter().collect::<String>(), end + 1 - i)
            } else {
                (c[i..(i + 4).min(c.len())].iter().collect::<String>(), 4)
            };
            if let Some(ch) = u32::from_str_radix(&h, 16).ok().and_then(char::from_u32) {
                s.push(ch);
            }
            i += n;
        }
        '\n' => {}
        other => s.push(other),
    }
    i
}

// ===== AST ===================================================================

pub struct FnDef {
    name: Option<Rc<str>>,
    params: Vec<Rc<str>>,
    rest: Option<Rc<str>>,
    defaults: Vec<Option<Expr>>,
    body: Vec<Stmt>,
    expr: Option<Expr>, // arrow with an expression body
    arrow: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum B {
    Add, Sub, Mul, Div, Mod, Pow, Eq, Ne, SEq, SNe, Lt, Le, Gt, Ge, And, Or, Xor, Shl, Shr, UShr, In, InstanceOf,
}

enum Expr {
    Num(f64),
    Str(Rc<str>),
    Tpl(Vec<Expr>),
    Id(Rc<str>, u32),
    This,
    Array(Vec<Expr>),
    Object(Vec<(Rc<str>, Expr)>),
    Func(Rc<FnDef>),
    Unary(&'static str, Box<Expr>),
    Update(bool, bool, Box<Expr>), // increment?, prefix?, target
    Bin(B, Box<Expr>, Box<Expr>),
    Logic(&'static str, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Assign(Option<B>, Box<Expr>, Box<Expr>),
    Member(Box<Expr>, Rc<str>, bool), // optional chaining
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>, u32),
    New(Box<Expr>, Vec<Expr>),
    Spread(Box<Expr>),
    Seq(Box<Expr>, Box<Expr>),
}

enum Stmt {
    Var(bool, Vec<(Rc<str>, Option<Expr>)>), // var (function scope)?
    Func(Rc<FnDef>),
    Expr(Expr),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Expr>, Box<Stmt>),
    ForIn(bool, Rc<str>, Expr, Box<Stmt>), // of?
    Block(Vec<Stmt>),
    Return(Option<Expr>),
    Break,
    Continue,
    Throw(Expr),
    Try(Vec<Stmt>, Option<(Option<Rc<str>>, Vec<Stmt>)>, Option<Vec<Stmt>>),
    Switch(Expr, Vec<(Option<Expr>, Vec<Stmt>)>),
    Line(u32, Box<Stmt>),
    Empty,
}

// ===== parser ================================================================

struct Parser {
    t: Vec<Tok>,
    p: usize,
}

type PR<X> = Result<X, String>;

impl Parser {
    fn peek(&self) -> &T {
        &self.t[self.p].t
    }
    fn peek_at(&self, n: usize) -> &T {
        &self.t[(self.p + n).min(self.t.len() - 1)].t
    }
    fn line(&self) -> u32 {
        self.t[self.p].line
    }
    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), T::P(x) if *x == p)
    }
    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), T::Id(x) if &**x == k)
    }
    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.p += 1;
            true
        } else {
            false
        }
    }
    fn eat_kw(&mut self, k: &str) -> bool {
        if self.is_kw(k) {
            self.p += 1;
            true
        } else {
            false
        }
    }
    fn err<X>(&self, what: &str) -> PR<X> {
        let got = match self.peek() {
            T::Num(n) => num_to_str(*n),
            T::Str(s) => format!("\"{}\"", s),
            T::Tpl(_) => "`...`".into(),
            T::Id(s) => s.to_string(),
            T::P(p) => p.to_string(),
            T::Eof => "akhir skrip".into(),
        };
        Err(format!("baris {}: {} (ditemukan '{}')", self.line(), what, got))
    }
    fn expect(&mut self, p: &str) -> PR<()> {
        if self.eat(p) {
            Ok(())
        } else {
            self.err(&format!("'{}' diharapkan", p))
        }
    }
    fn ident(&mut self) -> PR<Rc<str>> {
        match self.peek().clone() {
            T::Id(s) => {
                self.p += 1;
                Ok(s)
            }
            _ => self.err("nama diharapkan"),
        }
    }
    /// end of a statement: `;`, or a line break / `}` / end (ASI)
    fn semi(&mut self) -> PR<()> {
        if self.eat(";") || self.is("}") || matches!(self.peek(), T::Eof) || self.t[self.p].nl {
            return Ok(());
        }
        self.err("';' diharapkan")
    }

    fn program(&mut self) -> PR<Vec<Stmt>> {
        let mut v = Vec::new();
        while !matches!(self.peek(), T::Eof) {
            v.push(self.stmt()?);
        }
        Ok(v)
    }

    fn block(&mut self) -> PR<Vec<Stmt>> {
        self.expect("{")?;
        let mut v = Vec::new();
        while !self.is("}") {
            if matches!(self.peek(), T::Eof) {
                return self.err("'}' diharapkan");
            }
            v.push(self.stmt()?);
        }
        self.p += 1;
        Ok(v)
    }

    fn stmt(&mut self) -> PR<Stmt> {
        let line = self.line();
        let s = self.stmt_inner()?;
        Ok(match s {
            Stmt::Func(_) | Stmt::Block(_) | Stmt::Empty => s,
            s => Stmt::Line(line, Box::new(s)),
        })
    }

    fn var_decl(&mut self) -> PR<Stmt> {
        let is_var = self.is_kw("var");
        self.p += 1;
        let mut v = Vec::new();
        loop {
            let name = self.ident()?;
            let init = if self.eat("=") { Some(self.assign()?) } else { None };
            v.push((name, init));
            if !self.eat(",") {
                break;
            }
        }
        Ok(Stmt::Var(is_var, v))
    }

    fn stmt_inner(&mut self) -> PR<Stmt> {
        if self.is("{") {
            return Ok(Stmt::Block(self.block()?));
        }
        if self.eat(";") {
            return Ok(Stmt::Empty);
        }
        if let T::Id(k) = self.peek().clone() {
            match &*k {
                "var" | "let" | "const" => {
                    let s = self.var_decl()?;
                    self.semi()?;
                    return Ok(s);
                }
                "function" if matches!(self.peek_at(1), T::Id(_)) => {
                    self.p += 1;
                    let f = self.func_rest(true)?;
                    return Ok(Stmt::Func(f));
                }
                "if" => {
                    self.p += 1;
                    self.expect("(")?;
                    let c = self.expr()?;
                    self.expect(")")?;
                    let a = self.stmt()?;
                    let b = if self.eat_kw("else") { Some(Box::new(self.stmt()?)) } else { None };
                    return Ok(Stmt::If(c, Box::new(a), b));
                }
                "while" => {
                    self.p += 1;
                    self.expect("(")?;
                    let c = self.expr()?;
                    self.expect(")")?;
                    return Ok(Stmt::While(c, Box::new(self.stmt()?)));
                }
                "do" => {
                    self.p += 1;
                    let body = self.stmt()?;
                    if !self.eat_kw("while") {
                        return self.err("'while' diharapkan");
                    }
                    self.expect("(")?;
                    let c = self.expr()?;
                    self.expect(")")?;
                    self.eat(";");
                    return Ok(Stmt::DoWhile(Box::new(body), c));
                }
                "for" => return self.for_stmt(),
                "return" => {
                    self.p += 1;
                    let v = if self.is(";") || self.is("}") || self.t[self.p].nl || matches!(self.peek(), T::Eof) {
                        None
                    } else {
                        Some(self.expr()?)
                    };
                    self.semi()?;
                    return Ok(Stmt::Return(v));
                }
                "break" => {
                    self.p += 1;
                    self.semi()?;
                    return Ok(Stmt::Break);
                }
                "continue" => {
                    self.p += 1;
                    self.semi()?;
                    return Ok(Stmt::Continue);
                }
                "throw" => {
                    self.p += 1;
                    let e = self.expr()?;
                    self.semi()?;
                    return Ok(Stmt::Throw(e));
                }
                "try" => {
                    self.p += 1;
                    let body = self.block()?;
                    let catch = if self.eat_kw("catch") {
                        let name = if self.eat("(") {
                            let n = self.ident()?;
                            self.expect(")")?;
                            Some(n)
                        } else {
                            None
                        };
                        Some((name, self.block()?))
                    } else {
                        None
                    };
                    let fin = if self.eat_kw("finally") { Some(self.block()?) } else { None };
                    if catch.is_none() && fin.is_none() {
                        return self.err("'catch' atau 'finally' diharapkan");
                    }
                    return Ok(Stmt::Try(body, catch, fin));
                }
                "switch" => {
                    self.p += 1;
                    self.expect("(")?;
                    let d = self.expr()?;
                    self.expect(")")?;
                    self.expect("{")?;
                    let mut cases = Vec::new();
                    while !self.eat("}") {
                        let test = if self.eat_kw("case") {
                            Some(self.expr()?)
                        } else if self.eat_kw("default") {
                            None
                        } else {
                            return self.err("'case' diharapkan");
                        };
                        self.expect(":")?;
                        let mut body = Vec::new();
                        while !self.is_kw("case") && !self.is_kw("default") && !self.is("}") {
                            body.push(self.stmt()?);
                        }
                        cases.push((test, body));
                    }
                    return Ok(Stmt::Switch(d, cases));
                }
                "class" => return self.err("'class' belum didukung"),
                _ => {}
            }
        }
        let e = self.expr()?;
        self.semi()?;
        Ok(Stmt::Expr(e))
    }

    fn for_stmt(&mut self) -> PR<Stmt> {
        self.p += 1;
        self.expect("(")?;
        // for (let x of xs) / for (var k in o)
        let decl = self.is_kw("var") || self.is_kw("let") || self.is_kw("const");
        let n = if decl { 1 } else { 0 };
        if let (T::Id(name), T::Id(kw)) = (self.peek_at(n).clone(), self.peek_at(n + 1).clone()) {
            if &*kw == "of" || &*kw == "in" {
                self.p += n + 2;
                let it = self.expr()?;
                self.expect(")")?;
                let body = self.stmt()?;
                return Ok(Stmt::ForIn(&*kw == "of", name, it, Box::new(body)));
            }
        }
        let init = if self.is(";") {
            None
        } else if decl {
            Some(Box::new(self.var_decl()?))
        } else {
            Some(Box::new(Stmt::Expr(self.expr()?)))
        };
        self.expect(";")?;
        let cond = if self.is(";") { None } else { Some(self.expr()?) };
        self.expect(";")?;
        let step = if self.is(")") { None } else { Some(self.expr()?) };
        self.expect(")")?;
        let body = self.stmt()?;
        Ok(Stmt::For(init, cond, step, Box::new(body)))
    }

    /// after `function`: [name] (params) { body }
    fn func_rest(&mut self, need_name: bool) -> PR<Rc<FnDef>> {
        let name = if let T::Id(n) = self.peek().clone() {
            self.p += 1;
            Some(n)
        } else if need_name {
            return self.err("nama fungsi diharapkan");
        } else {
            None
        };
        self.expect("(")?;
        let (params, defaults, rest) = self.params()?;
        let body = self.block()?;
        Ok(Rc::new(FnDef { name, params, rest, defaults, body, expr: None, arrow: false }))
    }

    fn params(&mut self) -> PR<(Vec<Rc<str>>, Vec<Option<Expr>>, Option<Rc<str>>)> {
        let (mut ps, mut ds, mut rest) = (Vec::new(), Vec::new(), None);
        while !self.eat(")") {
            if self.eat("...") {
                rest = Some(self.ident()?);
            } else {
                ps.push(self.ident()?);
                ds.push(if self.eat("=") { Some(self.assign()?) } else { None });
            }
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        Ok((ps, ds, rest))
    }

    fn expr(&mut self) -> PR<Expr> {
        let mut e = self.assign()?;
        while self.eat(",") {
            let r = self.assign()?;
            e = Expr::Seq(Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    /// is this `(a, b) =>` / `x =>`?
    fn arrow_ahead(&self) -> bool {
        match self.peek() {
            T::Id(_) => matches!(self.peek_at(1), T::P("=>")),
            T::P("(") => {
                let mut depth = 0;
                let mut i = self.p;
                while i < self.t.len() {
                    match &self.t[i].t {
                        T::P("(") => depth += 1,
                        T::P(")") => {
                            depth -= 1;
                            if depth == 0 {
                                return matches!(self.t.get(i + 1).map(|t| &t.t), Some(T::P("=>")));
                            }
                        }
                        T::Eof => return false,
                        _ => {}
                    }
                    i += 1;
                }
                false
            }
            _ => false,
        }
    }

    fn arrow(&mut self) -> PR<Expr> {
        let (params, defaults, rest) = if self.eat("(") {
            self.params()?
        } else {
            (alloc::vec![self.ident()?], alloc::vec![None], None)
        };
        self.expect("=>")?;
        let (body, expr) = if self.is("{") { (self.block()?, None) } else { (Vec::new(), Some(self.assign()?)) };
        Ok(Expr::Func(Rc::new(FnDef { name: None, params, rest, defaults, body, expr, arrow: true })))
    }

    fn assign(&mut self) -> PR<Expr> {
        if self.arrow_ahead() {
            return self.arrow();
        }
        let l = self.cond()?;
        let op = match self.peek() {
            T::P("=") => None,
            T::P("+=") => Some(B::Add),
            T::P("-=") => Some(B::Sub),
            T::P("*=") => Some(B::Mul),
            T::P("/=") => Some(B::Div),
            T::P("%=") => Some(B::Mod),
            T::P("**=") => Some(B::Pow),
            T::P("&=") => Some(B::And),
            T::P("|=") => Some(B::Or),
            T::P("^=") => Some(B::Xor),
            T::P("<<=") => Some(B::Shl),
            T::P(">>=") => Some(B::Shr),
            T::P(">>>=") => Some(B::UShr),
            _ => return Ok(l),
        };
        if !matches!(l, Expr::Id(..) | Expr::Member(..) | Expr::Index(..)) {
            return self.err("sisi kiri penugasan tidak valid");
        }
        self.p += 1;
        let r = self.assign()?;
        Ok(Expr::Assign(op, Box::new(l), Box::new(r)))
    }

    fn cond(&mut self) -> PR<Expr> {
        let c = self.logic(0)?;
        if self.eat("?") {
            let a = self.assign()?;
            self.expect(":")?;
            let b = self.assign()?;
            return Ok(Expr::Cond(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }

    fn logic(&mut self, lvl: u8) -> PR<Expr> {
        // ?? (0), || (1), && (2), then binary operators
        if lvl == 3 {
            return self.binary(0);
        }
        let op = ["??", "||", "&&"][lvl as usize];
        let mut l = self.logic(lvl + 1)?;
        while self.is(op) {
            self.p += 1;
            let r = self.logic(lvl + 1)?;
            l = Expr::Logic(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn binary(&mut self, lvl: usize) -> PR<Expr> {
        const LEVELS: &[&[(&str, B)]] = &[
            &[("|", B::Or)],
            &[("^", B::Xor)],
            &[("&", B::And)],
            &[("===", B::SEq), ("!==", B::SNe), ("==", B::Eq), ("!=", B::Ne)],
            &[("<=", B::Le), (">=", B::Ge), ("<", B::Lt), (">", B::Gt), ("in", B::In), ("instanceof", B::InstanceOf)],
            &[("<<", B::Shl), (">>>", B::UShr), (">>", B::Shr)],
            &[("+", B::Add), ("-", B::Sub)],
            &[("*", B::Mul), ("/", B::Div), ("%", B::Mod)],
        ];
        if lvl == LEVELS.len() {
            return self.pow();
        }
        let mut l = self.binary(lvl + 1)?;
        'outer: loop {
            for (s, op) in LEVELS[lvl] {
                let hit = match self.peek() {
                    T::P(p) => p == s,
                    T::Id(k) => &**k == *s && (*s == "in" || *s == "instanceof"),
                    _ => false,
                };
                if hit {
                    self.p += 1;
                    let r = self.binary(lvl + 1)?;
                    l = Expr::Bin(*op, Box::new(l), Box::new(r));
                    continue 'outer;
                }
            }
            break;
        }
        Ok(l)
    }

    fn pow(&mut self) -> PR<Expr> {
        let b = self.unary()?;
        if self.eat("**") {
            let e = self.pow()?;
            return Ok(Expr::Bin(B::Pow, Box::new(b), Box::new(e)));
        }
        Ok(b)
    }

    fn unary(&mut self) -> PR<Expr> {
        for op in ["!", "-", "+", "~"] {
            if self.is(op) {
                self.p += 1;
                let e = self.unary()?;
                return Ok(Expr::Unary(op, Box::new(e)));
            }
        }
        for kw in ["typeof", "void", "delete"] {
            if self.is_kw(kw) {
                self.p += 1;
                let e = self.unary()?;
                return Ok(Expr::Unary(kw, Box::new(e)));
            }
        }
        if self.is("++") || self.is("--") {
            let inc = self.is("++");
            self.p += 1;
            let e = self.unary()?;
            return Ok(Expr::Update(inc, true, Box::new(e)));
        }
        let e = self.postfix()?;
        if (self.is("++") || self.is("--")) && !self.t[self.p].nl {
            let inc = self.is("++");
            self.p += 1;
            return Ok(Expr::Update(inc, false, Box::new(e)));
        }
        Ok(e)
    }

    fn args(&mut self) -> PR<Vec<Expr>> {
        let mut v = Vec::new();
        while !self.eat(")") {
            if self.eat("...") {
                v.push(Expr::Spread(Box::new(self.assign()?)));
            } else {
                v.push(self.assign()?);
            }
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        Ok(v)
    }

    fn postfix(&mut self) -> PR<Expr> {
        let mut e = if self.eat_kw("new") {
            let callee = self.primary()?;
            let mut callee = callee;
            while self.eat(".") {
                let n = self.prop_name()?;
                callee = Expr::Member(Box::new(callee), n, false);
            }
            let args = if self.eat("(") { self.args()? } else { Vec::new() };
            Expr::New(Box::new(callee), args)
        } else {
            self.primary()?
        };
        loop {
            if self.eat(".") {
                let n = self.prop_name()?;
                e = Expr::Member(Box::new(e), n, false);
            } else if self.eat("?.") {
                if self.eat("(") {
                    let line = self.line();
                    let a = self.args()?;
                    e = Expr::Call(Box::new(e), a, line);
                } else {
                    let n = self.prop_name()?;
                    e = Expr::Member(Box::new(e), n, true);
                }
            } else if self.eat("[") {
                let i = self.expr()?;
                self.expect("]")?;
                e = Expr::Index(Box::new(e), Box::new(i));
            } else if self.is("(") {
                let line = self.line();
                self.p += 1;
                let a = self.args()?;
                e = Expr::Call(Box::new(e), a, line);
            } else if let T::Tpl(_) = self.peek() {
                // tagged templates are not supported; treat as an error
                return self.err("template bertag belum didukung");
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn prop_name(&mut self) -> PR<Rc<str>> {
        match self.peek().clone() {
            T::Id(s) => {
                self.p += 1;
                Ok(s)
            }
            _ => self.err("nama properti diharapkan"),
        }
    }

    fn primary(&mut self) -> PR<Expr> {
        let line = self.line();
        match self.peek().clone() {
            T::Num(n) => {
                self.p += 1;
                Ok(Expr::Num(n))
            }
            T::Str(s) => {
                self.p += 1;
                Ok(Expr::Str(s))
            }
            T::Tpl(parts) => {
                self.p += 1;
                let mut v = Vec::new();
                for (is_expr, text) in parts {
                    if is_expr {
                        let toks = lex(&text)?;
                        let mut sub = Parser { t: toks, p: 0 };
                        v.push(sub.expr()?);
                    } else if !text.is_empty() {
                        v.push(Expr::Str(Rc::from(text.as_str())));
                    }
                }
                Ok(Expr::Tpl(v))
            }
            T::Id(k) => {
                self.p += 1;
                match &*k {
                    "this" => Ok(Expr::This),
                    "function" => Ok(Expr::Func(self.func_rest(false)?)),
                    _ => Ok(Expr::Id(k, line)),
                }
            }
            T::P("(") => {
                self.p += 1;
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            T::P("[") => {
                self.p += 1;
                let mut v = Vec::new();
                while !self.eat("]") {
                    if self.is(",") {
                        self.p += 1;
                        v.push(Expr::Id(Rc::from("undefined"), line));
                        continue;
                    }
                    if self.eat("...") {
                        v.push(Expr::Spread(Box::new(self.assign()?)));
                    } else {
                        v.push(self.assign()?);
                    }
                    if !self.eat(",") {
                        self.expect("]")?;
                        break;
                    }
                }
                Ok(Expr::Array(v))
            }
            T::P("{") => {
                self.p += 1;
                let mut v = Vec::new();
                while !self.eat("}") {
                    let key: Rc<str> = match self.peek().clone() {
                        T::Id(s) | T::Str(s) => {
                            self.p += 1;
                            s
                        }
                        T::Num(n) => {
                            self.p += 1;
                            Rc::from(num_to_str(n).as_str())
                        }
                        _ => return self.err("kunci objek diharapkan"),
                    };
                    if self.eat(":") {
                        v.push((key, self.assign()?));
                    } else if self.is("(") {
                        // method shorthand: name(a) { ... }
                        self.p += 1;
                        let (params, defaults, rest) = self.params()?;
                        let body = self.block()?;
                        v.push((key.clone(), Expr::Func(Rc::new(FnDef { name: Some(key), params, rest, defaults, body, expr: None, arrow: false }))));
                    } else {
                        v.push((key.clone(), Expr::Id(key, line))); // shorthand {a}
                    }
                    if !self.eat(",") {
                        self.expect("}")?;
                        break;
                    }
                }
                Ok(Expr::Object(v))
            }
            T::P("/") => self.err("regular expression belum didukung"),
            _ => self.err("ekspresi diharapkan"),
        }
    }
}

// ===== interpreter ===========================================================

pub type Env = Rc<RefCell<Scope>>;

pub struct Scope {
    vars: Vec<(Rc<str>, Value)>,
    parent: Option<Env>,
    func: bool, // a function's top scope (`var` lands here)
}

fn scope(parent: Option<Env>, func: bool) -> Env {
    Rc::new(RefCell::new(Scope { vars: Vec::new(), parent, func }))
}

fn declare(env: &Env, name: &Rc<str>, v: Value) {
    let mut s = env.borrow_mut();
    if let Some(slot) = s.vars.iter_mut().find(|(k, _)| k == name) {
        slot.1 = v;
    } else {
        s.vars.push((name.clone(), v));
    }
}

fn lookup(env: &Env, name: &str) -> Option<Value> {
    let mut cur = env.clone();
    loop {
        let next = {
            let s = cur.borrow();
            if let Some((_, v)) = s.vars.iter().find(|(k, _)| &**k == name) {
                return Some(v.clone());
            }
            s.parent.clone()
        };
        cur = next?;
    }
}

fn assign_var(env: &Env, name: &str, v: Value) -> bool {
    let mut cur = env.clone();
    loop {
        let next = {
            let mut s = cur.borrow_mut();
            if let Some(slot) = s.vars.iter_mut().find(|(k, _)| &**k == name) {
                slot.1 = v;
                return true;
            }
            s.parent.clone()
        };
        match next {
            Some(n) => cur = n,
            None => return false,
        }
    }
}

fn func_scope(env: &Env) -> Env {
    let mut cur = env.clone();
    loop {
        let next = {
            let s = cur.borrow();
            if s.func || s.parent.is_none() {
                None
            } else {
                s.parent.clone()
            }
        };
        match next {
            Some(n) => cur = n,
            None => return cur,
        }
    }
}

pub enum Ctl {
    Ret(Value),
    Break,
    Continue,
    Throw(Value),
    Halt(String), // out of steps: not catchable
}

type R<X> = Result<X, Ctl>;

pub struct Interp {
    pub global: Env,
    steps: u64,
    pub limit: u64,
    pub output: Vec<String>,
    host_objs: BTreeMap<u64, ObjRef>,
    obj_proto: ObjRef,
    func_proto: ObjRef,
    array_proto: ObjRef,
    error_proto: ObjRef,
    line: u32,
    depth: u32,
}

impl Interp {
    pub fn new() -> Self {
        let obj_proto = new_obj(Kind::Plain, None);
        let func_proto = new_obj(Kind::Plain, Some(obj_proto.clone()));
        let array_proto = new_obj(Kind::Plain, Some(obj_proto.clone()));
        let error_proto = new_obj(Kind::Plain, Some(obj_proto.clone()));
        let mut it = Interp {
            global: scope(None, true),
            steps: 0,
            limit: 20_000_000,
            output: Vec::new(),
            host_objs: BTreeMap::new(),
            obj_proto,
            func_proto,
            array_proto,
            error_proto,
            line: 0,
            depth: 0,
        };
        it.install_globals();
        it
    }

    fn native(&self, name: &str) -> Value {
        Value::Obj(new_obj(Kind::Func(Func::Native(Rc::from(name), None)), Some(self.func_proto.clone())))
    }

    fn install_globals(&mut self) {
        let g = self.global.clone();
        let d = |n: &str, v: Value| declare(&g, &Rc::from(n), v);
        d("undefined", Value::Undef);
        d("NaN", Value::Num(f64::NAN));
        d("Infinity", Value::Num(f64::INFINITY));
        for n in ["parseInt", "parseFloat", "isNaN", "isFinite", "String", "Number", "Boolean", "Array", "Object", "Error", "TypeError", "RangeError", "alert", "setTimeout", "setInterval", "clearTimeout", "clearInterval", "encodeURIComponent", "decodeURIComponent"] {
            let f = self.native(n);
            d(n, f);
        }
        // namespaces
        let math = new_obj(Kind::Plain, Some(self.obj_proto.clone()));
        {
            let mut m = math.borrow_mut();
            for n in ["floor", "ceil", "round", "abs", "min", "max", "random", "sqrt", "pow", "sin", "cos", "tan", "atan", "atan2", "exp", "log", "trunc", "sign", "hypot", "cbrt", "asin", "acos", "log2", "log10"] {
                set_own(&mut m, n, self.native(&format!("Math.{}", n)));
            }
            set_own(&mut m, "PI", Value::Num(core::f64::consts::PI));
            set_own(&mut m, "E", Value::Num(core::f64::consts::E));
        }
        d("Math", Value::Obj(math));
        let json = new_obj(Kind::Plain, Some(self.obj_proto.clone()));
        set_own(&mut json.borrow_mut(), "stringify", self.native("JSON.stringify"));
        set_own(&mut json.borrow_mut(), "parse", self.native("JSON.parse"));
        d("JSON", Value::Obj(json));
        let console = new_obj(Kind::Plain, Some(self.obj_proto.clone()));
        for n in ["log", "info", "warn", "error"] {
            set_own(&mut console.borrow_mut(), n, self.native("console.log"));
        }
        d("console", Value::Obj(console));
        // statics hang off the constructor objects
        for (ctor, names) in [
            ("Object", &["keys", "values", "entries", "assign", "create", "freeze"][..]),
            ("Array", &["isArray", "from", "of"][..]),
            ("String", &["fromCharCode"][..]),
            ("Number", &["isInteger", "isFinite", "isNaN", "parseFloat", "parseInt"][..]),
        ] {
            if let Some(Value::Obj(c)) = lookup(&g, ctor) {
                for n in names {
                    let f = self.native(&format!("{}.{}", ctor, n));
                    set_own(&mut c.borrow_mut(), n, f);
                }
                let proto = match ctor {
                    "Object" => self.obj_proto.clone(),
                    "Array" => self.array_proto.clone(),
                    _ => new_obj(Kind::Plain, Some(self.obj_proto.clone())),
                };
                set_own(&mut c.borrow_mut(), "prototype", Value::Obj(proto));
            }
        }
        if let Some(Value::Obj(c)) = lookup(&g, "Number") {
            set_own(&mut c.borrow_mut(), "MAX_SAFE_INTEGER", Value::Num(9007199254740991.0));
            set_own(&mut c.borrow_mut(), "EPSILON", Value::Num(f64::EPSILON));
        }
        for e in ["Error", "TypeError", "RangeError"] {
            if let Some(Value::Obj(c)) = lookup(&g, e) {
                set_own(&mut c.borrow_mut(), "prototype", Value::Obj(self.error_proto.clone()));
            }
        }
    }

    /// The host object for a handle (one per handle, so `===` and custom
    /// properties work).
    pub fn host_value(&mut self, h: u64) -> Value {
        let o = self
            .host_objs
            .entry(h)
            .or_insert_with(|| new_obj(Kind::Host(h), None))
            .clone();
        Value::Obj(o)
    }

    pub fn array(&self, v: Vec<Value>) -> Value {
        Value::Obj(new_obj(Kind::Array(v), Some(self.array_proto.clone())))
    }

    pub fn object(&self) -> ObjRef {
        new_obj(Kind::Plain, Some(self.obj_proto.clone()))
    }

    fn from_host(&mut self, o: HostOut) -> Value {
        match o {
            HostOut::Val(v) => v,
            HostOut::Elem(h) => self.host_value(h),
            HostOut::Elems(hs) => {
                let v: Vec<Value> = hs.into_iter().map(|h| self.host_value(h)).collect();
                self.array(v)
            }
        }
    }

    pub fn error(&self, name: &str, msg: &str) -> Value {
        let o = new_obj(Kind::Error, Some(self.error_proto.clone()));
        set_own(&mut o.borrow_mut(), "name", Value::str(name));
        set_own(&mut o.borrow_mut(), "message", Value::str(msg));
        Value::Obj(o)
    }

    fn throw<X>(&self, name: &str, msg: &str) -> R<X> {
        let e = self.error(name, msg);
        if let Value::Obj(o) = &e {
            set_own(&mut o.borrow_mut(), "lineNumber", Value::Num(self.line as f64));
        }
        Err(Ctl::Throw(e))
    }

    fn uncaught(&self, v: &Value) -> String {
        let line = v.as_obj().and_then(|o| get_own(&o.borrow(), "lineNumber")).map(|l| l.to_num() as u32).unwrap_or(self.line);
        format!("Uncaught {} (baris {})", v.to_str(), line)
    }

    /// Define a global (the embedder's `document`, `window`, ...).
    pub fn set_global(&mut self, name: &str, v: Value) {
        declare(&self.global, &Rc::from(name), v);
    }

    pub fn get_global(&self, name: &str) -> Option<Value> {
        lookup(&self.global, name)
    }

    /// Parse and run a script in the global scope. Err = an uncaught
    /// exception or a syntax error, as text.
    pub fn run(&mut self, src: &str, host: &mut dyn Host) -> Result<Value, String> {
        let toks = lex(src)?;
        let mut p = Parser { t: toks, p: 0 };
        let prog = p.program()?;
        let env = self.global.clone();
        self.steps = 0;
        match self.exec_body(&prog, &env, &Value::Undef, host) {
            Ok(()) => Ok(Value::Undef),
            Err(Ctl::Throw(v)) => Err(self.uncaught(&v)),
            Err(Ctl::Halt(m)) => Err(m),
            Err(Ctl::Ret(v)) => Ok(v),
            Err(_) => Ok(Value::Undef),
        }
    }

    /// Call a function value (an event handler, a timer).
    pub fn call_value(&mut self, f: &Value, this: &Value, args: &[Value], host: &mut dyn Host) -> Result<Value, String> {
        self.steps = 0;
        match self.call(f, this, args.to_vec(), host) {
            Ok(v) => Ok(v),
            Err(Ctl::Throw(v)) => Err(self.uncaught(&v)),
            Err(Ctl::Halt(m)) => Err(m),
            Err(_) => Ok(Value::Undef),
        }
    }

    fn tick(&mut self) -> R<()> {
        self.steps += 1;
        if self.steps > self.limit {
            return Err(Ctl::Halt(format!("skrip dihentikan: lebih dari {} langkah (baris {})", self.limit, self.line)));
        }
        Ok(())
    }

    fn hoist(&mut self, body: &[Stmt], env: &Env) {
        for s in body {
            let s = match s {
                Stmt::Line(_, s) => &**s,
                s => s,
            };
            match s {
                Stmt::Func(f) => {
                    let v = self.make_func(f, env, None);
                    declare(env, f.name.as_ref().unwrap(), v);
                }
                Stmt::Var(true, decls) => {
                    let fs = func_scope(env);
                    for (n, _) in decls {
                        if !fs.borrow().vars.iter().any(|(k, _)| k == n) {
                            declare(&fs, n, Value::Undef);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn exec_body(&mut self, body: &[Stmt], env: &Env, this: &Value, host: &mut dyn Host) -> R<()> {
        self.hoist(body, env);
        for s in body {
            self.exec(s, env, this, host)?;
        }
        Ok(())
    }

    fn make_func(&self, f: &Rc<FnDef>, env: &Env, this: Option<Value>) -> Value {
        let o = new_obj(Kind::Func(Func::Script(f.clone(), env.clone(), this)), Some(self.func_proto.clone()));
        if !f.arrow {
            let proto = self.object();
            set_own(&mut proto.borrow_mut(), "constructor", Value::Obj(o.clone()));
            set_own(&mut o.borrow_mut(), "prototype", Value::Obj(proto));
        }
        Value::Obj(o)
    }

    fn exec(&mut self, s: &Stmt, env: &Env, this: &Value, host: &mut dyn Host) -> R<()> {
        match s {
            Stmt::Line(l, s) => {
                self.line = *l;
                self.tick()?;
                self.exec(s, env, this, host)
            }
            Stmt::Empty | Stmt::Func(_) => Ok(()),
            Stmt::Var(is_var, decls) => {
                for (n, init) in decls {
                    let v = match init {
                        Some(e) => self.eval(e, env, this, host)?,
                        None if *is_var => continue,
                        None => Value::Undef,
                    };
                    if *is_var {
                        let fs = func_scope(env);
                        declare(&fs, n, v);
                    } else {
                        declare(env, n, v);
                    }
                }
                Ok(())
            }
            Stmt::Expr(e) => {
                self.eval(e, env, this, host)?;
                Ok(())
            }
            Stmt::If(c, a, b) => {
                if self.eval(c, env, this, host)?.truthy() {
                    self.exec(a, env, this, host)
                } else if let Some(b) = b {
                    self.exec(b, env, this, host)
                } else {
                    Ok(())
                }
            }
            Stmt::Block(v) => {
                let inner = scope(Some(env.clone()), false);
                self.exec_body(v, &inner, this, host)
            }
            Stmt::While(c, body) => {
                while self.eval(c, env, this, host)?.truthy() {
                    self.tick()?;
                    match self.exec(body, env, this, host) {
                        Err(Ctl::Break) => break,
                        Err(Ctl::Continue) => continue,
                        r => r?,
                    }
                }
                Ok(())
            }
            Stmt::DoWhile(body, c) => {
                loop {
                    self.tick()?;
                    match self.exec(body, env, this, host) {
                        Err(Ctl::Break) => break,
                        Err(Ctl::Continue) => {}
                        r => r?,
                    }
                    if !self.eval(c, env, this, host)?.truthy() {
                        break;
                    }
                }
                Ok(())
            }
            Stmt::For(init, cond, step, body) => {
                let fenv = scope(Some(env.clone()), false);
                if let Some(i) = init {
                    self.exec(i, &fenv, this, host)?;
                }
                loop {
                    if let Some(c) = cond {
                        if !self.eval(c, &fenv, this, host)?.truthy() {
                            break;
                        }
                    }
                    self.tick()?;
                    // a fresh copy of the loop variables per iteration, so
                    // closures capture this round's value (`let` semantics)
                    let iter = scope(Some(env.clone()), false);
                    iter.borrow_mut().vars = fenv.borrow().vars.clone();
                    let r = self.exec(body, &iter, this, host);
                    let n = fenv.borrow().vars.len();
                    let back: Vec<(Rc<str>, Value)> = iter.borrow().vars.iter().take(n).cloned().collect();
                    fenv.borrow_mut().vars = back;
                    match r {
                        Err(Ctl::Break) => break,
                        Err(Ctl::Continue) => {}
                        r => r?,
                    }
                    if let Some(st) = step {
                        self.eval(st, &fenv, this, host)?;
                    }
                }
                Ok(())
            }
            Stmt::ForIn(of, name, it, body) => {
                let coll = self.eval(it, env, this, host)?;
                let items: Vec<Value> = if *of {
                    match &coll {
                        Value::Str(s) => s.chars().map(|c| Value::str(&c.to_string())).collect(),
                        Value::Obj(o) => match &o.borrow().kind {
                            Kind::Array(a) => a.clone(),
                            _ => return self.throw("TypeError", "objek tidak dapat diiterasi"),
                        },
                        _ => return self.throw("TypeError", "nilai tidak dapat diiterasi"),
                    }
                } else {
                    self.keys(&coll).into_iter().map(|k| Value::Str(k)).collect()
                };
                for v in items {
                    self.tick()?;
                    let iter = scope(Some(env.clone()), false);
                    declare(&iter, name, v);
                    match self.exec(body, &iter, this, host) {
                        Err(Ctl::Break) => break,
                        Err(Ctl::Continue) => {}
                        r => r?,
                    }
                }
                Ok(())
            }
            Stmt::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(e, env, this, host)?,
                    None => Value::Undef,
                };
                Err(Ctl::Ret(v))
            }
            Stmt::Break => Err(Ctl::Break),
            Stmt::Continue => Err(Ctl::Continue),
            Stmt::Throw(e) => {
                let v = self.eval(e, env, this, host)?;
                Err(Ctl::Throw(v))
            }
            Stmt::Try(body, catch, fin) => {
                let inner = scope(Some(env.clone()), false);
                let mut r = self.exec_body(body, &inner, this, host);
                if let (Err(Ctl::Throw(v)), Some((name, cbody))) = (&r, catch) {
                    let v = v.clone();
                    let cenv = scope(Some(env.clone()), false);
                    if let Some(n) = name {
                        declare(&cenv, n, v);
                    }
                    r = self.exec_body(cbody, &cenv, this, host);
                }
                if let Some(f) = fin {
                    let fenv = scope(Some(env.clone()), false);
                    self.exec_body(f, &fenv, this, host)?;
                }
                r
            }
            Stmt::Switch(d, cases) => {
                let v = self.eval(d, env, this, host)?;
                let inner = scope(Some(env.clone()), false);
                let mut start = None;
                for (i, (t, _)) in cases.iter().enumerate() {
                    if let Some(t) = t {
                        let tv = self.eval(t, &inner, this, host)?;
                        if strict_eq(&v, &tv) {
                            start = Some(i);
                            break;
                        }
                    }
                }
                if start.is_none() {
                    start = cases.iter().position(|(t, _)| t.is_none());
                }
                if let Some(st) = start {
                    for (_, body) in &cases[st..] {
                        for s in body {
                            match self.exec(s, &inner, this, host) {
                                Err(Ctl::Break) => return Ok(()),
                                r => r?,
                            }
                        }
                    }
                }
                Ok(())
            }
        }
    }

    fn keys(&self, v: &Value) -> Vec<Rc<str>> {
        match v {
            Value::Obj(o) => {
                let o = o.borrow();
                let mut out: Vec<Rc<str>> = Vec::new();
                if let Kind::Array(a) = &o.kind {
                    for i in 0..a.len() {
                        out.push(Rc::from(i.to_string().as_str()));
                    }
                }
                for (k, _) in &o.props {
                    out.push(k.clone());
                }
                out
            }
            Value::Str(s) => (0..s.chars().count()).map(|i| Rc::from(i.to_string().as_str())).collect(),
            _ => Vec::new(),
        }
    }

    fn eval(&mut self, e: &Expr, env: &Env, this: &Value, host: &mut dyn Host) -> R<Value> {
        Ok(match e {
            Expr::Num(n) => Value::Num(*n),
            Expr::Str(s) => Value::Str(s.clone()),
            Expr::Tpl(parts) => {
                let mut s = String::new();
                for p in parts {
                    let v = self.eval(p, env, this, host)?;
                    s.push_str(&v.to_str());
                }
                Value::str(&s)
            }
            Expr::Id(n, line) => match &**n {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "null" => Value::Null,
                "undefined" => Value::Undef,
                _ => match lookup(env, n) {
                    Some(v) => v,
                    None => {
                        self.line = *line;
                        return self.throw("ReferenceError", &format!("{} tidak didefinisikan", n));
                    }
                },
            },
            Expr::This => this.clone(),
            Expr::Array(items) => {
                let mut v = Vec::new();
                for i in items {
                    if let Expr::Spread(s) = i {
                        let sv = self.eval(s, env, this, host)?;
                        v.extend(self.spread(&sv)?);
                    } else {
                        v.push(self.eval(i, env, this, host)?);
                    }
                }
                self.array(v)
            }
            Expr::Object(props) => {
                let o = self.object();
                for (k, e) in props {
                    let v = self.eval(e, env, this, host)?;
                    set_own(&mut o.borrow_mut(), k, v);
                }
                Value::Obj(o)
            }
            Expr::Func(f) => self.make_func(f, env, if f.arrow { Some(this.clone()) } else { None }),
            Expr::Spread(_) => return self.throw("SyntaxError", "'...' di tempat yang salah"),
            Expr::Seq(a, b) => {
                self.eval(a, env, this, host)?;
                self.eval(b, env, this, host)?
            }
            Expr::Unary(op, x) => {
                if *op == "typeof" {
                    if let Expr::Id(n, _) = &**x {
                        if !matches!(&**n, "true" | "false" | "null" | "undefined") && lookup(env, n).is_none() {
                            return Ok(Value::str("undefined"));
                        }
                    }
                    let v = self.eval(x, env, this, host)?;
                    return Ok(Value::str(v.type_of()));
                }
                if *op == "delete" {
                    match &**x {
                        Expr::Member(o, n, _) => {
                            if let Value::Obj(o) = self.eval(o, env, this, host)? {
                                o.borrow_mut().props.retain(|(k, _)| k != n);
                            }
                        }
                        Expr::Index(o, i) => {
                            let ov = self.eval(o, env, this, host)?;
                            let k = self.eval(i, env, this, host)?.to_str();
                            if let Value::Obj(o) = ov {
                                o.borrow_mut().props.retain(|(kk, _)| **kk != *k);
                            }
                        }
                        _ => {}
                    }
                    return Ok(Value::Bool(true));
                }
                let v = self.eval(x, env, this, host)?;
                match *op {
                    "!" => Value::Bool(!v.truthy()),
                    "-" => Value::Num(-v.to_num()),
                    "+" => Value::Num(v.to_num()),
                    "~" => Value::Num(!(to_i32(v.to_num())) as f64),
                    _ => Value::Undef, // void
                }
            }
            Expr::Update(inc, prefix, target) => {
                let old = self.eval(target, env, this, host)?.to_num();
                let new = if *inc { old + 1.0 } else { old - 1.0 };
                self.store(target, Value::Num(new), env, this, host)?;
                Value::Num(if *prefix { new } else { old })
            }
            Expr::Bin(op, a, b) => {
                let x = self.eval(a, env, this, host)?;
                let y = self.eval(b, env, this, host)?;
                self.binop(*op, &x, &y, host)?
            }
            Expr::Logic(op, a, b) => {
                let x = self.eval(a, env, this, host)?;
                match *op {
                    "&&" => if x.truthy() { self.eval(b, env, this, host)? } else { x },
                    "||" => if x.truthy() { x } else { self.eval(b, env, this, host)? },
                    _ => if matches!(x, Value::Undef | Value::Null) { self.eval(b, env, this, host)? } else { x },
                }
            }
            Expr::Cond(c, a, b) => {
                if self.eval(c, env, this, host)?.truthy() {
                    self.eval(a, env, this, host)?
                } else {
                    self.eval(b, env, this, host)?
                }
            }
            Expr::Assign(op, target, value) => {
                let v = match op {
                    None => self.eval(value, env, this, host)?,
                    Some(op) => {
                        let old = self.eval(target, env, this, host)?;
                        let r = self.eval(value, env, this, host)?;
                        self.binop(*op, &old, &r, host)?
                    }
                };
                self.store(target, v.clone(), env, this, host)?;
                v
            }
            Expr::Member(o, n, opt) => {
                let ov = self.eval(o, env, this, host)?;
                if *opt && matches!(ov, Value::Undef | Value::Null) {
                    return Ok(Value::Undef);
                }
                self.get(&ov, n, host)?
            }
            Expr::Index(o, i) => {
                let ov = self.eval(o, env, this, host)?;
                let iv = self.eval(i, env, this, host)?;
                self.get_index(&ov, &iv, host)?
            }
            Expr::Call(callee, args, line) => {
                let (f, recv) = match &**callee {
                    Expr::Member(o, n, opt) => {
                        let ov = self.eval(o, env, this, host)?;
                        if *opt && matches!(ov, Value::Undef | Value::Null) {
                            return Ok(Value::Undef);
                        }
                        (self.get(&ov, n, host)?, ov)
                    }
                    Expr::Index(o, i) => {
                        let ov = self.eval(o, env, this, host)?;
                        let iv = self.eval(i, env, this, host)?;
                        (self.get_index(&ov, &iv, host)?, ov)
                    }
                    other => (self.eval(other, env, this, host)?, Value::Undef),
                };
                let argv = self.eval_args(args, env, this, host)?;
                self.line = *line;
                if !is_callable(&f) {
                    let what = match &**callee {
                        Expr::Member(_, n, _) => n.to_string(),
                        Expr::Id(n, _) => n.to_string(),
                        _ => "ekspresi".into(),
                    };
                    return self.throw("TypeError", &format!("{} bukan fungsi", what));
                }
                self.call(&f, &recv, argv, host)?
            }
            Expr::New(callee, args) => {
                let f = self.eval(callee, env, this, host)?;
                let argv = self.eval_args(args, env, this, host)?;
                self.construct(&f, argv, host)?
            }
        })
    }

    fn eval_args(&mut self, args: &[Expr], env: &Env, this: &Value, host: &mut dyn Host) -> R<Vec<Value>> {
        let mut v = Vec::with_capacity(args.len());
        for a in args {
            if let Expr::Spread(s) = a {
                let sv = self.eval(s, env, this, host)?;
                v.extend(self.spread(&sv)?);
            } else {
                v.push(self.eval(a, env, this, host)?);
            }
        }
        Ok(v)
    }

    fn spread(&self, v: &Value) -> R<Vec<Value>> {
        match v {
            Value::Obj(o) => match &o.borrow().kind {
                Kind::Array(a) => Ok(a.clone()),
                _ => self.throw("TypeError", "hanya array yang bisa di-spread"),
            },
            Value::Str(s) => Ok(s.chars().map(|c| Value::str(&c.to_string())).collect()),
            _ => self.throw("TypeError", "nilai tidak bisa di-spread"),
        }
    }

    fn store(&mut self, target: &Expr, v: Value, env: &Env, this: &Value, host: &mut dyn Host) -> R<()> {
        match target {
            Expr::Id(n, _) => {
                if !assign_var(env, n, v.clone()) {
                    declare(&self.global, n, v); // sloppy mode: an implicit global
                }
                Ok(())
            }
            Expr::Member(o, n, _) => {
                let ov = self.eval(o, env, this, host)?;
                self.put(&ov, n, v, host)
            }
            Expr::Index(o, i) => {
                let ov = self.eval(o, env, this, host)?;
                let iv = self.eval(i, env, this, host)?;
                if let Value::Obj(obj) = &ov {
                    if let Kind::Array(a) = &mut obj.borrow_mut().kind {
                        if let Value::Num(n) = iv {
                            if n >= 0.0 && libm::floor(n) == n && n < 1e7 {
                                let i = n as usize;
                                if i >= a.len() {
                                    a.resize(i + 1, Value::Undef);
                                }
                                a[i] = v;
                                return Ok(());
                            }
                        }
                    }
                }
                let k = iv.to_str();
                self.put(&ov, &k, v, host)
            }
            _ => self.throw("SyntaxError", "sisi kiri penugasan tidak valid"),
        }
    }

    pub fn put(&mut self, o: &Value, name: &str, v: Value, host: &mut dyn Host) -> R<()> {
        match o {
            Value::Obj(obj) => {
                let h = if let Kind::Host(h) = obj.borrow().kind { Some(h) } else { None };
                if let Some(h) = h {
                    if host.set(h, name, &v) {
                        return Ok(());
                    }
                }
                let mut b = obj.borrow_mut();
                if let Kind::Array(a) = &mut b.kind {
                    if name == "length" {
                        let n = v.to_num().max(0.0) as usize;
                        a.resize(n, Value::Undef);
                        return Ok(());
                    }
                }
                set_own(&mut b, name, v);
                Ok(())
            }
            Value::Undef | Value::Null => self.throw("TypeError", &format!("tidak bisa menulis '{}' pada {}", name, o.to_str())),
            _ => Ok(()), // primitives silently drop writes
        }
    }

    fn get_index(&mut self, o: &Value, i: &Value, host: &mut dyn Host) -> R<Value> {
        if let Value::Num(n) = i {
            let n = *n;
            if n >= 0.0 && libm::floor(n) == n {
                match o {
                    Value::Obj(obj) => {
                        if let Kind::Array(a) = &obj.borrow().kind {
                            return Ok(a.get(n as usize).cloned().unwrap_or(Value::Undef));
                        }
                    }
                    Value::Str(s) => return Ok(s.chars().nth(n as usize).map(|c| Value::str(&c.to_string())).unwrap_or(Value::Undef)),
                    _ => {}
                }
            }
        }
        let k = i.to_str();
        self.get(o, &k, host)
    }

    pub fn get(&mut self, o: &Value, name: &str, host: &mut dyn Host) -> R<Value> {
        match o {
            Value::Undef | Value::Null => self.throw("TypeError", &format!("tidak bisa membaca '{}' dari {}", name, o.to_str())),
            Value::Str(s) => {
                if name == "length" {
                    return Ok(Value::Num(s.chars().count() as f64));
                }
                if let Ok(i) = name.parse::<usize>() {
                    return Ok(s.chars().nth(i).map(|c| Value::str(&c.to_string())).unwrap_or(Value::Undef));
                }
                Ok(self.method("String", name, o))
            }
            Value::Num(_) => Ok(self.method("Number", name, o)),
            Value::Bool(_) => Ok(self.method("Boolean", name, o)),
            Value::Obj(obj) => {
                let host_h = if let Kind::Host(h) = obj.borrow().kind { Some(h) } else { None };
                if let Some(h) = host_h {
                    if let Some(own) = get_own(&obj.borrow(), name) {
                        return Ok(own);
                    }
                    if let Some(v) = host.get(h, name) {
                        return Ok(self.from_host(v));
                    }
                    let f = Func::HostMethod(h, Rc::from(name));
                    return Ok(Value::Obj(new_obj(Kind::Func(f), Some(self.func_proto.clone()))));
                }
                {
                    let b = obj.borrow();
                    if let Kind::Array(a) = &b.kind {
                        if name == "length" {
                            return Ok(Value::Num(a.len() as f64));
                        }
                        if let Ok(i) = name.parse::<usize>() {
                            return Ok(a.get(i).cloned().unwrap_or(Value::Undef));
                        }
                    }
                    if let Kind::Func(Func::Script(f, ..)) = &b.kind {
                        if name == "name" {
                            return Ok(Value::str(f.name.as_deref().unwrap_or("")));
                        }
                        if name == "length" {
                            return Ok(Value::Num(f.params.len() as f64));
                        }
                    }
                }
                // own, then the prototype chain
                let mut cur = Some(obj.clone());
                let mut hops = 0;
                while let Some(c) = cur {
                    if let Some(v) = get_own(&c.borrow(), name) {
                        return Ok(v);
                    }
                    cur = c.borrow().proto.clone();
                    hops += 1;
                    if hops > 64 {
                        break;
                    }
                }
                let kind = match &obj.borrow().kind {
                    Kind::Array(_) => "Array",
                    Kind::Func(_) => "Function",
                    _ => "Object",
                };
                Ok(self.method(kind, name, o))
            }
        }
    }

    /// A built-in method bound to its receiver, or undefined.
    fn method(&self, kind: &str, name: &str, recv: &Value) -> Value {
        let key = format!("{}#{}", kind, name);
        if !METHODS.contains(&key.as_str()) && !(kind != "Object" && METHODS.contains(&format!("Object#{}", name).as_str())) {
            return Value::Undef;
        }
        let key = if METHODS.contains(&key.as_str()) { key } else { format!("Object#{}", name) };
        Value::Obj(new_obj(Kind::Func(Func::Native(Rc::from(key.as_str()), Some(recv.clone()))), Some(self.func_proto.clone())))
    }

    fn binop(&mut self, op: B, x: &Value, y: &Value, host: &mut dyn Host) -> R<Value> {
        let _ = host;
        Ok(match op {
            B::Add => {
                let (px, py) = (to_prim(x), to_prim(y));
                if matches!(px, Value::Str(_)) || matches!(py, Value::Str(_)) {
                    let mut s = px.to_str();
                    s.push_str(&py.to_str());
                    Value::str(&s)
                } else {
                    Value::Num(px.to_num() + py.to_num())
                }
            }
            B::Sub => Value::Num(x.to_num() - y.to_num()),
            B::Mul => Value::Num(x.to_num() * y.to_num()),
            B::Div => Value::Num(x.to_num() / y.to_num()),
            B::Mod => {
                let (a, b) = (x.to_num(), y.to_num());
                Value::Num(libm::fmod(a, b))
            }
            B::Pow => Value::Num(libm::pow(x.to_num(), y.to_num())),
            B::SEq => Value::Bool(strict_eq(x, y)),
            B::SNe => Value::Bool(!strict_eq(x, y)),
            B::Eq => Value::Bool(loose_eq(x, y)),
            B::Ne => Value::Bool(!loose_eq(x, y)),
            B::Lt | B::Le | B::Gt | B::Ge => {
                let (px, py) = (to_prim(x), to_prim(y));
                let ord = if let (Value::Str(a), Value::Str(b)) = (&px, &py) {
                    Some(a.cmp(b))
                } else {
                    px.to_num().partial_cmp(&py.to_num())
                };
                use core::cmp::Ordering::*;
                Value::Bool(match (op, ord) {
                    (_, None) => false,
                    (B::Lt, Some(o)) => o == Less,
                    (B::Le, Some(o)) => o != Greater,
                    (B::Gt, Some(o)) => o == Greater,
                    (_, Some(o)) => o != Less,
                })
            }
            B::And => Value::Num((to_i32(x.to_num()) & to_i32(y.to_num())) as f64),
            B::Or => Value::Num((to_i32(x.to_num()) | to_i32(y.to_num())) as f64),
            B::Xor => Value::Num((to_i32(x.to_num()) ^ to_i32(y.to_num())) as f64),
            B::Shl => Value::Num(to_i32(x.to_num()).wrapping_shl(to_i32(y.to_num()) as u32 & 31) as f64),
            B::Shr => Value::Num((to_i32(x.to_num()) >> (to_i32(y.to_num()) as u32 & 31)) as f64),
            B::UShr => Value::Num(((to_i32(x.to_num()) as u32) >> (to_i32(y.to_num()) as u32 & 31)) as f64),
            B::In => {
                let k = x.to_str();
                Value::Bool(match y {
                    Value::Obj(o) => {
                        let b = o.borrow();
                        get_own(&b, &k).is_some()
                            || matches!(&b.kind, Kind::Array(a) if k.parse::<usize>().map(|i| i < a.len()).unwrap_or(false) || k == "length")
                    }
                    _ => return self.throw("TypeError", "'in' butuh objek"),
                })
            }
            B::InstanceOf => {
                let proto = match y {
                    Value::Obj(c) => get_own(&c.borrow(), "prototype"),
                    _ => None,
                };
                let mut hit = false;
                if let (Some(Value::Obj(p)), Value::Obj(o)) = (proto, x) {
                    let mut cur = o.borrow().proto.clone();
                    while let Some(c) = cur {
                        if Rc::ptr_eq(&c, &p) {
                            hit = true;
                            break;
                        }
                        cur = c.borrow().proto.clone();
                    }
                    if !hit {
                        if let Kind::Array(_) = o.borrow().kind {
                            hit = Rc::ptr_eq(&p, &self.array_proto) || Rc::ptr_eq(&p, &self.obj_proto);
                        }
                    }
                }
                Value::Bool(hit)
            }
        })
    }

    fn construct(&mut self, f: &Value, args: Vec<Value>, host: &mut dyn Host) -> R<Value> {
        let Value::Obj(fo) = f else { return self.throw("TypeError", "bukan konstruktor") };
        let native = if let Kind::Func(Func::Native(n, None)) = &fo.borrow().kind { Some(n.clone()) } else { None };
        if let Some(n) = native {
            return match &*n {
                "Error" | "TypeError" | "RangeError" => Ok(self.error(&n, &args.first().map(|v| v.to_str()).unwrap_or_default())),
                "Array" => Ok(if args.len() == 1 {
                    if let Value::Num(n) = args[0] {
                        self.array(alloc::vec![Value::Undef; n.max(0.0) as usize])
                    } else {
                        self.array(args)
                    }
                } else {
                    self.array(args)
                }),
                "Object" => Ok(Value::Obj(self.object())),
                _ => self.call(f, &Value::Undef, args, host),
            };
        }
        let proto = match get_own(&fo.borrow(), "prototype") {
            Some(Value::Obj(p)) => p,
            _ => self.obj_proto.clone(),
        };
        let obj = Value::Obj(new_obj(Kind::Plain, Some(proto)));
        let r = self.call(f, &obj, args, host)?;
        Ok(if matches!(r, Value::Obj(_)) { r } else { obj })
    }

    pub fn call(&mut self, f: &Value, this: &Value, args: Vec<Value>, host: &mut dyn Host) -> R<Value> {
        let Value::Obj(fo) = f else { return self.throw("TypeError", "bukan fungsi") };
        let func = match &fo.borrow().kind {
            Kind::Func(f) => f.clone(),
            _ => return self.throw("TypeError", "bukan fungsi"),
        };
        match func {
            Func::Native(name, bound) => {
                let recv = bound.unwrap_or_else(|| this.clone());
                self.builtin(&name, &recv, args, host)
            }
            Func::HostMethod(h, name) => match host.call(h, &name, &args) {
                Some(Ok(v)) => Ok(self.from_host(v)),
                Some(Err(e)) => self.throw("Error", &e),
                None => self.throw("TypeError", &format!("{} bukan fungsi", name)),
            },
            Func::Script(def, cenv, arrow_this) => {
                self.depth += 1;
                if self.depth > 200 {
                    self.depth -= 1;
                    return self.throw("RangeError", "rekursi terlalu dalam");
                }
                let env = scope(Some(cenv), true);
                let mut rest = args.into_iter();
                for (i, p) in def.params.iter().enumerate() {
                    let mut v = rest.next().unwrap_or(Value::Undef);
                    if matches!(v, Value::Undef) {
                        if let Some(Some(d)) = def.defaults.get(i) {
                            let t = arrow_this.clone().unwrap_or_else(|| this.clone());
                            v = match self.eval(d, &env, &t, host) {
                                Ok(v) => v,
                                Err(e) => {
                                    self.depth -= 1;
                                    return Err(e);
                                }
                            };
                        }
                    }
                    declare(&env, p, v);
                }
                if let Some(r) = &def.rest {
                    let v: Vec<Value> = rest.collect();
                    let a = self.array(v);
                    declare(&env, r, a);
                }
                if let Some(n) = &def.name {
                    if !def.arrow && lookup(&env, n).is_none() {
                        declare(&env, n, f.clone());
                    }
                }
                let t = arrow_this.unwrap_or_else(|| this.clone());
                let r = if let Some(e) = &def.expr {
                    self.eval(e, &env, &t, host)
                } else {
                    match self.exec_body(&def.body, &env, &t, host) {
                        Ok(()) => Ok(Value::Undef),
                        Err(Ctl::Ret(v)) => Ok(v),
                        Err(Ctl::Break) | Err(Ctl::Continue) => Ok(Value::Undef),
                        Err(e) => Err(e),
                    }
                };
                self.depth -= 1;
                r
            }
        }
    }

    fn arr_of(&self, v: &Value) -> Option<ObjRef> {
        match v {
            Value::Obj(o) if matches!(o.borrow().kind, Kind::Array(_)) => Some(o.clone()),
            _ => None,
        }
    }

    fn arr_items(&self, v: &Value) -> Vec<Value> {
        match v {
            Value::Obj(o) => match &o.borrow().kind {
                Kind::Array(a) => a.clone(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn builtin(&mut self, name: &str, recv: &Value, args: Vec<Value>, host: &mut dyn Host) -> R<Value> {
        let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Undef);
        let num = |i: usize| args.get(i).map(|v| v.to_num()).unwrap_or(f64::NAN);
        // ---- global functions
        match name {
            "parseInt" => {
                let s = a(0).to_str();
                let t = s.trim();
                let radix = if args.len() > 1 { num(1) as u32 } else { 10 };
                let (neg, t) = match t.strip_prefix('-') {
                    Some(r) => (true, r),
                    None => (false, t.strip_prefix('+').unwrap_or(t)),
                };
                let (radix, t) = if (radix == 16 || radix == 0 || args.len() < 2) && (t.starts_with("0x") || t.starts_with("0X")) {
                    (16, &t[2..])
                } else {
                    (if radix == 0 { 10 } else { radix }, t)
                };
                let digits: String = t.chars().take_while(|c| c.is_digit(radix)).collect();
                if digits.is_empty() {
                    return Ok(Value::Num(f64::NAN));
                }
                let v = i64::from_str_radix(&digits, radix).map(|v| v as f64).unwrap_or(f64::NAN);
                return Ok(Value::Num(if neg { -v } else { v }));
            }
            "parseFloat" | "Number.parseFloat" => {
                let s = a(0).to_str();
                let t = s.trim();
                let mut end = 0;
                let b = t.as_bytes();
                let mut seen_dot = false;
                let mut seen_e = false;
                while end < b.len() {
                    let c = b[end];
                    if c.is_ascii_digit() || (end == 0 && (c == b'-' || c == b'+')) {
                    } else if c == b'.' && !seen_dot && !seen_e {
                        seen_dot = true;
                    } else if (c == b'e' || c == b'E') && !seen_e && end > 0 {
                        seen_e = true;
                    } else if (c == b'-' || c == b'+') && seen_e && (b[end - 1] == b'e' || b[end - 1] == b'E') {
                    } else {
                        break;
                    }
                    end += 1;
                }
                let mut s = &t[..end];
                while !s.is_empty() && s.parse::<f64>().is_err() {
                    s = &s[..s.len() - 1];
                }
                return Ok(Value::Num(s.parse::<f64>().unwrap_or(f64::NAN)));
            }
            "Number.parseInt" => return self.builtin("parseInt", recv, args, host),
            "isNaN" => return Ok(Value::Bool(num(0).is_nan())),
            "isFinite" => return Ok(Value::Bool(num(0).is_finite())),
            "Number.isNaN" => return Ok(Value::Bool(matches!(a(0), Value::Num(n) if n.is_nan()))),
            "Number.isFinite" => return Ok(Value::Bool(matches!(a(0), Value::Num(n) if n.is_finite()))),
            "Number.isInteger" => return Ok(Value::Bool(matches!(a(0), Value::Num(n) if n.is_finite() && libm::floor(n) == n))),
            "String" => return Ok(Value::str(&if args.is_empty() { String::new() } else { a(0).to_str() })),
            "Number" => return Ok(Value::Num(if args.is_empty() { 0.0 } else { num(0) })),
            "Boolean" => return Ok(Value::Bool(a(0).truthy())),
            "Array" | "Object" | "Error" | "TypeError" | "RangeError" => {
                let f = self.native(name);
                return self.construct(&f, args, host);
            }
            "console.log" => {
                let parts: Vec<String> = args.iter().map(|v| self.inspect(v, 0)).collect();
                self.output.push(parts.join(" "));
                return Ok(Value::Undef);
            }
            "alert" => {
                self.output.push(format!("[alert] {}", a(0).to_str()));
                return Ok(Value::Undef);
            }
            "encodeURIComponent" => {
                let mut s = String::new();
                for b in a(0).to_str().bytes() {
                    if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
                        s.push(b as char);
                    } else {
                        s.push_str(&format!("%{:02X}", b));
                    }
                }
                return Ok(Value::str(&s));
            }
            "decodeURIComponent" => {
                let s = a(0).to_str();
                let b = s.as_bytes();
                let mut out = Vec::new();
                let mut i = 0;
                while i < b.len() {
                    if b[i] == b'%' && i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 {
                        if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                            out.push(v);
                            i += 3;
                            continue;
                        }
                    }
                    out.push(b[i]);
                    i += 1;
                }
                return Ok(Value::str(&String::from_utf8_lossy(&out)));
            }
            "setTimeout" | "setInterval" | "clearTimeout" | "clearInterval" => {
                // timers belong to the embedder: it reads them via `host.call(0, ...)`
                let r = host.call(0, name, &args);
                return match r {
                    Some(Ok(v)) => Ok(self.from_host(v)),
                    Some(Err(e)) => self.throw("Error", &e),
                    None => Ok(Value::Undef),
                };
            }
            "JSON.stringify" => {
                let mut s = String::new();
                let indent = match a(2) {
                    Value::Num(n) => n as usize,
                    Value::Str(s) => s.len(),
                    _ => 0,
                };
                if !self.json_write(&a(0), &mut s, indent, 0) {
                    return Ok(Value::Undef);
                }
                return Ok(Value::str(&s));
            }
            "JSON.parse" => {
                let s = a(0).to_str();
                let c: Vec<char> = s.chars().collect();
                let mut i = 0;
                return match self.json_read(&c, &mut i) {
                    Some(v) => Ok(v),
                    None => self.throw("SyntaxError", "JSON tidak valid"),
                };
            }
            "Object.keys" => {
                let k: Vec<Value> = self.keys(&a(0)).into_iter().map(Value::Str).collect();
                return Ok(self.array(k));
            }
            "Object.values" | "Object.entries" => {
                let o = a(0);
                let mut out = Vec::new();
                for k in self.keys(&o) {
                    let v = self.get(&o, &k, host)?;
                    out.push(if name == "Object.values" { v } else { self.array(alloc::vec![Value::Str(k), v]) });
                }
                return Ok(self.array(out));
            }
            "Object.assign" => {
                let t = a(0);
                for src in args.iter().skip(1) {
                    for k in self.keys(src) {
                        let v = self.get(src, &k, host)?;
                        self.put(&t, &k, v, host)?;
                    }
                }
                return Ok(t);
            }
            "Object.create" => {
                let proto = a(0).as_obj().cloned();
                return Ok(Value::Obj(new_obj(Kind::Plain, proto)));
            }
            "Object.freeze" => return Ok(a(0)),
            "Array.isArray" => return Ok(Value::Bool(self.arr_of(&a(0)).is_some())),
            "Array.of" => return Ok(self.array(args)),
            "Array.from" => {
                let src = a(0);
                let items = match &src {
                    Value::Str(s) => s.chars().map(|c| Value::str(&c.to_string())).collect(),
                    Value::Obj(o) => {
                        let arr = if let Kind::Array(x) = &o.borrow().kind { Some(x.clone()) } else { None };
                        match arr {
                            Some(x) => x,
                            None => {
                                let n = self.get(&src, "length", host)?.to_num();
                                (0..(if n.is_finite() { n as usize } else { 0 })).map(|_| Value::Undef).collect()
                            }
                        }
                    }
                    _ => Vec::new(),
                };
                if args.len() > 1 {
                    let f = a(1);
                    let mut out = Vec::new();
                    for (i, v) in items.into_iter().enumerate() {
                        out.push(self.call(&f, &Value::Undef, alloc::vec![v, Value::Num(i as f64)], host)?);
                    }
                    return Ok(self.array(out));
                }
                return Ok(self.array(items));
            }
            "String.fromCharCode" => {
                let s: String = args.iter().filter_map(|v| char::from_u32(v.to_num() as u32)).collect();
                return Ok(Value::str(&s));
            }
            _ => {}
        }
        if let Some(m) = name.strip_prefix("Math.") {
            let x = num(0);
            return Ok(Value::Num(match m {
                "floor" => libm::floor(x),
                "ceil" => libm::ceil(x),
                "round" => libm::floor(x + 0.5),
                "trunc" => libm::trunc(x),
                "abs" => x.abs(),
                "sign" => if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { x },
                "sqrt" => libm::sqrt(x),
                "cbrt" => libm::cbrt(x),
                "pow" => libm::pow(x, num(1)),
                "sin" => libm::sin(x),
                "cos" => libm::cos(x),
                "tan" => libm::tan(x),
                "asin" => libm::asin(x),
                "acos" => libm::acos(x),
                "atan" => libm::atan(x),
                "atan2" => libm::atan2(x, num(1)),
                "exp" => libm::exp(x),
                "log" => libm::log(x),
                "log2" => libm::log2(x),
                "log10" => libm::log10(x),
                "hypot" => libm::hypot(x, num(1)),
                "min" => args.iter().map(|v| v.to_num()).fold(f64::INFINITY, |m, v| if v.is_nan() || m.is_nan() { f64::NAN } else { m.min(v) }),
                "max" => args.iter().map(|v| v.to_num()).fold(f64::NEG_INFINITY, |m, v| if v.is_nan() || m.is_nan() { f64::NAN } else { m.max(v) }),
                "random" => random(),
                _ => f64::NAN,
            }));
        }
        let (kind, m) = name.split_once('#').unwrap_or(("", name));
        match kind {
            "String" => self.string_method(m, &recv.to_str(), &args, host),
            "Number" => {
                let n = recv.to_num();
                Ok(match m {
                    "toFixed" => {
                        let d = (num(0) as usize).min(20);
                        let d = if args.is_empty() { 0 } else { d };
                        Value::str(&format!("{:.*}", d, n))
                    }
                    "toString" => {
                        let radix = if args.is_empty() { 10 } else { num(0) as u32 };
                        if radix == 10 || libm::floor(n) != n {
                            Value::str(&num_to_str(n))
                        } else {
                            Value::str(&to_radix(n as i64, radix))
                        }
                    }
                    "toPrecision" => Value::str(&num_to_str(n)),
                    _ => Value::Undef,
                })
            }
            "Array" => self.array_method(m, recv, args, host),
            "Function" => match m {
                "call" => {
                    let mut it = args.into_iter();
                    let t = it.next().unwrap_or(Value::Undef);
                    self.call(recv, &t, it.collect(), host)
                }
                "apply" => {
                    let t = a(0);
                    let items = self.arr_items(&a(1));
                    self.call(recv, &t, items, host)
                }
                "bind" => {
                    // a closure over the receiver and `this`
                    let o = self.object();
                    set_own(&mut o.borrow_mut(), "__target", recv.clone());
                    set_own(&mut o.borrow_mut(), "__this", a(0));
                    let bound = self.array(args.into_iter().skip(1).collect());
                    set_own(&mut o.borrow_mut(), "__args", bound);
                    Ok(Value::Obj(new_obj(Kind::Func(Func::Native(Rc::from("Function#__bound"), Some(Value::Obj(o)))), Some(self.func_proto.clone()))))
                }
                "__bound" => {
                    let Value::Obj(o) = recv else { return Ok(Value::Undef) };
                    let (t, th, pre) = {
                        let b = o.borrow();
                        (get_own(&b, "__target").unwrap_or(Value::Undef), get_own(&b, "__this").unwrap_or(Value::Undef), get_own(&b, "__args").unwrap_or(Value::Undef))
                    };
                    let mut all = self.arr_items(&pre);
                    all.extend(args);
                    self.call(&t, &th, all, host)
                }
                "toString" => Ok(Value::str("function () { [code] }")),
                _ => Ok(Value::Undef),
            },
            _ => match m {
                // Object.prototype
                "hasOwnProperty" => {
                    let k = a(0).to_str();
                    Ok(Value::Bool(match recv {
                        Value::Obj(o) => get_own(&o.borrow(), &k).is_some(),
                        _ => false,
                    }))
                }
                "toString" => Ok(Value::str(&recv.to_str())),
                "valueOf" => Ok(recv.clone()),
                _ => Ok(Value::Undef),
            },
        }
    }

    fn string_method(&mut self, m: &str, s: &str, args: &[Value], host: &mut dyn Host) -> R<Value> {
        let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Undef);
        let chars: Vec<char> = s.chars().collect();
        let len = chars.len() as f64;
        let idx = |v: Value, def: f64| -> usize {
            let n = if matches!(v, Value::Undef) { def } else { v.to_num() };
            let n = if n.is_nan() { 0.0 } else { n };
            let n = if n < 0.0 { (len + n).max(0.0) } else { n.min(len) };
            n as usize
        };
        let sub = |from: usize, to: usize| -> Value {
            let (f, t) = (from.min(chars.len()), to.min(chars.len()));
            if f >= t {
                Value::str("")
            } else {
                Value::str(&chars[f..t].iter().collect::<String>())
            }
        };
        Ok(match m {
            "charAt" => sub(a(0).to_num() as usize, a(0).to_num() as usize + 1),
            "charCodeAt" => {
                let i = if args.is_empty() { 0 } else { a(0).to_num() as usize };
                chars.get(i).map(|c| Value::Num(*c as u32 as f64)).unwrap_or(Value::Num(f64::NAN))
            }
            "indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith" => {
                let needle: Vec<char> = a(0).to_str().chars().collect();
                let find = |from: usize| -> Option<usize> {
                    (from..=chars.len().saturating_sub(needle.len())).find(|&i| i + needle.len() <= chars.len() && chars[i..i + needle.len()] == needle[..])
                };
                match m {
                    "indexOf" => Value::Num(find(idx(a(1), 0.0)).map(|i| i as f64).unwrap_or(-1.0)),
                    "lastIndexOf" => {
                        let mut last = -1.0;
                        let mut from = 0;
                        while let Some(i) = find(from) {
                            last = i as f64;
                            from = i + 1;
                            if from > chars.len() {
                                break;
                            }
                        }
                        Value::Num(last)
                    }
                    "includes" => Value::Bool(find(0).is_some()),
                    "startsWith" => Value::Bool(chars.len() >= needle.len() && chars[..needle.len()] == needle[..]),
                    _ => Value::Bool(chars.len() >= needle.len() && chars[chars.len() - needle.len()..] == needle[..]),
                }
            }
            "slice" => sub(idx(a(0), 0.0), idx(a(1), len)),
            "substring" => {
                let clamp = |v: Value, d: f64| -> usize {
                    let n = if matches!(v, Value::Undef) { d } else { v.to_num() };
                    if n.is_nan() || n < 0.0 { 0 } else { (n as usize).min(chars.len()) }
                };
                let (x, y) = (clamp(a(0), 0.0), clamp(a(1), len));
                sub(x.min(y), x.max(y))
            }
            "substr" => {
                let st = idx(a(0), 0.0);
                let n = if matches!(a(1), Value::Undef) { chars.len() } else { a(1).to_num().max(0.0) as usize };
                sub(st, st + n)
            }
            "toUpperCase" | "toLocaleUpperCase" => Value::str(&s.to_uppercase()),
            "toLowerCase" | "toLocaleLowerCase" => Value::str(&s.to_lowercase()),
            "trim" => Value::str(s.trim()),
            "trimStart" => Value::str(s.trim_start()),
            "trimEnd" => Value::str(s.trim_end()),
            "split" => {
                let parts: Vec<Value> = match a(0) {
                    Value::Undef => alloc::vec![Value::str(s)],
                    sep => {
                        let sep = sep.to_str();
                        if sep.is_empty() {
                            chars.iter().map(|c| Value::str(&c.to_string())).collect()
                        } else {
                            s.split(sep.as_str()).map(Value::str).collect()
                        }
                    }
                };
                let parts = match a(1) {
                    Value::Num(n) => parts.into_iter().take(n as usize).collect(),
                    _ => parts,
                };
                self.array(parts)
            }
            "replace" | "replaceAll" => {
                let pat = a(0).to_str();
                let rep = a(1);
                let mut out = String::new();
                let mut rest = s;
                let mut done = false;
                while let Some(i) = if done || pat.is_empty() { None } else { rest.find(pat.as_str()) } {
                    out.push_str(&rest[..i]);
                    if is_callable(&rep) {
                        let r = self.call(&rep, &Value::Undef, alloc::vec![Value::str(&pat)], host)?;
                        out.push_str(&r.to_str());
                    } else {
                        out.push_str(&rep.to_str());
                    }
                    rest = &rest[i + pat.len()..];
                    if m == "replace" {
                        done = true;
                    }
                }
                out.push_str(rest);
                Value::str(&out)
            }
            "repeat" => Value::str(&s.repeat(a(0).to_num().max(0.0) as usize)),
            "padStart" | "padEnd" => {
                let n = a(0).to_num().max(0.0) as usize;
                let fill = if matches!(a(1), Value::Undef) { String::from(" ") } else { a(1).to_str() };
                if chars.len() >= n || fill.is_empty() {
                    Value::str(s)
                } else {
                    let pad: String = fill.chars().cycle().take(n - chars.len()).collect();
                    Value::str(&if m == "padStart" { pad + s } else { String::from(s) + &pad })
                }
            }
            "concat" => {
                let mut o = String::from(s);
                for v in args {
                    o.push_str(&v.to_str());
                }
                Value::str(&o)
            }
            "at" => {
                let n = a(0).to_num();
                let i = if n < 0.0 { len + n } else { n };
                if i < 0.0 { Value::Undef } else { chars.get(i as usize).map(|c| Value::str(&c.to_string())).unwrap_or(Value::Undef) }
            }
            "toString" | "valueOf" => Value::str(s),
            "localeCompare" => {
                let o = a(0).to_str();
                Value::Num(match s.cmp(o.as_str()) {
                    core::cmp::Ordering::Less => -1.0,
                    core::cmp::Ordering::Equal => 0.0,
                    _ => 1.0,
                })
            }
            _ => Value::Undef,
        })
    }

    fn array_method(&mut self, m: &str, recv: &Value, args: Vec<Value>, host: &mut dyn Host) -> R<Value> {
        let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Undef);
        let Some(arr) = self.arr_of(recv) else { return Ok(Value::Undef) };
        let items = || -> Vec<Value> {
            if let Kind::Array(x) = &arr.borrow().kind { x.clone() } else { Vec::new() }
        };
        let with = |f: &mut dyn FnMut(&mut Vec<Value>) -> Value| -> Value {
            if let Kind::Array(x) = &mut arr.borrow_mut().kind { f(x) } else { Value::Undef }
        };
        let len = items().len() as f64;
        let idx = |v: Value, def: f64| -> usize {
            let n = if matches!(v, Value::Undef) { def } else { v.to_num() };
            let n = if n.is_nan() { 0.0 } else { n };
            (if n < 0.0 { (len + n).max(0.0) } else { n.min(len) }) as usize
        };
        Ok(match m {
            "push" => with(&mut |x| {
                x.extend(args.iter().cloned());
                Value::Num(x.len() as f64)
            }),
            "pop" => with(&mut |x| x.pop().unwrap_or(Value::Undef)),
            "shift" => with(&mut |x| if x.is_empty() { Value::Undef } else { x.remove(0) }),
            "unshift" => with(&mut |x| {
                for (i, v) in args.iter().enumerate() {
                    x.insert(i, v.clone());
                }
                Value::Num(x.len() as f64)
            }),
            "join" => {
                let sep = if matches!(a(0), Value::Undef) { String::from(",") } else { a(0).to_str() };
                let parts: Vec<String> = items()
                    .iter()
                    .map(|v| if matches!(v, Value::Undef | Value::Null) { String::new() } else { v.to_str() })
                    .collect();
                Value::str(&parts.join(&sep))
            }
            "toString" => Value::str(&recv.to_str()),
            "indexOf" | "includes" | "lastIndexOf" => {
                let t = a(0);
                let it = items();
                match m {
                    "indexOf" => Value::Num(it.iter().position(|v| strict_eq(v, &t)).map(|i| i as f64).unwrap_or(-1.0)),
                    "lastIndexOf" => Value::Num(it.iter().rposition(|v| strict_eq(v, &t)).map(|i| i as f64).unwrap_or(-1.0)),
                    _ => Value::Bool(it.iter().any(|v| strict_eq(v, &t) || (matches!((v, &t), (Value::Num(x), Value::Num(y)) if x.is_nan() && y.is_nan())))),
                }
            }
            "slice" => {
                let it = items();
                let (f, t) = (idx(a(0), 0.0), idx(a(1), len));
                self.array(if f < t { it[f..t].to_vec() } else { Vec::new() })
            }
            "splice" => {
                let st = idx(a(0), 0.0);
                let n = if args.len() < 2 { len as usize - st } else { (a(1).to_num().max(0.0) as usize).min(len as usize - st) };
                let ins: Vec<Value> = args.iter().skip(2).cloned().collect();
                let removed = with(&mut |x| {
                    let r: Vec<Value> = x.splice(st..st + n, ins.iter().cloned()).collect();
                    Value::Obj(new_obj(Kind::Array(r), None))
                });
                // give the removed list the array prototype
                if let Value::Obj(o) = &removed {
                    o.borrow_mut().proto = Some(self.array_proto.clone());
                }
                removed
            }
            "concat" => {
                let mut it = items();
                for v in &args {
                    match self.arr_of(v) {
                        Some(o) => it.extend(self.arr_items(&Value::Obj(o))),
                        None => it.push(v.clone()),
                    }
                }
                self.array(it)
            }
            "reverse" => {
                with(&mut |x| {
                    x.reverse();
                    Value::Undef
                });
                recv.clone()
            }
            "fill" => {
                let v = a(0);
                with(&mut |x| {
                    for e in x.iter_mut() {
                        *e = v.clone();
                    }
                    Value::Undef
                });
                recv.clone()
            }
            "sort" => {
                let mut it = items();
                let f = a(0);
                self.merge_sort(&mut it, &f, host)?;
                with(&mut |x| {
                    *x = core::mem::take(&mut it);
                    Value::Undef
                });
                recv.clone()
            }
            "forEach" | "map" | "filter" | "find" | "findIndex" | "some" | "every" => {
                let f = a(0);
                let it = items();
                let mut out = Vec::new();
                for (i, v) in it.into_iter().enumerate() {
                    let r = self.call(&f, &Value::Undef, alloc::vec![v.clone(), Value::Num(i as f64), recv.clone()], host)?;
                    match m {
                        "map" => out.push(r),
                        "filter" => {
                            if r.truthy() {
                                out.push(v)
                            }
                        }
                        "find" => {
                            if r.truthy() {
                                return Ok(v);
                            }
                        }
                        "findIndex" => {
                            if r.truthy() {
                                return Ok(Value::Num(i as f64));
                            }
                        }
                        "some" => {
                            if r.truthy() {
                                return Ok(Value::Bool(true));
                            }
                        }
                        "every" => {
                            if !r.truthy() {
                                return Ok(Value::Bool(false));
                            }
                        }
                        _ => {}
                    }
                }
                match m {
                    "map" | "filter" => self.array(out),
                    "find" => Value::Undef,
                    "findIndex" => Value::Num(-1.0),
                    "some" => Value::Bool(false),
                    "every" => Value::Bool(true),
                    _ => Value::Undef,
                }
            }
            "reduce" | "reduceRight" => {
                let f = a(0);
                let mut it = items();
                if m == "reduceRight" {
                    it.reverse();
                }
                let mut iter = it.into_iter().enumerate();
                let mut acc = if args.len() > 1 {
                    a(1)
                } else {
                    match iter.next() {
                        Some((_, v)) => v,
                        None => return self.throw("TypeError", "reduce pada array kosong tanpa nilai awal"),
                    }
                };
                for (i, v) in iter {
                    acc = self.call(&f, &Value::Undef, alloc::vec![acc, v, Value::Num(i as f64)], host)?;
                }
                acc
            }
            "at" => {
                let n = a(0).to_num();
                let i = if n < 0.0 { len + n } else { n };
                if i < 0.0 { Value::Undef } else { items().get(i as usize).cloned().unwrap_or(Value::Undef) }
            }
            "flat" => {
                let mut out = Vec::new();
                for v in items() {
                    match self.arr_of(&v) {
                        Some(o) => out.extend(self.arr_items(&Value::Obj(o))),
                        None => out.push(v),
                    }
                }
                self.array(out)
            }
            "keys" => self.array((0..len as usize).map(|i| Value::Num(i as f64)).collect()),
            _ => Value::Undef,
        })
    }

    fn merge_sort(&mut self, v: &mut Vec<Value>, f: &Value, host: &mut dyn Host) -> R<()> {
        if v.len() < 2 {
            return Ok(());
        }
        let mut right = v.split_off(v.len() / 2);
        self.merge_sort(v, f, host)?;
        self.merge_sort(&mut right, f, host)?;
        let left = core::mem::take(v);
        let (mut i, mut j) = (0, 0);
        while i < left.len() && j < right.len() {
            let le = if is_callable(f) {
                let r = self.call(f, &Value::Undef, alloc::vec![left[i].clone(), right[j].clone()], host)?;
                !(r.to_num() > 0.0)
            } else {
                // default order: undefined last, then by string
                match (&left[i], &right[j]) {
                    (Value::Undef, _) => false,
                    (_, Value::Undef) => true,
                    (x, y) => x.to_str() <= y.to_str(),
                }
            };
            if le {
                v.push(left[i].clone());
                i += 1;
            } else {
                v.push(right[j].clone());
                j += 1;
            }
        }
        v.extend_from_slice(&left[i..]);
        v.extend_from_slice(&right[j..]);
        Ok(())
    }

    /// console.log formatting: strings plain at the top, quoted inside.
    pub fn inspect(&self, v: &Value, depth: u32) -> String {
        match v {
            Value::Str(s) if depth > 0 => format!("'{}'", s),
            Value::Obj(o) if depth < 3 => {
                let b = o.borrow();
                match &b.kind {
                    Kind::Array(a) => {
                        let parts: Vec<String> = a.iter().take(50).map(|x| self.inspect(x, depth + 1)).collect();
                        format!("[ {} ]", parts.join(", "))
                    }
                    Kind::Plain => {
                        let parts: Vec<String> = b.props.iter().take(50).map(|(k, x)| format!("{}: {}", k, self.inspect(x, depth + 1))).collect();
                        if parts.is_empty() { "{}".into() } else { format!("{{ {} }}", parts.join(", ")) }
                    }
                    _ => v.to_str(),
                }
            }
            _ => v.to_str(),
        }
    }

    fn json_write(&self, v: &Value, s: &mut String, indent: usize, depth: usize) -> bool {
        let nl = |s: &mut String, d: usize| {
            if indent > 0 {
                s.push('\n');
                for _ in 0..d * indent {
                    s.push(' ');
                }
            }
        };
        match v {
            Value::Undef => return false,
            Value::Null => s.push_str("null"),
            Value::Bool(b) => s.push_str(if *b { "true" } else { "false" }),
            Value::Num(n) => s.push_str(&if n.is_finite() { num_to_str(*n) } else { "null".into() }),
            Value::Str(t) => json_str(t, s),
            Value::Obj(o) => {
                if depth > 32 {
                    s.push_str("null");
                    return true;
                }
                let b = o.borrow();
                match &b.kind {
                    Kind::Array(a) => {
                        s.push('[');
                        for (i, x) in a.iter().enumerate() {
                            if i > 0 {
                                s.push(',');
                            }
                            nl(s, depth + 1);
                            if !self.json_write(x, s, indent, depth + 1) {
                                s.push_str("null");
                            }
                        }
                        if !a.is_empty() {
                            nl(s, depth);
                        }
                        s.push(']');
                    }
                    Kind::Func(_) => return false,
                    _ => {
                        s.push('{');
                        let mut first = true;
                        for (k, x) in &b.props {
                            let mut part = String::new();
                            if !self.json_write(x, &mut part, indent, depth + 1) {
                                continue;
                            }
                            if !first {
                                s.push(',');
                            }
                            first = false;
                            nl(s, depth + 1);
                            json_str(k, s);
                            s.push(':');
                            if indent > 0 {
                                s.push(' ');
                            }
                            s.push_str(&part);
                        }
                        if !first {
                            nl(s, depth);
                        }
                        s.push('}');
                    }
                }
            }
        }
        true
    }

    fn json_read(&self, c: &[char], i: &mut usize) -> Option<Value> {
        let ws = |i: &mut usize| {
            while *i < c.len() && c[*i].is_whitespace() {
                *i += 1;
            }
        };
        ws(i);
        let ch = *c.get(*i)?;
        let v = match ch {
            '{' => {
                *i += 1;
                let o = self.object();
                ws(i);
                if c.get(*i) == Some(&'}') {
                    *i += 1;
                    return Some(Value::Obj(o));
                }
                loop {
                    ws(i);
                    let k = match self.json_read(c, i)? {
                        Value::Str(s) => s,
                        _ => return None,
                    };
                    ws(i);
                    if c.get(*i) != Some(&':') {
                        return None;
                    }
                    *i += 1;
                    let v = self.json_read(c, i)?;
                    set_own(&mut o.borrow_mut(), &k, v);
                    ws(i);
                    match c.get(*i) {
                        Some(',') => *i += 1,
                        Some('}') => {
                            *i += 1;
                            break;
                        }
                        _ => return None,
                    }
                }
                Value::Obj(o)
            }
            '[' => {
                *i += 1;
                let mut v = Vec::new();
                ws(i);
                if c.get(*i) == Some(&']') {
                    *i += 1;
                    return Some(self.array(v));
                }
                loop {
                    v.push(self.json_read(c, i)?);
                    ws(i);
                    match c.get(*i) {
                        Some(',') => *i += 1,
                        Some(']') => {
                            *i += 1;
                            break;
                        }
                        _ => return None,
                    }
                }
                self.array(v)
            }
            '"' => {
                *i += 1;
                let mut s = String::new();
                while *i < c.len() && c[*i] != '"' {
                    if c[*i] == '\\' {
                        *i += 1;
                        *i = escape(c, *i, &mut s);
                        continue;
                    }
                    s.push(c[*i]);
                    *i += 1;
                }
                *i += 1;
                Value::str(&s)
            }
            't' if c[*i..].starts_with(&['t', 'r', 'u', 'e']) => {
                *i += 4;
                Value::Bool(true)
            }
            'f' if c[*i..].starts_with(&['f', 'a', 'l', 's', 'e']) => {
                *i += 5;
                Value::Bool(false)
            }
            'n' if c[*i..].starts_with(&['n', 'u', 'l', 'l']) => {
                *i += 4;
                Value::Null
            }
            _ => {
                let st = *i;
                while *i < c.len() && (c[*i].is_ascii_digit() || "+-.eE".contains(c[*i])) {
                    *i += 1;
                }
                let t: String = c[st..*i].iter().collect();
                Value::Num(t.parse().ok()?)
            }
        };
        Some(v)
    }
}

impl Default for Interp {
    fn default() -> Self {
        Self::new()
    }
}

fn json_str(t: &str, s: &mut String) {
    s.push('"');
    for ch in t.chars() {
        match ch {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            c if (c as u32) < 0x20 => s.push_str(&format!("\\u{:04x}", c as u32)),
            c => s.push(c),
        }
    }
    s.push('"');
}

fn to_radix(mut n: i64, radix: u32) -> String {
    if !(2..=36).contains(&radix) {
        return n.to_string();
    }
    let neg = n < 0;
    if neg {
        n = -n;
    }
    let mut d = Vec::new();
    loop {
        d.push(core::char::from_digit((n % radix as i64) as u32, radix).unwrap());
        n /= radix as i64;
        if n == 0 {
            break;
        }
    }
    if neg {
        d.push('-');
    }
    d.iter().rev().collect()
}

fn to_i32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    let m = libm::fmod(libm::trunc(n), 4294967296.0);
    let m = if m < 0.0 { m + 4294967296.0 } else { m };
    (m as u64 as u32) as i32
}

fn to_prim(v: &Value) -> Value {
    match v {
        Value::Obj(_) => Value::str(&v.to_str()),
        _ => v.clone(),
    }
}

fn is_callable(v: &Value) -> bool {
    matches!(v, Value::Obj(o) if matches!(o.borrow().kind, Kind::Func(_)))
}

pub fn strict_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undef, Value::Undef) | (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Num(x), Value::Num(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Obj(x), Value::Obj(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

fn loose_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undef | Value::Null, Value::Undef | Value::Null) => true,
        (Value::Undef | Value::Null, _) | (_, Value::Undef | Value::Null) => false,
        (Value::Obj(_), Value::Obj(_)) => strict_eq(a, b),
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Obj(_), _) | (_, Value::Obj(_)) => loose_eq(&to_prim(a), &to_prim(b)),
        _ => a.to_num() == b.to_num(),
    }
}

/// Built-in methods by receiver kind (`Kind#name`).
const METHODS: &[&str] = &[
    "String#charAt", "String#charCodeAt", "String#indexOf", "String#lastIndexOf", "String#includes", "String#startsWith",
    "String#endsWith", "String#slice", "String#substring", "String#substr", "String#toUpperCase", "String#toLowerCase",
    "String#toLocaleUpperCase", "String#toLocaleLowerCase", "String#trim", "String#trimStart", "String#trimEnd",
    "String#split", "String#replace", "String#replaceAll", "String#repeat", "String#padStart", "String#padEnd",
    "String#concat", "String#at", "String#toString", "String#valueOf", "String#localeCompare",
    "Number#toFixed", "Number#toString", "Number#toPrecision",
    "Array#push", "Array#pop", "Array#shift", "Array#unshift", "Array#join", "Array#toString", "Array#indexOf",
    "Array#lastIndexOf", "Array#includes", "Array#slice", "Array#splice", "Array#concat", "Array#reverse", "Array#fill",
    "Array#sort", "Array#forEach", "Array#map", "Array#filter", "Array#find", "Array#findIndex", "Array#some",
    "Array#every", "Array#reduce", "Array#reduceRight", "Array#at", "Array#flat", "Array#keys",
    "Function#call", "Function#apply", "Function#bind", "Function#toString",
    "Object#hasOwnProperty", "Object#toString", "Object#valueOf",
];

fn random() -> f64 {
    use core::sync::atomic::{AtomicU64, Ordering};
    static STATE: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
    let mut x = STATE.load(Ordering::Relaxed);
    if x == 0x9E37_79B9_7F4A_7C15 {
        x ^= seed();
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    STATE.store(x, Ordering::Relaxed);
    (x >> 11) as f64 / (1u64 << 53) as f64
}

static SEED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// The embedder seeds `Math.random` (the device's RANDOM syscall).
pub fn set_seed(v: u64) {
    SEED.store(v | 1, core::sync::atomic::Ordering::Relaxed);
}

fn seed() -> u64 {
    SEED.load(core::sync::atomic::Ordering::Relaxed)
}
