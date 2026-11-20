//! Natives for fonts (FONT_OP, syscall 71): managed `Buitenzorg.Text.Fonts`
//! and Buitenzorg.UI's measuring go through these.

use alloc::vec::Vec;

use rustclr_core::{ExecResult, Interpreter, Value};

use crate::bzapi::{int, string};
use crate::sys;

const FONT_OP: u64 = 71;
const COUNT: u64 = 0;
const INFO: u64 = 1;
const FIND: u64 = 2;
const METRICS: u64 = 3;
const MEASURE: u64 = 4;
const INSTALL: u64 = 5;
const REMOVE: u64 = 6;
const GENERATION: u64 = 7;

fn cstr(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.bytes().filter(|&b| b != 0).collect();
    v.push(0);
    v
}

fn a0(op: u64, id: i64, px: i64) -> u64 {
    op | (id as u64 & 0xFFFF) << 8 | (px as u64 & 0xFF) << 24
}

fn ret(r: u64) -> ExecResult<Option<Value>> {
    Ok(Some(Value::I64(if sys::is_err(r) { -1 } else { r as i64 })))
}

/// `FontCount()` -> face slots (ids 1..=n).
fn font_count(_i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    ret(sys::syscall(FONT_OP, COUNT, 0, 0))
}

/// `FontInfo(int id)` -> "family TAB Regular|Bold TAB mono TAB file", or "" for none.
fn font_info(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let mut buf = alloc::vec![0u8; 160];
    let n = sys::syscall(FONT_OP, a0(INFO, int(a, 0), 0), buf.as_mut_ptr() as u64, buf.len() as u64);
    let s = if sys::is_err(n) { alloc::string::String::new() } else { alloc::string::String::from_utf8_lossy(&buf[..n as usize]).into_owned() };
    Ok(Some(Value::Obj(i.alloc_string(&s))))
}

/// `FontFind(string family, int bold)` -> id or -1.
fn font_find(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let f = cstr(&string(i, a, 0));
    ret(sys::syscall(FONT_OP, FIND, f.as_ptr() as u64, int(a, 1) as u64))
}

/// `FontMetrics(int id, int px)` -> ascent | descent << 16 | line height << 32, -1 = no such face.
fn font_metrics(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    ret(sys::syscall(FONT_OP, a0(METRICS, int(a, 0), int(a, 1)), 0, 0))
}

/// `FontMeasure(int id, int px, string text)` -> width in pixels, -1 = no such face.
fn font_measure(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let t = string(i, a, 2);
    if t.is_empty() {
        return Ok(Some(Value::I64(0)));
    }
    ret(sys::syscall(FONT_OP, a0(MEASURE, int(a, 0), int(a, 1)), t.as_ptr() as u64, t.len() as u64))
}

/// `FontInstall(string request)` ("SOURCE\nFILE.TTF\nSHA256") -> faces registered, -1 = refused.
fn font_install(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = cstr(&string(i, a, 0));
    ret(sys::syscall(FONT_OP, INSTALL, r.as_ptr() as u64, 0))
}

/// `FontRemove(string file)` -> faces removed, -1 = none.
fn font_remove(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let f = cstr(&string(i, a, 0));
    ret(sys::syscall(FONT_OP, REMOVE, f.as_ptr() as u64, 0))
}

/// `FontGeneration()` -> changes whenever text looks different.
fn font_generation(_i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    ret(sys::syscall(FONT_OP, GENERATION, 0, 0))
}

pub fn register(interp: &mut Interpreter) {
    let natives: [(&str, rustclr_core::interp::NativeFn); 8] = [
        ("FontGeneration", font_generation),
        ("FontCount", font_count),
        ("FontInfo", font_info),
        ("FontFind", font_find),
        ("FontMetrics", font_metrics),
        ("FontMeasure", font_measure),
        ("FontInstall", font_install),
        ("FontRemove", font_remove),
    ];
    for (name, f) in natives {
        interp.register_native(alloc::format!("pinvoke:buitenzorg!{}", name), f);
    }
}
