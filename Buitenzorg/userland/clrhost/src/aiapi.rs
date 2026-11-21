//! The OS's built-in AI for programs: managed `Buitenzorg.AI` (in
//! BZAPP.DLL) and the script functions of `scriptui.rs` both come here.
//!
//! - Language models: GGUF llama models through `llm.rs` (the model
//!   `/disk/STORIES.GGU` is on every image). A model is loaded once per
//!   process and named by a small handle; a generation runs either whole
//!   (`AiGenerate`) or one token per call (`AiBegin` + `AiNext`), which is
//!   what lets a UI show the text as it arrives. Matrix products go to the
//!   kernel's GPU compute backend (`COMPUTE_OP`) when one is installed.
//! - Vision: Sobel edges, grayscale, a brightness histogram -- per-pixel
//!   work is far too slow in the interpreter, so it runs here on `int[]`
//!   pictures of 0xAARRGGBB.
//! - The compute backend's name.
//!
//! Managed natives (`pinvoke:buitenzorg!Ai*`):
//! `AiLoad(path, flags)` -> handle >= 1, or -1 no file / -2 not a model /
//! -3 unsupported model (flags 1 = CPU only, 2 = GPU even if it is the
//! kernel's CPU reference); `AiInfo(h, what)` (0 layers, 1 dim, 2 heads, 3
//! KV heads, 4 vocabulary, 5 context, 6 parameters, 7 on GPU, 8 tokens of the
//! last run, 9 its ms, 10 its prompt tokens); `AiText(h, what)` (0 path, 1
//! backend, 2 last error); `AiBegin(h, prompt, steps, temp/1000, seed)` ->
//! prompt tokens; `AiNext(h)` -> text or null at the end; `AiGenerate(h,
//! prompt, steps, temp/1000, seed)` -> text; `AiTokens(h, text)` -> token
//! count; `AiFree(h)`; `AiEdges(int[] src, w, h, int[] dst)`,
//! `AiGray(int[] src, w, h, int[] dst)`, `AiHistogram(int[] src, int[] bins)`;
//! `AiCompute()` -> backend name.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use rustclr_core::{ArrayStorage, ClrArray, ExecResult, Interpreter, Value};
use rustclr_thread::sync::Mutex;

use crate::bzapi::{int, string};
use crate::llm::{Gguf, Model, Stream};
use crate::sys;

/// The longest context a program's model gets (KV cache size).
const MAX_SEQ: usize = 512;

pub struct Slot {
    pub model: Model,
    pub path: String,
    stream: Option<Stream>,
    pub last_tokens: usize,
    pub last_ms: u64,
    pub last_prompt: usize,
    pub error: String,
}

static SLOTS: Mutex<Vec<Option<Box<Slot>>>> = Mutex::new(Vec::new());

/// Load a model (or find it already loaded by this process). Ok(handle) or
/// Err(code, message).
pub fn load(path: &str, flags: i64) -> Result<i64, (i64, String)> {
    {
        let slots = SLOTS.lock().unwrap();
        for (i, s) in slots.iter().enumerate() {
            if s.as_ref().is_some_and(|s| s.path.eq_ignore_ascii_case(path)) {
                return Ok(i as i64 + 1);
            }
        }
    }
    let Some(bytes) = sys::read_file(path) else {
        return Err((-1, alloc::format!("{}: file not found", path)));
    };
    let g = Gguf::parse(&bytes).map_err(|e| (-2, e))?;
    let mut model = Model::load(&g, MAX_SEQ).map_err(|e| (-3, e))?;
    drop(g);
    drop(bytes);
    if flags & 1 == 0 {
        // the GPU when there is one; the CPU otherwise (no error)
        let _ = model.attach_gpu(flags & 2 != 0);
    }
    let slot = Box::new(Slot {
        model,
        path: String::from(path),
        stream: None,
        last_tokens: 0,
        last_ms: 0,
        last_prompt: 0,
        error: String::new(),
    });
    let mut slots = SLOTS.lock().unwrap();
    let i = match slots.iter().position(Option::is_none) {
        Some(i) => {
            slots[i] = Some(slot);
            i
        }
        None => {
            slots.push(Some(slot));
            slots.len() - 1
        }
    };
    Ok(i as i64 + 1)
}

/// Run `f` on a loaded model.
pub fn with<R>(h: i64, f: impl FnOnce(&mut Slot) -> R) -> Option<R> {
    if h < 1 {
        return None;
    }
    let mut slots = SLOTS.lock().unwrap();
    let s = slots.get_mut(h as usize - 1)?.as_mut()?;
    Some(f(s))
}

pub fn free(h: i64) {
    let mut slots = SLOTS.lock().unwrap();
    if h >= 1 && (h as usize) <= slots.len() {
        slots[h as usize - 1] = None;
    }
}

fn temp(milli: i64) -> f32 {
    milli.clamp(0, 5000) as f32 / 1000.0
}

/// Start a generation. Returns the prompt's token count.
pub fn begin(h: i64, prompt: &str, steps: i64, temp_milli: i64, seed: i64) -> Option<usize> {
    with(h, |s| {
        let st = Stream::begin(&mut s.model, prompt, steps.clamp(0, MAX_SEQ as i64) as usize, temp(temp_milli), seed as u64);
        let n = st.prompt_tokens;
        s.last_prompt = n;
        s.last_tokens = 0;
        s.last_ms = 0;
        s.stream = Some(st);
        n
    })
}

/// The next piece of text, or None when the generation is over (or none
/// was started).
pub fn next(h: i64) -> Option<String> {
    with(h, |s| {
        let mut st = s.stream.take()?;
        // pieces that are half a UTF-8 character are joined with the next one
        let mut bytes = Vec::new();
        let r = loop {
            match st.next(&mut s.model) {
                Ok(Some(b)) => {
                    bytes.extend_from_slice(&b);
                    if core::str::from_utf8(&bytes).is_ok() {
                        break Some(());
                    }
                }
                Ok(None) => break None,
                Err(e) => {
                    s.error = e;
                    break None;
                }
            }
        };
        s.last_tokens = st.generated;
        s.last_ms = st.elapsed_ms();
        if r.is_none() && bytes.is_empty() {
            return None;
        }
        s.stream = Some(st);
        Some(String::from_utf8_lossy(&bytes).into_owned())
    })
    .flatten()
}

/// A whole generation.
pub fn generate(h: i64, prompt: &str, steps: i64, temp_milli: i64, seed: i64) -> Option<String> {
    begin(h, prompt, steps, temp_milli, seed)?;
    let mut out = String::new();
    while let Some(p) = next(h) {
        out.push_str(&p);
    }
    Some(out)
}

pub fn info(h: i64, what: i64) -> i64 {
    with(h, |s| {
        let m = &s.model;
        (match what {
            0 => m.layers_n,
            1 => m.dim,
            2 => m.heads,
            3 => m.kv_heads,
            4 => m.vocab,
            5 => m.seq,
            6 => m.params(),
            7 => m.gpu.is_some() as usize,
            8 => s.last_tokens,
            9 => s.last_ms as usize,
            10 => s.last_prompt,
            _ => return -1,
        }) as i64
    })
    .unwrap_or(-1)
}

pub fn text(h: i64, what: i64) -> String {
    with(h, |s| match what {
        0 => s.path.clone(),
        1 => s.model.gpu.as_ref().map_or(String::from("CPU"), |g| g.name.clone()),
        2 => s.error.clone(),
        _ => String::new(),
    })
    .unwrap_or_default()
}

/// Name of the kernel's compute backend ("" when there is none).
pub fn compute_name() -> String {
    let mut name = alloc::vec![0u8; 64];
    let r = sys::syscall(72, 0, name.as_mut_ptr() as u64, name.len() as u64);
    if sys::is_err(r) {
        return String::new();
    }
    let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    String::from_utf8_lossy(&name[..end]).into_owned()
}

// ---- vision -------------------------------------------------------------------------

fn luma(p: i32) -> i32 {
    let p = p as u32;
    ((p >> 16 & 0xFF) * 77 + (p >> 8 & 0xFF) * 150 + (p & 0xFF) * 29) as i32 >> 8
}

/// Sobel edge magnitude as gray pixels (opaque).
pub fn edges(src: &[i32], w: usize, h: usize, dst: &mut [i32]) -> bool {
    if w < 3 || h < 3 || src.len() < w * h || dst.len() < w * h {
        return false;
    }
    let l: Vec<i32> = src[..w * h].iter().map(|&p| luma(p)).collect();
    for y in 0..h {
        for x in 0..w {
            let v = if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                0
            } else {
                let at = |dx: isize, dy: isize| l[(y as isize + dy) as usize * w + (x as isize + dx) as usize];
                let gx = -at(-1, -1) - 2 * at(-1, 0) - at(-1, 1) + at(1, -1) + 2 * at(1, 0) + at(1, 1);
                let gy = -at(-1, -1) - 2 * at(0, -1) - at(1, -1) + at(-1, 1) + 2 * at(0, 1) + at(1, 1);
                ((gx.abs() + gy.abs()) / 2).min(255)
            };
            dst[y * w + x] = (0xFF00_0000u32 | (v as u32) << 16 | (v as u32) << 8 | v as u32) as i32;
        }
    }
    true
}

pub fn gray(src: &[i32], w: usize, h: usize, dst: &mut [i32]) -> bool {
    if src.len() < w * h || dst.len() < w * h {
        return false;
    }
    for i in 0..w * h {
        let v = luma(src[i]) as u32;
        dst[i] = ((src[i] as u32 & 0xFF00_0000) | v << 16 | v << 8 | v) as i32;
    }
    true
}

// ---- managed natives ----------------------------------------------------------------

fn ints(i: &Interpreter, a: &[Value], idx: usize) -> Vec<i32> {
    let Some(Value::Obj(h)) = a.get(idx) else { return Vec::new() };
    i.heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::I32(v) => v.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

fn put_ints(i: &mut Interpreter, a: &[Value], idx: usize, data: &[i32]) -> bool {
    let Some(Value::Obj(h)) = a.get(idx) else { return false };
    i.heap
        .with_mut::<ClrArray, _>(*h, |arr| match &mut arr.storage {
            ArrayStorage::I32(v) => {
                let n = v.len().min(data.len());
                v[..n].copy_from_slice(&data[..n]);
                true
            }
            _ => false,
        })
        .unwrap_or(false)
}

fn str_or_null(i: &mut Interpreter, s: Option<String>) -> ExecResult<Option<Value>> {
    Ok(Some(match s {
        Some(s) => Value::Obj(i.alloc_string(&s)),
        None => Value::Null,
    }))
}

fn ai_load(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (path, flags) = (string(i, a, 0), int(a, 1));
    // loading reads and converts the whole file: no safe point in between
    let r = i.blocking(|_| load(&path, flags));
    Ok(Some(Value::I32(match r {
        Ok(h) => h as i32,
        Err((code, msg)) => {
            sys::print(&alloc::format!("[ai] {}\n", msg));
            code as i32
        }
    })))
}

fn ai_info(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    Ok(Some(Value::I64(info(int(a, 0), int(a, 1)))))
}

fn ai_text(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let s = text(int(a, 0), int(a, 1));
    Ok(Some(Value::Obj(i.alloc_string(&s))))
}

fn ai_begin(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let p = string(i, a, 1);
    let (h, steps, t, seed) = (int(a, 0), int(a, 2), int(a, 3), int(a, 4));
    let r = i.blocking(|_| begin(h, &p, steps, t, seed));
    Ok(Some(Value::I32(r.map_or(-1, |n| n as i32))))
}

fn ai_next(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let h = int(a, 0);
    let r = i.blocking(|_| next(h));
    str_or_null(i, r)
}

fn ai_generate(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let p = string(i, a, 1);
    let (h, steps, t, seed) = (int(a, 0), int(a, 2), int(a, 3), int(a, 4));
    let r = i.blocking(|_| generate(h, &p, steps, t, seed));
    str_or_null(i, r)
}

fn ai_tokens(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let t = string(i, a, 1);
    let n = with(int(a, 0), |s| s.model.encode(&t, false).len() as i32).unwrap_or(-1);
    Ok(Some(Value::I32(n)))
}

fn ai_free(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    free(int(a, 0));
    Ok(None)
}

fn ai_edges(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let src = ints(i, a, 0);
    let (w, h) = (int(a, 1).max(0) as usize, int(a, 2).max(0) as usize);
    let mut dst = alloc::vec![0i32; w * h];
    let ok = edges(&src, w, h, &mut dst) && put_ints(i, a, 3, &dst);
    Ok(Some(Value::I32(if ok { 0 } else { -1 })))
}

fn ai_gray(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let src = ints(i, a, 0);
    let (w, h) = (int(a, 1).max(0) as usize, int(a, 2).max(0) as usize);
    let mut dst = alloc::vec![0i32; w * h];
    let ok = gray(&src, w, h, &mut dst) && put_ints(i, a, 3, &dst);
    Ok(Some(Value::I32(if ok { 0 } else { -1 })))
}

/// `AiHistogram(int[] src, int[] bins)`: brightness counts in bins.Length buckets.
fn ai_histogram(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let src = ints(i, a, 0);
    let n = ints(i, a, 1).len();
    if n == 0 {
        return Ok(Some(Value::I32(-1)));
    }
    let mut bins = alloc::vec![0i32; n];
    for &p in &src {
        bins[(luma(p) as usize * n / 256).min(n - 1)] += 1;
    }
    let ok = put_ints(i, a, 1, &bins);
    Ok(Some(Value::I32(if ok { 0 } else { -1 })))
}

fn ai_compute(i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    let s = compute_name();
    Ok(Some(Value::Obj(i.alloc_string(&s))))
}

pub fn register(interp: &mut Interpreter) {
    let natives: [(&str, rustclr_core::interp::NativeFn); 13] = [
        ("AiLoad", ai_load),
        ("AiInfo", ai_info),
        ("AiText", ai_text),
        ("AiBegin", ai_begin),
        ("AiNext", ai_next),
        ("AiGenerate", ai_generate),
        ("AiTokens", ai_tokens),
        ("AiFree", ai_free),
        ("AiEdges", ai_edges),
        ("AiGray", ai_gray),
        ("AiHistogram", ai_histogram),
        ("AiCompute", ai_compute),
        ("AiDefaultModel", ai_default_model),
    ];
    for (name, f) in natives {
        interp.register_native(alloc::format!("pinvoke:buitenzorg!{}", name), f);
    }
}

/// The model every image carries.
pub const DEFAULT_MODEL: &str = "/disk/STORIES.GGU";

fn ai_default_model(i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    Ok(Some(Value::Obj(i.alloc_string(DEFAULT_MODEL))))
}
