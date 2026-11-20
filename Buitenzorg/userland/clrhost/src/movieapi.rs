//! The natives of `movie.rs` (video files for managed programs).

use alloc::string::String;
use alloc::vec::Vec;

use rustclr_core::{ArrayStorage, ClrArray, ExecResult, Interpreter, Value};

use crate::bzapi::int;
use crate::movie::{open, Movie};

struct Movies {
    next: i64,
    list: Vec<(i64, Movie)>,
}

static MOVIES: crate::netapi::Lock<Movies> = crate::netapi::Lock::new(Movies { next: 1, list: Vec::new() });

fn bytes_arg(i: &Interpreter, a: &[Value], k: usize) -> Vec<u8> {
    let Some(Value::Obj(h)) = a.get(k) else { return Vec::new() };
    i.heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::U8(v) => v.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

fn movie_open(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let data = bytes_arg(i, a, 0);
    let Some(m) = open(data) else { return Ok(Some(Value::I64(-1))) };
    let mut t = MOVIES.lock();
    let id = t.next;
    t.next += 1;
    if t.list.len() >= 2 {
        t.list.remove(0); // a program that forgets MovieFree keeps at most two
    }
    t.list.push((id, m));
    Ok(Some(Value::I64(id)))
}

fn movie_info(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let id = int(a, 0);
    let s = {
        let t = MOVIES.lock();
        match t.list.iter().find(|(k, _)| *k == id) {
            None => String::new(),
            Some((_, m)) => alloc::format!(
                "{} {} {} {} {} {} {} {} {} {}\n{}\n{}",
                m.width,
                m.height,
                m.fps_x1000(),
                m.frames.len(),
                m.duration_ms,
                if m.vname.is_empty() { "-" } else { m.vname.trim() },
                if m.aname.is_empty() { "-" } else { m.aname.trim() },
                m.rate,
                m.channels,
                m.pcm.len(),
                m.title.replace('\n', " "),
                m.unsupported
            ),
        }
    };
    Ok(Some(Value::Obj(i.alloc_string(&s))))
}

fn movie_frame(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (id, index, dw, dh) = (int(a, 0), int(a, 1), int(a, 3).clamp(1, 2048) as usize, int(a, 4).clamp(1, 2048) as usize);
    let mut px = alloc::vec![0i32; dw * dh];
    let ok = {
        let mut t = MOVIES.lock();
        match t.list.iter_mut().find(|(k, _)| *k == id) {
            Some((_, m)) if index >= 0 => m.frame(index as usize, &mut px, dw, dh),
            _ => false,
        }
    };
    if !ok {
        return Ok(Some(Value::I32(-1)));
    }
    let Some(Value::Obj(h)) = a.get(2) else { return Ok(Some(Value::I32(-1))) };
    let done = i
        .heap
        .with_mut::<ClrArray, _>(*h, |arr| match &mut arr.storage {
            ArrayStorage::I32(v) if v.len() >= px.len() => {
                v[..px.len()].copy_from_slice(&px);
                true
            }
            _ => false,
        })
        .unwrap_or(false);
    Ok(Some(Value::I32(if done { 0 } else { -1 })))
}

fn movie_audio(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (id, off) = (int(a, 0), int(a, 2).max(0) as usize);
    let Some(Value::Obj(h)) = a.get(1) else { return Ok(Some(Value::I32(-1))) };
    let t = MOVIES.lock();
    let Some((_, m)) = t.list.iter().find(|(k, _)| *k == id) else { return Ok(Some(Value::I32(-1))) };
    let n = i
        .heap
        .with_mut::<ClrArray, _>(*h, |arr| match &mut arr.storage {
            ArrayStorage::U8(v) => {
                let n = v.len().min(m.pcm.len().saturating_sub(off));
                v[..n].copy_from_slice(&m.pcm[off..off + n]);
                n as i32
            }
            _ => -1,
        })
        .unwrap_or(-1);
    Ok(Some(Value::I32(n)))
}

fn movie_free(_: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let id = int(a, 0);
    MOVIES.lock().list.retain(|(k, _)| *k != id);
    Ok(Some(Value::I32(0)))
}

pub fn register(interp: &mut Interpreter) {
    let natives: [(&str, rustclr_core::interp::NativeFn); 5] =
        [("MovieOpen", movie_open), ("MovieInfo", movie_info), ("MovieFrame", movie_frame), ("MovieAudio", movie_audio), ("MovieFree", movie_free)];
    for (name, f) in natives {
        interp.register_native(alloc::format!("pinvoke:buitenzorg!{}", name), f);
    }
}
