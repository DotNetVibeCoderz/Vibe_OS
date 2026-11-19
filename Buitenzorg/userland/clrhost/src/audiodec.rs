//! Compressed audio for managed programs (Musik): decode a whole file to
//! 16-bit PCM in the host (an interpreter would take minutes per song).
//! Natives: `AudioDecodeInfo(byte[] file)` -> `rate | channels << 20 | 16 << 24
//! | frames << 32` (0 = not a format we decode), `AudioDecodeInto(byte[] file,
//! byte[] pcm)` -> bytes written. Formats: FLAC (`flac.rs`). The last decode
//! is cached, so Info + Into decode once.

use alloc::vec::Vec;

use rustclr_core::{ArrayStorage, ClrArray, ExecResult, Interpreter, Value};

/// (length + a sample of the file's bytes, decoded PCM, info word)
static CACHE: crate::netapi::Lock<Option<(u64, Vec<u8>, u64)>> = crate::netapi::Lock::new(None);

fn file_bytes(i: &Interpreter, a: &[Value]) -> Vec<u8> {
    let Some(Value::Obj(h)) = a.first() else { return Vec::new() };
    i.heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::U8(v) => v.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

fn key(d: &[u8]) -> u64 {
    let mut k = d.len() as u64 ^ 0xcbf2_9ce4_8422_2325;
    for (n, b) in d.iter().step_by((d.len() / 509).max(1)).enumerate() {
        k = (k ^ *b as u64 ^ (n as u64) << 8).wrapping_mul(0x100_0000_01b3);
    }
    k
}

/// Decode (or take from the cache): (info word, pcm length).
fn decode(d: &[u8]) -> (u64, usize) {
    let k = key(d);
    let mut c = CACHE.lock();
    if let Some((ck, pcm, word)) = c.as_ref() {
        if *ck == k {
            return (*word, pcm.len());
        }
    }
    let Some((info, pcm)) = crate::flac::decode(d) else { return (0, 0) };
    let word = info.rate as u64 | (info.channels as u64) << 20 | 16u64 << 24 | info.frames << 32;
    let len = pcm.len();
    *c = Some((k, pcm, word));
    (word, len)
}

fn decode_info(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let d = file_bytes(i, a);
    Ok(Some(Value::I64(decode(&d).0 as i64)))
}

fn decode_into(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let d = file_bytes(i, a);
    let (word, _) = decode(&d);
    if word == 0 {
        return Ok(Some(Value::I32(-1)));
    }
    let pcm = CACHE.lock().as_ref().map(|c| c.1.clone()).unwrap_or_default();
    crate::bzapi::store_bytes(i, a, 1, &pcm);
    Ok(Some(Value::I32(pcm.len() as i32)))
}

pub fn register(interp: &mut Interpreter) {
    let natives: [(&str, rustclr_core::interp::NativeFn); 2] = [("AudioDecodeInfo", decode_info), ("AudioDecodeInto", decode_into)];
    for (name, f) in natives {
        interp.register_native(alloc::format!("pinvoke:buitenzorg!{}", name), f);
    }
}
