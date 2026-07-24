//! Polyglot script runtime (v0.14 "Babel"): one interpreter that runs
//! JavaScript, TypeScript, and Python side by side, over a shared AST,
//! evaluator, and **uniform host binding API** (`print` / `console.log` both
//! land on the same host `emit`). requirements.md §11 calls for JS/TS + Python
//! runtimes; production would embed V8/CPython, but freestanding zerolib apps
//! can't host those, so this is a genuine, compact tree-walking interpreter
//! (like the tiny-but-real AI and VMM subsystems).
//!
//! Supported subset (all three languages): int/string/bool/nil values, the
//! usual arithmetic/comparison/logical operators, string `+` concatenation,
//! variables, `if/else`, `while`, `for` (C-style in JS/TS, `range()` in
//! Python), functions with recursion, and `print`/`console.log`/`str`/`len`.
//! TypeScript is JavaScript after type-stripping (its own real transpile step).

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Js,
    Ts,
    Python,
}

impl Lang {
    pub fn name(self) -> &'static str {
        match self {
            Lang::Js => "JavaScript",
            Lang::Ts => "TypeScript",
            Lang::Python => "Python",
        }
    }
}

// --- Values ------------------------------------------------------------------

#[derive(Clone)]
enum Value {
    Int(i64),
    Str(String),
    Bool(bool),
    Nil,
}

impl Value {
    fn display(&self) -> String {
        match self {
            Value::Int(n) => n.to_string(),
            Value::Str(s) => s.clone(),
            Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            Value::Nil => "nil".to_string(),
        }
    }
    fn truthy(&self) -> bool {
        match self {
            Value::Int(n) => *n != 0,
            Value::Str(s) => !s.is_empty(),
            Value::Bool(b) => *b,
            Value::Nil => false,
        }
    }
    fn as_int(&self) -> i64 {
        match self {
            Value::Int(n) => *n,
            Value::Bool(b) => *b as i64,
            _ => 0,
        }
    }
}

// --- Tokens (shared by the JS and Python lexers) -----------------------------

#[derive(Clone, PartialEq)]
enum Tok {
    Int(i64),
    Str(String),
    Ident(String),
    Bool(bool),
    Nil,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    Assign,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Semi,
    Colon,
    Dot,
    Newline,
    Indent,
    Dedent,
}

// --- AST ---------------------------------------------------------------------

#[derive(Clone, Copy)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Clone)]
enum Expr {
    Int(i64),
    Str(String),
    Bool(bool),
    Nil,
    Var(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

#[derive(Clone)]
enum Stmt {
    Assign(String, Expr),
    ExprStmt(Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    While(Expr, Vec<Stmt>),
    ForC(Box<Stmt>, Expr, Box<Stmt>, Vec<Stmt>), // init, cond, incr, body
    ForRange(String, Expr, Expr, Vec<Stmt>),     // var, start, end(excl), body
    Func(String, Vec<String>, Vec<Stmt>),
    Return(Option<Expr>),
}

// --- JavaScript lexer --------------------------------------------------------

fn lex_js(src: &str) -> Result<Vec<Tok>, String> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'"' || c == b'\'' {
            let (s, ni) = lex_string(b, i)?;
            out.push(Tok::Str(s));
            i = ni;
            continue;
        }
        if c.is_ascii_digit() {
            let (n, ni) = lex_number(b, i);
            out.push(Tok::Int(n));
            i = ni;
            continue;
        }
        if c == b'_' || c.is_ascii_alphabetic() {
            let (w, ni) = lex_ident(b, i);
            out.push(ident_tok(&w));
            i = ni;
            continue;
        }
        let (t, ni) = lex_op(b, i)?;
        out.push(t);
        i = ni;
    }
    Ok(out)
}

fn lex_string(b: &[u8], start: usize) -> Result<(String, usize), String> {
    let quote = b[start];
    let mut i = start + 1;
    let mut s = String::new();
    while i < b.len() && b[i] != quote {
        if b[i] == b'\\' && i + 1 < b.len() {
            i += 1;
            let e = match b[i] {
                b'n' => '\n',
                b't' => '\t',
                b'\\' => '\\',
                b'"' => '"',
                b'\'' => '\'',
                other => other as char,
            };
            s.push(e);
        } else {
            s.push(b[i] as char);
        }
        i += 1;
    }
    if i >= b.len() {
        return Err(String::from("string tak ditutup"));
    }
    Ok((s, i + 1))
}

fn lex_number(b: &[u8], start: usize) -> (i64, usize) {
    let mut i = start;
    let mut n: i64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        n = n * 10 + (b[i] - b'0') as i64;
        i += 1;
    }
    (n, i)
}

fn lex_ident(b: &[u8], start: usize) -> (String, usize) {
    let mut i = start;
    let mut w = String::new();
    while i < b.len() && (b[i] == b'_' || b[i].is_ascii_alphanumeric()) {
        w.push(b[i] as char);
        i += 1;
    }
    (w, i)
}

fn ident_tok(w: &str) -> Tok {
    match w {
        "true" | "True" => Tok::Bool(true),
        "false" | "False" => Tok::Bool(false),
        "null" | "nil" | "None" => Tok::Nil,
        "and" => Tok::AndAnd,
        "or" => Tok::OrOr,
        "not" => Tok::Bang,
        _ => Tok::Ident(w.to_string()),
    }
}

fn lex_op(b: &[u8], i: usize) -> Result<(Tok, usize), String> {
    let two = |a: u8, c: u8| i + 1 < b.len() && b[i] == a && b[i + 1] == c;
    if two(b'=', b'=') {
        return Ok((Tok::EqEq, i + 2));
    }
    if two(b'!', b'=') {
        return Ok((Tok::Ne, i + 2));
    }
    if two(b'<', b'=') {
        return Ok((Tok::Le, i + 2));
    }
    if two(b'>', b'=') {
        return Ok((Tok::Ge, i + 2));
    }
    if two(b'&', b'&') {
        return Ok((Tok::AndAnd, i + 2));
    }
    if two(b'|', b'|') {
        return Ok((Tok::OrOr, i + 2));
    }
    let t = match b[i] {
        b'+' => Tok::Plus,
        b'-' => Tok::Minus,
        b'*' => Tok::Star,
        b'/' => Tok::Slash,
        b'%' => Tok::Percent,
        b'<' => Tok::Lt,
        b'>' => Tok::Gt,
        b'!' => Tok::Bang,
        b'=' => Tok::Assign,
        b'(' => Tok::LParen,
        b')' => Tok::RParen,
        b'{' => Tok::LBrace,
        b'}' => Tok::RBrace,
        b',' => Tok::Comma,
        b';' => Tok::Semi,
        b':' => Tok::Colon,
        b'.' => Tok::Dot,
        other => return Err(format!("karakter tak dikenal '{}'", other as char)),
    };
    Ok((t, i + 1))
}

// --- Python lexer (indentation-based) ----------------------------------------

fn lex_py(src: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let mut indents: Vec<usize> = alloc::vec![0];
    for raw_line in src.lines() {
        // strip comments
        let line = match raw_line.find('#') {
            Some(p) => &raw_line[..p],
            None => raw_line,
        };
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent > *indents.last().unwrap() {
            indents.push(indent);
            out.push(Tok::Indent);
        } else {
            while indent < *indents.last().unwrap() {
                indents.pop();
                out.push(Tok::Dedent);
            }
        }
        // lex the line content with the JS lexer (same tokens, no braces/;)
        let toks = lex_js(line.trim())?;
        out.extend(toks);
        out.push(Tok::Newline);
    }
    while indents.len() > 1 {
        indents.pop();
        out.push(Tok::Dedent);
    }
    Ok(out)
}

// --- TypeScript -> JavaScript (type stripping) -------------------------------

/// Strip TypeScript type syntax so the JS front-end can run it. In this subset
/// a `:` only ever introduces a type annotation (no object literals or ternary),
/// so removing `interface`/`type` declarations and every `: Type` up to a
/// delimiter yields valid JS — a real, if minimal, transpile.
fn ts_to_js(src: &str) -> String {
    // 1. Drop `interface Name { ... }` and `type Name = ...;` declarations.
    let mut s = strip_word_block(src, "interface");
    s = strip_type_alias(&s);
    // 2. Remove `: Type` annotations (outside string literals).
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    let mut in_str: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = in_str {
            out.push(c as char);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1] as char);
                i += 2;
                continue;
            }
            if c == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        if c == b'"' || c == b'\'' {
            in_str = Some(c);
            out.push(c as char);
            i += 1;
            continue;
        }
        if c == b':' {
            // skip the type expression up to a delimiter we must keep.
            i += 1;
            while i < b.len() && !matches!(b[i], b',' | b')' | b';' | b'{' | b'=' | b'\n') {
                i += 1;
            }
            continue;
        }
        out.push(c as char);
        i += 1;
    }
    out
}

fn strip_word_block(src: &str, word: &str) -> String {
    let mut out = String::new();
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if word_at(b, i, word) {
            // find first '{', then skip to matching '}'
            let mut j = i;
            while j < b.len() && b[j] != b'{' {
                j += 1;
            }
            if j >= b.len() {
                break;
            }
            let mut depth = 0;
            while j < b.len() {
                if b[j] == b'{' {
                    depth += 1;
                }
                if b[j] == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        j += 1;
                        break;
                    }
                }
                j += 1;
            }
            i = j;
            continue;
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

fn strip_type_alias(src: &str) -> String {
    let mut out = String::new();
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if word_at(b, i, "type") {
            while i < b.len() && b[i] != b';' && b[i] != b'\n' {
                i += 1;
            }
            if i < b.len() {
                i += 1;
            }
            continue;
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

fn word_at(b: &[u8], i: usize, word: &str) -> bool {
    let w = word.as_bytes();
    if i + w.len() > b.len() || &b[i..i + w.len()] != w {
        return false;
    }
    let before_ok = i == 0 || !(b[i - 1] == b'_' || b[i - 1].is_ascii_alphanumeric());
    let after = i + w.len();
    let after_ok = after >= b.len() || !(b[after] == b'_' || b[after].is_ascii_alphanumeric());
    before_ok && after_ok
}

// --- Parser (shared expressions; per-language statements) --------------------

struct Parser {
    t: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.pos)
    }
    fn peek2(&self) -> Option<&Tok> {
        self.t.get(self.pos + 1)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.t.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, t: &Tok, what: &str) -> Result<(), String> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(format!("diharapkan {}", what))
        }
    }
    fn skip_newlines(&mut self) {
        while self.peek() == Some(&Tok::Newline) {
            self.pos += 1;
        }
    }

    // expression: precedence climbing
    fn expr(&mut self, min_bp: u8) -> Result<Expr, String> {
        let mut lhs = self.unary()?;
        loop {
            let (op, bp) = match self.peek() {
                Some(Tok::OrOr) => (BinOp::Or, 1),
                Some(Tok::AndAnd) => (BinOp::And, 2),
                Some(Tok::EqEq) => (BinOp::Eq, 3),
                Some(Tok::Ne) => (BinOp::Ne, 3),
                Some(Tok::Lt) => (BinOp::Lt, 4),
                Some(Tok::Le) => (BinOp::Le, 4),
                Some(Tok::Gt) => (BinOp::Gt, 4),
                Some(Tok::Ge) => (BinOp::Ge, 4),
                Some(Tok::Plus) => (BinOp::Add, 5),
                Some(Tok::Minus) => (BinOp::Sub, 5),
                Some(Tok::Star) => (BinOp::Mul, 6),
                Some(Tok::Slash) => (BinOp::Div, 6),
                Some(Tok::Percent) => (BinOp::Mod, 6),
                _ => break,
            };
            if bp < min_bp {
                break;
            }
            self.pos += 1;
            let rhs = self.expr(bp + 1)?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Tok::Minus) => {
                self.pos += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(Tok::Bang) => {
                self.pos += 1;
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Int(n)) => Ok(Expr::Int(n)),
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::Bool(b)) => Ok(Expr::Bool(b)),
            Some(Tok::Nil) => Ok(Expr::Nil),
            Some(Tok::LParen) => {
                let e = self.expr(0)?;
                self.expect(&Tok::RParen, "')'")?;
                Ok(e)
            }
            Some(Tok::Ident(mut name)) => {
                // dotted name (console.log)
                while self.peek() == Some(&Tok::Dot) {
                    self.pos += 1;
                    if let Some(Tok::Ident(part)) = self.next() {
                        name.push('.');
                        name.push_str(&part);
                    } else {
                        return Err(String::from("nama setelah '.' tak valid"));
                    }
                }
                if self.eat(&Tok::LParen) {
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.expr(0)?);
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(&Tok::RParen, "')'")?;
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            other => Err(format!("token tak terduga di ekspresi: {}", tok_name(other.as_ref()))),
        }
    }
}

fn tok_name(t: Option<&Tok>) -> &'static str {
    match t {
        None => "<akhir>",
        Some(Tok::Newline) => "<newline>",
        Some(Tok::Indent) => "<indent>",
        Some(Tok::Dedent) => "<dedent>",
        _ => "<token>",
    }
}

fn is_kw(t: Option<&Tok>, kw: &str) -> bool {
    matches!(t, Some(Tok::Ident(s)) if s == kw)
}

// --- JavaScript statement parser (braces + ';') ------------------------------

fn parse_js(toks: Vec<Tok>) -> Result<Vec<Stmt>, String> {
    let mut p = Parser { t: toks, pos: 0 };
    let mut out = Vec::new();
    while p.peek().is_some() {
        out.push(js_stmt(&mut p)?);
    }
    Ok(out)
}

fn js_block(p: &mut Parser) -> Result<Vec<Stmt>, String> {
    p.expect(&Tok::LBrace, "'{'")?;
    let mut out = Vec::new();
    while p.peek().is_some() && p.peek() != Some(&Tok::RBrace) {
        out.push(js_stmt(p)?);
    }
    p.expect(&Tok::RBrace, "'}'")?;
    Ok(out)
}

fn js_stmt(p: &mut Parser) -> Result<Stmt, String> {
    if is_kw(p.peek(), "function") {
        p.pos += 1;
        let name = ident_name(p)?;
        let params = js_params(p)?;
        let body = js_block(p)?;
        return Ok(Stmt::Func(name, params, body));
    }
    if is_kw(p.peek(), "if") {
        p.pos += 1;
        p.expect(&Tok::LParen, "'('")?;
        let cond = p.expr(0)?;
        p.expect(&Tok::RParen, "')'")?;
        let then = js_block(p)?;
        let els = if is_kw(p.peek(), "else") {
            p.pos += 1;
            if is_kw(p.peek(), "if") {
                alloc::vec![js_stmt(p)?]
            } else {
                js_block(p)?
            }
        } else {
            Vec::new()
        };
        return Ok(Stmt::If(cond, then, els));
    }
    if is_kw(p.peek(), "while") {
        p.pos += 1;
        p.expect(&Tok::LParen, "'('")?;
        let cond = p.expr(0)?;
        p.expect(&Tok::RParen, "')'")?;
        let body = js_block(p)?;
        return Ok(Stmt::While(cond, body));
    }
    if is_kw(p.peek(), "for") {
        p.pos += 1;
        p.expect(&Tok::LParen, "'('")?;
        let init = js_simple(p)?;
        p.expect(&Tok::Semi, "';'")?;
        let cond = p.expr(0)?;
        p.expect(&Tok::Semi, "';'")?;
        let incr = js_simple(p)?;
        p.expect(&Tok::RParen, "')'")?;
        let body = js_block(p)?;
        return Ok(Stmt::ForC(Box::new(init), cond, Box::new(incr), body));
    }
    if is_kw(p.peek(), "return") {
        p.pos += 1;
        let e = if p.peek() == Some(&Tok::Semi) {
            None
        } else {
            Some(p.expr(0)?)
        };
        p.eat(&Tok::Semi);
        return Ok(Stmt::Return(e));
    }
    let s = js_simple(p)?;
    p.eat(&Tok::Semi);
    Ok(s)
}

fn js_simple(p: &mut Parser) -> Result<Stmt, String> {
    if is_kw(p.peek(), "let") || is_kw(p.peek(), "var") || is_kw(p.peek(), "const") {
        p.pos += 1;
        let name = ident_name(p)?;
        p.expect(&Tok::Assign, "'='")?;
        let e = p.expr(0)?;
        return Ok(Stmt::Assign(name, e));
    }
    if let (Some(Tok::Ident(name)), Some(Tok::Assign)) = (p.peek().cloned(), p.peek2()) {
        p.pos += 2;
        let e = p.expr(0)?;
        return Ok(Stmt::Assign(name, e));
    }
    Ok(Stmt::ExprStmt(p.expr(0)?))
}

fn js_params(p: &mut Parser) -> Result<Vec<String>, String> {
    p.expect(&Tok::LParen, "'('")?;
    let mut out = Vec::new();
    if p.peek() != Some(&Tok::RParen) {
        loop {
            out.push(ident_name(p)?);
            if !p.eat(&Tok::Comma) {
                break;
            }
        }
    }
    p.expect(&Tok::RParen, "')'")?;
    Ok(out)
}

fn ident_name(p: &mut Parser) -> Result<String, String> {
    match p.next() {
        Some(Tok::Ident(s)) => Ok(s),
        _ => Err(String::from("diharapkan nama")),
    }
}

// --- Python statement parser (indent + ':') ----------------------------------

fn parse_py(toks: Vec<Tok>) -> Result<Vec<Stmt>, String> {
    let mut p = Parser { t: toks, pos: 0 };
    let mut out = Vec::new();
    p.skip_newlines();
    while p.peek().is_some() {
        out.push(py_stmt(&mut p)?);
        p.skip_newlines();
    }
    Ok(out)
}

fn py_suite(p: &mut Parser) -> Result<Vec<Stmt>, String> {
    p.expect(&Tok::Colon, "':'")?;
    p.expect(&Tok::Newline, "newline")?;
    p.expect(&Tok::Indent, "indent")?;
    let mut out = Vec::new();
    p.skip_newlines();
    while p.peek().is_some() && p.peek() != Some(&Tok::Dedent) {
        out.push(py_stmt(p)?);
        p.skip_newlines();
    }
    p.expect(&Tok::Dedent, "dedent")?;
    Ok(out)
}

fn py_stmt(p: &mut Parser) -> Result<Stmt, String> {
    if is_kw(p.peek(), "def") {
        p.pos += 1;
        let name = ident_name(p)?;
        let params = js_params(p)?; // same `(a, b)` shape
        let body = py_suite(p)?;
        return Ok(Stmt::Func(name, params, body));
    }
    if is_kw(p.peek(), "if") {
        p.pos += 1;
        let cond = p.expr(0)?;
        let then = py_suite(p)?;
        let els = if is_kw(p.peek(), "elif") {
            // desugar `elif` into a nested if in the else branch
            alloc::vec![py_stmt_elif(p)?]
        } else if is_kw(p.peek(), "else") {
            p.pos += 1;
            py_suite(p)?
        } else {
            Vec::new()
        };
        return Ok(Stmt::If(cond, then, els));
    }
    if is_kw(p.peek(), "while") {
        p.pos += 1;
        let cond = p.expr(0)?;
        let body = py_suite(p)?;
        return Ok(Stmt::While(cond, body));
    }
    if is_kw(p.peek(), "for") {
        p.pos += 1;
        let var = ident_name(p)?;
        if !is_kw(p.peek(), "in") {
            return Err(String::from("diharapkan 'in' pada for"));
        }
        p.pos += 1;
        if !is_kw(p.peek(), "range") {
            return Err(String::from("hanya 'for x in range(...)' didukung"));
        }
        p.pos += 1;
        p.expect(&Tok::LParen, "'('")?;
        let first = p.expr(0)?;
        let (start, end) = if p.eat(&Tok::Comma) {
            (first, p.expr(0)?)
        } else {
            (Expr::Int(0), first)
        };
        p.expect(&Tok::RParen, "')'")?;
        let body = py_suite(p)?;
        return Ok(Stmt::ForRange(var, start, end, body));
    }
    if is_kw(p.peek(), "return") {
        p.pos += 1;
        let e = if p.peek() == Some(&Tok::Newline) {
            None
        } else {
            Some(p.expr(0)?)
        };
        return Ok(Stmt::Return(e));
    }
    // assignment or expression
    if let (Some(Tok::Ident(name)), Some(Tok::Assign)) = (p.peek().cloned(), p.peek2()) {
        p.pos += 2;
        let e = p.expr(0)?;
        return Ok(Stmt::Assign(name, e));
    }
    Ok(Stmt::ExprStmt(p.expr(0)?))
}

fn py_stmt_elif(p: &mut Parser) -> Result<Stmt, String> {
    // called with peek == "elif"; treat like `if`
    p.pos += 1;
    let cond = p.expr(0)?;
    let then = py_suite(p)?;
    let els = if is_kw(p.peek(), "elif") {
        alloc::vec![py_stmt_elif(p)?]
    } else if is_kw(p.peek(), "else") {
        p.pos += 1;
        py_suite(p)?
    } else {
        Vec::new()
    };
    Ok(Stmt::If(cond, then, els))
}

// --- Interpreter -------------------------------------------------------------

enum Flow {
    Normal,
    Return(Value),
}

struct Interp {
    funcs: BTreeMap<String, (Vec<String>, Vec<Stmt>)>,
    scopes: Vec<BTreeMap<String, Value>>,
    out: String,
    steps: u64,
}

const STEP_LIMIT: u64 = 5_000_000;
const OUT_LIMIT: usize = 16 * 1024;

impl Interp {
    fn new() -> Self {
        Interp {
            funcs: BTreeMap::new(),
            scopes: alloc::vec![BTreeMap::new()],
            out: String::new(),
            steps: 0,
        }
    }

    fn get(&self, name: &str) -> Option<Value> {
        for s in self.scopes.iter().rev() {
            if let Some(v) = s.get(name) {
                return Some(v.clone());
            }
        }
        None
    }
    fn set(&mut self, name: &str, v: Value) {
        for s in self.scopes.iter_mut().rev() {
            if s.contains_key(name) {
                s.insert(name.to_string(), v);
                return;
            }
        }
        self.scopes.last_mut().unwrap().insert(name.to_string(), v);
    }

    fn tick(&mut self) -> Result<(), String> {
        self.steps += 1;
        if self.steps > STEP_LIMIT {
            Err(String::from("batas langkah tercapai (kemungkinan loop tak berujung)"))
        } else {
            Ok(())
        }
    }

    fn run_block(&mut self, body: &[Stmt]) -> Result<Flow, String> {
        for s in body {
            if let Flow::Return(v) = self.exec(s)? {
                return Ok(Flow::Return(v));
            }
        }
        Ok(Flow::Normal)
    }

    fn exec(&mut self, s: &Stmt) -> Result<Flow, String> {
        self.tick()?;
        match s {
            Stmt::Func(name, params, body) => {
                self.funcs.insert(name.clone(), (params.clone(), body.clone()));
                Ok(Flow::Normal)
            }
            Stmt::Assign(name, e) => {
                let v = self.eval(e)?;
                self.set(name, v);
                Ok(Flow::Normal)
            }
            Stmt::ExprStmt(e) => {
                self.eval(e)?;
                Ok(Flow::Normal)
            }
            Stmt::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(e)?,
                    None => Value::Nil,
                };
                Ok(Flow::Return(v))
            }
            Stmt::If(cond, then, els) => {
                if self.eval(cond)?.truthy() {
                    self.run_block(then)
                } else {
                    self.run_block(els)
                }
            }
            Stmt::While(cond, body) => {
                while self.eval(cond)?.truthy() {
                    self.tick()?;
                    if let Flow::Return(v) = self.run_block(body)? {
                        return Ok(Flow::Return(v));
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::ForC(init, cond, incr, body) => {
                self.exec(init)?;
                while self.eval(cond)?.truthy() {
                    self.tick()?;
                    if let Flow::Return(v) = self.run_block(body)? {
                        return Ok(Flow::Return(v));
                    }
                    self.exec(incr)?;
                }
                Ok(Flow::Normal)
            }
            Stmt::ForRange(var, start, end, body) => {
                let a = self.eval(start)?.as_int();
                let b = self.eval(end)?.as_int();
                let mut i = a;
                while i < b {
                    self.tick()?;
                    self.set(var, Value::Int(i));
                    if let Flow::Return(v) = self.run_block(body)? {
                        return Ok(Flow::Return(v));
                    }
                    i += 1;
                }
                Ok(Flow::Normal)
            }
        }
    }

    fn eval(&mut self, e: &Expr) -> Result<Value, String> {
        self.tick()?;
        match e {
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Nil => Ok(Value::Nil),
            Expr::Var(name) => self
                .get(name)
                .ok_or_else(|| format!("variabel '{}' tak dikenal", name)),
            Expr::Neg(e) => Ok(Value::Int(-self.eval(e)?.as_int())),
            Expr::Not(e) => Ok(Value::Bool(!self.eval(e)?.truthy())),
            Expr::Bin(op, a, b) => self.eval_bin(*op, a, b),
            Expr::Call(name, args) => self.eval_call(name, args),
        }
    }

    fn eval_bin(&mut self, op: BinOp, a: &Expr, b: &Expr) -> Result<Value, String> {
        // short-circuit logical ops
        match op {
            BinOp::And => {
                let l = self.eval(a)?;
                return if !l.truthy() { Ok(Value::Bool(false)) } else { Ok(Value::Bool(self.eval(b)?.truthy())) };
            }
            BinOp::Or => {
                let l = self.eval(a)?;
                return if l.truthy() { Ok(Value::Bool(true)) } else { Ok(Value::Bool(self.eval(b)?.truthy())) };
            }
            _ => {}
        }
        let l = self.eval(a)?;
        let r = self.eval(b)?;
        Ok(match op {
            BinOp::Add => match (&l, &r) {
                (Value::Str(_), _) | (_, Value::Str(_)) => {
                    Value::Str(format!("{}{}", l.display(), r.display()))
                }
                _ => Value::Int(l.as_int().wrapping_add(r.as_int())),
            },
            BinOp::Sub => Value::Int(l.as_int().wrapping_sub(r.as_int())),
            BinOp::Mul => Value::Int(l.as_int().wrapping_mul(r.as_int())),
            BinOp::Div => {
                let d = r.as_int();
                Value::Int(if d == 0 { 0 } else { l.as_int() / d })
            }
            BinOp::Mod => {
                let d = r.as_int();
                Value::Int(if d == 0 { 0 } else { l.as_int() % d })
            }
            BinOp::Eq => Value::Bool(values_eq(&l, &r)),
            BinOp::Ne => Value::Bool(!values_eq(&l, &r)),
            BinOp::Lt => Value::Bool(l.as_int() < r.as_int()),
            BinOp::Le => Value::Bool(l.as_int() <= r.as_int()),
            BinOp::Gt => Value::Bool(l.as_int() > r.as_int()),
            BinOp::Ge => Value::Bool(l.as_int() >= r.as_int()),
            BinOp::And | BinOp::Or => unreachable!(),
        })
    }

    fn eval_call(&mut self, name: &str, args: &[Expr]) -> Result<Value, String> {
        // evaluate arguments
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.eval(a)?);
        }
        // uniform host bindings shared by all three languages
        match name {
            "print" | "console.log" => {
                let mut line = String::new();
                for (k, v) in vals.iter().enumerate() {
                    if k > 0 {
                        line.push(' ');
                    }
                    line.push_str(&v.display());
                }
                if self.out.len() < OUT_LIMIT {
                    self.out.push_str(&line);
                    self.out.push('\n');
                }
                return Ok(Value::Nil);
            }
            "str" => return Ok(Value::Str(vals.first().map(|v| v.display()).unwrap_or_default())),
            "len" => {
                let n = match vals.first() {
                    Some(Value::Str(s)) => s.len() as i64,
                    _ => 0,
                };
                return Ok(Value::Int(n));
            }
            "abs" => return Ok(Value::Int(vals.first().map(|v| v.as_int().abs()).unwrap_or(0))),
            _ => {}
        }
        // user-defined function
        let (params, body) = self
            .funcs
            .get(name)
            .cloned()
            .ok_or_else(|| format!("fungsi '{}' tak dikenal", name))?;
        if params.len() != vals.len() {
            return Err(format!(
                "'{}' butuh {} argumen, diberi {}",
                name,
                params.len(),
                vals.len()
            ));
        }
        let mut local = BTreeMap::new();
        for (p, v) in params.iter().zip(vals) {
            local.insert(p.clone(), v);
        }
        self.scopes.push(local);
        let r = self.run_block(&body);
        self.scopes.pop();
        match r? {
            Flow::Return(v) => Ok(v),
            Flow::Normal => Ok(Value::Nil),
        }
    }
}

fn values_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Nil, Value::Nil) => true,
        _ => a.as_int() == b.as_int(),
    }
}

// --- Public entry point ------------------------------------------------------

pub struct Output {
    pub lines: Vec<String>,
    pub error: Option<String>,
    pub steps: u64,
}

/// Parse and run `src` in the given language. Never panics; parse/runtime
/// errors are returned in `error` alongside any output produced so far.
pub fn run(lang: Lang, src: &str) -> Output {
    let parsed: Result<Vec<Stmt>, String> = match lang {
        Lang::Js => lex_js(src).and_then(parse_js),
        Lang::Ts => {
            let js = ts_to_js(src);
            lex_js(&js).and_then(parse_js)
        }
        Lang::Python => lex_py(src).and_then(parse_py),
    };
    let program = match parsed {
        Ok(p) => p,
        Err(e) => {
            return Output { lines: Vec::new(), error: Some(format!("parse: {}", e)), steps: 0 }
        }
    };
    let mut it = Interp::new();
    // hoist function definitions first (so calls before defs work)
    for s in &program {
        if let Stmt::Func(..) = s {
            let _ = it.exec(s);
        }
    }
    let mut error = None;
    for s in &program {
        if matches!(s, Stmt::Func(..)) {
            continue;
        }
        if let Err(e) = it.exec(s) {
            error = Some(e);
            break;
        }
    }
    let lines: Vec<String> = it.out.lines().map(|l| l.to_string()).collect();
    Output { lines, error, steps: it.steps }
}

// --- Built-in sample programs (the polyglot templates, in three languages) ---

pub const DEMO_JS: &str = r#"// Buitenzorg polyglot demo (JavaScript)
function fib(n) {
  if (n < 2) { return n; }
  return fib(n - 1) + fib(n - 2);
}
let total = 0;
for (let i = 0; i < 10; i = i + 1) {
  total = total + fib(i);
}
console.log("js: fib(10) = " + fib(10));
console.log("js: sum fib(0..9) = " + total);
"#;

pub const DEMO_TS: &str = r#"// Buitenzorg polyglot demo (TypeScript)
interface Calc { n: number; }
type Num = number;
function fib(n: number): number {
  if (n < 2) { return n; }
  return fib(n - 1) + fib(n - 2);
}
let total: Num = 0;
for (let i: number = 0; i < 10; i = i + 1) {
  total = total + fib(i);
}
console.log("ts: fib(10) = " + fib(10));
console.log("ts: sum fib(0..9) = " + total);
"#;

pub const DEMO_PY: &str = r#"# Buitenzorg polyglot demo (Python)
def fib(n):
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

total = 0
for i in range(0, 10):
    total = total + fib(i)

print("py: fib(10) = " + str(fib(10)))
print("py: sum fib(0..9) = " + str(total))
"#;

pub fn demo_source(lang: Lang) -> &'static str {
    match lang {
        Lang::Js => DEMO_JS,
        Lang::Ts => DEMO_TS,
        Lang::Python => DEMO_PY,
    }
}
