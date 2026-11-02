//! The native half of `Buitenzorg.App` (`/disk/BZAPP.DLL`): every
//! `[DllImport("buitenzorg")] static extern ...` in that library reaches
//! RustCLR as the binding key `pinvoke:buitenzorg!<Name>`, registered here.
//!
//! The marshalling happens on this side. A pointer in RustCLR is a
//! (buffer, offset) reference into the managed heap, not an address the
//! kernel could read, so a managed string is copied out as UTF-8, an `int[]`
//! as a pixel buffer, and the `DrawCmd` the WIN_CMD syscall takes is built
//! here, in the host's own memory.

use alloc::string::String;
use alloc::vec::Vec;

use rustclr_core::{ArrayStorage, ClrArray, ExecResult, Interpreter, Value};

use crate::sys;

const FB_INFO: u64 = 5;
const WIN_CREATE: u64 = 6;
const WIN_CMD: u64 = 7;
const WIN_PRESENT: u64 = 8;
const KEY_READ: u64 = 9;
const IS_INTERACTIVE: u64 = 31;
const PROC_SPAWN: u64 = 40;
const PROC_WAIT: u64 = 41;

/// `bz_abi::draw_op::BLIT`.
const BLIT: u64 = 7;

/// `bz_abi::DrawCmd`.
#[repr(C)]
struct DrawCmd {
    op: u64,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: u32,
    _pad: u32,
    text_ptr: u64,
    text_len: u64,
}

fn int(a: &[Value], i: usize) -> i64 {
    match a.get(i) {
        Some(Value::I32(v)) => *v as i64,
        Some(Value::I64(v)) | Some(Value::NativeInt(v)) => *v,
        _ => 0,
    }
}

fn string(interp: &Interpreter, a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::Obj(h)) if !h.is_null() => interp.string_value(*h).unwrap_or_default(),
        _ => String::new(),
    }
}

/// An `int[]` (or `uint[]`) argument as 32-bit pixels.
fn pixels(interp: &Interpreter, a: &[Value], i: usize) -> Vec<u32> {
    let Some(Value::Obj(h)) = a.get(i) else { return Vec::new() };
    interp
        .heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::I32(v) => v.iter().map(|&p| p as u32).collect(),
            ArrayStorage::U32(v) => v.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

/// A NUL-terminated copy of `s` (PROC_SPAWN takes C strings).
fn cstr(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s.as_bytes());
    v.push(0);
    v
}

fn ret_i32(v: i64) -> ExecResult<Option<Value>> {
    Ok(Some(Value::I32(v as i32)))
}

fn win_cmd(win: u64, cmd: &DrawCmd) -> i64 {
    let r = sys::syscall(WIN_CMD, win, cmd as *const DrawCmd as u64, 0);
    if r == 0 { 0 } else { -1 }
}

/// `WinCreate(string title, int width, int height)` -> window id (0: failed).
fn win_create(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let title = string(i, a, 0);
    let (w, h) = (int(a, 1).clamp(1, 4096) as u64, int(a, 2).clamp(1, 4096) as u64);
    let id = sys::syscall(WIN_CREATE, title.as_ptr() as u64, title.len() as u64, (w << 32) | h);
    ret_i32(if sys::is_err(id) { 0 } else { id as i64 })
}

/// `WinDraw(int window, int op, int x, int y, int w, int h, int color, string text)`.
fn win_draw(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let text = string(i, a, 7);
    let cmd = DrawCmd {
        op: int(a, 1) as u64,
        x: int(a, 2) as i32,
        y: int(a, 3) as i32,
        w: int(a, 4) as i32,
        h: int(a, 5) as i32,
        color: int(a, 6) as u32 & 0x00FF_FFFF,
        _pad: 0,
        text_ptr: if text.is_empty() { 0 } else { text.as_ptr() as u64 },
        text_len: text.len() as u64,
    };
    ret_i32(win_cmd(int(a, 0) as u64, &cmd))
}

/// `WinBlit(int window, int x, int y, int w, int h, int[] pixels)`.
fn win_blit(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let px = pixels(i, a, 5);
    let (w, h) = (int(a, 3), int(a, 4));
    if w <= 0 || h <= 0 || (px.len() as i64) < w * h {
        return ret_i32(-1);
    }
    let cmd = DrawCmd {
        op: BLIT,
        x: int(a, 1) as i32,
        y: int(a, 2) as i32,
        w: w as i32,
        h: h as i32,
        color: 0,
        _pad: 0,
        text_ptr: px.as_ptr() as u64,
        text_len: (px.len() * 4) as u64,
    };
    ret_i32(win_cmd(int(a, 0) as u64, &cmd))
}

fn win_present(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    sys::syscall(WIN_PRESENT, int(a, 0) as u64, 0, 0);
    ret_i32(0)
}

fn key_read(_i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    ret_i32(sys::syscall(KEY_READ, 0, 0, 0) as i64)
}

/// `bz_abi::FramebufferInfo`: [address, size, width, height, stride, bpp, format].
fn fb_info() -> [u64; 7] {
    let mut fb = [0u64; 7];
    sys::syscall(FB_INFO, fb.as_mut_ptr() as u64, 0, 0);
    fb
}

fn screen_width(_i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    ret_i32(fb_info()[2] as i64)
}

fn screen_height(_i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    ret_i32(fb_info()[3] as i64)
}

fn is_interactive(_i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    ret_i32(sys::syscall(IS_INTERACTIVE, 0, 0, 0) as i64)
}

/// `Spawn(string path, string args)` -> pid (negative: failed).
fn spawn(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (path, args) = (cstr(&string(i, a, 0)), cstr(&string(i, a, 1)));
    let pid = sys::syscall(PROC_SPAWN, path.as_ptr() as u64, args.as_ptr() as u64, 0);
    Ok(Some(Value::I64(if sys::is_err(pid) { -1 } else { pid as i64 })))
}

/// `Wait(long pid)` -> exit code. A blocking call: other managed threads run
/// meanwhile, and a collection must not wait for this one -- it is announced
/// blocked, with its roots handed over.
fn wait(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let pid = int(a, 0) as u64;
    let code = i.blocking(|_| sys::syscall(PROC_WAIT, pid, 0, 0));
    Ok(Some(Value::I64(code as i64)))
}

/// Register the bindings.
pub fn install(interp: &mut Interpreter) {
    let natives: [(&str, rustclr_core::interp::NativeFn); 10] = [
        ("WinCreate", win_create),
        ("WinDraw", win_draw),
        ("WinBlit", win_blit),
        ("WinPresent", win_present),
        ("KeyRead", key_read),
        ("ScreenWidth", screen_width),
        ("ScreenHeight", screen_height),
        ("IsInteractive", is_interactive),
        ("Spawn", spawn),
        ("Wait", wait),
    ];
    for (name, f) in natives {
        interp.register_native(alloc::format!("pinvoke:buitenzorg!{}", name), f);
    }
}
