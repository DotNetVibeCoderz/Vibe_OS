//! C# script (`.csx`) engine: a subset of C# compiled to bytecode and run by
//! a stack VM. Generic Rust (`alloc` only, no `crate::` references), so the
//! same source runs in the kernel (`csx FILE`, console scripts under a step
//! budget) and in `CLRHOST.ELF` (UI scripts with `Ui.Window(...)`, no budget,
//! see userland/clrhost/src/scriptui.rs).
//!
//! Why bytecode: names are resolved once, at compile time -- locals become
//! frame slots, globals indices, functions indices, method and static names
//! constant-pool entries -- so the VM never searches a scope at run time (the
//! tree-walking interpreter of `script.rs` looks every variable up in maps).
//!
//! Language (C# script style: top-level statements, methods, `using` ignored):
//! types `int long double float decimal bool char string var object`, arrays
//! `T[]` (`new int[n]`, `new[] {..}`, `{..}`), `List<T>`, `Dictionary<K,V>`,
//! `StringBuilder`, `Random`; methods with typed parameters (recursion,
//! defined anywhere); `if/else`, `while`, `do/while`, `for`, `foreach`,
//! `switch` (int/string/char cases), `break`, `continue`, `return`,
//! `try/catch/finally`, `throw new Exception(..)`; operators `+ - * / %`,
//! `== != < <= > >=`, `&& || !`, `& | ^ ~ << >>`, `?:`, `??`, `= += -= *= /=
//! %=`, `++ --` (pre/post), casts `(int)x`; interpolated strings `$"{x}"`
//! with formats `F2 N0 D3 X`, verbatim `@".."`; `Console.WriteLine/Write`,
//! `Math.*`, `int/long/double.Parse`, `Convert.*`, `string.Join/IsNullOrEmpty`,
//! `char.IsDigit/IsLetter/..`, string/list/dictionary methods, `Args`.
//! Not supported: classes/structs/records, lambdas and LINQ, generics of your
//! own, `out`/`ref`, properties, namespaces.

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

// ============================================================================
// Values
// ============================================================================

/// A C# script value.
#[derive(Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Double(f64),
    Char(char),
    Str(Rc<str>),
    /// An array (`false`) or a `List<T>` (`true`).
    Seq(Rc<RefCell<Vec<Value>>>, bool),
    /// `Dictionary<K,V>`: pairs in insertion order.
    Map(Rc<RefCell<Vec<(Value, Value)>>>),
    /// A `KeyValuePair` from `foreach` over a dictionary.
    Pair(Rc<(Value, Value)>),
    Sb(Rc<RefCell<String>>),
    Rng(Rc<Cell<u64>>),
    /// An exception (`catch (Exception e)` -> `e.Message`).
    Exc(Rc<str>),
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }

    /// The text `Console.WriteLine` shows (C# formatting: `True`, `2.5`).
    pub fn display(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(b) => String::from(if *b { "True" } else { "False" }),
            Value::Int(n) => n.to_string(),
            Value::Double(d) => fmt_double(*d),
            Value::Char(c) => c.to_string(),
            Value::Str(s) => String::from(&**s),
            Value::Seq(_, list) => String::from(if *list { "System.Collections.Generic.List`1" } else { "System.Array" }),
            Value::Map(_) => String::from("System.Collections.Generic.Dictionary`2"),
            Value::Pair(p) => format!("[{}, {}]", p.0.display(), p.1.display()),
            Value::Sb(s) => s.borrow().clone(),
            Value::Rng(_) => String::from("System.Random"),
            Value::Exc(m) => format!("System.Exception: {}", m),
        }
    }

    pub fn as_int(&self) -> i64 {
        match self {
            Value::Int(n) => *n,
            Value::Double(d) => *d as i64,
            Value::Char(c) => *c as i64,
            Value::Bool(b) => *b as i64,
            _ => 0,
        }
    }

    pub fn as_f64(&self) -> f64 {
        match self {
            Value::Int(n) => *n as f64,
            Value::Double(d) => *d,
            Value::Char(c) => *c as u32 as f64,
            _ => 0.0,
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Null => false,
            _ => true,
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Double(_) => "double",
            Value::Char(_) => "char",
            Value::Str(_) => "string",
            Value::Seq(_, false) => "array",
            Value::Seq(_, true) => "List",
            Value::Map(_) => "Dictionary",
            Value::Pair(_) => "KeyValuePair",
            Value::Sb(_) => "StringBuilder",
            Value::Rng(_) => "Random",
            Value::Exc(_) => "Exception",
        }
    }
}

fn values_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Null, _) | (_, Value::Null) => false,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Double(_), _) | (_, Value::Double(_)) => a.as_f64() == b.as_f64(),
        (Value::Int(_) | Value::Char(_), Value::Int(_) | Value::Char(_)) => a.as_int() == b.as_int(),
        (Value::Seq(x, _), Value::Seq(y, _)) => Rc::ptr_eq(x, y),
        (Value::Map(x), Value::Map(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

fn fmt_double(d: f64) -> String {
    if d.is_nan() {
        return String::from("NaN");
    }
    if d.is_infinite() {
        return String::from(if d > 0.0 { "∞" } else { "-∞" });
    }
    format!("{}", d)
}

// ============================================================================
// Math without libm (the kernel has none): enough precision for scripts
// ============================================================================

mod m {
    pub const PI: f64 = core::f64::consts::PI;
    const LN2: f64 = core::f64::consts::LN_2;

    pub fn floor(x: f64) -> f64 {
        if !(x.abs() < 4.5e15) {
            return x;
        }
        let t = x as i64 as f64;
        if t > x { t - 1.0 } else { t }
    }
    pub fn ceil(x: f64) -> f64 {
        -floor(-x)
    }
    /// Banker's rounding, like Math.Round.
    pub fn round(x: f64) -> f64 {
        let f = floor(x);
        let d = x - f;
        if d > 0.5 || (d == 0.5 && (f as i64) % 2 != 0) { f + 1.0 } else { f }
    }
    pub fn round_away(x: f64) -> f64 {
        if x >= 0.0 { floor(x + 0.5) } else { -floor(-x + 0.5) }
    }
    pub fn sqrt(x: f64) -> f64 {
        if x < 0.0 || x.is_nan() {
            return f64::NAN;
        }
        if x == 0.0 || x.is_infinite() {
            return x;
        }
        let mut g = f64::from_bits((x.to_bits() >> 1) + (1023u64 << 51));
        for _ in 0..6 {
            g = 0.5 * (g + x / g);
        }
        g
    }
    pub fn exp(x: f64) -> f64 {
        if x > 709.0 {
            return f64::INFINITY;
        }
        if x < -745.0 {
            return 0.0;
        }
        let k = round_away(x / LN2);
        let r = x - k * LN2;
        let (mut term, mut sum) = (1.0, 1.0);
        for i in 1..25 {
            term *= r / i as f64;
            sum += term;
        }
        let ki = k as i64;
        // scale by 2^k in two steps (k may be below the normal range)
        let half = ki / 2;
        let s1 = f64::from_bits(((1023 + half) as u64) << 52);
        let s2 = f64::from_bits(((1023 + ki - half) as u64) << 52);
        sum * s1 * s2
    }
    pub fn ln(x: f64) -> f64 {
        if x < 0.0 || x.is_nan() {
            return f64::NAN;
        }
        if x == 0.0 {
            return f64::NEG_INFINITY;
        }
        if x.is_infinite() {
            return x;
        }
        let bits = x.to_bits();
        let mut e = ((bits >> 52) & 0x7FF) as i64;
        let mut mant = f64::from_bits((bits & 0x000F_FFFF_FFFF_FFFF) | (1023u64 << 52));
        if e == 0 {
            // subnormal
            let y = x * f64::from_bits(1077u64 << 52); // 2^54
            return ln(y) - 54.0 * LN2;
        }
        e -= 1023;
        if mant > core::f64::consts::SQRT_2 {
            mant /= 2.0;
            e += 1;
        }
        let z = (mant - 1.0) / (mant + 1.0);
        let z2 = z * z;
        let (mut term, mut sum) = (z, 0.0);
        let mut k = 1.0;
        for _ in 0..40 {
            sum += term / k;
            term *= z2;
            k += 2.0;
        }
        2.0 * sum + e as f64 * LN2
    }
    pub fn pow(a: f64, b: f64) -> f64 {
        if b == 0.0 {
            return 1.0;
        }
        if b == floor(b) && b.abs() < 1e9 {
            let mut n = b.abs() as u64;
            let (mut base, mut acc) = (a, 1.0);
            while n > 0 {
                if n & 1 == 1 {
                    acc *= base;
                }
                base *= base;
                n >>= 1;
            }
            return if b < 0.0 { 1.0 / acc } else { acc };
        }
        if a < 0.0 {
            return f64::NAN;
        }
        exp(b * ln(a))
    }
    fn reduce(x: f64) -> f64 {
        // to [-pi, pi]
        let two_pi = 2.0 * PI;
        let k = floor(x / two_pi + 0.5);
        x - k * two_pi
    }
    pub fn sin(x: f64) -> f64 {
        if !x.is_finite() {
            return f64::NAN;
        }
        let mut r = reduce(x);
        // to [-pi/2, pi/2]: sin(pi - r) = sin(r)
        if r > PI / 2.0 {
            r = PI - r;
        } else if r < -PI / 2.0 {
            r = -PI - r;
        }
        let r2 = r * r;
        let (mut term, mut sum) = (r, r);
        for i in 1..12 {
            term *= -r2 / ((2 * i) as f64 * (2 * i + 1) as f64);
            sum += term;
        }
        sum
    }
    pub fn cos(x: f64) -> f64 {
        sin(x + PI / 2.0)
    }
    pub fn atan(x: f64) -> f64 {
        if x.is_nan() {
            return x;
        }
        if x.abs() > 1.0 {
            let r = PI / 2.0 - atan(1.0 / x.abs());
            return if x < 0.0 { -r } else { r };
        }
        // halve the argument twice: atan(x) = 2 atan(x / (1 + sqrt(1 + x^2)))
        let mut y = x;
        for _ in 0..2 {
            y /= 1.0 + sqrt(1.0 + y * y);
        }
        let y2 = y * y;
        let (mut term, mut sum) = (y, 0.0);
        let mut k = 1.0;
        for i in 0..30 {
            if i % 2 == 0 { sum += term / k } else { sum -= term / k }
            term *= y2;
            k += 2.0;
        }
        4.0 * sum
    }
    pub fn atan2(y: f64, x: f64) -> f64 {
        if x > 0.0 {
            atan(y / x)
        } else if x < 0.0 {
            if y >= 0.0 { atan(y / x) + PI } else { atan(y / x) - PI }
        } else if y > 0.0 {
            PI / 2.0
        } else if y < 0.0 {
            -PI / 2.0
        } else {
            0.0
        }
    }
}

// ============================================================================
// Lexer
// ============================================================================

#[derive(Clone, PartialEq)]
enum T {
    Int(i64),
    Dbl(f64),
    Str(String),
    /// `$"..."`: literal text and `{expression[:format]}` holes.
    Interp(Vec<(bool, String)>),
    Chr(char),
    Id(String),
    P(&'static str),
    Eof,
}

#[derive(Clone)]
struct Tok {
    t: T,
    line: u32,
    /// No whitespace before this token (`>>` = two glued `>`).
    glued: bool,
}

const PUNCT: &[&str] = &[
    "<<=", "==", "!=", "<=", ">=", "&&", "||", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "=>",
    "<<", "??", "+", "-", "*", "/", "%", "=", "<", ">", "!", "&", "|", "^", "~", "?", ":", ";", ",", ".", "(", ")",
    "[", "]", "{", "}",
];

fn escape(c: char) -> char {
    match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '0' => '\0',
        'a' => '\x07',
        'b' => '\x08',
        other => other,
    }
}

fn lex(src: &str) -> Result<Vec<Tok>, (String, u32)> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    let mut glued = false;
    let n = c.len();
    while i < n {
        let ch = c[i];
        if ch == '\n' {
            line += 1;
            i += 1;
            glued = false;
            continue;
        }
        if ch.is_whitespace() {
            i += 1;
            glued = false;
            continue;
        }
        // comments and preprocessor lines (#r, #load: ignored)
        if ch == '/' && i + 1 < n && c[i + 1] == '/' {
            while i < n && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == '/' && i + 1 < n && c[i + 1] == '*' {
            i += 2;
            while i + 1 < n && !(c[i] == '*' && c[i + 1] == '/') {
                if c[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
            glued = false;
            continue;
        }
        if ch == '#' && out.last().map_or(true, |t: &Tok| t.line < line) {
            while i < n && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        let tline = line;
        let g = glued;
        glued = true;
        // strings: "..", @"..", $"..", $@"..", @$".."
        let (mut interp, mut verbatim, mut j) = (false, false, i);
        while j < n && (c[j] == '$' || c[j] == '@') && j - i < 2 {
            if c[j] == '$' { interp = true } else { verbatim = true }
            j += 1;
        }
        if j < n && c[j] == '"' && (interp || verbatim || j == i) {
            i = j + 1;
            let mut parts: Vec<(bool, String)> = Vec::new();
            let mut cur = String::new();
            loop {
                if i >= n {
                    return Err((String::from("string tidak ditutup"), tline));
                }
                let d = c[i];
                if d == '"' {
                    if verbatim && i + 1 < n && c[i + 1] == '"' {
                        cur.push('"');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                if d == '\n' {
                    if !verbatim {
                        return Err((String::from("string tidak ditutup"), tline));
                    }
                    line += 1;
                }
                if d == '\\' && !verbatim && i + 1 < n {
                    let e = c[i + 1];
                    if e == 'u' && i + 5 < n {
                        let hex: String = c[i + 2..i + 6].iter().collect();
                        cur.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap_or(63)).unwrap_or('?'));
                        i += 6;
                    } else {
                        cur.push(escape(e));
                        i += 2;
                    }
                    continue;
                }
                if interp && d == '{' {
                    if i + 1 < n && c[i + 1] == '{' {
                        cur.push('{');
                        i += 2;
                        continue;
                    }
                    parts.push((false, core::mem::take(&mut cur)));
                    let mut depth = 1;
                    let mut expr = String::new();
                    i += 1;
                    let mut in_str = false;
                    while i < n {
                        let e = c[i];
                        if e == '"' {
                            in_str = !in_str;
                        }
                        if !in_str {
                            if e == '{' {
                                depth += 1;
                            } else if e == '}' {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                        }
                        expr.push(e);
                        i += 1;
                    }
                    if i >= n {
                        return Err((String::from("'}' hilang dalam string $\"...\""), tline));
                    }
                    i += 1;
                    parts.push((true, expr));
                    continue;
                }
                if interp && d == '}' && i + 1 < n && c[i + 1] == '}' {
                    cur.push('}');
                    i += 2;
                    continue;
                }
                cur.push(d);
                i += 1;
            }
            let t = if interp {
                parts.push((false, cur));
                T::Interp(parts)
            } else {
                T::Str(cur)
            };
            out.push(Tok { t, line: tline, glued: g });
            continue;
        }
        if ch == '\'' {
            let (v, len) = if i + 2 < n && c[i + 1] == '\\' {
                (escape(c[i + 2]), 4)
            } else {
                (c.get(i + 1).copied().unwrap_or('?'), 3)
            };
            if c.get(i + len - 1) != Some(&'\'') {
                return Err((String::from("karakter tidak ditutup"), tline));
            }
            out.push(Tok { t: T::Chr(v), line: tline, glued: g });
            i += len;
            continue;
        }
        if ch.is_ascii_digit() || (ch == '.' && i + 1 < n && c[i + 1].is_ascii_digit()) {
            let start = i;
            if ch == '0' && i + 1 < n && (c[i + 1] == 'x' || c[i + 1] == 'X') {
                i += 2;
                let mut v: i64 = 0;
                while i < n && (c[i].is_ascii_hexdigit() || c[i] == '_') {
                    if c[i] != '_' {
                        v = v.wrapping_mul(16) + c[i].to_digit(16).unwrap() as i64;
                    }
                    i += 1;
                }
                while i < n && matches!(c[i], 'L' | 'l' | 'U' | 'u') {
                    i += 1;
                }
                out.push(Tok { t: T::Int(v), line: tline, glued: g });
                continue;
            }
            let mut is_f = false;
            while i < n && (c[i].is_ascii_digit() || c[i] == '_' || (c[i] == '.' && i + 1 < n && c[i + 1].is_ascii_digit())) {
                if c[i] == '.' {
                    is_f = true;
                }
                i += 1;
            }
            if i < n && (c[i] == 'e' || c[i] == 'E') {
                is_f = true;
                i += 1;
                if i < n && (c[i] == '+' || c[i] == '-') {
                    i += 1;
                }
                while i < n && c[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let text: String = c[start..i].iter().filter(|&&d| d != '_').collect();
            if i < n && matches!(c[i], 'f' | 'F' | 'd' | 'D' | 'm' | 'M') {
                is_f = true;
                i += 1;
            } else {
                while i < n && matches!(c[i], 'L' | 'l' | 'U' | 'u') {
                    i += 1;
                }
            }
            let t = if is_f { T::Dbl(parse_f64(&text).unwrap_or(0.0)) } else { T::Int(text.parse::<i64>().unwrap_or(i64::MAX)) };
            out.push(Tok { t, line: tline, glued: g });
            continue;
        }
        if ch.is_alphabetic() || ch == '_' || (ch == '@' && i + 1 < n && c[i + 1].is_alphabetic()) {
            let start = if ch == '@' { i + 1 } else { i };
            i = start;
            while i < n && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let w: String = c[start..i].iter().collect();
            out.push(Tok { t: T::Id(w), line: tline, glued: g });
            continue;
        }
        let mut found = None;
        for p in PUNCT {
            let pc: Vec<char> = p.chars().collect();
            if i + pc.len() <= n && c[i..i + pc.len()] == pc[..] {
                found = Some(*p);
                break;
            }
        }
        match found {
            Some(p) => {
                out.push(Tok { t: T::P(p), line: tline, glued: g });
                i += p.len();
            }
            None => return Err((format!("karakter tak dikenal '{}'", ch), tline)),
        }
    }
    out.push(Tok { t: T::Eof, line, glued: false });
    Ok(out)
}

/// A decimal number (no `str::parse::<f64>` dependence on float formatting quirks).
fn parse_f64(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok()
}

// ============================================================================
// AST
// ============================================================================

#[derive(Clone, PartialEq, Debug)]
enum Ty {
    Int,
    Double,
    Bool,
    Str,
    Char,
    Var,
    Void,
    Obj,
    Arr(Box<Ty>),
    List(Box<Ty>),
    Map(Box<Ty>, Box<Ty>),
    Named(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Bop {
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
    BAnd,
    BOr,
    Xor,
    Shl,
    Shr,
}

#[derive(Clone)]
enum E {
    Lit(Value),
    Interp(Vec<(Option<E>, String)>), // (expression, format) or (None, literal text)
    Name(String, u32),
    Neg(Box<E>),
    Not(Box<E>),
    BNot(Box<E>),
    Bin(Bop, Box<E>, Box<E>),
    And(Box<E>, Box<E>),
    Or(Box<E>, Box<E>),
    Coalesce(Box<E>, Box<E>),
    Cond(Box<E>, Box<E>, Box<E>),
    Assign(Box<E>, Option<Bop>, Box<E>),
    IncDec(Box<E>, bool, i64), // target, prefix, +1 / -1
    Call(Box<E>, Vec<E>, u32),
    Index(Box<E>, Box<E>),
    Member(Box<E>, String),
    New(Ty, Vec<E>, Option<Vec<E>>),
    ArrayInit(Vec<E>),
    Cast(Ty, Box<E>),
}

#[derive(Clone)]
enum S {
    Decl(Ty, Vec<(String, Option<E>)>),
    Expr(E),
    Block(Vec<St>),
    If(E, Box<St>, Option<Box<St>>),
    While(E, Box<St>),
    DoWhile(Box<St>, E),
    For(Option<Box<St>>, Option<E>, Vec<E>, Box<St>),
    Foreach(Ty, String, E, Box<St>),
    Switch(E, Vec<(Vec<Option<E>>, Vec<St>)>),
    Break,
    Continue,
    Return(Option<E>),
    Throw(Option<E>),
    Try(Vec<St>, Option<String>, Vec<St>, Vec<St>), // body, catch var, catch body, finally
    Nop,
}

#[derive(Clone)]
struct St {
    s: S,
    line: u32,
}

#[derive(Clone)]
struct FuncDecl {
    name: String,
    params: Vec<(Ty, String)>,
    ret: Ty,
    body: Vec<St>,
    line: u32,
}

// ============================================================================
// Parser
// ============================================================================

struct Parser {
    t: Vec<Tok>,
    p: usize,
    funcs: Vec<FuncDecl>,
}

type PResult<X> = Result<X, (String, u32)>;

const TYPE_WORDS: &[&str] = &[
    "int", "long", "short", "byte", "sbyte", "uint", "ulong", "ushort", "double", "float", "decimal", "bool", "char",
    "string", "var", "void", "object", "dynamic",
];
const STATIC_CLASSES: &[&str] = &[
    "Console", "Math", "int", "long", "double", "float", "decimal", "string", "char", "bool", "Convert", "Environment",
    "String", "Int32", "Int64", "Double", "Char", "Ui", "MathF", "DateTime",
];

impl Parser {
    fn peek(&self) -> &T {
        &self.t[self.p].t
    }
    fn peek_at(&self, k: usize) -> &T {
        &self.t[(self.p + k).min(self.t.len() - 1)].t
    }
    fn line(&self) -> u32 {
        self.t[self.p].line
    }
    fn is_p(&self, s: &str) -> bool {
        matches!(self.peek(), T::P(p) if *p == s)
    }
    fn is_id(&self, s: &str) -> bool {
        matches!(self.peek(), T::Id(w) if w == s)
    }
    fn eat_p(&mut self, s: &str) -> bool {
        if self.is_p(s) {
            self.p += 1;
            true
        } else {
            false
        }
    }
    fn eat_id(&mut self, s: &str) -> bool {
        if self.is_id(s) {
            self.p += 1;
            true
        } else {
            false
        }
    }
    fn err<X>(&self, msg: &str) -> PResult<X> {
        let near = match self.peek() {
            T::Id(w) => format!(" (dekat '{}')", w),
            T::P(p) => format!(" (dekat '{}')", p),
            T::Eof => String::from(" (di akhir berkas)"),
            _ => String::new(),
        };
        Err((format!("{}{}", msg, near), self.line()))
    }
    fn expect_p(&mut self, s: &str) -> PResult<()> {
        if self.eat_p(s) {
            Ok(())
        } else {
            self.err(&format!("'{}' diharapkan", s))
        }
    }
    fn ident(&mut self) -> PResult<String> {
        if let T::Id(w) = self.peek().clone() {
            self.p += 1;
            Ok(w)
        } else {
            self.err("nama diharapkan")
        }
    }

    /// A type at the cursor, or None (cursor unchanged).
    fn try_type(&mut self) -> Option<Ty> {
        let save = self.p;
        let r = self.type_inner();
        if r.is_none() {
            self.p = save;
        }
        r
    }

    fn type_inner(&mut self) -> Option<Ty> {
        let T::Id(w) = self.peek().clone() else { return None };
        if is_keyword(&w) && !TYPE_WORDS.contains(&w.as_str()) {
            return None;
        }
        self.p += 1;
        let mut ty = match w.as_str() {
            "int" | "long" | "short" | "byte" | "sbyte" | "uint" | "ulong" | "ushort" => Ty::Int,
            "double" | "float" | "decimal" => Ty::Double,
            "bool" => Ty::Bool,
            "string" | "String" => Ty::Str,
            "char" => Ty::Char,
            "var" | "dynamic" => Ty::Var,
            "void" => Ty::Void,
            "object" => Ty::Obj,
            _ => Ty::Named(w.clone()),
        };
        // qualified names: System.Collections.Generic.List<int>
        while self.is_p(".") {
            if let T::Id(next) = self.peek_at(1).clone() {
                self.p += 2;
                ty = Ty::Named(next);
            } else {
                return None;
            }
        }
        if self.is_p("<") {
            self.p += 1;
            let mut args = Vec::new();
            loop {
                args.push(self.type_inner()?);
                if self.eat_p(",") {
                    continue;
                }
                break;
            }
            // `>` closes; `>>` would have been lexed as two `>`
            if !self.eat_p(">") {
                return None;
            }
            ty = match (&ty, args.len()) {
                (Ty::Named(n), 1) if n == "List" || n == "IList" || n == "IEnumerable" => Ty::List(Box::new(args.remove(0))),
                (Ty::Named(n), 2) if n == "Dictionary" || n == "IDictionary" => {
                    let v = args.pop().unwrap();
                    Ty::Map(Box::new(args.pop().unwrap()), Box::new(v))
                }
                _ => ty,
            };
        }
        // nullable `int?` (only when a name follows, else it is `?:`)
        if self.is_p("?") && matches!(self.peek_at(1), T::Id(_)) {
            self.p += 1;
        }
        while self.is_p("[") && matches!(self.peek_at(1), T::P("]")) {
            self.p += 2;
            ty = Ty::Arr(Box::new(ty));
        }
        Some(ty)
    }

    fn program(&mut self) -> PResult<Vec<St>> {
        let mut main = Vec::new();
        while !matches!(self.peek(), T::Eof) {
            // using directives / namespace lines
            if self.is_id("using") && !matches!(self.peek_at(1), T::P("(")) {
                while !self.is_p(";") && !matches!(self.peek(), T::Eof) {
                    self.p += 1;
                }
                self.eat_p(";");
                continue;
            }
            let s = self.statement()?;
            main.push(s);
        }
        Ok(main)
    }

    fn skip_modifiers(&mut self) {
        while let T::Id(w) = self.peek() {
            if matches!(w.as_str(), "static" | "public" | "private" | "internal" | "protected" | "readonly" | "const" | "async" | "unsafe" | "extern" | "sealed") {
                self.p += 1;
            } else {
                break;
            }
        }
    }

    fn block_or_stmt(&mut self) -> PResult<St> {
        self.statement()
    }

    fn block(&mut self) -> PResult<Vec<St>> {
        self.expect_p("{")?;
        let mut v = Vec::new();
        while !self.is_p("}") {
            if matches!(self.peek(), T::Eof) {
                return self.err("'}' hilang");
            }
            v.push(self.statement()?);
        }
        self.p += 1;
        Ok(v)
    }

    fn statement(&mut self) -> PResult<St> {
        let line = self.line();
        let s = self.statement_inner()?;
        Ok(St { s, line })
    }

    fn statement_inner(&mut self) -> PResult<S> {
        if self.is_p("{") {
            return Ok(S::Block(self.block()?));
        }
        if self.eat_p(";") {
            return Ok(S::Nop);
        }
        if let T::Id(w) = self.peek().clone() {
            match w.as_str() {
                "if" => {
                    self.p += 1;
                    self.expect_p("(")?;
                    let c = self.expr()?;
                    self.expect_p(")")?;
                    let then = Box::new(self.block_or_stmt()?);
                    let els = if self.eat_id("else") { Some(Box::new(self.block_or_stmt()?)) } else { None };
                    return Ok(S::If(c, then, els));
                }
                "while" => {
                    self.p += 1;
                    self.expect_p("(")?;
                    let c = self.expr()?;
                    self.expect_p(")")?;
                    return Ok(S::While(c, Box::new(self.block_or_stmt()?)));
                }
                "do" => {
                    self.p += 1;
                    let body = Box::new(self.block_or_stmt()?);
                    if !self.eat_id("while") {
                        return self.err("'while' diharapkan setelah do");
                    }
                    self.expect_p("(")?;
                    let c = self.expr()?;
                    self.expect_p(")")?;
                    self.expect_p(";")?;
                    return Ok(S::DoWhile(body, c));
                }
                "for" => {
                    self.p += 1;
                    self.expect_p("(")?;
                    let init = if self.is_p(";") {
                        None
                    } else {
                        let line = self.line();
                        Some(Box::new(St { s: self.simple_statement()?, line }))
                    };
                    self.expect_p(";")?;
                    let cond = if self.is_p(";") { None } else { Some(self.expr()?) };
                    self.expect_p(";")?;
                    let mut step = Vec::new();
                    while !self.is_p(")") {
                        step.push(self.expr()?);
                        if !self.eat_p(",") {
                            break;
                        }
                    }
                    self.expect_p(")")?;
                    return Ok(S::For(init, cond, step, Box::new(self.block_or_stmt()?)));
                }
                "foreach" => {
                    self.p += 1;
                    self.expect_p("(")?;
                    let ty = match self.try_type() {
                        Some(t) => t,
                        None => return self.err("tipe diharapkan di foreach"),
                    };
                    let name = self.ident()?;
                    if !self.eat_id("in") {
                        return self.err("'in' diharapkan");
                    }
                    let seq = self.expr()?;
                    self.expect_p(")")?;
                    return Ok(S::Foreach(ty, name, seq, Box::new(self.block_or_stmt()?)));
                }
                "switch" => {
                    self.p += 1;
                    self.expect_p("(")?;
                    let subject = self.expr()?;
                    self.expect_p(")")?;
                    self.expect_p("{")?;
                    let mut sections: Vec<(Vec<Option<E>>, Vec<St>)> = Vec::new();
                    while !self.eat_p("}") {
                        let mut labels = Vec::new();
                        loop {
                            if self.eat_id("case") {
                                labels.push(Some(self.expr()?));
                                self.expect_p(":")?;
                            } else if self.eat_id("default") {
                                labels.push(None);
                                self.expect_p(":")?;
                            } else {
                                break;
                            }
                        }
                        if labels.is_empty() {
                            return self.err("'case' atau 'default' diharapkan");
                        }
                        let mut body = Vec::new();
                        while !self.is_id("case") && !self.is_id("default") && !self.is_p("}") {
                            body.push(self.statement()?);
                        }
                        sections.push((labels, body));
                    }
                    return Ok(S::Switch(subject, sections));
                }
                "break" => {
                    self.p += 1;
                    self.expect_p(";")?;
                    return Ok(S::Break);
                }
                "continue" => {
                    self.p += 1;
                    self.expect_p(";")?;
                    return Ok(S::Continue);
                }
                "return" => {
                    self.p += 1;
                    let e = if self.is_p(";") { None } else { Some(self.expr()?) };
                    self.expect_p(";")?;
                    return Ok(S::Return(e));
                }
                "throw" => {
                    self.p += 1;
                    let e = if self.is_p(";") { None } else { Some(self.expr()?) };
                    self.expect_p(";")?;
                    return Ok(S::Throw(e));
                }
                "try" => {
                    self.p += 1;
                    let body = self.block()?;
                    let (mut var, mut catch, mut fin) = (None, Vec::new(), Vec::new());
                    let mut any = false;
                    while self.eat_id("catch") {
                        any = true;
                        if self.eat_p("(") {
                            let _ = self.try_type();
                            if let T::Id(n) = self.peek().clone() {
                                self.p += 1;
                                var = Some(n);
                            }
                            self.expect_p(")")?;
                        }
                        let b = self.block()?;
                        if catch.is_empty() {
                            catch = b; // the first catch handles every exception
                        }
                    }
                    if self.eat_id("finally") {
                        any = true;
                        fin = self.block()?;
                    }
                    if !any {
                        return self.err("'catch' atau 'finally' diharapkan");
                    }
                    return Ok(S::Try(body, var, catch, fin));
                }
                "class" | "struct" | "record" | "interface" | "enum" | "namespace" => {
                    return self.err("class/struct/record/namespace belum didukung di C# script Buitenzorg");
                }
                _ => {}
            }
        }
        let s = self.simple_statement()?;
        if let S::Nop = s {
            return Ok(s); // a method declaration was hoisted
        }
        self.expect_p(";")?;
        Ok(s)
    }

    /// A declaration or an expression (no trailing `;`); a method declaration
    /// is parsed whole, hoisted into `funcs`, and returns Nop.
    fn simple_statement(&mut self) -> PResult<S> {
        let save = self.p;
        self.skip_modifiers();
        let line = self.line();
        if let Some(ty) = self.try_type() {
            if let T::Id(name) = self.peek().clone() {
                if !is_keyword(&name) {
                    self.p += 1;
                    if self.is_p("(") {
                        // method declaration
                        self.p += 1;
                        let mut params = Vec::new();
                        while !self.is_p(")") {
                            self.skip_modifiers();
                            let pty = match self.try_type() {
                                Some(t) => t,
                                None => return self.err("tipe parameter diharapkan"),
                            };
                            let pname = self.ident()?;
                            if self.eat_p("=") {
                                return self.err("parameter default belum didukung");
                            }
                            params.push((pty, pname));
                            if !self.eat_p(",") {
                                break;
                            }
                        }
                        self.expect_p(")")?;
                        let body = if self.eat_p("=>") {
                            let e = self.expr()?;
                            self.expect_p(";")?;
                            if ty == Ty::Void {
                                vec![St { s: S::Expr(e), line }]
                            } else {
                                vec![St { s: S::Return(Some(e)), line }]
                            }
                        } else {
                            self.block()?
                        };
                        self.funcs.push(FuncDecl { name, params, ret: ty, body, line });
                        return Ok(S::Nop);
                    }
                    if self.is_p("=") || self.is_p(";") || self.is_p(",") || self.is_id("in") {
                        let mut vars = Vec::new();
                        let mut first = Some(name);
                        loop {
                            let n = match first.take() {
                                Some(n) => n,
                                None => self.ident()?,
                            };
                            let init = if self.eat_p("=") {
                                if self.is_p("{") {
                                    Some(self.array_init()?)
                                } else {
                                    Some(self.expr()?)
                                }
                            } else {
                                None
                            };
                            vars.push((n, init));
                            if !self.eat_p(",") {
                                break;
                            }
                        }
                        return Ok(S::Decl(ty, vars));
                    }
                }
            }
        }
        self.p = save;
        Ok(S::Expr(self.expr()?))
    }

    fn array_init(&mut self) -> PResult<E> {
        self.expect_p("{")?;
        let mut items = Vec::new();
        while !self.is_p("}") {
            items.push(if self.is_p("{") { self.array_init()? } else { self.expr()? });
            if !self.eat_p(",") {
                break;
            }
        }
        self.expect_p("}")?;
        Ok(E::ArrayInit(items))
    }

    // ---- expressions ----

    fn expr(&mut self) -> PResult<E> {
        self.assignment()
    }

    fn assignment(&mut self) -> PResult<E> {
        let lhs = self.conditional()?;
        let op = match self.peek() {
            T::P("=") => None,
            T::P("+=") => Some(Bop::Add),
            T::P("-=") => Some(Bop::Sub),
            T::P("*=") => Some(Bop::Mul),
            T::P("/=") => Some(Bop::Div),
            T::P("%=") => Some(Bop::Mod),
            T::P("&=") => Some(Bop::BAnd),
            T::P("|=") => Some(Bop::BOr),
            T::P("^=") => Some(Bop::Xor),
            T::P("<<=") => Some(Bop::Shl),
            T::P("=>") => return self.err("lambda belum didukung"),
            _ => return Ok(lhs),
        };
        self.p += 1;
        if !matches!(lhs, E::Name(..) | E::Index(..)) {
            return self.err("hanya variabel atau elemen yang bisa diberi nilai");
        }
        let rhs = if self.is_p("{") { self.array_init()? } else { self.assignment()? };
        Ok(E::Assign(Box::new(lhs), op, Box::new(rhs)))
    }

    fn conditional(&mut self) -> PResult<E> {
        let c = self.coalesce()?;
        if self.eat_p("?") {
            let a = self.assignment()?;
            self.expect_p(":")?;
            let b = self.assignment()?;
            return Ok(E::Cond(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }

    fn coalesce(&mut self) -> PResult<E> {
        let a = self.or()?;
        if self.eat_p("??") {
            let b = self.coalesce()?;
            return Ok(E::Coalesce(Box::new(a), Box::new(b)));
        }
        Ok(a)
    }

    fn or(&mut self) -> PResult<E> {
        let mut a = self.and()?;
        while self.eat_p("||") {
            a = E::Or(Box::new(a), Box::new(self.and()?));
        }
        Ok(a)
    }

    fn and(&mut self) -> PResult<E> {
        let mut a = self.bitor()?;
        while self.eat_p("&&") {
            a = E::And(Box::new(a), Box::new(self.bitor()?));
        }
        Ok(a)
    }

    fn bitor(&mut self) -> PResult<E> {
        let mut a = self.xor()?;
        while self.eat_p("|") {
            a = E::Bin(Bop::BOr, Box::new(a), Box::new(self.xor()?));
        }
        Ok(a)
    }

    fn xor(&mut self) -> PResult<E> {
        let mut a = self.bitand()?;
        while self.eat_p("^") {
            a = E::Bin(Bop::Xor, Box::new(a), Box::new(self.bitand()?));
        }
        Ok(a)
    }

    fn bitand(&mut self) -> PResult<E> {
        let mut a = self.equality()?;
        while self.eat_p("&") {
            a = E::Bin(Bop::BAnd, Box::new(a), Box::new(self.equality()?));
        }
        Ok(a)
    }

    fn equality(&mut self) -> PResult<E> {
        let mut a = self.relational()?;
        loop {
            let op = if self.eat_p("==") {
                Bop::Eq
            } else if self.eat_p("!=") {
                Bop::Ne
            } else {
                break;
            };
            a = E::Bin(op, Box::new(a), Box::new(self.relational()?));
        }
        Ok(a)
    }

    fn relational(&mut self) -> PResult<E> {
        let mut a = self.shift()?;
        loop {
            let op = match self.peek() {
                T::P("<") => Bop::Lt,
                T::P("<=") => Bop::Le,
                T::P(">") if !matches!(self.peek_at(1), T::P(">")) || !self.t[self.p + 1].glued => Bop::Gt,
                T::P(">=") => Bop::Ge,
                _ => break,
            };
            self.p += 1;
            a = E::Bin(op, Box::new(a), Box::new(self.shift()?));
        }
        Ok(a)
    }

    fn shift(&mut self) -> PResult<E> {
        let mut a = self.additive()?;
        loop {
            if self.eat_p("<<") {
                a = E::Bin(Bop::Shl, Box::new(a), Box::new(self.additive()?));
            } else if self.is_p(">") && matches!(self.peek_at(1), T::P(">")) && self.t[self.p + 1].glued {
                self.p += 2;
                a = E::Bin(Bop::Shr, Box::new(a), Box::new(self.additive()?));
            } else {
                break;
            }
        }
        Ok(a)
    }

    fn additive(&mut self) -> PResult<E> {
        let mut a = self.multiplicative()?;
        loop {
            let op = if self.eat_p("+") {
                Bop::Add
            } else if self.eat_p("-") {
                Bop::Sub
            } else {
                break;
            };
            a = E::Bin(op, Box::new(a), Box::new(self.multiplicative()?));
        }
        Ok(a)
    }

    fn multiplicative(&mut self) -> PResult<E> {
        let mut a = self.unary()?;
        loop {
            let op = if self.eat_p("*") {
                Bop::Mul
            } else if self.eat_p("/") {
                Bop::Div
            } else if self.eat_p("%") {
                Bop::Mod
            } else {
                break;
            };
            a = E::Bin(op, Box::new(a), Box::new(self.unary()?));
        }
        Ok(a)
    }

    fn unary(&mut self) -> PResult<E> {
        if self.eat_p("-") {
            return Ok(E::Neg(Box::new(self.unary()?)));
        }
        if self.eat_p("+") {
            return self.unary();
        }
        if self.eat_p("!") {
            return Ok(E::Not(Box::new(self.unary()?)));
        }
        if self.eat_p("~") {
            return Ok(E::BNot(Box::new(self.unary()?)));
        }
        if self.eat_p("++") {
            return Ok(E::IncDec(Box::new(self.unary()?), true, 1));
        }
        if self.eat_p("--") {
            return Ok(E::IncDec(Box::new(self.unary()?), true, -1));
        }
        // cast: (int) x -- only built-in type names, so (a) + b stays a sum
        if self.is_p("(") {
            if let T::Id(w) = self.peek_at(1).clone() {
                if TYPE_WORDS.contains(&w.as_str()) && w != "var" && w != "void" {
                    let save = self.p;
                    self.p += 1;
                    if let Some(ty) = self.try_type() {
                        if self.eat_p(")") {
                            return Ok(E::Cast(ty, Box::new(self.unary()?)));
                        }
                    }
                    self.p = save;
                }
            }
        }
        self.postfix()
    }

    fn args(&mut self) -> PResult<Vec<E>> {
        let mut v = Vec::new();
        while !self.is_p(")") {
            if self.is_id("out") || self.is_id("ref") {
                return self.err("parameter out/ref belum didukung");
            }
            v.push(self.expr()?);
            if !self.eat_p(",") {
                break;
            }
        }
        self.expect_p(")")?;
        Ok(v)
    }

    fn postfix(&mut self) -> PResult<E> {
        let mut e = self.primary()?;
        loop {
            if self.is_p("(") {
                let line = self.line();
                self.p += 1;
                let a = self.args()?;
                e = E::Call(Box::new(e), a, line);
            } else if self.eat_p("[") {
                let i = self.expr()?;
                self.expect_p("]")?;
                e = E::Index(Box::new(e), Box::new(i));
            } else if self.is_p(".") {
                self.p += 1;
                let n = self.ident()?;
                e = E::Member(Box::new(e), n);
            } else if self.eat_p("++") {
                e = E::IncDec(Box::new(e), false, 1);
            } else if self.eat_p("--") {
                e = E::IncDec(Box::new(e), false, -1);
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn primary(&mut self) -> PResult<E> {
        let line = self.line();
        let t = self.peek().clone();
        self.p += 1;
        Ok(match t {
            T::Int(n) => E::Lit(Value::Int(n)),
            T::Dbl(d) => E::Lit(Value::Double(d)),
            T::Str(s) => E::Lit(Value::str(&s)),
            T::Chr(c) => E::Lit(Value::Char(c)),
            T::Interp(parts) => {
                let mut out = Vec::new();
                for (is_expr, text) in parts {
                    if !is_expr {
                        if !text.is_empty() {
                            out.push((None, text));
                        }
                        continue;
                    }
                    // {expr[,align][:format]} -- a ':' at depth 0 outside a ?: starts the format
                    let (src, fmt) = split_format(&text);
                    let toks = lex(&src).map_err(|(m, _)| (m, line))?;
                    let toks = toks.into_iter().map(|mut t| { t.line = line; t }).collect();
                    let mut sub = Parser { t: toks, p: 0, funcs: Vec::new() };
                    let ex = sub.expr()?;
                    out.push((Some(ex), fmt));
                }
                E::Interp(out)
            }
            T::P("(") => {
                let e = self.expr()?;
                self.expect_p(")")?;
                e
            }
            T::Id(w) => match w.as_str() {
                "true" => E::Lit(Value::Bool(true)),
                "false" => E::Lit(Value::Bool(false)),
                "null" => E::Lit(Value::Null),
                "new" => {
                    // new[] { .. }
                    if self.is_p("[") && matches!(self.peek_at(1), T::P("]")) {
                        self.p += 2;
                        return self.array_init();
                    }
                    let Some(ty) = self.try_type_no_array() else { return self.err("tipe diharapkan setelah new") };
                    if self.eat_p("[") {
                        // new int[n] or new int[] { .. }
                        if self.eat_p("]") {
                            while self.is_p("[") && matches!(self.peek_at(1), T::P("]")) {
                                self.p += 2;
                            }
                            let init = self.array_init()?;
                            return Ok(E::New(Ty::Arr(Box::new(ty)), Vec::new(), Some(match init {
                                E::ArrayInit(v) => v,
                                _ => Vec::new(),
                            })));
                        }
                        let len = self.expr()?;
                        self.expect_p("]")?;
                        while self.is_p("[") && matches!(self.peek_at(1), T::P("]")) {
                            self.p += 2;
                        }
                        return Ok(E::New(Ty::Arr(Box::new(ty)), vec![len], None));
                    }
                    let args = if self.eat_p("(") { self.args()? } else { Vec::new() };
                    let init = if self.is_p("{") {
                        match self.collection_init()? {
                            E::ArrayInit(v) => Some(v),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    E::New(ty, args, init)
                }
                "typeof" | "nameof" | "default" | "await" | "sizeof" | "is" | "as" => return self.err(&format!("'{}' belum didukung", w)),
                _ => E::Name(w, line),
            },
            _ => {
                self.p -= 1;
                return self.err("ekspresi diharapkan");
            }
        })
    }

    /// `{ a, b }` or, for dictionaries, `{ { k, v }, ... }` / `{ ["k"] = v }`.
    fn collection_init(&mut self) -> PResult<E> {
        self.expect_p("{")?;
        let mut items = Vec::new();
        while !self.is_p("}") {
            if self.eat_p("[") {
                let k = self.expr()?;
                self.expect_p("]")?;
                self.expect_p("=")?;
                let v = self.expr()?;
                items.push(E::ArrayInit(vec![k, v]));
            } else if self.is_p("{") {
                items.push(self.array_init()?);
            } else {
                items.push(self.expr()?);
            }
            if !self.eat_p(",") {
                break;
            }
        }
        self.expect_p("}")?;
        Ok(E::ArrayInit(items))
    }

    fn try_type_no_array(&mut self) -> Option<Ty> {
        // like try_type, but `new int[5]` must leave the `[` for the length
        let save = self.p;
        let T::Id(w) = self.peek().clone() else { return None };
        if is_keyword(&w) && !TYPE_WORDS.contains(&w.as_str()) {
            return None;
        }
        // parse the type, then give back any `[]` it consumed
        let ty = self.type_inner();
        match ty {
            Some(mut t) => {
                // count trailing [] pairs that were consumed and step back over them
                while let Ty::Arr(inner) = t.clone() {
                    if self.p >= 2 && matches!(self.t[self.p - 1].t, T::P("]")) && matches!(self.t[self.p - 2].t, T::P("[")) {
                        self.p -= 2;
                        t = *inner;
                    } else {
                        break;
                    }
                }
                Some(t)
            }
            None => {
                self.p = save;
                None
            }
        }
    }
}

/// `{expr,align:format}` -> (expr, ",align:format" or "format").
fn split_format(text: &str) -> (String, String) {
    let mut depth = 0i32;
    let mut q = 0i32;
    let mut in_str = false;
    for (i, ch) in text.char_indices() {
        match ch {
            '"' => in_str = !in_str,
            '(' | '[' | '{' if !in_str => depth += 1,
            ')' | ']' | '}' if !in_str => depth -= 1,
            '?' if !in_str && depth == 0 => q += 1,
            ',' if !in_str && depth == 0 => {
                return (String::from(&text[..i]), String::from(&text[i..]));
            }
            ':' if !in_str && depth == 0 => {
                if q > 0 {
                    q -= 1;
                } else {
                    return (String::from(&text[..i]), String::from(&text[i + 1..]));
                }
            }
            _ => {}
        }
    }
    (String::from(text), String::new())
}

fn is_keyword(w: &str) -> bool {
    matches!(
        w,
        "if" | "else" | "while" | "do" | "for" | "foreach" | "in" | "switch" | "case" | "default" | "break" | "continue"
            | "return" | "new" | "true" | "false" | "null" | "try" | "catch" | "finally" | "throw" | "using" | "class"
            | "struct" | "namespace" | "static" | "public" | "private" | "is" | "as" | "out" | "ref" | "typeof"
            | "const" | "readonly" | "this" | "base" | "params" | "operator" | "record" | "interface" | "enum"
    )
}

// ============================================================================
// Bytecode
// ============================================================================

#[derive(Clone, Copy)]
enum Op {
    Const(u32),
    Int(i64),
    Null,
    Pop,
    Dup,
    Dup2,
    LLoad(u32),
    LStore(u32),
    GLoad(u32),
    GStore(u32),
    Bin(Bop),
    Neg,
    Not,
    BNot,
    ToDouble,
    ToInt,
    ToChar,
    ToStr,
    ToBool,
    Jmp(u32),
    Jf(u32),
    JfKeep(u32),
    JtKeep(u32),
    JNotNullKeep(u32),
    Call(u32, u32),
    Native(u32, u32),
    Method(u32, u32),
    Member(u32),
    Index,
    SetIndex,
    NewArr(u8),
    MakeSeq(u32, bool),
    MakeMap(u32),
    NewObj(u32, u32),
    Len,
    IterSeq,
    Fmt(u32),
    Concat(u32),
    Ret,
    RetNull,
    Try(u32, u32), // handler pc, catch slot (u32::MAX: none)
    EndTry,
    Throw,
}

struct Func {
    name: String,
    nparams: u32,
    nlocals: u32,
    code: Rc<[Op]>,
    lines: Rc<[u32]>,
    /// local variables for the debugger: (name, slot, first pc, end pc)
    vars: Rc<[(String, u32, u32, u32)]>,
}

struct Program {
    funcs: Vec<Func>,
    consts: Vec<Value>,
    nglobals: usize,
    global_names: Vec<String>,
    main: u32,
}

struct Loop {
    breaks: Vec<usize>,
    continues: Vec<usize>,
}

struct FnCtx {
    code: Vec<Op>,
    lines: Vec<u32>,
    vars: Vec<(String, u32, u32, u32)>,
    scopes: Vec<Vec<(String, u32, Ty)>>,
    next_slot: u32,
    max_slot: u32,
    loops: Vec<Loop>,
    ret: Ty,
    is_main: bool,
}

struct Compiler {
    consts: Vec<Value>,
    const_strs: Vec<(String, u32)>,
    globals: Vec<(String, Ty)>,
    funcs: Vec<(String, Vec<Ty>, Ty)>, // signatures, index = function id
    line: u32,
}

type CResult<X> = Result<X, (String, u32)>;

impl Compiler {
    fn str_const(&mut self, s: &str) -> u32 {
        if let Some((_, i)) = self.const_strs.iter().find(|(t, _)| t == s) {
            return *i;
        }
        let i = self.consts.len() as u32;
        self.consts.push(Value::str(s));
        self.const_strs.push((String::from(s), i));
        i
    }

    fn val_const(&mut self, v: Value) -> u32 {
        if let Value::Str(s) = &v {
            let s = String::from(&**s);
            return self.str_const(&s);
        }
        self.consts.push(v);
        (self.consts.len() - 1) as u32
    }

    fn emit(&self, f: &mut FnCtx, op: Op) -> usize {
        f.code.push(op);
        f.lines.push(self.line);
        f.code.len() - 1
    }

    fn patch(f: &mut FnCtx, at: usize, target: usize) {
        let t = target as u32;
        f.code[at] = match f.code[at] {
            Op::Jmp(_) => Op::Jmp(t),
            Op::Jf(_) => Op::Jf(t),
            Op::JfKeep(_) => Op::JfKeep(t),
            Op::JtKeep(_) => Op::JtKeep(t),
            Op::JNotNullKeep(_) => Op::JNotNullKeep(t),
            Op::Try(_, s) => Op::Try(t, s),
            other => other,
        };
    }

    fn err<X>(&self, msg: String) -> CResult<X> {
        Err((msg, self.line))
    }

    fn declare(&mut self, f: &mut FnCtx, name: &str, ty: Ty) -> CResult<Option<u32>> {
        // depth-0 statements of the top-level code are fields (globals)
        if f.is_main && f.scopes.len() == 1 {
            if let Some(g) = self.globals.iter().position(|(n, _)| n == name) {
                self.globals[g].1 = ty;
                return Ok(Some(g as u32 | 0x8000_0000));
            }
        }
        if f.scopes.last().unwrap().iter().any(|(n, _, _)| n == name) {
            return self.err(format!("variabel '{}' sudah dideklarasikan", name));
        }
        let slot = f.next_slot;
        f.next_slot += 1;
        f.max_slot = f.max_slot.max(f.next_slot);
        f.scopes.last_mut().unwrap().push((String::from(name), slot, ty));
        f.vars.push((String::from(name), slot, f.code.len() as u32, u32::MAX));
        Ok(Some(slot))
    }

    /// Leave a block: its variables end here (for the debugger).
    fn pop_scope(f: &mut FnCtx) {
        if let Some(scope) = f.scopes.pop() {
            let end = f.code.len() as u32;
            for (_, slot, _) in scope {
                if let Some(v) = f.vars.iter_mut().rev().find(|v| v.1 == slot && v.3 == u32::MAX) {
                    v.3 = end;
                }
            }
        }
    }

    /// (is_global, index, type)
    fn resolve(&self, f: &FnCtx, name: &str) -> Option<(bool, u32, Ty)> {
        for sc in f.scopes.iter().rev() {
            if let Some((_, s, t)) = sc.iter().rev().find(|(n, _, _)| n == name) {
                return Some((false, *s, t.clone()));
            }
        }
        self.globals.iter().position(|(n, _)| n == name).map(|g| (true, g as u32, self.globals[g].1.clone()))
    }

    fn store(&self, f: &mut FnCtx, slot: u32) {
        if slot & 0x8000_0000 != 0 {
            self.emit(f, Op::GStore(slot & 0x7FFF_FFFF));
        } else {
            self.emit(f, Op::LStore(slot));
        }
    }

    fn coerce(&self, f: &mut FnCtx, ty: &Ty) {
        match ty {
            Ty::Double => {
                self.emit(f, Op::ToDouble);
            }
            Ty::Int => {
                // implicit char -> int
                self.emit(f, Op::ToInt);
            }
            _ => {}
        }
    }

    fn function(&mut self, d: &FuncDecl, is_main: bool, globals_main: &[St]) -> CResult<Func> {
        let mut f = FnCtx {
            code: Vec::new(),
            lines: Vec::new(),
            vars: Vec::new(),
            scopes: vec![Vec::new()],
            next_slot: 0,
            max_slot: 0,
            loops: Vec::new(),
            ret: d.ret.clone(),
            is_main,
        };
        self.line = d.line;
        for (ty, n) in &d.params {
            f.scopes[0].push((n.clone(), f.next_slot, ty.clone()));
            f.vars.push((n.clone(), f.next_slot, 0, u32::MAX));
            f.next_slot += 1;
        }
        f.max_slot = f.next_slot;
        // int -> double for double parameters
        for (i, (ty, _)) in d.params.iter().enumerate() {
            if *ty == Ty::Double {
                self.emit(&mut f, Op::LLoad(i as u32));
                self.emit(&mut f, Op::ToDouble);
                self.emit(&mut f, Op::LStore(i as u32));
            }
        }
        let body = if is_main { globals_main } else { &d.body[..] };
        // the top-level code's own scope is scope 0 too (fields)
        for s in body {
            self.stmt(&mut f, s)?;
        }
        self.emit(&mut f, Op::RetNull);
        Ok(Func {
            name: d.name.clone(),
            nparams: d.params.len() as u32,
            nlocals: f.max_slot,
            code: Rc::from(f.code),
            lines: Rc::from(f.lines),
            vars: Rc::from(f.vars),
        })
    }

    fn block(&mut self, f: &mut FnCtx, body: &[St]) -> CResult<()> {
        f.scopes.push(Vec::new());
        let save = f.next_slot;
        for s in body {
            self.stmt(f, s)?;
        }
        Self::pop_scope(f);
        f.next_slot = save;
        Ok(())
    }

    fn scoped(&mut self, f: &mut FnCtx, s: &St) -> CResult<()> {
        match &s.s {
            S::Block(b) => self.block(f, b),
            _ => {
                f.scopes.push(Vec::new());
                let save = f.next_slot;
                self.stmt(f, s)?;
                Self::pop_scope(f);
                f.next_slot = save;
                Ok(())
            }
        }
    }

    fn default_value(&mut self, f: &mut FnCtx, ty: &Ty) {
        match ty {
            Ty::Int => {
                self.emit(f, Op::Int(0));
            }
            Ty::Double => {
                let c = self.val_const(Value::Double(0.0));
                self.emit(f, Op::Const(c));
            }
            Ty::Bool => {
                let c = self.val_const(Value::Bool(false));
                self.emit(f, Op::Const(c));
            }
            Ty::Char => {
                let c = self.val_const(Value::Char('\0'));
                self.emit(f, Op::Const(c));
            }
            _ => {
                self.emit(f, Op::Null);
            }
        }
    }

    fn stmt(&mut self, f: &mut FnCtx, st: &St) -> CResult<()> {
        self.line = st.line;
        match &st.s {
            S::Nop => {}
            S::Expr(e) => {
                self.expr(f, e)?;
                self.emit(f, Op::Pop);
            }
            S::Decl(ty, vars) => {
                for (name, init) in vars {
                    match init {
                        Some(E::ArrayInit(items)) if matches!(ty, Ty::Arr(_) | Ty::List(_) | Ty::Map(..)) => {
                            let ty2 = ty.clone();
                            self.collection(f, &ty2, items)?;
                        }
                        Some(e) => {
                            self.expr(f, e)?;
                            self.coerce(f, ty);
                        }
                        None => {
                            if *ty == Ty::Var {
                                return self.err(format!("'var {}' butuh nilai awal", name));
                            }
                            self.default_value(f, ty);
                        }
                    }
                    let slot = self.declare(f, name, ty.clone())?.unwrap();
                    self.store(f, slot);
                }
            }
            S::Block(b) => self.block(f, b)?,
            S::If(c, a, b) => {
                self.expr(f, c)?;
                let jf = self.emit(f, Op::Jf(0));
                self.scoped(f, a)?;
                if let Some(b) = b {
                    let j = self.emit(f, Op::Jmp(0));
                    let here = f.code.len();
                    Self::patch(f, jf, here);
                    self.scoped(f, b)?;
                    let end = f.code.len();
                    Self::patch(f, j, end);
                } else {
                    let here = f.code.len();
                    Self::patch(f, jf, here);
                }
            }
            S::While(c, body) => {
                let top = f.code.len();
                self.expr(f, c)?;
                let jf = self.emit(f, Op::Jf(0));
                f.loops.push(Loop { breaks: Vec::new(), continues: Vec::new() });
                self.scoped(f, body)?;
                self.emit(f, Op::Jmp(top as u32));
                let end = f.code.len();
                Self::patch(f, jf, end);
                self.close_loop(f, top, end);
            }
            S::DoWhile(body, c) => {
                let top = f.code.len();
                f.loops.push(Loop { breaks: Vec::new(), continues: Vec::new() });
                self.scoped(f, body)?;
                let cont = f.code.len();
                self.line = st.line;
                self.expr(f, c)?;
                let jf = self.emit(f, Op::Jf(0));
                self.emit(f, Op::Jmp(top as u32));
                let end = f.code.len();
                Self::patch(f, jf, end);
                self.close_loop(f, cont, end);
            }
            S::For(init, cond, step, body) => {
                f.scopes.push(Vec::new());
                let save = f.next_slot;
                if let Some(i) = init {
                    self.stmt(f, i)?;
                }
                let top = f.code.len();
                let jf = match cond {
                    Some(c) => {
                        self.expr(f, c)?;
                        Some(self.emit(f, Op::Jf(0)))
                    }
                    None => None,
                };
                f.loops.push(Loop { breaks: Vec::new(), continues: Vec::new() });
                self.scoped(f, body)?;
                let cont = f.code.len();
                self.line = st.line;
                for e in step {
                    self.expr(f, e)?;
                    self.emit(f, Op::Pop);
                }
                self.emit(f, Op::Jmp(top as u32));
                let end = f.code.len();
                if let Some(j) = jf {
                    Self::patch(f, j, end);
                }
                self.close_loop(f, cont, end);
                Self::pop_scope(f);
                f.next_slot = save;
            }
            S::Foreach(ty, name, seq, body) => {
                f.scopes.push(Vec::new());
                let save = f.next_slot;
                self.expr(f, seq)?;
                self.emit(f, Op::IterSeq);
                let s_slot = self.declare(f, "\u{1}seq", Ty::Obj)?.unwrap();
                self.store(f, s_slot);
                self.emit(f, Op::Int(0));
                let i_slot = self.declare(f, "\u{1}i", Ty::Int)?.unwrap();
                self.store(f, i_slot);
                let item_slot = self.declare(f, name, ty.clone())?.unwrap();
                let top = f.code.len();
                self.emit(f, Op::LLoad(i_slot));
                self.emit(f, Op::LLoad(s_slot));
                self.emit(f, Op::Len);
                self.emit(f, Op::Bin(Bop::Lt));
                let jf = self.emit(f, Op::Jf(0));
                self.emit(f, Op::LLoad(s_slot));
                self.emit(f, Op::LLoad(i_slot));
                self.emit(f, Op::Index);
                self.coerce(f, ty);
                self.store(f, item_slot);
                f.loops.push(Loop { breaks: Vec::new(), continues: Vec::new() });
                self.scoped(f, body)?;
                let cont = f.code.len();
                self.emit(f, Op::LLoad(i_slot));
                self.emit(f, Op::Int(1));
                self.emit(f, Op::Bin(Bop::Add));
                self.emit(f, Op::LStore(i_slot));
                self.emit(f, Op::Jmp(top as u32));
                let end = f.code.len();
                Self::patch(f, jf, end);
                self.close_loop(f, cont, end);
                Self::pop_scope(f);
                f.next_slot = save;
            }
            S::Switch(subject, sections) => {
                f.scopes.push(Vec::new());
                let save = f.next_slot;
                self.expr(f, subject)?;
                let tmp = self.declare(f, "\u{1}sw", Ty::Obj)?.unwrap();
                self.store(f, tmp);
                let mut jumps: Vec<(usize, usize)> = Vec::new(); // (jump op, section)
                let mut default = None;
                for (k, (labels, _)) in sections.iter().enumerate() {
                    for l in labels {
                        match l {
                            Some(e) => {
                                self.emit(f, Op::LLoad(tmp));
                                self.expr(f, e)?;
                                self.emit(f, Op::Bin(Bop::Eq));
                                let jf = self.emit(f, Op::Jf(0));
                                let j = self.emit(f, Op::Jmp(0));
                                let here = f.code.len();
                                Self::patch(f, jf, here);
                                jumps.push((j, k));
                            }
                            None => default = Some(k),
                        }
                    }
                }
                let to_default = self.emit(f, Op::Jmp(0));
                f.loops.push(Loop { breaks: Vec::new(), continues: Vec::new() });
                let mut starts = Vec::new();
                for (_, body) in sections {
                    starts.push(f.code.len());
                    self.block(f, body)?;
                }
                let end = f.code.len();
                for (j, k) in jumps {
                    Self::patch(f, j, starts[k]);
                }
                Self::patch(f, to_default, default.map(|k| starts[k]).unwrap_or(end));
                let lp = f.loops.pop().unwrap();
                for b in lp.breaks {
                    Self::patch(f, b, end);
                }
                // `continue` inside a switch belongs to the enclosing loop
                if let Some(outer) = f.loops.last_mut() {
                    outer.continues.extend(lp.continues);
                } else if !lp.continues.is_empty() {
                    return self.err(String::from("'continue' di luar perulangan"));
                }
                Self::pop_scope(f);
                f.next_slot = save;
            }
            S::Break => {
                let j = self.emit(f, Op::Jmp(0));
                match f.loops.last_mut() {
                    Some(l) => l.breaks.push(j),
                    None => return self.err(String::from("'break' di luar perulangan/switch")),
                }
            }
            S::Continue => {
                let j = self.emit(f, Op::Jmp(0));
                match f.loops.last_mut() {
                    Some(l) => l.continues.push(j),
                    None => return self.err(String::from("'continue' di luar perulangan")),
                }
            }
            S::Return(e) => {
                match e {
                    Some(e) => {
                        self.expr(f, e)?;
                        let rt = f.ret.clone();
                        if rt == Ty::Double {
                            self.emit(f, Op::ToDouble);
                        }
                        self.emit(f, Op::Ret);
                    }
                    None => {
                        self.emit(f, Op::RetNull);
                    }
                }
            }
            S::Throw(e) => {
                match e {
                    Some(e) => self.expr(f, e)?,
                    None => {
                        self.emit(f, Op::Null);
                    }
                }
                self.emit(f, Op::Throw);
            }
            S::Try(body, var, catch, fin) => {
                f.scopes.push(Vec::new());
                let save = f.next_slot;
                let slot = match var {
                    Some(v) => self.declare(f, v, Ty::Obj)?.unwrap(),
                    None => u32::MAX,
                };
                let slot = if slot != u32::MAX && slot & 0x8000_0000 != 0 { u32::MAX } else { slot };
                let t = self.emit(f, Op::Try(0, slot));
                self.block(f, body)?;
                self.emit(f, Op::EndTry);
                let j = self.emit(f, Op::Jmp(0));
                let handler = f.code.len();
                Self::patch(f, t, handler);
                self.block(f, catch)?;
                let end = f.code.len();
                Self::patch(f, j, end);
                self.block(f, fin)?;
                Self::pop_scope(f);
                f.next_slot = save;
            }
        }
        Ok(())
    }

    fn close_loop(&mut self, f: &mut FnCtx, cont: usize, end: usize) {
        let lp = f.loops.pop().unwrap();
        for b in lp.breaks {
            Self::patch(f, b, end);
        }
        for c in lp.continues {
            Self::patch(f, c, cont);
        }
    }

    fn collection(&mut self, f: &mut FnCtx, ty: &Ty, items: &[E]) -> CResult<()> {
        match ty {
            Ty::Map(..) => {
                for it in items {
                    match it {
                        E::ArrayInit(kv) if kv.len() == 2 => {
                            self.expr(f, &kv[0])?;
                            self.expr(f, &kv[1])?;
                        }
                        _ => return self.err(String::from("isi Dictionary: { kunci, nilai }")),
                    }
                }
                self.emit(f, Op::MakeMap(items.len() as u32));
            }
            _ => {
                let elem = match ty {
                    Ty::Arr(e) | Ty::List(e) => (**e).clone(),
                    _ => Ty::Var,
                };
                for it in items {
                    match it {
                        E::ArrayInit(inner) => {
                            let inner_ty = Ty::Arr(Box::new(match &elem {
                                Ty::Arr(e) => (**e).clone(),
                                other => other.clone(),
                            }));
                            self.collection(f, &inner_ty, inner)?;
                        }
                        e => {
                            self.expr(f, e)?;
                            self.coerce(f, &elem);
                        }
                    }
                }
                self.emit(f, Op::MakeSeq(items.len() as u32, matches!(ty, Ty::List(_))));
            }
        }
        Ok(())
    }

    /// Load the value of an assignable target (and leave what is needed to store back).
    fn expr(&mut self, f: &mut FnCtx, e: &E) -> CResult<()> {
        match e {
            E::Lit(v) => match v {
                Value::Int(n) => {
                    self.emit(f, Op::Int(*n));
                }
                Value::Null => {
                    self.emit(f, Op::Null);
                }
                other => {
                    let c = self.val_const(other.clone());
                    self.emit(f, Op::Const(c));
                }
            },
            E::Interp(parts) => {
                for (ex, text) in parts {
                    match ex {
                        None => {
                            let c = self.str_const(text);
                            self.emit(f, Op::Const(c));
                        }
                        Some(ex) => {
                            self.expr(f, ex)?;
                            if text.is_empty() {
                                self.emit(f, Op::ToStr);
                            } else {
                                let c = self.str_const(text);
                                self.emit(f, Op::Fmt(c));
                            }
                        }
                    }
                }
                if parts.is_empty() {
                    let c = self.str_const("");
                    self.emit(f, Op::Const(c));
                } else {
                    self.emit(f, Op::Concat(parts.len() as u32));
                }
            }
            E::Name(n, line) => match self.resolve(f, n) {
                Some((true, g, _)) => {
                    self.emit(f, Op::GLoad(g));
                }
                Some((false, s, _)) => {
                    self.emit(f, Op::LLoad(s));
                }
                None => {
                    if n == "Args" {
                        let c = self.str_const("Args");
                        self.emit(f, Op::Native(c, 0));
                    } else {
                        return Err((format!("nama '{}' tidak dikenal", n), *line));
                    }
                }
            },
            E::Neg(a) => {
                self.expr(f, a)?;
                self.emit(f, Op::Neg);
            }
            E::Not(a) => {
                self.expr(f, a)?;
                self.emit(f, Op::Not);
            }
            E::BNot(a) => {
                self.expr(f, a)?;
                self.emit(f, Op::BNot);
            }
            E::Bin(op, a, b) => {
                self.expr(f, a)?;
                self.expr(f, b)?;
                self.emit(f, Op::Bin(*op));
            }
            E::And(a, b) => {
                self.expr(f, a)?;
                let j = self.emit(f, Op::JfKeep(0));
                self.emit(f, Op::Pop);
                self.expr(f, b)?;
                let here = f.code.len();
                Self::patch(f, j, here);
            }
            E::Or(a, b) => {
                self.expr(f, a)?;
                let j = self.emit(f, Op::JtKeep(0));
                self.emit(f, Op::Pop);
                self.expr(f, b)?;
                let here = f.code.len();
                Self::patch(f, j, here);
            }
            E::Coalesce(a, b) => {
                self.expr(f, a)?;
                let j = self.emit(f, Op::JNotNullKeep(0));
                self.emit(f, Op::Pop);
                self.expr(f, b)?;
                let here = f.code.len();
                Self::patch(f, j, here);
            }
            E::Cond(c, a, b) => {
                self.expr(f, c)?;
                let jf = self.emit(f, Op::Jf(0));
                self.expr(f, a)?;
                let j = self.emit(f, Op::Jmp(0));
                let here = f.code.len();
                Self::patch(f, jf, here);
                self.expr(f, b)?;
                let end = f.code.len();
                Self::patch(f, j, end);
            }
            E::Assign(target, op, rhs) => match &**target {
                E::Name(n, line) => {
                    let Some((g, idx, ty)) = self.resolve(f, n) else {
                        return Err((format!("nama '{}' tidak dikenal", n), *line));
                    };
                    if let Some(op) = op {
                        self.emit(f, if g { Op::GLoad(idx) } else { Op::LLoad(idx) });
                        self.expr(f, rhs)?;
                        self.emit(f, Op::Bin(*op));
                    } else if let E::ArrayInit(items) = &**rhs {
                        self.collection(f, &ty, items)?;
                    } else {
                        self.expr(f, rhs)?;
                    }
                    self.coerce(f, &ty);
                    self.emit(f, Op::Dup);
                    self.emit(f, if g { Op::GStore(idx) } else { Op::LStore(idx) });
                }
                E::Index(obj, i) => {
                    self.expr(f, obj)?;
                    self.expr(f, i)?;
                    if let Some(op) = op {
                        self.emit(f, Op::Dup2);
                        self.emit(f, Op::Index);
                        self.expr(f, rhs)?;
                        self.emit(f, Op::Bin(*op));
                    } else {
                        self.expr(f, rhs)?;
                    }
                    self.emit(f, Op::SetIndex);
                }
                _ => return self.err(String::from("target assignment tidak sah")),
            },
            E::IncDec(target, prefix, delta) => match &**target {
                E::Name(n, line) => {
                    let Some((g, idx, ty)) = self.resolve(f, n) else {
                        return Err((format!("nama '{}' tidak dikenal", n), *line));
                    };
                    let load = if g { Op::GLoad(idx) } else { Op::LLoad(idx) };
                    let store = if g { Op::GStore(idx) } else { Op::LStore(idx) };
                    self.emit(f, load);
                    if !*prefix {
                        self.emit(f, Op::Dup);
                    }
                    self.emit(f, Op::Int(*delta));
                    self.emit(f, Op::Bin(Bop::Add));
                    if ty == Ty::Char {
                        self.emit(f, Op::ToChar);
                    }
                    if *prefix {
                        self.emit(f, Op::Dup);
                    }
                    self.emit(f, store);
                }
                E::Index(obj, i) => {
                    // a[i]++ -> value after (prefix) or before (postfix): both give the new value here
                    self.expr(f, obj)?;
                    self.expr(f, i)?;
                    self.emit(f, Op::Dup2);
                    self.emit(f, Op::Index);
                    self.emit(f, Op::Int(*delta));
                    self.emit(f, Op::Bin(Bop::Add));
                    self.emit(f, Op::SetIndex);
                    if !*prefix {
                        self.emit(f, Op::Int(-*delta));
                        self.emit(f, Op::Bin(Bop::Add));
                    }
                }
                _ => return self.err(String::from("++/-- hanya untuk variabel atau elemen")),
            },
            E::Call(callee, args, line) => {
                self.line = *line;
                match &**callee {
                    E::Name(n, _) => {
                        if let Some(fi) = self.funcs.iter().position(|(name, _, _)| name == n) {
                            let params = self.funcs[fi].1.clone();
                            if params.len() != args.len() {
                                return self.err(format!("'{}' butuh {} argumen, diberi {}", n, params.len(), args.len()));
                            }
                            for a in args {
                                self.expr(f, a)?;
                            }
                            self.emit(f, Op::Call(fi as u32, args.len() as u32));
                        } else {
                            for a in args {
                                self.expr(f, a)?;
                            }
                            let c = self.str_const(n);
                            self.emit(f, Op::Native(c, args.len() as u32));
                        }
                    }
                    E::Member(obj, m) => {
                        if let Some(class) = self.static_class(f, obj) {
                            for a in args {
                                self.expr(f, a)?;
                            }
                            let c = self.str_const(&format!("{}.{}", class, m));
                            self.emit(f, Op::Native(c, args.len() as u32));
                        } else {
                            self.expr(f, obj)?;
                            for a in args {
                                self.expr(f, a)?;
                            }
                            let c = self.str_const(m);
                            self.emit(f, Op::Method(c, args.len() as u32));
                        }
                    }
                    _ => return self.err(String::from("pemanggilan tidak sah")),
                }
            }
            E::Index(obj, i) => {
                self.expr(f, obj)?;
                self.expr(f, i)?;
                self.emit(f, Op::Index);
            }
            E::Member(obj, m) => {
                if let Some(class) = self.static_class(f, obj) {
                    let c = self.str_const(&format!("{}.{}", class, m));
                    self.emit(f, Op::Native(c, u32::MAX)); // a static property
                } else {
                    self.expr(f, obj)?;
                    let c = self.str_const(m);
                    self.emit(f, Op::Member(c));
                }
            }
            E::New(ty, args, init) => match ty {
                Ty::Arr(elem) => {
                    if let Some(items) = init {
                        self.collection(f, ty, items)?;
                    } else {
                        self.expr(f, &args[0])?;
                        let kind = match **elem {
                            Ty::Int => 1,
                            Ty::Double => 2,
                            Ty::Bool => 3,
                            Ty::Char => 4,
                            _ => 0,
                        };
                        self.emit(f, Op::NewArr(kind));
                    }
                }
                Ty::List(_) | Ty::Map(..) => {
                    let items = init.clone().unwrap_or_default();
                    if !args.is_empty() && items.is_empty() {
                        // new List<int>(capacity) / new List<int>(other)
                        self.expr(f, &args[0])?;
                        let c = self.str_const(if matches!(ty, Ty::List(_)) { "\u{1}list" } else { "\u{1}map" });
                        self.emit(f, Op::NewObj(c, 1));
                    } else {
                        self.collection(f, ty, &items)?;
                    }
                }
                Ty::Named(n) => {
                    for a in args {
                        self.expr(f, a)?;
                    }
                    let c = self.str_const(n);
                    self.emit(f, Op::NewObj(c, args.len() as u32));
                }
                Ty::Str => {
                    // new string('*', 5)
                    for a in args {
                        self.expr(f, a)?;
                    }
                    let c = self.str_const("String");
                    self.emit(f, Op::NewObj(c, args.len() as u32));
                }
                _ => return self.err(String::from("tipe ini tidak bisa dibuat dengan new")),
            },
            E::ArrayInit(items) => {
                self.collection(f, &Ty::Arr(Box::new(Ty::Var)), items)?;
            }
            E::Cast(ty, a) => {
                self.expr(f, a)?;
                match ty {
                    Ty::Int => {
                        self.emit(f, Op::ToInt);
                    }
                    Ty::Double => {
                        self.emit(f, Op::ToDouble);
                    }
                    Ty::Char => {
                        self.emit(f, Op::ToChar);
                    }
                    Ty::Str => {
                        self.emit(f, Op::ToStr);
                    }
                    Ty::Bool => {
                        self.emit(f, Op::ToBool);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// `Math` in `Math.Sqrt(x)` (a static class, not a variable of that name).
    fn static_class(&self, f: &FnCtx, obj: &E) -> Option<String> {
        if let E::Name(n, _) = obj {
            if self.resolve(f, n).is_none() && (STATIC_CLASSES.contains(&n.as_str()) || n.chars().next().is_some_and(|c| c.is_uppercase())) {
                return Some(String::from(match n.as_str() {
                    "String" => "string",
                    "Int32" => "int",
                    "Int64" => "long",
                    "Double" => "double",
                    "Char" => "char",
                    "MathF" => "Math",
                    other => other,
                }));
            }
        }
        None
    }
}

/// Compile a script. The top-level statements become function `main`.
fn compile(src: &str) -> CResult<Program> {
    let toks = lex(src)?;
    let mut p = Parser { t: toks, p: 0, funcs: Vec::new() };
    let main = p.program()?;
    let funcs = p.funcs;
    let mut c = Compiler { consts: Vec::new(), const_strs: Vec::new(), globals: Vec::new(), funcs: Vec::new(), line: 1 };
    // fields: depth-0 declarations of the top-level code
    for s in &main {
        if let S::Decl(ty, vars) = &s.s {
            for (n, _) in vars {
                if c.globals.iter().any(|(g, _)| g == n) {
                    return Err((format!("variabel '{}' sudah dideklarasikan", n), s.line));
                }
                c.globals.push((n.clone(), ty.clone()));
            }
        }
    }
    for d in &funcs {
        if c.funcs.iter().any(|(n, _, _)| *n == d.name) {
            return Err((format!("method '{}' sudah ada (overload belum didukung)", d.name), d.line));
        }
        c.funcs.push((d.name.clone(), d.params.iter().map(|(t, _)| t.clone()).collect(), d.ret.clone()));
    }
    let mut out = Vec::new();
    for d in &funcs {
        out.push(c.function(d, false, &[])?);
    }
    let main_decl = FuncDecl { name: String::from("<main>"), params: Vec::new(), ret: Ty::Void, body: Vec::new(), line: 1 };
    out.push(c.function(&main_decl, true, &main)?);
    let nglobals = c.globals.len();
    let global_names = c.globals.iter().map(|(n, _)| n.clone()).collect();
    Ok(Program { main: (out.len() - 1) as u32, funcs: out, consts: c.consts, nglobals, global_names })
}

// ============================================================================
// VM
// ============================================================================

/// The embedder's functions (`Ui.Window(...)` in CLRHOST.ELF): called for a
/// static call or a global function the engine does not know.
pub trait Host {
    fn call(&mut self, name: &str, args: &[Value]) -> Option<Result<Value, String>>;
    /// A Console.Write(Line); true when the host showed it.
    fn print(&mut self, _text: &str) -> bool {
        false
    }
}

pub struct Output {
    pub lines: Vec<String>,
    pub error: Option<String>,
    /// Instructions executed.
    pub steps: u64,
    /// the debugger's stops ([`run_debug`]; empty otherwise)
    pub snaps: Vec<Snapshot>,
}

/// The program's state when it reached a stop (as script.rs's).
pub struct Snapshot {
    pub line: u32,
    /// innermost first: (method, line)
    pub stack: Vec<(String, u32)>,
    pub locals: Vec<(String, String)>,
    pub globals: Vec<(String, String)>,
    pub out_lines: usize,
}

struct Dbg {
    breaks: Vec<u32>,
    every: bool,
    max: usize,
    snaps: Vec<Snapshot>,
    /// the last line a stop was considered on, per frame depth
    last: Vec<u32>,
}

/// A value as the debugger shows it: strings quoted, lists and arrays with
/// their first items, dictionaries as `{k: v}`.
fn show(v: &Value) -> String {
    match v {
        Value::Str(t) => {
            let c: String = t.chars().take(60).collect();
            format!("\"{}\"{}", c, if t.chars().count() > 60 { "..." } else { "" })
        }
        Value::Char(c) => format!("'{}'", c),
        Value::Null => String::from("null"),
        Value::Seq(items, list) => {
            let items = items.borrow();
            let shown: Vec<String> = items.iter().take(8).map(show).collect();
            format!("{}[{}{}] ({})", if *list { "List" } else { "" }, shown.join(", "), if items.len() > 8 { ", ..." } else { "" }, items.len())
        }
        Value::Map(pairs) => {
            let pairs = pairs.borrow();
            let shown: Vec<String> = pairs.iter().take(6).map(|(k, x)| format!("{}: {}", show(k), show(x))).collect();
            format!("{{{}{}}} ({})", shown.join(", "), if pairs.len() > 6 { ", ..." } else { "" }, pairs.len())
        }
        v => v.display(),
    }
}

struct Frame {
    func: u32,
    pc: usize,
    base: usize,
    code: Rc<[Op]>,
    lines: Rc<[u32]>,
}

struct Handler {
    frame: usize,
    pc: usize,
    slot: u32,
    stack: usize,
}

struct Vm<'h> {
    dbg: Option<Dbg>,
    prog: Program,
    globals: Vec<Value>,
    stack: Vec<Value>,
    frames: Vec<Frame>,
    handlers: Vec<Handler>,
    out: String,
    host: Option<&'h mut dyn Host>,
    args: Vec<String>,
    steps: u64,
    limit: u64,
    rng: u64,
}

type VResult<X> = Result<X, String>;

fn exc<X>(kind: &str, msg: &str) -> VResult<X> {
    Err(format!("{}: {}", kind, msg))
}

fn index_of(len: usize, i: &Value) -> VResult<usize> {
    let n = i.as_int();
    if n < 0 || n as usize >= len {
        return exc("IndexOutOfRangeException", &format!("indeks {} di luar 0..{}", n, len));
    }
    Ok(n as usize)
}

fn format_value(v: &Value, fmt: &str) -> String {
    let (spec, rest) = fmt.split_at(fmt.char_indices().nth(1).map(|(i, _)| i).unwrap_or(fmt.len()).min(fmt.len()));
    let digits: Option<usize> = rest.parse().ok();
    match spec {
        "F" | "f" => format!("{:.*}", digits.unwrap_or(2), v.as_f64()),
        "N" | "n" => {
            let d = digits.unwrap_or(2);
            let s = format!("{:.*}", d, v.as_f64());
            let (int_part, frac) = match s.find('.') {
                Some(i) => (&s[..i], &s[i..]),
                None => (&s[..], ""),
            };
            let neg = int_part.starts_with('-');
            let digits_only = int_part.trim_start_matches('-');
            let mut grouped = String::new();
            for (i, ch) in digits_only.chars().enumerate() {
                if i > 0 && (digits_only.len() - i) % 3 == 0 {
                    grouped.push(',');
                }
                grouped.push(ch);
            }
            format!("{}{}{}", if neg { "-" } else { "" }, grouped, frac)
        }
        "D" | "d" => {
            let n = v.as_int();
            let s = n.abs().to_string();
            let w = digits.unwrap_or(0);
            let pad = if s.len() < w { "0".repeat(w - s.len()) } else { String::new() };
            format!("{}{}{}", if n < 0 { "-" } else { "" }, pad, s)
        }
        "X" => format!("{:X}", v.as_int()),
        "x" => format!("{:x}", v.as_int()),
        "P" | "p" => format!("{:.*}%", digits.unwrap_or(2), v.as_f64() * 100.0),
        _ => v.display(),
    }
}

impl<'h> Vm<'h> {
    fn print(&mut self, text: &str) {
        if let Some(h) = self.host.as_mut() {
            if h.print(text) {
                return;
            }
        }
        if self.out.len() < 64 * 1024 {
            self.out.push_str(text);
        }
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().unwrap_or(Value::Null)
    }

    fn arith(&self, op: Bop, a: Value, b: Value) -> VResult<Value> {
        use Value::*;
        Ok(match op {
            Bop::Add => match (&a, &b) {
                (Str(_), _) | (_, Str(_)) => Value::str(&(a.display() + &b.display())),
                (Double(_), _) | (_, Double(_)) => Double(a.as_f64() + b.as_f64()),
                _ => Int(a.as_int().wrapping_add(b.as_int())),
            },
            Bop::Sub | Bop::Mul | Bop::Div | Bop::Mod => {
                if matches!(a, Double(_)) || matches!(b, Double(_)) {
                    let (x, y) = (a.as_f64(), b.as_f64());
                    Double(match op {
                        Bop::Sub => x - y,
                        Bop::Mul => x * y,
                        Bop::Div => x / y,
                        // C# %: the remainder has the dividend's sign (truncated division)
                        _ => x - ((x / y) as i64 as f64) * y,
                    })
                } else {
                    let (x, y) = (a.as_int(), b.as_int());
                    Int(match op {
                        Bop::Sub => x.wrapping_sub(y),
                        Bop::Mul => x.wrapping_mul(y),
                        Bop::Div => {
                            if y == 0 {
                                return exc("DivideByZeroException", "pembagian dengan nol");
                            }
                            x.wrapping_div(y)
                        }
                        _ => {
                            if y == 0 {
                                return exc("DivideByZeroException", "pembagian dengan nol");
                            }
                            x.wrapping_rem(y)
                        }
                    })
                }
            }
            Bop::Eq => Bool(values_eq(&a, &b)),
            Bop::Ne => Bool(!values_eq(&a, &b)),
            Bop::Lt | Bop::Le | Bop::Gt | Bop::Ge => {
                let ord = if matches!(a, Double(_)) || matches!(b, Double(_)) {
                    a.as_f64().partial_cmp(&b.as_f64())
                } else if let (Str(x), Str(y)) = (&a, &b) {
                    Some(x.cmp(y))
                } else {
                    Some(a.as_int().cmp(&b.as_int()))
                };
                let Some(o) = ord else { return Ok(Bool(false)) };
                Bool(match op {
                    Bop::Lt => o.is_lt(),
                    Bop::Le => o.is_le(),
                    Bop::Gt => o.is_gt(),
                    _ => o.is_ge(),
                })
            }
            Bop::BAnd => match (&a, &b) {
                (Bool(x), Bool(y)) => Bool(*x & *y),
                _ => Int(a.as_int() & b.as_int()),
            },
            Bop::BOr => match (&a, &b) {
                (Bool(x), Bool(y)) => Bool(*x | *y),
                _ => Int(a.as_int() | b.as_int()),
            },
            Bop::Xor => match (&a, &b) {
                (Bool(x), Bool(y)) => Bool(*x ^ *y),
                _ => Int(a.as_int() ^ b.as_int()),
            },
            Bop::Shl => Int(a.as_int().wrapping_shl(b.as_int() as u32)),
            Bop::Shr => Int(a.as_int().wrapping_shr(b.as_int() as u32)),
        })
    }

    fn index(&self, obj: &Value, i: &Value) -> VResult<Value> {
        match obj {
            Value::Seq(s, _) => {
                let s = s.borrow();
                let k = index_of(s.len(), i)?;
                Ok(s[k].clone())
            }
            Value::Str(s) => {
                let k = i.as_int();
                match s.chars().nth(k.max(0) as usize) {
                    Some(c) if k >= 0 => Ok(Value::Char(c)),
                    _ => exc("IndexOutOfRangeException", &format!("indeks {} di luar string", k)),
                }
            }
            Value::Map(m) => {
                let m = m.borrow();
                match m.iter().find(|(k, _)| values_eq(k, i)) {
                    Some((_, v)) => Ok(v.clone()),
                    None => exc("KeyNotFoundException", &format!("kunci '{}' tidak ada", i.display())),
                }
            }
            Value::Sb(s) => Ok(s.borrow().chars().nth(i.as_int().max(0) as usize).map(Value::Char).unwrap_or(Value::Null)),
            Value::Null => exc("NullReferenceException", "indeks pada null"),
            other => exc("InvalidOperationException", &format!("{} tidak bisa diindeks", other.type_name())),
        }
    }

    fn set_index(&self, obj: &Value, i: &Value, v: Value) -> VResult<()> {
        match obj {
            Value::Seq(s, _) => {
                let mut s = s.borrow_mut();
                let k = index_of(s.len(), i)?;
                s[k] = v;
                Ok(())
            }
            Value::Map(m) => {
                let mut m = m.borrow_mut();
                match m.iter_mut().find(|(k, _)| values_eq(k, i)) {
                    Some(slot) => slot.1 = v,
                    None => m.push((i.clone(), v)),
                }
                Ok(())
            }
            Value::Null => exc("NullReferenceException", "indeks pada null"),
            other => exc("InvalidOperationException", &format!("elemen {} tidak bisa diubah", other.type_name())),
        }
    }

    fn rand(&mut self, cell: Option<&Rc<Cell<u64>>>) -> u64 {
        let mut x = cell.map(|c| c.get()).unwrap_or(self.rng);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        match cell {
            Some(c) => c.set(x),
            None => self.rng = x,
        }
        x
    }

    fn member(&mut self, obj: &Value, name: &str) -> VResult<Value> {
        Ok(match (obj, name) {
            (Value::Seq(s, false), "Length") | (Value::Seq(s, true), "Count") => Value::Int(s.borrow().len() as i64),
            (Value::Seq(s, false), "Count") => Value::Int(s.borrow().len() as i64),
            (Value::Str(s), "Length") => Value::Int(s.chars().count() as i64),
            (Value::Map(m), "Count") => Value::Int(m.borrow().len() as i64),
            (Value::Map(m), "Keys") => Value::Seq(Rc::new(RefCell::new(m.borrow().iter().map(|(k, _)| k.clone()).collect())), true),
            (Value::Map(m), "Values") => Value::Seq(Rc::new(RefCell::new(m.borrow().iter().map(|(_, v)| v.clone()).collect())), true),
            (Value::Pair(p), "Key") => p.0.clone(),
            (Value::Pair(p), "Value") => p.1.clone(),
            (Value::Sb(s), "Length") => Value::Int(s.borrow().chars().count() as i64),
            (Value::Exc(m), "Message") => Value::Str(m.clone()),
            (Value::Null, _) => return exc("NullReferenceException", &format!("'.{}' pada null", name)),
            (other, _) => return exc("MissingMemberException", &format!("{} tidak punya '{}'", other.type_name(), name)),
        })
    }

    fn method(&mut self, recv: Value, name: &str, a: &[Value]) -> VResult<Value> {
        let arg_str = |i: usize| a.get(i).map(|v| v.display()).unwrap_or_default();
        let new_seq = |v: Vec<Value>, list: bool| Value::Seq(Rc::new(RefCell::new(v)), list);
        if name == "ToString" {
            return Ok(match a.first() {
                Some(f) => Value::str(&format_value(&recv, &f.display())),
                None => Value::str(&recv.display()),
            });
        }
        if name == "Equals" && a.len() == 1 {
            return Ok(Value::Bool(values_eq(&recv, &a[0])));
        }
        match &recv {
            Value::Str(s) => {
                let s: &str = s;
                return Ok(match name {
                    "ToUpper" | "ToUpperInvariant" => Value::str(&s.to_uppercase()),
                    "ToLower" | "ToLowerInvariant" => Value::str(&s.to_lowercase()),
                    "Trim" => Value::str(s.trim()),
                    "TrimStart" => Value::str(s.trim_start()),
                    "TrimEnd" => Value::str(s.trim_end()),
                    "Contains" => Value::Bool(s.contains(arg_str(0).as_str())),
                    "StartsWith" => Value::Bool(s.starts_with(arg_str(0).as_str())),
                    "EndsWith" => Value::Bool(s.ends_with(arg_str(0).as_str())),
                    "IndexOf" | "LastIndexOf" => {
                        let needle = arg_str(0);
                        let found = if name == "IndexOf" { s.find(needle.as_str()) } else { s.rfind(needle.as_str()) };
                        Value::Int(found.map(|b| s[..b].chars().count() as i64).unwrap_or(-1))
                    }
                    "Substring" => {
                        let chars: Vec<char> = s.chars().collect();
                        let start = a.first().map(|v| v.as_int()).unwrap_or(0);
                        let len = a.get(1).map(|v| v.as_int()).unwrap_or(chars.len() as i64 - start);
                        if start < 0 || len < 0 || (start + len) as usize > chars.len() {
                            return exc("ArgumentOutOfRangeException", "Substring di luar string");
                        }
                        Value::str(&chars[start as usize..(start + len) as usize].iter().collect::<String>())
                    }
                    "Replace" => Value::str(&s.replace(arg_str(0).as_str(), arg_str(1).as_str())),
                    "Split" => {
                        let sep = arg_str(0);
                        let parts: Vec<Value> = if sep.is_empty() {
                            s.split_whitespace().map(Value::str).collect()
                        } else {
                            s.split(sep.as_str()).map(Value::str).collect()
                        };
                        new_seq(parts, false)
                    }
                    "PadLeft" | "PadRight" => {
                        let w = a.first().map(|v| v.as_int()).unwrap_or(0).max(0) as usize;
                        let c = match a.get(1) {
                            Some(Value::Char(c)) => *c,
                            _ => ' ',
                        };
                        let n = s.chars().count();
                        let pad: String = core::iter::repeat(c).take(w.saturating_sub(n)).collect();
                        Value::str(&if name == "PadLeft" { pad + s } else { String::from(s) + &pad })
                    }
                    "ToCharArray" => new_seq(s.chars().map(Value::Char).collect(), false),
                    "CompareTo" => Value::Int(match s.cmp(arg_str(0).as_str()) {
                        core::cmp::Ordering::Less => -1,
                        core::cmp::Ordering::Equal => 0,
                        core::cmp::Ordering::Greater => 1,
                    }),
                    _ => return exc("MissingMethodException", &format!("string tidak punya method '{}'", name)),
                });
            }
            Value::Seq(s, list) => {
                let list = *list;
                match name {
                    "Add" if list => {
                        s.borrow_mut().push(a.first().cloned().unwrap_or(Value::Null));
                        return Ok(Value::Null);
                    }
                    "AddRange" if list => {
                        if let Some(Value::Seq(o, _)) = a.first() {
                            let items: Vec<Value> = o.borrow().clone();
                            s.borrow_mut().extend(items);
                        }
                        return Ok(Value::Null);
                    }
                    "Insert" if list => {
                        let i = a.first().map(|v| v.as_int()).unwrap_or(0);
                        let len = s.borrow().len();
                        if i < 0 || i as usize > len {
                            return exc("ArgumentOutOfRangeException", "Insert di luar list");
                        }
                        s.borrow_mut().insert(i as usize, a.get(1).cloned().unwrap_or(Value::Null));
                        return Ok(Value::Null);
                    }
                    "RemoveAt" if list => {
                        let len = s.borrow().len();
                        let k = index_of(len, a.first().unwrap_or(&Value::Int(-1)))?;
                        s.borrow_mut().remove(k);
                        return Ok(Value::Null);
                    }
                    "Remove" if list => {
                        let target = a.first().cloned().unwrap_or(Value::Null);
                        let pos = s.borrow().iter().position(|v| values_eq(v, &target));
                        if let Some(p) = pos {
                            s.borrow_mut().remove(p);
                        }
                        return Ok(Value::Bool(pos.is_some()));
                    }
                    "Clear" if list => {
                        s.borrow_mut().clear();
                        return Ok(Value::Null);
                    }
                    "Contains" => {
                        let target = a.first().cloned().unwrap_or(Value::Null);
                        return Ok(Value::Bool(s.borrow().iter().any(|v| values_eq(v, &target))));
                    }
                    "IndexOf" => {
                        let target = a.first().cloned().unwrap_or(Value::Null);
                        return Ok(Value::Int(s.borrow().iter().position(|v| values_eq(v, &target)).map(|p| p as i64).unwrap_or(-1)));
                    }
                    "Sort" => {
                        let mut v = s.borrow_mut();
                        v.sort_by(|x, y| match (x, y) {
                            (Value::Str(p), Value::Str(q)) => p.cmp(q),
                            _ => x.as_f64().partial_cmp(&y.as_f64()).unwrap_or(core::cmp::Ordering::Equal),
                        });
                        return Ok(Value::Null);
                    }
                    "Reverse" => {
                        s.borrow_mut().reverse();
                        return Ok(Value::Null);
                    }
                    "ToArray" => return Ok(new_seq(s.borrow().clone(), false)),
                    "ToList" => return Ok(new_seq(s.borrow().clone(), true)),
                    // a few aggregate helpers (LINQ-like, no lambdas needed)
                    "Sum" => {
                        let v = s.borrow();
                        return Ok(if v.iter().any(|x| matches!(x, Value::Double(_))) {
                            Value::Double(v.iter().map(|x| x.as_f64()).sum())
                        } else {
                            Value::Int(v.iter().map(|x| x.as_int()).sum())
                        });
                    }
                    "Max" | "Min" => {
                        let v = s.borrow();
                        if v.is_empty() {
                            return exc("InvalidOperationException", "list kosong");
                        }
                        let mut best = v[0].clone();
                        for x in v.iter().skip(1) {
                            let better = if name == "Max" { x.as_f64() > best.as_f64() } else { x.as_f64() < best.as_f64() };
                            if better {
                                best = x.clone();
                            }
                        }
                        return Ok(best);
                    }
                    "Average" => {
                        let v = s.borrow();
                        if v.is_empty() {
                            return exc("InvalidOperationException", "list kosong");
                        }
                        return Ok(Value::Double(v.iter().map(|x| x.as_f64()).sum::<f64>() / v.len() as f64));
                    }
                    _ => {}
                }
                return exc("MissingMethodException", &format!("{} tidak punya method '{}'", if list { "List" } else { "array" }, name));
            }
            Value::Map(m) => {
                return Ok(match name {
                    "ContainsKey" => {
                        let k = a.first().cloned().unwrap_or(Value::Null);
                        Value::Bool(m.borrow().iter().any(|(x, _)| values_eq(x, &k)))
                    }
                    "ContainsValue" => {
                        let k = a.first().cloned().unwrap_or(Value::Null);
                        Value::Bool(m.borrow().iter().any(|(_, x)| values_eq(x, &k)))
                    }
                    "Add" => {
                        let k = a.first().cloned().unwrap_or(Value::Null);
                        if m.borrow().iter().any(|(x, _)| values_eq(x, &k)) {
                            return exc("ArgumentException", &format!("kunci '{}' sudah ada", k.display()));
                        }
                        m.borrow_mut().push((k, a.get(1).cloned().unwrap_or(Value::Null)));
                        Value::Null
                    }
                    "Remove" => {
                        let k = a.first().cloned().unwrap_or(Value::Null);
                        let pos = m.borrow().iter().position(|(x, _)| values_eq(x, &k));
                        if let Some(p) = pos {
                            m.borrow_mut().remove(p);
                        }
                        Value::Bool(pos.is_some())
                    }
                    "Clear" => {
                        m.borrow_mut().clear();
                        Value::Null
                    }
                    "GetValueOrDefault" => {
                        let k = a.first().cloned().unwrap_or(Value::Null);
                        m.borrow().iter().find(|(x, _)| values_eq(x, &k)).map(|(_, v)| v.clone()).unwrap_or(a.get(1).cloned().unwrap_or(Value::Null))
                    }
                    _ => return exc("MissingMethodException", &format!("Dictionary tidak punya method '{}'", name)),
                });
            }
            Value::Sb(s) => {
                return Ok(match name {
                    "Append" => {
                        s.borrow_mut().push_str(&arg_str(0));
                        recv.clone()
                    }
                    "AppendLine" => {
                        s.borrow_mut().push_str(&arg_str(0));
                        s.borrow_mut().push('\n');
                        recv.clone()
                    }
                    "Clear" => {
                        s.borrow_mut().clear();
                        recv.clone()
                    }
                    _ => return exc("MissingMethodException", &format!("StringBuilder tidak punya method '{}'", name)),
                });
            }
            Value::Rng(cell) => {
                let cell = cell.clone();
                return Ok(match name {
                    "Next" => {
                        let (lo, hi) = match a.len() {
                            0 => (0, i32::MAX as i64),
                            1 => (0, a[0].as_int()),
                            _ => (a[0].as_int(), a[1].as_int()),
                        };
                        if hi <= lo {
                            Value::Int(lo)
                        } else {
                            let r = self.rand(Some(&cell));
                            Value::Int(lo + (r % (hi - lo) as u64) as i64)
                        }
                    }
                    "NextDouble" => {
                        let r = self.rand(Some(&cell));
                        Value::Double((r >> 11) as f64 / (1u64 << 53) as f64)
                    }
                    _ => return exc("MissingMethodException", &format!("Random tidak punya method '{}'", name)),
                });
            }
            Value::Char(c) => {
                return Ok(match name {
                    "CompareTo" => Value::Int((*c as i64) - a.first().map(|v| v.as_int()).unwrap_or(0)),
                    _ => return exc("MissingMethodException", &format!("char tidak punya method '{}'", name)),
                });
            }
            Value::Null => return exc("NullReferenceException", &format!("'.{}()' pada null", name)),
            _ => {}
        }
        if name == "CompareTo" {
            let (x, y) = (recv.as_f64(), a.first().map(|v| v.as_f64()).unwrap_or(0.0));
            return Ok(Value::Int(if x < y { -1 } else if x > y { 1 } else { 0 }));
        }
        exc("MissingMethodException", &format!("{} tidak punya method '{}'", recv.type_name(), name))
    }

    fn new_obj(&mut self, class: &str, a: &[Value]) -> VResult<Value> {
        Ok(match class {
            "Random" => {
                let seed = match a.first() {
                    Some(v) => (v.as_int() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
                    None => {
                        let r = self.rand(None);
                        r | 1
                    }
                };
                Value::Rng(Rc::new(Cell::new(seed)))
            }
            "StringBuilder" => Value::Sb(Rc::new(RefCell::new(a.first().map(|v| v.display()).unwrap_or_default()))),
            "Exception" | "InvalidOperationException" | "ArgumentException" | "ArgumentOutOfRangeException" | "FormatException" | "NotImplementedException" | "ApplicationException" => {
                let msg = a.first().map(|v| v.display()).unwrap_or_else(|| String::from(class));
                Value::Exc(Rc::from(format!("{}: {}", class, msg).as_str()))
            }
            "String" => {
                let c = a.first().map(|v| v.display()).unwrap_or_default();
                let n = a.get(1).map(|v| v.as_int()).unwrap_or(0).max(0) as usize;
                Value::str(&c.repeat(n))
            }
            "\u{1}list" => match a.first() {
                Some(Value::Seq(s, _)) => Value::Seq(Rc::new(RefCell::new(s.borrow().clone())), true),
                _ => Value::Seq(Rc::new(RefCell::new(Vec::new())), true),
            },
            "\u{1}map" => match a.first() {
                Some(Value::Map(m)) => Value::Map(Rc::new(RefCell::new(m.borrow().clone()))),
                _ => Value::Map(Rc::new(RefCell::new(Vec::new()))),
            },
            other => {
                if let Some(h) = self.host.as_mut() {
                    if let Some(r) = h.call(&format!("new {}", other), a) {
                        return r;
                    }
                }
                return exc("TypeLoadException", &format!("tipe '{}' tidak dikenal", other));
            }
        })
    }

    fn native(&mut self, name: &str, a: &[Value], property: bool) -> VResult<Value> {
        let f = |i: usize| a.get(i).map(|v| v.as_f64()).unwrap_or(0.0);
        let int_like = |v: &Value| matches!(v, Value::Int(_) | Value::Char(_));
        let parse_int = |s: &str| -> VResult<Value> {
            match s.trim().parse::<i64>() {
                Ok(n) => Ok(Value::Int(n)),
                Err(_) => exc("FormatException", &format!("'{}' bukan bilangan bulat", s)),
            }
        };
        if property {
            return Ok(match name {
                "Math.PI" => Value::Double(m::PI),
                "Math.E" => Value::Double(core::f64::consts::E),
                "int.MaxValue" => Value::Int(i32::MAX as i64),
                "int.MinValue" => Value::Int(i32::MIN as i64),
                "long.MaxValue" => Value::Int(i64::MAX),
                "long.MinValue" => Value::Int(i64::MIN),
                "double.MaxValue" => Value::Double(f64::MAX),
                "double.NaN" => Value::Double(f64::NAN),
                "double.PositiveInfinity" => Value::Double(f64::INFINITY),
                "string.Empty" => Value::str(""),
                "Environment.NewLine" => Value::str("\n"),
                "Environment.TickCount" | "Environment.TickCount64" => {
                    if let Some(h) = self.host.as_mut() {
                        if let Some(r) = h.call("Environment.TickCount", &[]) {
                            return r;
                        }
                    }
                    Value::Int((self.steps / 1000) as i64)
                }
                _ => {
                    if let Some(h) = self.host.as_mut() {
                        if let Some(r) = h.call(name, &[]) {
                            return r;
                        }
                    }
                    return exc("MissingMemberException", &format!("'{}' tidak dikenal", name));
                }
            });
        }
        Ok(match name {
            "Console.WriteLine" | "Console.Write" => {
                let mut text = match a.len() {
                    0 => String::new(),
                    1 => a[0].display(),
                    _ => {
                        // Console.WriteLine("{0} + {1}", a, b)
                        let tmpl = a[0].display();
                        let mut s = tmpl.clone();
                        for (i, v) in a.iter().enumerate().skip(1) {
                            s = s.replace(&format!("{{{}}}", i - 1), &v.display());
                        }
                        s
                    }
                };
                if name == "Console.WriteLine" {
                    text.push('\n');
                }
                self.print(&text);
                Value::Null
            }
            "Console.ReadLine" => Value::Null,
            "Math.Abs" => match a.first() {
                Some(v) if int_like(v) => Value::Int(v.as_int().abs()),
                _ => Value::Double(f(0).abs()),
            },
            "Math.Min" | "Math.Max" => {
                let (x, y) = (a.first().cloned().unwrap_or(Value::Int(0)), a.get(1).cloned().unwrap_or(Value::Int(0)));
                if int_like(&x) && int_like(&y) {
                    let (p, q) = (x.as_int(), y.as_int());
                    Value::Int(if (name == "Math.Min") == (p < q) { p } else { q })
                } else {
                    let (p, q) = (x.as_f64(), y.as_f64());
                    Value::Double(if (name == "Math.Min") == (p < q) { p } else { q })
                }
            }
            "Math.Clamp" => {
                if a.iter().all(int_like) {
                    Value::Int(a[0].as_int().max(a[1].as_int()).min(a[2].as_int()))
                } else {
                    Value::Double(f(0).max(f(1)).min(f(2)))
                }
            }
            "Math.Sign" => Value::Int(if f(0) > 0.0 { 1 } else if f(0) < 0.0 { -1 } else { 0 }),
            "Math.Sqrt" => Value::Double(m::sqrt(f(0))),
            "Math.Pow" => Value::Double(m::pow(f(0), f(1))),
            "Math.Exp" => Value::Double(m::exp(f(0))),
            "Math.Log" => Value::Double(if a.len() > 1 { m::ln(f(0)) / m::ln(f(1)) } else { m::ln(f(0)) }),
            "Math.Log10" => Value::Double(m::ln(f(0)) / core::f64::consts::LN_10),
            "Math.Log2" => Value::Double(m::ln(f(0)) / core::f64::consts::LN_2),
            "Math.Sin" => Value::Double(m::sin(f(0))),
            "Math.Cos" => Value::Double(m::cos(f(0))),
            "Math.Tan" => Value::Double(m::sin(f(0)) / m::cos(f(0))),
            "Math.Atan" => Value::Double(m::atan(f(0))),
            "Math.Atan2" => Value::Double(m::atan2(f(0), f(1))),
            "Math.Floor" => Value::Double(m::floor(f(0))),
            "Math.Ceiling" => Value::Double(m::ceil(f(0))),
            "Math.Truncate" => Value::Double(f(0) as i64 as f64),
            "Math.Round" => {
                if a.len() > 1 {
                    let p = m::pow(10.0, a[1].as_int() as f64);
                    Value::Double(m::round(f(0) * p) / p)
                } else {
                    Value::Double(m::round(f(0)))
                }
            }
            "int.Parse" | "long.Parse" | "Convert.ToInt32" | "Convert.ToInt64" => match a.first() {
                Some(Value::Str(s)) => parse_int(s)?,
                Some(v @ Value::Double(_)) => Value::Int(m::round(v.as_f64()) as i64),
                Some(v) => Value::Int(v.as_int()),
                None => Value::Int(0),
            },
            "double.Parse" | "float.Parse" | "decimal.Parse" | "Convert.ToDouble" => match a.first() {
                Some(Value::Str(s)) => match parse_f64(s) {
                    Some(d) => Value::Double(d),
                    None => return exc("FormatException", &format!("'{}' bukan bilangan", s)),
                },
                Some(v) => Value::Double(v.as_f64()),
                None => Value::Double(0.0),
            },
            "Convert.ToString" | "string.Concat" => Value::str(&a.iter().map(|v| v.display()).collect::<String>()),
            "Convert.ToBoolean" | "bool.Parse" => Value::Bool(matches!(a.first(), Some(v) if v.display().eq_ignore_ascii_case("true") || (int_like(v) && v.as_int() != 0))),
            "Convert.ToChar" => Value::Char(char::from_u32(a.first().map(|v| v.as_int()).unwrap_or(0) as u32).unwrap_or('?')),
            "string.IsNullOrEmpty" => Value::Bool(matches!(a.first(), None | Some(Value::Null)) || a[0].display().is_empty()),
            "string.IsNullOrWhiteSpace" => Value::Bool(matches!(a.first(), None | Some(Value::Null)) || a[0].display().trim().is_empty()),
            "string.Join" => {
                let sep = a.first().map(|v| v.display()).unwrap_or_default();
                let items: Vec<String> = match a.get(1) {
                    Some(Value::Seq(s, _)) => s.borrow().iter().map(|v| v.display()).collect(),
                    _ => a.iter().skip(1).map(|v| v.display()).collect(),
                };
                Value::str(&items.join(&sep))
            }
            "string.Format" => {
                let tmpl = a.first().map(|v| v.display()).unwrap_or_default();
                let mut s = tmpl;
                for (i, v) in a.iter().enumerate().skip(1) {
                    s = s.replace(&format!("{{{}}}", i - 1), &v.display());
                }
                Value::str(&s)
            }
            "char.IsDigit" => Value::Bool(matches!(a.first(), Some(Value::Char(c)) if c.is_ascii_digit())),
            "char.IsLetter" => Value::Bool(matches!(a.first(), Some(Value::Char(c)) if c.is_alphabetic())),
            "char.IsLetterOrDigit" => Value::Bool(matches!(a.first(), Some(Value::Char(c)) if c.is_alphanumeric())),
            "char.IsWhiteSpace" => Value::Bool(matches!(a.first(), Some(Value::Char(c)) if c.is_whitespace())),
            "char.IsUpper" => Value::Bool(matches!(a.first(), Some(Value::Char(c)) if c.is_uppercase())),
            "char.IsLower" => Value::Bool(matches!(a.first(), Some(Value::Char(c)) if c.is_lowercase())),
            "char.ToUpper" => match a.first() {
                Some(Value::Char(c)) => Value::Char(c.to_uppercase().next().unwrap_or(*c)),
                _ => Value::Null,
            },
            "char.ToLower" => match a.first() {
                Some(Value::Char(c)) => Value::Char(c.to_lowercase().next().unwrap_or(*c)),
                _ => Value::Null,
            },
            "Args" => Value::Seq(Rc::new(RefCell::new(self.args.iter().map(|s| Value::str(s)).collect())), false),
            _ => {
                if let Some(h) = self.host.as_mut() {
                    if let Some(r) = h.call(name, a) {
                        return r;
                    }
                }
                return exc("MissingMethodException", &format!("'{}' tidak dikenal", name));
            }
        })
    }

    /// Run `func` (main). Returns Err("message (baris N)").
    fn run(&mut self) -> Result<(), String> {
        let main = self.prog.main;
        let f = &self.prog.funcs[main as usize];
        let (code, lines, nlocals) = (f.code.clone(), f.lines.clone(), f.nlocals);
        self.stack.resize(nlocals as usize, Value::Null);
        self.frames.push(Frame { func: main, pc: 0, base: 0, code, lines });
        loop {
            match self.step_loop() {
                Ok(()) => return Ok(()),
                Err(e) => {
                    let line = {
                        let fr = self.frames.last().unwrap();
                        fr.lines.get(fr.pc.saturating_sub(1)).copied().unwrap_or(0)
                    };
                    // a budget overrun or an internal stop is not catchable
                    let catchable = !e.starts_with("\u{1}");
                    if catchable {
                        if let Some(h) = self.handlers.pop() {
                            self.frames.truncate(h.frame + 1);
                            self.stack.truncate(h.stack);
                            let fr = self.frames.last_mut().unwrap();
                            fr.pc = h.pc;
                            if h.slot != u32::MAX {
                                let base = fr.base;
                                // "FormatException: 'x' bukan ..." -> e.Message = "'x' bukan ..."
                                let msg = match e.split_once(": ") {
                                    Some((kind, rest)) if kind.ends_with("Exception") => String::from(rest),
                                    _ => e.clone(),
                                };
                                self.stack[base + h.slot as usize] = Value::Exc(Rc::from(msg.as_str()));
                            }
                            continue;
                        }
                    }
                    let msg = e.trim_start_matches('\u{1}');
                    return Err(format!("baris {}: {}", line, msg));
                }
            }
        }
    }

    /// Before an instruction: a new line in this frame is a statement start;
    /// record a stop there if the debugger wants one.
    fn dbg_check(&mut self, fi: usize) {
        let fr = &self.frames[fi];
        let Some(&line) = fr.lines.get(fr.pc) else { return };
        let d = self.dbg.as_mut().unwrap();
        d.last.resize(fi + 1, 0);
        if d.last[fi] == line || line == 0 {
            return;
        }
        d.last[fi] = line;
        if d.snaps.len() >= d.max || !(d.every || d.breaks.contains(&line)) {
            return;
        }
        let mut stack = Vec::new();
        for (k, f) in self.frames.iter().enumerate().rev() {
            let at = if k == fi { line } else { f.lines.get(f.pc.saturating_sub(1)).copied().unwrap_or(0) };
            stack.push((self.prog.funcs[f.func as usize].name.clone(), at));
        }
        let func = &self.prog.funcs[fr.func as usize];
        let pc = fr.pc as u32;
        let mut locals: Vec<(String, String)> = Vec::new();
        for (name, slot, start, end) in func.vars.iter() {
            if pc >= *start && pc < *end && !name.starts_with('\u{1}') {
                let v = self.stack.get(fr.base + *slot as usize).map(show).unwrap_or_default();
                locals.retain(|(n, _)| n != name); // an inner variable hides an outer one
                locals.push((name.clone(), v));
            }
        }
        let globals = self.prog.global_names.iter().zip(self.globals.iter()).map(|(n, v)| (n.clone(), show(v))).collect();
        let snap = Snapshot { line, stack, locals, globals, out_lines: self.out.lines().count() };
        self.dbg.as_mut().unwrap().snaps.push(snap);
    }

    fn step_loop(&mut self) -> VResult<()> {
        loop {
            let fi = self.frames.len() - 1;
            if self.dbg.is_some() {
                self.dbg_check(fi);
            }
            let (op, base) = {
                let fr = &mut self.frames[fi];
                let Some(op) = fr.code.get(fr.pc).copied() else { return Ok(()) };
                fr.pc += 1;
                (op, fr.base)
            };
            self.steps += 1;
            if self.steps & 0x3FF == 0 && self.steps > self.limit {
                return Err(String::from("\u{1}batas eksekusi tercapai (kemungkinan loop tak berujung)"));
            }
            match op {
                Op::Const(c) => {
                    let v = self.prog.consts[c as usize].clone();
                    self.stack.push(v);
                }
                Op::Int(n) => self.stack.push(Value::Int(n)),
                Op::Null => self.stack.push(Value::Null),
                Op::Pop => {
                    self.stack.pop();
                }
                Op::Dup => {
                    let v = self.stack.last().cloned().unwrap_or(Value::Null);
                    self.stack.push(v);
                }
                Op::Dup2 => {
                    let n = self.stack.len();
                    let (a, b) = (self.stack[n - 2].clone(), self.stack[n - 1].clone());
                    self.stack.push(a);
                    self.stack.push(b);
                }
                Op::LLoad(s) => {
                    let v = self.stack[base + s as usize].clone();
                    self.stack.push(v);
                }
                Op::LStore(s) => {
                    let v = self.pop();
                    self.stack[base + s as usize] = v;
                }
                Op::GLoad(g) => {
                    let v = self.globals[g as usize].clone();
                    self.stack.push(v);
                }
                Op::GStore(g) => {
                    let v = self.pop();
                    self.globals[g as usize] = v;
                }
                Op::Bin(b) => {
                    let y = self.pop();
                    let x = self.pop();
                    // fast path: int op int
                    let r = match (b, &x, &y) {
                        (Bop::Add, Value::Int(p), Value::Int(q)) => Value::Int(p.wrapping_add(*q)),
                        (Bop::Sub, Value::Int(p), Value::Int(q)) => Value::Int(p.wrapping_sub(*q)),
                        (Bop::Mul, Value::Int(p), Value::Int(q)) => Value::Int(p.wrapping_mul(*q)),
                        (Bop::Lt, Value::Int(p), Value::Int(q)) => Value::Bool(p < q),
                        (Bop::Le, Value::Int(p), Value::Int(q)) => Value::Bool(p <= q),
                        (Bop::Gt, Value::Int(p), Value::Int(q)) => Value::Bool(p > q),
                        (Bop::Ge, Value::Int(p), Value::Int(q)) => Value::Bool(p >= q),
                        (Bop::Eq, Value::Int(p), Value::Int(q)) => Value::Bool(p == q),
                        _ => self.arith(b, x, y)?,
                    };
                    self.stack.push(r);
                }
                Op::Neg => {
                    let v = self.pop();
                    self.stack.push(match v {
                        Value::Double(d) => Value::Double(-d),
                        other => Value::Int(other.as_int().wrapping_neg()),
                    });
                }
                Op::Not => {
                    let v = self.pop();
                    self.stack.push(Value::Bool(!v.truthy()));
                }
                Op::BNot => {
                    let v = self.pop();
                    self.stack.push(Value::Int(!v.as_int()));
                }
                Op::ToDouble => {
                    let v = self.pop();
                    self.stack.push(match v {
                        Value::Int(_) | Value::Char(_) => Value::Double(v.as_f64()),
                        other => other,
                    });
                }
                Op::ToInt => {
                    let v = self.pop();
                    self.stack.push(match v {
                        Value::Double(d) => Value::Int(d as i64),
                        Value::Char(c) => Value::Int(c as i64),
                        other => other,
                    });
                }
                Op::ToChar => {
                    let v = self.pop();
                    self.stack.push(match v {
                        Value::Int(_) | Value::Double(_) => Value::Char(char::from_u32(v.as_int() as u32).unwrap_or('?')),
                        other => other,
                    });
                }
                Op::ToStr => {
                    let v = self.pop();
                    self.stack.push(match v {
                        s @ Value::Str(_) => s,
                        other => Value::str(&other.display()),
                    });
                }
                Op::ToBool => {
                    let v = self.pop();
                    self.stack.push(Value::Bool(v.truthy()));
                }
                Op::Jmp(t) => self.frames[fi].pc = t as usize,
                Op::Jf(t) => {
                    if !self.pop().truthy() {
                        self.frames[fi].pc = t as usize;
                    }
                }
                Op::JfKeep(t) => {
                    if !self.stack.last().is_some_and(|v| v.truthy()) {
                        self.frames[fi].pc = t as usize;
                    }
                }
                Op::JtKeep(t) => {
                    if self.stack.last().is_some_and(|v| v.truthy()) {
                        self.frames[fi].pc = t as usize;
                    }
                }
                Op::JNotNullKeep(t) => {
                    if !matches!(self.stack.last(), Some(Value::Null) | None) {
                        self.frames[fi].pc = t as usize;
                    }
                }
                Op::Call(func, argc) => {
                    if self.frames.len() > 3000 {
                        return exc("StackOverflowException", "rekursi terlalu dalam");
                    }
                    let callee = &self.prog.funcs[func as usize];
                    let (code, lines, nlocals) = (callee.code.clone(), callee.lines.clone(), callee.nlocals as usize);
                    let new_base = self.stack.len() - argc as usize;
                    self.stack.resize(new_base + nlocals.max(argc as usize), Value::Null);
                    self.frames.push(Frame { func, pc: 0, base: new_base, code, lines });
                }
                Op::Ret | Op::RetNull => {
                    let v = if matches!(op, Op::Ret) { self.pop() } else { Value::Null };
                    let fr = self.frames.pop().unwrap();
                    // handlers of the returning frame are gone
                    while self.handlers.last().is_some_and(|h| h.frame >= self.frames.len()) {
                        self.handlers.pop();
                    }
                    if self.frames.is_empty() {
                        return Ok(());
                    }
                    self.stack.truncate(fr.base);
                    let _ = fr.func;
                    self.stack.push(v);
                }
                Op::Native(name, argc) => {
                    let property = argc == u32::MAX;
                    let argc = if property { 0 } else { argc as usize };
                    let args: Vec<Value> = self.stack.split_off(self.stack.len() - argc);
                    let n = self.prog.consts[name as usize].display();
                    let r = self.native(&n, &args, property)?;
                    self.stack.push(r);
                }
                Op::Method(name, argc) => {
                    let args: Vec<Value> = self.stack.split_off(self.stack.len() - argc as usize);
                    let recv = self.pop();
                    let n = self.prog.consts[name as usize].display();
                    let r = self.method(recv, &n, &args)?;
                    self.stack.push(r);
                }
                Op::Member(name) => {
                    let obj = self.pop();
                    let n = self.prog.consts[name as usize].display();
                    let r = self.member(&obj, &n)?;
                    self.stack.push(r);
                }
                Op::Index => {
                    let i = self.pop();
                    let obj = self.pop();
                    let r = self.index(&obj, &i)?;
                    self.stack.push(r);
                }
                Op::SetIndex => {
                    let v = self.pop();
                    let i = self.pop();
                    let obj = self.pop();
                    self.set_index(&obj, &i, v.clone())?;
                    self.stack.push(v);
                }
                Op::NewArr(kind) => {
                    let n = self.pop().as_int();
                    if !(0..=16_000_000).contains(&n) {
                        return exc("OverflowException", &format!("panjang array {} tidak sah", n));
                    }
                    let fill = match kind {
                        1 => Value::Int(0),
                        2 => Value::Double(0.0),
                        3 => Value::Bool(false),
                        4 => Value::Char('\0'),
                        _ => Value::Null,
                    };
                    self.stack.push(Value::Seq(Rc::new(RefCell::new(vec![fill; n as usize])), false));
                }
                Op::MakeSeq(n, list) => {
                    let items = self.stack.split_off(self.stack.len() - n as usize);
                    self.stack.push(Value::Seq(Rc::new(RefCell::new(items)), list));
                }
                Op::MakeMap(n) => {
                    let flat = self.stack.split_off(self.stack.len() - 2 * n as usize);
                    let mut pairs: Vec<(Value, Value)> = Vec::new();
                    let mut it = flat.into_iter();
                    while let (Some(k), Some(v)) = (it.next(), it.next()) {
                        match pairs.iter_mut().find(|(x, _)| values_eq(x, &k)) {
                            Some(p) => p.1 = v,
                            None => pairs.push((k, v)),
                        }
                    }
                    self.stack.push(Value::Map(Rc::new(RefCell::new(pairs))));
                }
                Op::NewObj(class, argc) => {
                    let args = self.stack.split_off(self.stack.len() - argc as usize);
                    let n = self.prog.consts[class as usize].display();
                    let r = self.new_obj(&n, &args)?;
                    self.stack.push(r);
                }
                Op::Len => {
                    let v = self.pop();
                    let n = match &v {
                        Value::Seq(s, _) => s.borrow().len(),
                        Value::Str(s) => s.chars().count(),
                        _ => 0,
                    };
                    self.stack.push(Value::Int(n as i64));
                }
                Op::IterSeq => {
                    let v = self.pop();
                    self.stack.push(match v {
                        Value::Seq(s, _) => Value::Seq(Rc::new(RefCell::new(s.borrow().clone())), false),
                        Value::Str(s) => Value::Seq(Rc::new(RefCell::new(s.chars().map(Value::Char).collect())), false),
                        Value::Map(m) => Value::Seq(Rc::new(RefCell::new(m.borrow().iter().map(|(k, v)| Value::Pair(Rc::new((k.clone(), v.clone())))).collect())), false),
                        Value::Null => return exc("NullReferenceException", "foreach pada null"),
                        other => return exc("InvalidOperationException", &format!("foreach tidak bisa atas {}", other.type_name())),
                    });
                }
                Op::Fmt(c) => {
                    let v = self.pop();
                    let fmt = self.prog.consts[c as usize].display();
                    // ",8:F2" (alignment, then format) or "F2" (format only)
                    let (align, spec) = if fmt.starts_with(',') {
                        match fmt.split_once(':') {
                            Some((a, s)) => (a.to_string(), s.to_string()),
                            None => (fmt.clone(), String::new()),
                        }
                    } else {
                        (String::new(), fmt.clone())
                    };
                    let mut s = if spec.is_empty() { v.display() } else { format_value(&v, &spec) };
                    if let Some(w) = align.strip_prefix(',').and_then(|w| w.trim().parse::<i64>().ok()) {
                        let n = s.chars().count() as i64;
                        if w.abs() > n {
                            let pad = " ".repeat((w.abs() - n) as usize);
                            s = if w > 0 { pad + &s } else { s + &pad };
                        }
                    }
                    self.stack.push(Value::str(&s));
                }
                Op::Concat(n) => {
                    let parts = self.stack.split_off(self.stack.len() - n as usize);
                    let mut s = String::new();
                    for p in parts {
                        s.push_str(&p.display());
                    }
                    self.stack.push(Value::str(&s));
                }
                Op::Try(handler, slot) => {
                    self.handlers.push(Handler { frame: fi, pc: handler as usize, slot, stack: self.stack.len() });
                }
                Op::EndTry => {
                    self.handlers.pop();
                }
                Op::Throw => {
                    let v = self.pop();
                    return Err(match v {
                        Value::Exc(m) => String::from(&*m),
                        Value::Null => String::from("Exception: dilempar ulang"),
                        other => format!("Exception: {}", other.display()),
                    });
                }
            }
        }
    }
}

/// Compile and run a C# script. `step_limit` bounds the instructions (the
/// kernel's console path); a host gets `Console` output and unknown calls.
pub fn run_with(src: &str, args: &[String], host: Option<&mut dyn Host>, step_limit: u64) -> Output {
    run_inner(src, args, host, step_limit, None)
}

/// Run under the debugger: a [`Snapshot`] at every statement on a line in
/// `breaks` (or on every line with `every`), at most `max`.
pub fn run_debug(src: &str, breaks: Vec<u32>, every: bool, max: usize) -> Output {
    run_inner(src, &[], None, 50_000_000, Some(Dbg { breaks, every, max, snaps: Vec::new(), last: Vec::new() }))
}

fn run_inner(src: &str, args: &[String], host: Option<&mut dyn Host>, step_limit: u64, dbg: Option<Dbg>) -> Output {
    let prog = match compile(src) {
        Ok(p) => p,
        Err((msg, line)) => {
            return Output { lines: Vec::new(), error: Some(format!("parse: baris {}: {}", line, msg)), steps: 0, snaps: Vec::new() }
        }
    };
    let nglobals = prog.nglobals;
    let mut vm = Vm {
        dbg,
        prog,
        globals: vec![Value::Null; nglobals],
        stack: Vec::with_capacity(256),
        frames: Vec::new(),
        handlers: Vec::new(),
        out: String::new(),
        host,
        args: args.to_vec(),
        steps: 0,
        limit: step_limit,
        rng: 0x2545_F491_4F6C_DD1D,
    };
    let error = vm.run().err();
    let lines = vm.out.lines().map(String::from).collect();
    let snaps = vm.dbg.take().map(|d| d.snaps).unwrap_or_default();
    Output { lines, error, steps: vm.steps, snaps }
}

/// The kernel's console path: 50 million instructions at most.
pub fn run(src: &str, args: &[String]) -> Output {
    run_with(src, args, None, 50_000_000)
}

/// Function names of a script (for tooling).
#[allow(dead_code)]
pub fn functions(src: &str) -> Vec<String> {
    match compile(src) {
        Ok(p) => p.funcs.iter().filter(|f| f.name != "<main>").map(|f| format!("{}({})", f.name, f.nparams)).collect(),
        Err(_) => Vec::new(),
    }
}

/// A built-in sample (shell `csx` with no file).
pub const DEMO: &str = r#"// C# script demo
int Fib(int n) => n < 2 ? n : Fib(n - 1) + Fib(n - 2);
var primes = new List<int>();
for (int i = 2; primes.Count < 10; i++)
{
    bool prime = true;
    for (int d = 2; d * d <= i; d++) if (i % d == 0) { prime = false; break; }
    if (prime) primes.Add(i);
}
Console.WriteLine($"Fib(20) = {Fib(20)}");
Console.WriteLine($"10 prima pertama: {string.Join(", ", primes)}");
Console.WriteLine($"akar 2 = {Math.Sqrt(2):F6}, pi = {Math.PI:F4}");
"#;
