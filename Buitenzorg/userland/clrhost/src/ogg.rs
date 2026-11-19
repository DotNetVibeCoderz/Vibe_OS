//! Ogg audio for `audiodec.rs`: Vorbis (the vendored `rustb-vorbis`, a port of
//! stb_vorbis) and Opus (the vendored `opus-pure`, decoded at 48 kHz). The
//! Ogg container is read here: pages ("OggS", lacing values) are joined into
//! the packets of the first logical stream; for Opus the identification header
//! (`OpusHead`) gives the channels and the pre-skip (trimmed at the start) and
//! the last page's granule position where the audio ends (trimmed at the end).

use alloc::vec;
use alloc::vec::Vec;

use crate::flac::Info;

/// The packets of the first logical stream (by serial number).
pub fn packets(d: &[u8]) -> Vec<Vec<u8>> {
    packets_and_end(d).0
}

/// The packets of the first logical stream and the granule position of its
/// last page (Opus: the number of 48 kHz samples up to the end, pre-skip
/// included).
pub fn packets_and_end(d: &[u8]) -> (Vec<Vec<u8>>, u64) {
    let mut out = Vec::new();
    let mut granule = 0u64;
    let mut cur: Vec<u8> = Vec::new();
    let mut serial: Option<u32> = None;
    let mut p = 0usize;
    while p + 27 <= d.len() {
        if &d[p..p + 4] != b"OggS" {
            // resync on the next capture pattern
            match d[p + 1..].windows(4).position(|w| w == b"OggS") {
                Some(i) => {
                    p += 1 + i;
                    continue;
                }
                None => break,
            }
        }
        let segs = d[p + 26] as usize;
        let table = p + 27;
        if table + segs > d.len() {
            break;
        }
        let sn = u32::from_le_bytes([d[p + 14], d[p + 15], d[p + 16], d[p + 17]]);
        let mut body = table + segs;
        let mine = *serial.get_or_insert(sn) == sn;
        if mine {
            let g = u64::from_le_bytes([d[p + 6], d[p + 7], d[p + 8], d[p + 9], d[p + 10], d[p + 11], d[p + 12], d[p + 13]]);
            if g != u64::MAX {
                granule = g; // pages that end no packet carry -1
            }
        }
        for k in 0..segs {
            let n = d[table + k] as usize;
            if body + n > d.len() {
                return (out, granule);
            }
            if mine {
                cur.extend_from_slice(&d[body..body + n]);
                if n < 255 {
                    out.push(core::mem::take(&mut cur));
                }
            }
            body += n;
        }
        p = body;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    (out, granule)
}

/// Whether `d` is an Ogg file whose first stream is Vorbis or Opus.
pub fn kind(d: &[u8]) -> Option<&'static str> {
    if d.get(..4)? != b"OggS" || d.len() < 28 + 8 {
        return None;
    }
    // the first packet starts right after the first page's lacing table
    let body = d.get(27 + d[26] as usize..)?;
    if body.len() >= 7 && body[0] == 1 && &body[1..7] == b"vorbis" {
        Some("vorbis")
    } else if body.len() >= 8 && &body[..8] == b"OpusHead" {
        Some("opus")
    } else {
        None
    }
}

/// Decode an Ogg Vorbis or Ogg Opus file to 16-bit little-endian PCM.
pub fn decode(d: &[u8]) -> Option<(Info, Vec<u8>)> {
    match kind(d)? {
        "vorbis" => vorbis(d),
        _ => opus(d),
    }
}

fn to_bytes(samples: &[i16]) -> Vec<u8> {
    let mut pcm = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        pcm.extend_from_slice(&s.to_le_bytes());
    }
    pcm
}

fn vorbis(d: &[u8]) -> Option<(Info, Vec<u8>)> {
    let (channels, rate, samples) = rustb_vorbis::decode_memory(d).ok()?;
    if channels == 0 || rate == 0 || samples.is_empty() {
        return None;
    }
    // the mixer takes at most two channels: keep the first two
    let (ch, samples) = if channels > 2 {
        let mut s = Vec::with_capacity(samples.len() / channels * 2);
        for f in samples.chunks(channels) {
            s.push(f[0]);
            s.push(f[1]);
        }
        (2usize, s)
    } else {
        (channels, samples)
    };
    let frames = (samples.len() / ch) as u64;
    Some((Info { rate, channels: ch as u32, bits: 16, frames }, to_bytes(&samples)))
}

fn opus(d: &[u8]) -> Option<(Info, Vec<u8>)> {
    let (pk, end) = packets_and_end(d);
    let head = pk.first()?;
    if head.len() < 19 || &head[..8] != b"OpusHead" {
        return None;
    }
    let channels = head[9].clamp(1, 2) as usize; // mapping family 0 (mono/stereo); more: refused below
    if head[9] > 2 {
        return None;
    }
    let pre_skip = u16::from_le_bytes([head[10], head[11]]) as usize;
    let mut dec = opus_pure::OpusDecoder::new(48_000, channels).ok()?;
    let mut out: Vec<i16> = Vec::new();
    let mut buf = vec![0f32; 5760 * channels]; // 120 ms at 48 kHz, the longest Opus packet
    // packet 0 = OpusHead, packet 1 = OpusTags, then audio
    for p in pk.iter().skip(2) {
        if p.is_empty() {
            continue;
        }
        let n = dec.decode(p, 5760, &mut buf).ok()?;
        for x in &buf[..n * channels] {
            out.push(libm::rintf(x * 32768.0).clamp(-32768.0, 32767.0) as i16);
        }
        if out.len() > 48_000 * 2 * 60 * 20 {
            break; // 20 minutes is plenty for one decode in memory
        }
    }
    // the end: the last granule position counts samples, pre-skip included
    let total = (end as usize).saturating_sub(pre_skip);
    if end > 0 && (pre_skip + total) * channels <= out.len() {
        out.truncate((pre_skip + total) * channels);
    }
    let skip = (pre_skip * channels).min(out.len());
    out.drain(..skip);
    if out.is_empty() {
        return None;
    }
    let frames = (out.len() / channels) as u64;
    Some((Info { rate: 48_000, channels: channels as u32, bits: 16, frames }, to_bytes(&out)))
}
