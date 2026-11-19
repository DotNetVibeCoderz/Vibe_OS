//! A small MIDI synthesizer (no_std): Standard MIDI Files, format 0 and 1,
//! rendered to 16-bit mono PCM at 22 050 Hz.
//!
//! Parsing: every track's events merged by absolute tick, running status,
//! tempo changes (meta 0x51), note on / off, program change, controller 7
//! (volume) and 10 (pan, ignored in mono), sysex and other meta events
//! skipped. Synthesis: up to 48 voices, each a phase accumulator through a
//! timbre picked by the General MIDI program family (piano-like decaying
//! harmonics, organ, plucked strings, bowed strings, brass/reed, flute,
//! square-wave synth, bass) with an attack / decay / sustain / release
//! envelope; channel 10 is percussion (kick, snare, hi-hat and toms from
//! noise and pitched sine drops). Everything is integer arithmetic over a
//! sine table, so it runs the same on the soft-float targets.

use alloc::vec;
use alloc::vec::Vec;

pub const RATE: u32 = 22_050;
/// Longest piece rendered (seconds).
const MAX_SECONDS: u64 = 600;
const VOICES: usize = 48;

#[derive(Clone, Copy)]
struct Event {
    tick: u64,
    order: u32,
    kind: u8, // 0x80 off, 0x90 on, 0xC0 program, 0xB0 controller, 0xFF tempo
    ch: u8,
    a: u32,
    b: u8,
}

fn vlq(d: &[u8], p: &mut usize) -> Option<u32> {
    let mut v = 0u32;
    for _ in 0..4 {
        let b = *d.get(*p)?;
        *p += 1;
        v = v << 7 | (b & 0x7F) as u32;
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

fn be32(d: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(p..p + 4)?.try_into().ok()?))
}

/// (ticks per quarter note, events sorted by tick)
fn parse(d: &[u8]) -> Option<(u32, Vec<Event>)> {
    if d.get(..4)? != b"MThd" {
        return None;
    }
    let hlen = be32(d, 4)? as usize;
    let ntracks = u16::from_be_bytes([*d.get(10)?, *d.get(11)?]) as usize;
    let division = u16::from_be_bytes([*d.get(12)?, *d.get(13)?]) as u32;
    if division & 0x8000 != 0 || division == 0 {
        return None; // SMPTE time is not supported
    }
    let mut p = 8 + hlen;
    let mut events = Vec::new();
    let mut order = 0u32;
    for _ in 0..ntracks {
        if d.get(p..p + 4)? != b"MTrk" {
            return None;
        }
        let len = be32(d, p + 4)? as usize;
        let end = (p + 8 + len).min(d.len());
        let mut q = p + 8;
        let mut tick = 0u64;
        let mut status = 0u8;
        while q < end {
            tick += vlq(d, &mut q)? as u64;
            let mut b = *d.get(q)?;
            if b & 0x80 != 0 {
                status = b;
                q += 1;
            } else if status == 0 {
                return None;
            } else {
                b = status;
            }
            let _ = b;
            match status {
                0xFF => {
                    let ty = *d.get(q)?;
                    q += 1;
                    let l = vlq(d, &mut q)? as usize;
                    if ty == 0x51 && l == 3 {
                        let t = (*d.get(q)? as u32) << 16 | (*d.get(q + 1)? as u32) << 8 | *d.get(q + 2)? as u32;
                        events.push(Event { tick, order, kind: 0xFF, ch: 0, a: t, b: 0 });
                        order += 1;
                    }
                    q += l;
                    if ty == 0x2F {
                        break;
                    }
                }
                0xF0 | 0xF7 => {
                    let l = vlq(d, &mut q)? as usize;
                    q += l;
                }
                _ => {
                    let kind = status & 0xF0;
                    let ch = status & 0x0F;
                    let a = *d.get(q)? as u32;
                    let two = !matches!(kind, 0xC0 | 0xD0);
                    let bv = if two { *d.get(q + 1)? } else { 0 };
                    q += if two { 2 } else { 1 };
                    let kind = if kind == 0x90 && bv == 0 { 0x80 } else { kind };
                    if matches!(kind, 0x80 | 0x90 | 0xC0 | 0xB0) {
                        events.push(Event { tick, order, kind, ch, a, b: bv });
                        order += 1;
                    }
                }
            }
        }
        p = p + 8 + len;
        if p >= d.len() {
            break;
        }
    }
    events.sort_by(|x, y| x.tick.cmp(&y.tick).then(x.order.cmp(&y.order)));
    Some((division, events))
}

/// sin(2*pi*i/1024) * 32767
fn sine_table() -> Vec<i32> {
    // a quarter wave from a Taylor series in fixed point (no floats)
    let mut t = vec![0i32; 1024];
    for i in 0..1024 {
        // x in [-pi, pi) as Q16
        let x = ((i as i64 * 411_775) >> 10) - 205_887; // 2*pi*Q16 = 411775, pi = 205887
        let x2 = (x * x) >> 16;
        // sin(x) ~ x - x^3/6 + x^5/120 - x^7/5040 + x^9/362880
        let mut term = x;
        let mut sum = x;
        for k in [6i64, 20, 42, 72] {
            term = -(term * x2 >> 16) / k;
            sum += term;
        }
        // shift by half a period: table index i is the angle 2*pi*i/1024
        t[(i + 512) % 1024] = ((sum * 32767) >> 16).clamp(-32767, 32767) as i32;
    }
    t
}

#[derive(Clone, Copy, PartialEq)]
enum Timbre {
    Piano,
    Organ,
    Pluck,
    Strings,
    Brass,
    Flute,
    Square,
    Bass,
    Drum(u8),
}

fn timbre(program: u8) -> Timbre {
    match program / 8 {
        0 | 1 => Timbre::Piano, // pianos, chromatic percussion
        2 => Timbre::Organ,
        3 => Timbre::Pluck, // guitars
        4 => Timbre::Bass,
        5 | 6 => Timbre::Strings, // strings, ensemble
        7 | 8 => Timbre::Brass,   // brass, reed
        9 => Timbre::Flute,
        _ => Timbre::Square, // synths, ethnic, effects
    }
}

#[derive(Clone, Copy)]
struct Voice {
    on: bool,
    ch: u8,
    note: u8,
    timbre: Timbre,
    phase: u32, // 10.22 fixed point index into the sine table
    step: u32,
    amp: i32,    // 0..32767 from velocity and channel volume
    age: u32,    // samples since note on
    rel: u32,    // samples since note off (0 = held)
    noise: u32,
}

/// (attack, decay, sustain level 0..256, release) in samples
fn envelope(t: Timbre) -> (u32, u32, u32, u32) {
    let ms = |m: u32| m * RATE / 1000;
    match t {
        Timbre::Piano => (ms(4), ms(900), 40, ms(250)),
        Timbre::Organ => (ms(8), ms(10), 256, ms(60)),
        Timbre::Pluck => (ms(3), ms(500), 20, ms(150)),
        Timbre::Strings => (ms(80), ms(200), 200, ms(300)),
        Timbre::Brass => (ms(30), ms(150), 190, ms(150)),
        Timbre::Flute => (ms(40), ms(100), 210, ms(120)),
        Timbre::Square => (ms(5), ms(200), 150, ms(120)),
        Timbre::Bass => (ms(5), ms(400), 120, ms(100)),
        Timbre::Drum(_) => (ms(1), ms(150), 0, ms(50)),
    }
}

fn level(v: &Voice) -> i32 {
    let (a, d, s, r) = envelope(v.timbre);
    let mut l = if v.age < a {
        256 * v.age / a.max(1)
    } else if v.age < a + d {
        256 - (256 - s) * (v.age - a) / d.max(1)
    } else {
        s
    } as i32;
    if v.rel > 0 {
        l = l * (r.saturating_sub(v.rel)) as i32 / r.max(1) as i32;
    }
    l.max(0)
}

/// Note -> phase step (10.22 fixed point per sample): 440 Hz * 2^((n-69)/12).
fn step_for(note: u8, semitone: &[u32; 12]) -> u32 {
    // semitone[k] = step of note 60 + k (middle C octave); shift per octave
    let n = note as i32;
    let oct = (n - 60).div_euclid(12);
    let k = (n - 60).rem_euclid(12) as usize;
    let base = semitone[k];
    if oct >= 0 { base << oct.min(6) } else { base >> (-oct).min(10) }
}

fn sample(v: &mut Voice, sine: &[i32]) -> i32 {
    let idx = |ph: u32| (ph >> 22) as usize & 1023;
    let s = |ph: u32| sine[idx(ph)];
    let ph = v.phase;
    let out = match v.timbre {
        Timbre::Piano => (s(ph) * 6 + s(ph.wrapping_mul(2)) * 3 + s(ph.wrapping_mul(3))) / 10,
        Timbre::Organ => (s(ph) * 5 + s(ph.wrapping_mul(2)) * 3 + s(ph.wrapping_mul(4)) * 2) / 10,
        Timbre::Pluck => {
            let bright = (256 - (v.age / 40).min(256)) as i32;
            (s(ph) * 7 + s(ph.wrapping_mul(2)) * 3 * bright / 256 + s(ph.wrapping_mul(3)) * 2 * bright / 256) / 10
        }
        Timbre::Strings => {
            // a soft sawtooth: harmonics 1/n
            (s(ph) * 60 + s(ph.wrapping_mul(2)) * 30 + s(ph.wrapping_mul(3)) * 20 + s(ph.wrapping_mul(4)) * 15) / 125
        }
        Timbre::Brass => (s(ph) * 5 + s(ph.wrapping_mul(2)) * 4 + s(ph.wrapping_mul(3)) * 3 + s(ph.wrapping_mul(5))) / 13,
        Timbre::Flute => (s(ph) * 9 + s(ph.wrapping_mul(2))) / 10,
        Timbre::Square => {
            if (ph >> 31) == 0 { 18_000 } else { -18_000 }
        }
        Timbre::Bass => (s(ph) * 8 + s(ph.wrapping_mul(2)) * 2) / 10,
        Timbre::Drum(n) => {
            v.noise = v.noise.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let noise = ((v.noise >> 16) as i32 & 0x7FFF) - 0x4000;
            match n {
                35 | 36 => {
                    // kick: a sine falling from ~150 Hz
                    s(ph) * 2
                }
                38 | 40 => noise + s(ph) / 2, // snare
                42 | 44 | 46 => noise / 2,    // hi-hats
                41 | 43 | 45 | 47 | 48 | 50 => s(ph), // toms
                _ => noise / 2 + s(ph) / 2,
            }
        }
    };
    v.phase = v.phase.wrapping_add(v.step);
    if let Timbre::Drum(n) = v.timbre {
        if matches!(n, 35 | 36 | 41 | 43 | 45 | 47 | 48 | 50) {
            // pitch drop
            v.step = v.step - v.step / 2000;
        }
    }
    out
}

/// Render a Standard MIDI File: 16-bit mono PCM at [`RATE`] Hz.
pub fn render(d: &[u8]) -> Option<Vec<u8>> {
    let (division, events) = parse(d)?;
    let sine = sine_table();
    // phase step of C4..B4: f * 2^32 / RATE, f = 261.63 * 2^(k/12)
    const C4_MILLIHZ: [u64; 12] = [261_626, 277_183, 293_665, 311_127, 329_628, 349_228, 369_994, 391_995, 415_305, 440_000, 466_164, 493_883];
    let mut semitone = [0u32; 12];
    for (k, f) in C4_MILLIHZ.iter().enumerate() {
        semitone[k] = ((f << 32) / (RATE as u64 * 1000)) as u32;
    }
    let mut voices = [Voice { on: false, ch: 0, note: 0, timbre: Timbre::Piano, phase: 0, step: 0, amp: 0, age: 0, rel: 0, noise: 1 }; VOICES];
    let mut program = [0u8; 16];
    let mut volume = [100u32; 16];
    let mut tempo: u64 = 500_000; // microseconds per quarter note
    let mut out: Vec<u8> = Vec::new();
    let mut tick = 0u64;
    let max_samples = MAX_SECONDS * RATE as u64;
    let mut written = 0u64;
    let mut frac = 0u64; // sub-sample remainder, in microseconds * RATE
    let render_until = |samples: u64, voices: &mut [Voice; VOICES], out: &mut Vec<u8>, written: &mut u64| {
        for _ in 0..samples {
            if *written >= max_samples {
                return;
            }
            let mut mix: i32 = 0;
            for v in voices.iter_mut().filter(|v| v.on) {
                let l = level(v);
                if v.rel > 0 && l == 0 || v.age > RATE * 20 && v.rel == 0 && matches!(v.timbre, Timbre::Drum(_)) {
                    v.on = false;
                    continue;
                }
                let s = sample(v, &sine);
                mix += (s * l >> 8) * v.amp >> 15;
                v.age += 1;
                if v.rel > 0 {
                    v.rel += 1;
                }
            }
            let s = (mix / 3).clamp(-32767, 32767) as i16;
            out.extend_from_slice(&s.to_le_bytes());
            *written += 1;
        }
    };
    for e in &events {
        if e.tick > tick {
            // ticks -> samples at the current tempo (carry the remainder)
            let us = (e.tick - tick) * tempo;
            let num = us * RATE as u64 + frac;
            let den = division as u64 * 1_000_000;
            let n = num / den;
            frac = num % den;
            render_until(n, &mut voices, &mut out, &mut written);
            tick = e.tick;
            if written >= max_samples {
                break;
            }
        }
        let ch = e.ch as usize;
        match e.kind {
            0xFF => tempo = (e.a as u64).max(1),
            0xC0 => program[ch] = e.a as u8,
            0xB0 if e.a == 7 => volume[ch] = e.b as u32,
            0x90 => {
                let drum = ch == 9;
                let t = if drum { Timbre::Drum(e.a as u8) } else { timbre(program[ch]) };
                let note = e.a as u8;
                let step = if drum {
                    match e.a {
                        35 | 36 => step_for(31, &semitone), // ~49 Hz falling
                        38 | 40 => step_for(55, &semitone),
                        n => step_for((n as u8).saturating_add(10).min(90), &semitone),
                    }
                } else {
                    step_for(note, &semitone)
                };
                // a free voice, else the oldest released one, else the oldest
                let slot = voices
                    .iter()
                    .position(|v| !v.on)
                    .or_else(|| (0..VOICES).filter(|&i| voices[i].rel > 0).max_by_key(|&i| voices[i].rel))
                    .unwrap_or_else(|| (0..VOICES).max_by_key(|&i| voices[i].age).unwrap_or(0));
                let amp = (e.b as i32 * 258) * volume[ch] as i32 / 127;
                voices[slot] = Voice { on: true, ch: e.ch, note, timbre: t, phase: 0, step, amp, age: 0, rel: 0, noise: 0x1234_5678 ^ e.tick as u32 };
            }
            0x80 => {
                for v in voices.iter_mut() {
                    if v.on && v.rel == 0 && v.ch == e.ch && v.note == e.a as u8 && !matches!(v.timbre, Timbre::Drum(_)) {
                        v.rel = 1;
                    }
                }
            }
            _ => {}
        }
    }
    // let the last notes ring out (at most 3 s)
    for v in voices.iter_mut().filter(|v| v.on && v.rel == 0) {
        v.rel = 1;
    }
    let tail = (3 * RATE) as u64;
    render_until(tail, &mut voices, &mut out, &mut written);
    // trim trailing silence
    while out.len() >= 2 && out[out.len() - 2] == 0 && out[out.len() - 1] == 0 {
        out.truncate(out.len() - 2);
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}
