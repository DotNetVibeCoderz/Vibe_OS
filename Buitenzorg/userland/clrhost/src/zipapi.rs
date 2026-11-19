//! ZIP archives for managed programs (EPUB books in Baca): the central
//! directory, stored and deflated entries (`miniz_oxide`, no_std). Natives:
//! `ZipList(byte[] zip)` -> entry names, one per line; `ZipText(byte[] zip,
//! string name)` -> the entry decoded as UTF-8 ("" when missing or broken);
//! `ZipEntrySize(byte[] zip, string name)` -> its uncompressed size (-1 when
//! missing) and `ZipEntryInto(byte[] zip, string name, byte[] buffer)` -> bytes
//! written (comic pages: pictures inside a CBZ).

use alloc::string::String;
use alloc::vec::Vec;

use rustclr_core::{ArrayStorage, ClrArray, ExecResult, Interpreter, Value};

use crate::bzapi::string;

fn u16le(d: &[u8], o: usize) -> Option<usize> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
}
fn u32le(d: &[u8], o: usize) -> Option<usize> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

struct Entry {
    name: String,
    method: usize,
    comp: usize,
    size: usize,
    local: usize,
}

/// The central directory (found through the end-of-central-directory record).
fn entries(d: &[u8]) -> Vec<Entry> {
    let mut out = Vec::new();
    if d.len() < 22 {
        return out;
    }
    let lo = d.len().saturating_sub(22 + 65535);
    let Some(eocd) = (lo..=d.len() - 22).rev().find(|&i| d[i..i + 4] == [0x50, 0x4B, 0x05, 0x06]) else { return out };
    let (Some(count), Some(mut p)) = (u16le(d, eocd + 10), u32le(d, eocd + 16)) else { return out };
    for _ in 0..count {
        if d.get(p..p + 4) != Some(&[0x50, 0x4B, 0x01, 0x02]) {
            break;
        }
        let (Some(method), Some(comp), Some(size), Some(nlen), Some(xlen), Some(clen), Some(local)) =
            (u16le(d, p + 10), u32le(d, p + 20), u32le(d, p + 24), u16le(d, p + 28), u16le(d, p + 30), u16le(d, p + 32), u32le(d, p + 42))
        else {
            break;
        };
        let Some(name) = d.get(p + 46..p + 46 + nlen) else { break };
        out.push(Entry { name: String::from_utf8_lossy(name).into_owned(), method, comp, size, local });
        p += 46 + nlen + xlen + clen;
    }
    out
}

fn read(d: &[u8], e: &Entry) -> Option<Vec<u8>> {
    if d.get(e.local..e.local + 4)? != [0x50, 0x4B, 0x03, 0x04] {
        return None;
    }
    let start = e.local + 30 + u16le(d, e.local + 26)? + u16le(d, e.local + 28)?;
    let raw = d.get(start..start.checked_add(e.comp)?)?;
    match e.method {
        0 => Some(raw.to_vec()),
        8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, e.size.max(1).min(64 << 20)).ok(),
        _ => None,
    }
}

fn zip_bytes(i: &Interpreter, a: &[Value]) -> Vec<u8> {
    let Some(Value::Obj(h)) = a.first() else { return Vec::new() };
    i.heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::U8(v) => v.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

fn zip_list(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let d = zip_bytes(i, a);
    let mut s = String::new();
    for e in entries(&d) {
        s.push_str(&e.name);
        s.push('\n');
    }
    Ok(Some(Value::Obj(i.alloc_string(&s))))
}

fn zip_text(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let d = zip_bytes(i, a);
    let name = string(i, a, 1);
    let s = entries(&d)
        .iter()
        .find(|e| e.name == name)
        .and_then(|e| read(&d, e))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    Ok(Some(Value::Obj(i.alloc_string(&s))))
}

fn zip_entry_size(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let d = zip_bytes(i, a);
    let name = string(i, a, 1);
    let n = entries(&d).iter().find(|e| e.name == name).map_or(-1, |e| e.size as i32);
    Ok(Some(Value::I32(n)))
}

fn zip_entry_into(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let d = zip_bytes(i, a);
    let name = string(i, a, 1);
    let data = entries(&d).iter().find(|e| e.name == name).and_then(|e| read(&d, e));
    match data {
        Some(bytes) => {
            crate::bzapi::store_bytes(i, a, 2, &bytes);
            Ok(Some(Value::I32(bytes.len() as i32)))
        }
        None => Ok(Some(Value::I32(-1))),
    }
}

pub fn register(interp: &mut Interpreter) {
    let natives: [(&str, rustclr_core::interp::NativeFn); 4] = [
        ("ZipList", zip_list),
        ("ZipText", zip_text),
        ("ZipEntrySize", zip_entry_size),
        ("ZipEntryInto", zip_entry_into),
    ];
    for (name, f) in natives {
        interp.register_native(alloc::format!("pinvoke:buitenzorg!{}", name), f);
    }
}
