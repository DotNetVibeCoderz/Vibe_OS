//! The software audio mixer: one stream per program, summed into the sound
//! card's ring (AUDIO_WRITE / AUDIO_APP_VOLUME).
//!
//! A program queues PCM in any common format (8-bit unsigned or 16-bit
//! signed, mono or stereo, any rate); [`write`] converts it to the device
//! format -- 48 kHz, stereo, 16-bit -- with linear interpolation and keeps
//! up to [`MAX_QUEUED`] frames (about two seconds) per program. The device
//! driver's pump asks [`mix`] for the next frames: every stream contributes
//! at its own volume, sums saturate. A stream that runs dry is silence, not a
//! glitch in the others. Streams end with their process ([`on_exit`]).
//!
//! Generic code: the drivers that stream (PC AC'97 / Intel HDA) call `mix`;
//! [`Stream`] state per pid lives here.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;

pub const RATE: u32 = 48_000;
/// About two seconds of stereo frames per program.
pub const MAX_QUEUED: usize = 2 * RATE as usize;

struct Stream {
    pid: u64,
    /// Interleaved stereo frames (left, right) at 48 kHz.
    frames: VecDeque<(i16, i16)>,
    /// 0..=100.
    volume: u32,
    /// Resampler state: the source position carried between writes (16.16)
    /// and the last source frame.
    pos: u64,
    last: (i16, i16),
    format: u32,
}

static STREAMS: Mutex<Vec<Stream>> = Mutex::new(Vec::new());
/// Volumes set for programs that have no stream yet (pid, volume).
static VOLUMES: Mutex<Vec<(u64, u32)>> = Mutex::new(Vec::new());

/// (rate, channels, bits) of an AUDIO_WRITE format word (0 = the device format).
pub fn decode_format(f: u32) -> Option<(u32, u32, u32)> {
    if f == 0 {
        return Some((RATE, 2, 16));
    }
    let rate = f & 0xF_FFFF;
    let ch = (f >> 20) & 0xF;
    let bits = (f >> 24) & 0xFF;
    ((4_000..=192_000).contains(&rate) && (ch == 1 || ch == 2) && (bits == 8 || bits == 16)).then_some((rate, ch, bits))
}

/// Source bytes per frame of a format.
pub fn frame_bytes(f: u32) -> usize {
    decode_format(f).map_or(4, |(_, ch, bits)| (ch * bits / 8) as usize)
}

fn sample(bytes: &[u8], i: usize, ch: u32, bits: u32) -> (i16, i16) {
    let one = |k: usize| -> i16 {
        if bits == 8 {
            ((bytes[k] as i16) - 128) << 8
        } else {
            i16::from_le_bytes([bytes[k], bytes[k + 1]])
        }
    };
    let bps = (bits / 8) as usize;
    let at = i * ch as usize * bps;
    let l = one(at);
    let r = if ch == 2 { one(at + bps) } else { l };
    (l, r)
}

/// Queue `bytes` (whole frames of `format`) for `pid`. Returns the source
/// bytes taken: fewer when the stream is full.
pub fn write(pid: u64, bytes: &[u8], format: u32) -> usize {
    let Some((rate, ch, bits)) = decode_format(format) else { return 0 };
    let fb = (ch * bits / 8) as usize;
    let n_src = bytes.len() / fb;
    let mut streams = STREAMS.lock();
    let i = match streams.iter().position(|s| s.pid == pid) {
        Some(i) => i,
        None => {
            let volume = VOLUMES.lock().iter().find(|v| v.0 == pid).map_or(100, |v| v.1);
            streams.push(Stream { pid, frames: VecDeque::new(), volume, pos: 0, last: (0, 0), format });
            streams.len() - 1
        }
    };
    let s = &mut streams[i];
    if s.format != format {
        s.format = format;
        s.pos = 0;
    }
    let room = MAX_QUEUED.saturating_sub(s.frames.len());
    if room == 0 || n_src == 0 {
        return 0;
    }
    if rate == RATE {
        let take = n_src.min(room);
        for k in 0..take {
            s.frames.push_back(sample(bytes, k, ch, bits));
        }
        if take > 0 {
            s.last = sample(bytes, take - 1, ch, bits);
        }
        return take * fb;
    }
    // linear interpolation: output frame t sits at source position pos (16.16)
    let step = ((rate as u64) << 16) / RATE as u64;
    let mut produced = 0;
    loop {
        let idx = (s.pos >> 16) as usize;
        if idx >= n_src || produced >= room {
            break;
        }
        let frac = (s.pos & 0xFFFF) as i32;
        let a = if idx == 0 { s.last } else { sample(bytes, idx - 1, ch, bits) };
        let b = sample(bytes, idx, ch, bits);
        let lerp = |x: i16, y: i16| (x as i32 + ((y as i32 - x as i32) * frac >> 16)) as i16;
        s.frames.push_back((lerp(a.0, b.0), lerp(a.1, b.1)));
        produced += 1;
        s.pos += step;
    }
    // consume whole source frames; the position keeps its fraction
    let consumed = ((s.pos >> 16) as usize).min(n_src);
    if consumed > 0 {
        s.last = sample(bytes, consumed - 1, ch, bits);
    }
    s.pos -= (consumed as u64) << 16;
    consumed * fb
}

/// Milliseconds queued for `pid`.
pub fn queued_ms(pid: u64) -> u64 {
    STREAMS.lock().iter().find(|s| s.pid == pid).map_or(0, |s| s.frames.len() as u64 * 1000 / RATE as u64)
}

/// Whether any stream holds frames (the pump starts / keeps the device).
pub fn has_data() -> bool {
    STREAMS.lock().iter().any(|s| !s.frames.is_empty())
}

/// Fill `out` (interleaved stereo i16) with the next frames of every stream,
/// mixed. Returns whether any stream contributed.
pub fn mix(out: &mut [i16]) -> bool {
    let frames = out.len() / 2;
    let mut acc = alloc::vec![0i32; frames * 2];
    let mut any = false;
    {
        let mut streams = STREAMS.lock();
        for s in streams.iter_mut() {
            let n = frames.min(s.frames.len());
            if n == 0 {
                continue;
            }
            any = true;
            let v = s.volume as i32;
            for k in 0..n {
                let (l, r) = s.frames.pop_front().unwrap();
                acc[2 * k] += l as i32 * v / 100;
                acc[2 * k + 1] += r as i32 * v / 100;
            }
        }
    }
    for (o, a) in out.iter_mut().zip(acc.iter()) {
        *o = (*a).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    }
    any
}

pub fn set_volume(pid: u64, volume: u32) -> u32 {
    let v = volume.min(100);
    if let Some(s) = STREAMS.lock().iter_mut().find(|s| s.pid == pid) {
        s.volume = v;
    }
    let mut vols = VOLUMES.lock();
    match vols.iter_mut().find(|x| x.0 == pid) {
        Some(x) => x.1 = v,
        None => {
            if vols.len() >= 64 {
                vols.remove(0);
            }
            vols.push((pid, v));
        }
    }
    v
}

pub fn volume(pid: u64) -> u32 {
    STREAMS.lock().iter().find(|s| s.pid == pid).map(|s| s.volume).or_else(|| VOLUMES.lock().iter().find(|v| v.0 == pid).map(|v| v.1)).unwrap_or(100)
}

/// The program ended: its stream goes.
pub fn on_exit(pid: u64) {
    STREAMS.lock().retain(|s| s.pid != pid);
    VOLUMES.lock().retain(|v| v.0 != pid);
}

/// (pid, queued ms, volume) of every stream (`mixer` in the shell).
pub fn list() -> Vec<(u64, u64, u32)> {
    STREAMS.lock().iter().map(|s| (s.pid, s.frames.len() as u64 * 1000 / RATE as u64, s.volume)).collect()
}

/// Boot self-test (no sound card needed): conversion, resampling, volume,
/// saturation and two streams mixed.
pub fn self_test() -> bool {
    const A: u64 = u64::MAX - 1;
    const B: u64 = u64::MAX - 2;
    on_exit(A);
    on_exit(B);
    // 8-bit mono at 24 kHz: 4 source frames become 8 device frames
    let src8 = [128u8, 192, 64, 128];
    let fmt8 = 24_000 | 1 << 20 | 8 << 24;
    let took = write(A, &src8, fmt8);
    let n8 = STREAMS.lock().iter().find(|s| s.pid == A).map_or(0, |s| s.frames.len());
    // 16-bit stereo at 48 kHz, loud, on a second stream at half volume
    let mut loud = Vec::new();
    for _ in 0..8 {
        loud.extend_from_slice(&30_000i16.to_le_bytes());
        loud.extend_from_slice(&(-30_000i16).to_le_bytes());
    }
    let took16 = write(B, &loud, 0);
    set_volume(B, 50);
    let mut out = [0i16; 16];
    let any = mix(&mut out);
    // frame 2 of A: between 192 and 64 -> ~ +64<<8 .. ; B at half: +15000 / -15000
    let half = out[1] < -14_000 && out[1] > -16_000;
    // saturation: two full-scale streams
    let max = [i16::MAX; 4].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
    write(A, &max, 0);
    set_volume(A, 100);
    write(B, &max, 0);
    set_volume(B, 100);
    let mut out2 = [0i16; 4];
    // drain what A still had first
    while STREAMS.lock().iter().find(|s| s.pid == A).map_or(0, |s| s.frames.len()) > 2 {
        let mut tmp = [0i16; 2];
        mix(&mut tmp);
    }
    mix(&mut out2);
    let saturated = out2.iter().any(|v| *v == i16::MAX);
    let ms = queued_ms(A);
    on_exit(A);
    on_exit(B);
    let ok = took == 4 && n8 == 8 && took16 == loud.len() && any && half && saturated && ms < 10 && list().iter().all(|s| s.0 != A && s.0 != B);
    if ok {
        crate::println!("[kernel] MILESTONE: MIXER OK (8-bit mono 24 kHz -> 48 kHz stereo, per-program volume, two streams summed with saturation)");
    } else {
        crate::println!("[mixer] self-test failed: took {} n8 {} took16 {} any {} half {} ({:?}) sat {} ({:?})", took, n8, took16, any, half, &out[..4], saturated, out2);
    }
    ok
}
