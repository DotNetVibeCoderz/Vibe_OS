//! Natives for the NES emulator (`nes.rs`), used by the managed app MNes:
//!
//! - `NesLoad(byte[] rom)` -> id, -1 not an iNES file / truncated, -2 an
//!   unsupported mapper (the mapper number is in `NesInfo(-1, 0)`).
//! - `NesFrame(id, int buttons, int[] pixels)` -> frames run so far: one frame
//!   with controller 1 held as `buttons` (A 1, B 2, Select 4, Start 8, Up 16,
//!   Down 32, Left 64, Right 128), the 256x240 picture as 0xFFRRGGBB (an
//!   array of 256s x 240s gets it zoomed s times, s = 2..4).
//! - `NesAudio(id, byte[] out)` -> bytes: the frame's sound, 48 kHz mono
//!   16-bit little-endian (what is not taken waits for the next call).
//! - `NesPeek(id, addr)` -> a byte of the CPU's address space (tests).
//! - `NesInfo(id, what)` -> 0 mapper, 1 PRG bytes, 2 CHR bytes, 3 jammed,
//!   4 instructions run, 5 frames.
//! - `NesFree(id)`.

use alloc::boxed::Box;
use alloc::vec::Vec;

use rustclr_core::{ArrayStorage, ClrArray, ExecResult, Interpreter, Value};

use crate::bzapi::int;
use crate::nes::{Nes, LoadError, WIDTH, HEIGHT};

struct Slot {
    nes: Box<Nes>,
    audio: Vec<i16>,
}

static SLOTS: crate::netapi::Lock<Vec<Option<Slot>>> = crate::netapi::Lock::new(Vec::new());
static LAST_MAPPER: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

fn i32r(v: i64) -> ExecResult<Option<Value>> {
    Ok(Some(Value::I32(v as i32)))
}

fn bytes_arg(i: &Interpreter, a: &[Value], idx: usize) -> Vec<u8> {
    match a.get(idx) {
        Some(Value::Obj(h)) if !h.is_null() => i
            .heap
            .with::<ClrArray, _>(*h, |arr| match &arr.storage {
                ArrayStorage::U8(v) => v.clone(),
                _ => Vec::new(),
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn nes_load(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let rom = bytes_arg(i, a, 0);
    match Nes::new(&rom) {
        Ok(n) => {
            let mut s = SLOTS.lock();
            let slot = Slot { nes: Box::new(n), audio: Vec::new() };
            let id = match s.iter().position(|x| x.is_none()) {
                Some(k) => {
                    s[k] = Some(slot);
                    k
                }
                None => {
                    s.push(Some(slot));
                    s.len() - 1
                }
            };
            i32r(id as i64 + 1)
        }
        Err(LoadError::Mapper(m)) => {
            LAST_MAPPER.store(m as u32, core::sync::atomic::Ordering::Relaxed);
            i32r(-2)
        }
        Err(_) => i32r(-1),
    }
}

fn with_slot<R>(id: i64, f: impl FnOnce(&mut Slot) -> R) -> Option<R> {
    let mut s = SLOTS.lock();
    let k = (id - 1) as usize;
    if id < 1 || k >= s.len() {
        return None;
    }
    s[k].as_mut().map(f)
}

fn nes_frame(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (id, buttons) = (int(a, 0), int(a, 1) as u8);
    let mut px = alloc::vec![0u32; WIDTH * HEIGHT];
    let Some(frames) = with_slot(id, |s| {
        s.nes.set_buttons(0, buttons);
        s.nes.run_frame();
        let pcm = s.nes.take_audio();
        // keep at most a second of unread sound
        if s.audio.len() + pcm.len() > 48_000 {
            let drop = (s.audio.len() + pcm.len() - 48_000).min(s.audio.len());
            s.audio.drain(..drop);
        }
        s.audio.extend(pcm);
        s.nes.frame_rgb(&mut px);
        s.nes.frames
    }) else {
        return i32r(-1);
    };
    if let Some(Value::Obj(h)) = a.get(2) {
        i.heap.with_mut::<ClrArray, _>(*h, |arr| {
            if let ArrayStorage::I32(v) = &mut arr.storage {
                // an array of (256 s) x (240 s) gets the picture zoomed s times
                let s = (1..=4).find(|&s| v.len() == WIDTH * HEIGHT * s * s).unwrap_or(1);
                if s == 1 {
                    for (o, p) in v.iter_mut().zip(px.iter()) {
                        *o = (0xFF00_0000 | p) as i32;
                    }
                } else {
                    let w = WIDTH * s;
                    for y in 0..HEIGHT {
                        let row = y * s * w;
                        for x in 0..WIDTH {
                            let c = (0xFF00_0000 | px[y * WIDTH + x]) as i32;
                            v[row + x * s..row + x * s + s].fill(c);
                        }
                        for k in 1..s {
                            v.copy_within(row..row + w, row + k * w);
                        }
                    }
                }
            }
        });
    }
    i32r(frames as i64)
}

fn nes_audio(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let id = int(a, 0);
    let cap = match a.get(1) {
        Some(Value::Obj(h)) if !h.is_null() => i.heap.with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::U8(v) => v.len(),
            _ => 0,
        }).unwrap_or(0),
        _ => 0,
    };
    let Some(samples) = with_slot(id, |s| {
        let n = s.audio.len().min(cap / 2);
        s.audio.drain(..n).collect::<Vec<i16>>()
    }) else {
        return i32r(-1);
    };
    if let Some(Value::Obj(h)) = a.get(1) {
        i.heap.with_mut::<ClrArray, _>(*h, |arr| {
            if let ArrayStorage::U8(v) = &mut arr.storage {
                for (k, s) in samples.iter().enumerate() {
                    let b = s.to_le_bytes();
                    v[k * 2] = b[0];
                    v[k * 2 + 1] = b[1];
                }
            }
        });
    }
    i32r(samples.len() as i64 * 2)
}

fn nes_peek(_: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (id, addr) = (int(a, 0), int(a, 1) as u16);
    i32r(with_slot(id, |s| s.nes.peek(addr) as i64).unwrap_or(-1))
}

fn nes_info(_: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (id, what) = (int(a, 0), int(a, 1));
    if id == -1 {
        return i32r(LAST_MAPPER.load(core::sync::atomic::Ordering::Relaxed) as i64);
    }
    i32r(
        with_slot(id, |s| match what {
            0 => s.nes.mapper() as i64,
            1 => s.nes.sizes().0 as i64,
            2 => s.nes.sizes().1 as i64,
            3 => s.nes.jammed as i64,
            4 => s.nes.instructions.min(i32::MAX as u64) as i64,
            5 => s.nes.frames as i64,
            _ => -1,
        })
        .unwrap_or(-1),
    )
}

fn nes_free(_: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let id = int(a, 0);
    let mut s = SLOTS.lock();
    let k = (id - 1) as usize;
    if id >= 1 && k < s.len() {
        s[k] = None;
    }
    i32r(0)
}

pub fn register(interp: &mut Interpreter) {
    let natives: [(&str, rustclr_core::interp::NativeFn); 6] = [
        ("NesLoad", nes_load),
        ("NesFrame", nes_frame),
        ("NesAudio", nes_audio),
        ("NesPeek", nes_peek),
        ("NesInfo", nes_info),
        ("NesFree", nes_free),
    ];
    for (name, f) in natives {
        interp.register_native(alloc::format!("pinvoke:buitenzorg!{}", name), f);
    }
}
