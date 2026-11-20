//! Video files for managed programs (Video, MVideo): Matroska / WebM and MP4
//! (ISO BMFF, also QuickTime .mov) are demuxed here, video frames decoded on
//! request -- VP8 (`vp8.rs`, a port of the RFC 6386 reference decoder) or
//! Motion JPEG (`imgdecode.rs`) -- and the whole audio track decoded to 16-bit
//! PCM when the file is opened: Opus and Vorbis (repackaged as an Ogg stream
//! for `ogg.rs`), MP3 (`audiodec.rs`), or uncompressed PCM.
//!
//! H.264 / H.265 / AV1 / VP9 video and AAC audio are recognised and reported,
//! not decoded (H.264 and AAC are patent-encumbered; the others are later work).
//!
//! The natives are in `movieapi.rs`: `MovieOpen(byte[] file) -> id` (negative = not a video we read),
//! `MovieInfo(id) -> string` (`W H FPSx1000 FRAMES MS VCODEC ACODEC RATE CH
//! PCMBYTES`, then a line with the title, then one with what is unsupported),
//! `MovieFrame(id, index, int[] dst, dw, dh) -> 0 / -1` (frame `index` scaled
//! to dw x dh, 0xAARRGGBB), `MovieAudio(id, byte[] dst, offset) -> bytes`,
//! `MovieFree(id)`.

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum VCodec {
    None,
    Vp8,
    Mjpeg,
    Other,
}

/// A demuxed file: frames (offset, length, time in ms) and the audio track.
pub struct Movie {
    pub data: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub vcodec: VCodec,
    pub vname: String,
    pub frames: Vec<(usize, usize, u64)>,
    pub duration_ms: u64,
    pub title: String,
    pub aname: String,
    pub rate: u32,
    pub channels: u32,
    pub pcm: Vec<u8>,
    pub unsupported: String,
    // VP8 decoding state: the decoder and the last decoded frame index
    dec: Option<crate::vp8::Vp8>,
    decoded: Option<usize>,
    rgb_cache: Option<(usize, usize, usize, Vec<i32>)>,
}

impl Movie {
    fn empty(data: Vec<u8>) -> Movie {
        Movie {
            data,
            width: 0,
            height: 0,
            vcodec: VCodec::None,
            vname: String::new(),
            frames: Vec::new(),
            duration_ms: 0,
            title: String::new(),
            aname: String::new(),
            rate: 0,
            channels: 0,
            pcm: Vec::new(),
            unsupported: String::new(),
            dec: None,
            decoded: None,
            rgb_cache: None,
        }
    }

    pub fn fps_x1000(&self) -> u64 {
        if self.frames.len() < 2 {
            return 0;
        }
        let span = self.frames[self.frames.len() - 1].2.saturating_sub(self.frames[0].2);
        if span == 0 {
            return 0;
        }
        (self.frames.len() as u64 - 1) * 1_000_000 / span
    }

    /// Decode frame `index` and scale it into `out` (dw x dh).
    pub fn frame(&mut self, index: usize, out: &mut [i32], dw: usize, dh: usize) -> bool {
        if index >= self.frames.len() || out.len() < dw * dh || dw == 0 || dh == 0 {
            return false;
        }
        match self.vcodec {
            VCodec::Mjpeg => {
                let (o, n, _) = self.frames[index];
                let Some(p) = crate::imgdecode::decode(&self.data[o..o + n]) else { return false };
                let (w, h) = (p.width, p.height);
                let px = &p.frames[0].0;
                for y in 0..dh {
                    let sy = y * h / dh;
                    for x in 0..dw {
                        out[y * dw + x] = (px[sy * w + x * w / dw] | 0xFF00_0000) as i32;
                    }
                }
                true
            }
            VCodec::Vp8 => {
                // sequential decoding from the closest keyframe at or before `index`
                let start = match self.decoded {
                    Some(d) if d <= index => d + 1,
                    _ => {
                        let mut k = index;
                        while k > 0 && !is_vp8_key(&self.data[self.frames[k].0..]) {
                            k -= 1;
                        }
                        self.dec = Some(crate::vp8::Vp8::new());
                        k
                    }
                };
                if self.decoded == Some(index) {
                    if let Some((ci, cw, ch, px)) = &self.rgb_cache {
                        if *ci == index && *cw == dw && *ch == dh {
                            out[..dw * dh].copy_from_slice(px);
                            return true;
                        }
                    }
                }
                let dec = self.dec.get_or_insert_with(crate::vp8::Vp8::new);
                for i in start..=index {
                    let (o, n, _) = self.frames[i];
                    if dec.decode(&self.data[o..o + n]).is_err() {
                        self.decoded = None;
                        return false;
                    }
                    self.decoded = Some(i);
                }
                let Some(img) = dec.image() else { return false };
                crate::vp8::to_rgb(img, dec.width, dec.height, out, dw, dh);
                self.rgb_cache = Some((index, dw, dh, out[..dw * dh].to_vec()));
                true
            }
            _ => false,
        }
    }
}

fn is_vp8_key(f: &[u8]) -> bool {
    !f.is_empty() && f[0] & 1 == 0
}

// ---- Matroska / WebM ------------------------------------------------------------------

/// An EBML variable-length integer at `p`: (value, length); IDs keep the marker.
fn vint(d: &[u8], p: usize, keep_marker: bool) -> Option<(u64, usize)> {
    let first = *d.get(p)?;
    if first == 0 {
        return None;
    }
    let len = first.leading_zeros() as usize + 1;
    if p + len > d.len() || len > 8 {
        return None;
    }
    let mut v = if keep_marker { first as u64 } else { (first as u64) & ((1u64 << (8 - len)) - 1) };
    for k in 1..len {
        v = v << 8 | d[p + k] as u64;
    }
    Some((v, len))
}

fn be_uint(d: &[u8]) -> u64 {
    d.iter().fold(0u64, |a, b| a << 8 | *b as u64)
}

fn be_float(d: &[u8]) -> f64 {
    match d.len() {
        4 => f32::from_bits(be_uint(d) as u32) as f64,
        8 => f64::from_bits(be_uint(d)),
        _ => 0.0,
    }
}

struct MkvTrack {
    number: u64,
    kind: u64,
    codec: String,
    private: Vec<u8>,
    width: u64,
    height: u64,
    rate: f64,
    channels: u64,
    default_duration: u64,
}

/// Walk the elements of `d[start..end]`, calling `f(id, body_start, body_end)`.
fn elements(d: &[u8], start: usize, end: usize, mut f: impl FnMut(u64, usize, usize)) {
    let mut p = start;
    while p < end {
        let Some((id, il)) = vint(d, p, true) else { return };
        let Some((size, sl)) = vint(d, p + il, false) else { return };
        let body = p + il + sl;
        let unknown = size == (1u64 << (7 * sl)) - 1;
        let stop = if unknown { end } else { (body as u64 + size).min(end as u64) as usize };
        f(id, body, stop);
        p = stop;
    }
}

fn mkv(m: &mut Movie) -> bool {
    let d = core::mem::take(&mut m.data);
    let mut tracks: Vec<MkvTrack> = Vec::new();
    let mut scale = 1_000_000u64; // ns per timecode unit
    let mut duration = 0f64;
    let mut title = String::new();
    // (track, time in units, data offset, length)
    let mut blocks: Vec<(u64, i64, usize, usize)> = Vec::new();
    let mut seg = None;
    elements(&d, 0, d.len(), |id, s, e| {
        if id == 0x18538067 {
            seg = Some((s, e));
        }
    });
    let Some((ss, se)) = seg else {
        m.data = d;
        return false;
    };
    elements(&d, ss, se, |id, s, e| match id {
        0x1549A966 => elements(&d, s, e, |id, s, e| match id {
            0x2AD7B1 => scale = be_uint(&d[s..e]).max(1),
            0x4489 => duration = be_float(&d[s..e]),
            0x7BA9 => title = String::from_utf8_lossy(&d[s..e]).into_owned(),
            _ => {}
        }),
        0x1654AE6B => elements(&d, s, e, |id, s, e| {
            if id != 0xAE {
                return;
            }
            let mut t = MkvTrack { number: 0, kind: 0, codec: String::new(), private: Vec::new(), width: 0, height: 0, rate: 8000.0, channels: 1, default_duration: 0 };
            elements(&d, s, e, |id, s, e| match id {
                0xD7 => t.number = be_uint(&d[s..e]),
                0x83 => t.kind = be_uint(&d[s..e]),
                0x86 => t.codec = String::from_utf8_lossy(&d[s..e]).into_owned(),
                0x63A2 => t.private = d[s..e].to_vec(),
                0x23E383 => t.default_duration = be_uint(&d[s..e]),
                0xE0 => elements(&d, s, e, |id, s, e| match id {
                    0xB0 => t.width = be_uint(&d[s..e]),
                    0xBA => t.height = be_uint(&d[s..e]),
                    _ => {}
                }),
                0xE1 => elements(&d, s, e, |id, s, e| match id {
                    0xB5 => t.rate = be_float(&d[s..e]),
                    0x9F => t.channels = be_uint(&d[s..e]),
                    _ => {}
                }),
                _ => {}
            });
            tracks.push(t);
        }),
        0x1F43B675 => {
            let mut base = 0u64;
            elements(&d, s, e, |id, s, e| match id {
                0xE7 => base = be_uint(&d[s..e]),
                0xA3 => block(&d, s, e, base, &mut blocks),
                0xA0 => elements(&d, s, e, |id, s, e| {
                    if id == 0xA1 {
                        block(&d, s, e, base, &mut blocks);
                    }
                }),
                _ => {}
            });
        }
        _ => {}
    });
    m.data = d;
    m.title = title;
    let video = tracks.iter().find(|t| t.kind == 1);
    let audio = tracks.iter().find(|t| t.kind == 2);
    let to_ms = |t: i64| -> u64 { (t.max(0) as u64).saturating_mul(scale) / 1_000_000 };
    if let Some(v) = video {
        m.width = v.width as usize;
        m.height = v.height as usize;
        m.vname = v.codec.clone();
        m.vcodec = match v.codec.as_str() {
            "V_VP8" => VCodec::Vp8,
            "V_MJPEG" => VCodec::Mjpeg,
            _ => VCodec::Other,
        };
        for b in blocks.iter().filter(|b| b.0 == v.number) {
            m.frames.push((b.2, b.3, to_ms(b.1)));
        }
        m.frames.sort_by_key(|f| f.2);
        // VP8 invisible (alt-ref) frames: keep them for decoding, they share a time
    }
    m.duration_ms = if duration > 0.0 { (duration * scale as f64 / 1_000_000.0) as u64 } else { m.frames.last().map(|f| f.2).unwrap_or(0) };
    if let Some(a) = audio {
        m.aname = a.codec.clone();
        let packets: Vec<&[u8]> = blocks.iter().filter(|b| b.0 == a.number).map(|b| &m.data[b.2..b.2 + b.3]).collect();
        let decoded = match a.codec.as_str() {
            "A_OPUS" => crate::ogg::decode(&ogg_stream(&[&a.private, b"OpusTags\x00\x00\x00\x00\x00\x00\x00\x00"], &packets)),
            "A_VORBIS" => xiph_headers(&a.private).and_then(|h| crate::ogg::decode(&ogg_stream(&h.iter().map(|v| v.as_slice()).collect::<Vec<_>>(), &packets))),
            "A_MPEG/L3" => crate::audiodec::mp3(&packets.concat()),
            "A_PCM/INT/LIT" => {
                let pcm = packets.concat();
                Some((crate::flac::Info { rate: a.rate as u32, channels: a.channels as u32, bits: 16, frames: (pcm.len() / 2 / a.channels.max(1) as usize) as u64 }, pcm))
            }
            _ => None,
        };
        set_audio(m, decoded, &a.codec);
    }
    m.vcodec != VCodec::None || !m.pcm.is_empty()
}

/// One (Simple)Block: track, relative time, flags, lacing.
fn block(d: &[u8], s: usize, e: usize, base: u64, out: &mut Vec<(u64, i64, usize, usize)>) {
    let Some((track, tl)) = vint(d, s, false) else { return };
    let p = s + tl;
    if p + 3 > e {
        return;
    }
    let rel = i16::from_be_bytes([d[p], d[p + 1]]) as i64;
    let flags = d[p + 2];
    let t = base as i64 + rel;
    let mut q = p + 3;
    let lacing = (flags >> 1) & 3;
    if lacing == 0 {
        out.push((track, t, q, e - q));
        return;
    }
    let n = d[q] as usize + 1;
    q += 1;
    let mut sizes: Vec<usize> = Vec::new();
    match lacing {
        1 => {
            // Xiph
            for _ in 0..n - 1 {
                let mut sz = 0;
                while q < e {
                    let b = d[q] as usize;
                    q += 1;
                    sz += b;
                    if b < 255 {
                        break;
                    }
                }
                sizes.push(sz);
            }
        }
        3 => {
            // EBML: first size, then signed differences
            let Some((first, l)) = vint(d, q, false) else { return };
            q += l;
            sizes.push(first as usize);
            let mut prev = first as i64;
            for _ in 1..n - 1 {
                let Some((v, l)) = vint(d, q, false) else { return };
                let bias = (1i64 << (7 * l - 1)) - 1;
                prev += v as i64 - bias;
                q += l;
                sizes.push(prev.max(0) as usize);
            }
        }
        _ => {
            // fixed
            let each = (e - q) / n;
            for _ in 0..n - 1 {
                sizes.push(each);
            }
        }
    }
    let used: usize = sizes.iter().sum();
    if q + used > e {
        return;
    }
    sizes.push(e - q - used);
    for sz in sizes {
        out.push((track, t, q, sz));
        q += sz;
    }
}

/// Vorbis codec private data: three Xiph-laced headers.
fn xiph_headers(p: &[u8]) -> Option<Vec<Vec<u8>>> {
    let n = *p.first()? as usize + 1;
    let mut q = 1;
    let mut sizes = Vec::new();
    for _ in 0..n - 1 {
        let mut sz = 0;
        loop {
            let b = *p.get(q)? as usize;
            q += 1;
            sz += b;
            if b < 255 {
                break;
            }
        }
        sizes.push(sz);
    }
    let used: usize = sizes.iter().sum();
    if q + used > p.len() {
        return None;
    }
    sizes.push(p.len() - q - used);
    let mut out = Vec::new();
    for sz in sizes {
        out.push(p[q..q + sz].to_vec());
        q += sz;
    }
    Some(out)
}

fn ogg_crc(d: &[u8]) -> u32 {
    let mut crc = 0u32;
    for b in d {
        crc ^= (*b as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 { crc << 1 ^ 0x04c1_1db7 } else { crc << 1 };
        }
    }
    crc
}

/// An Ogg stream with one packet per page: the header packets, then the audio.
fn ogg_stream(headers: &[&[u8]], packets: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    let all: Vec<&[u8]> = headers.iter().copied().chain(packets.iter().copied()).collect();
    let last = all.len().saturating_sub(1);
    for (seq, p) in all.iter().enumerate() {
        let mut lacing = Vec::new();
        let mut left = p.len();
        loop {
            let n = left.min(255);
            lacing.push(n as u8);
            left -= n;
            if n < 255 {
                break;
            }
        }
        if lacing.len() > 255 {
            continue; // a packet over 64 KiB: not audio we expect
        }
        let flags = if seq == 0 { 2 } else if seq == last { 4 } else { 0 };
        let mut page = Vec::with_capacity(27 + lacing.len() + p.len());
        page.extend_from_slice(b"OggS");
        page.push(0);
        page.push(flags);
        page.extend_from_slice(&u64::MAX.to_le_bytes()); // no granule positions
        page.extend_from_slice(&0x425a_4f53u32.to_le_bytes()); // serial "BZOS"
        page.extend_from_slice(&(seq as u32).to_le_bytes());
        page.extend_from_slice(&0u32.to_le_bytes());
        page.push(lacing.len() as u8);
        page.extend_from_slice(&lacing);
        page.extend_from_slice(p);
        let crc = ogg_crc(&page);
        page[22..26].copy_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&page);
    }
    out
}

fn set_audio(m: &mut Movie, decoded: Option<(crate::flac::Info, Vec<u8>)>, name: &str) {
    match decoded {
        Some((info, pcm)) if !pcm.is_empty() && info.channels >= 1 && info.channels <= 2 => {
            m.rate = info.rate;
            m.channels = info.channels;
            m.pcm = pcm;
        }
        _ => {
            if !m.unsupported.is_empty() {
                m.unsupported.push_str("; ");
            }
            m.unsupported.push_str(&alloc::format!("audio {} not decoded", name));
        }
    }
}

// ---- MP4 / QuickTime ------------------------------------------------------------------

fn be32(d: &[u8], p: usize) -> u32 {
    u32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]])
}

/// Boxes of `d[s..e]`: (type, body start, end).
fn boxes(d: &[u8], s: usize, e: usize, mut f: impl FnMut(&[u8; 4], usize, usize)) {
    let mut p = s;
    while p + 8 <= e {
        let mut size = be32(d, p) as u64;
        let ty: [u8; 4] = d[p + 4..p + 8].try_into().unwrap();
        let mut body = p + 8;
        if size == 1 && p + 16 <= e {
            size = u64::from_be_bytes(d[p + 8..p + 16].try_into().unwrap());
            body = p + 16;
        } else if size == 0 {
            size = (e - p) as u64;
        }
        if size < 8 {
            return;
        }
        let end = (p as u64 + size).min(e as u64) as usize;
        f(&ty, body, end);
        p = end;
    }
}

#[derive(Default)]
struct Mp4Track {
    handler: [u8; 4],
    timescale: u32,
    entry: [u8; 4],
    width: u32,
    height: u32,
    channels: u32,
    rate: u32,
    bits: u32,
    oti: u8, // esds object type (0x40 AAC, 0x69 / 0x6B MP3)
    dops: Vec<u8>,
    stts: Vec<(u32, u32)>,
    stsc: Vec<(u32, u32)>,
    sizes: Vec<u32>,
    offsets: Vec<u64>,
}

fn mp4(m: &mut Movie) -> bool {
    let d = core::mem::take(&mut m.data);
    let mut tracks: Vec<Mp4Track> = Vec::new();
    boxes(&d, 0, d.len(), |ty, s, e| {
        if ty != b"moov" {
            return;
        }
        boxes(&d, s, e, |ty, s, e| {
            if ty == b"udta" {
                boxes(&d, s, e, |ty, s, e| {
                    // QuickTime: udta/(c)nam = length + language + text
                    if ty == b"\xa9nam" && e - s > 4 {
                        m.title = String::from_utf8_lossy(&d[s + 4..e]).into_owned();
                    }
                    // MP4: udta/meta/ilst/(c)nam/data = type + locale + text
                    if ty == b"meta" && e >= s + 4 {
                        boxes(&d, s + 4, e, |ty, s, e| {
                            if ty == b"ilst" {
                                boxes(&d, s, e, |ty, s, e| {
                                    if ty == b"\xa9nam" {
                                        boxes(&d, s, e, |ty, s, e| {
                                            if ty == b"data" && e > s + 8 {
                                                m.title = String::from_utf8_lossy(&d[s + 8..e]).into_owned();
                                            }
                                        });
                                    }
                                });
                            }
                        });
                    }
                });
            }
            if ty != b"trak" {
                return;
            }
            let mut t = Mp4Track::default();
            trak(&d, s, e, &mut t);
            tracks.push(t);
        });
    });
    m.data = d;
    let samples = |t: &Mp4Track| -> Vec<(usize, usize, u64)> {
        // chunk offsets + samples per chunk -> per-sample offsets; stts -> times
        let mut out = Vec::with_capacity(t.sizes.len());
        let mut si = 0usize;
        for (ci, off) in t.offsets.iter().enumerate() {
            let chunk = ci as u32 + 1;
            let per = t.stsc.iter().rev().find(|(first, _)| *first <= chunk).map(|x| x.1).unwrap_or(1);
            let mut o = *off as usize;
            for _ in 0..per {
                if si >= t.sizes.len() {
                    break;
                }
                let sz = t.sizes[si] as usize;
                out.push((o, sz, 0));
                o += sz;
                si += 1;
            }
        }
        let mut time = 0u64;
        let mut k = 0usize;
        for (count, delta) in &t.stts {
            for _ in 0..*count {
                if k < out.len() {
                    out[k].2 = time * 1000 / t.timescale.max(1) as u64;
                    k += 1;
                }
                time += *delta as u64;
            }
        }
        out.retain(|(o, sz, _)| o + sz <= m.data.len());
        out
    };
    let video = tracks.iter().find(|t| &t.handler == b"vide");
    let audio = tracks.iter().find(|t| &t.handler == b"soun");
    if let Some(v) = video {
        m.width = v.width as usize;
        m.height = v.height as usize;
        m.vname = String::from_utf8_lossy(&v.entry).into_owned();
        m.vcodec = match &v.entry {
            b"jpeg" | b"mjpa" | b"mjpb" | b"MJPG" => VCodec::Mjpeg,
            b"mp4v" if v.oti == 0x6C => VCodec::Mjpeg,
            b"vp08" => VCodec::Vp8,
            _ => VCodec::Other,
        };
        m.frames = samples(v);
        if m.vcodec == VCodec::Other {
            m.unsupported.push_str(&alloc::format!(
                "video {} not decoded{}",
                m.vname,
                if &v.entry == b"avc1" || &v.entry == b"avc3" { " (H.264 is patent-encumbered)" } else { "" }
            ));
        }
        m.duration_ms = m.frames.last().map(|f| f.2).unwrap_or(0) + if m.frames.len() > 1 { m.frames[1].2 - m.frames[0].2 } else { 0 };
    }
    if let Some(a) = audio {
        let pcm_entry = matches!(&a.entry, b"sowt" | b"twos" | b"lpcm" | b"ipcm" | b"raw ");
        let s = if pcm_entry {
            // QuickTime PCM: stsz counts single samples; read whole chunks
            let frame = (a.channels.max(1) * a.bits.max(8) / 8) as usize;
            let mut v = Vec::new();
            for (ci, off) in a.offsets.iter().enumerate() {
                let chunk = ci as u32 + 1;
                let per = a.stsc.iter().rev().find(|(first, _)| *first <= chunk).map(|x| x.1).unwrap_or(1) as usize;
                let (o, n) = (*off as usize, per * frame);
                if o + n <= m.data.len() {
                    v.push((o, n, 0u64));
                }
            }
            v
        } else {
            samples(a)
        };
        let packets: Vec<&[u8]> = s.iter().map(|(o, n, _)| &m.data[*o..*o + *n]).collect();
        let name = String::from_utf8_lossy(&a.entry).into_owned();
        let decoded = match &a.entry {
            b".mp3" => crate::audiodec::mp3(&packets.concat()),
            b"mp4a" if a.oti == 0x69 || a.oti == 0x6B => crate::audiodec::mp3(&packets.concat()),
            b"sowt" | b"lpcm" | b"ipcm" if a.bits == 16 => {
                let pcm = packets.concat();
                Some((crate::flac::Info { rate: a.rate, channels: a.channels, bits: 16, frames: (pcm.len() / 2 / a.channels.max(1) as usize) as u64 }, pcm))
            }
            b"twos" if a.bits == 16 => {
                let mut pcm = packets.concat();
                for w in pcm.chunks_exact_mut(2) {
                    w.swap(0, 1);
                }
                Some((crate::flac::Info { rate: a.rate, channels: a.channels, bits: 16, frames: (pcm.len() / 2 / a.channels.max(1) as usize) as u64 }, pcm))
            }
            b"Opus" if a.dops.len() >= 11 => {
                // dOps (big endian) -> OpusHead (little endian)
                let o = &a.dops;
                let mut head = Vec::from(&b"OpusHead"[..]);
                head.push(1);
                head.push(o[1]);
                head.extend_from_slice(&u16::from_be_bytes([o[2], o[3]]).to_le_bytes());
                head.extend_from_slice(&u32::from_be_bytes([o[4], o[5], o[6], o[7]]).to_le_bytes());
                head.extend_from_slice(&i16::from_be_bytes([o[8], o[9]]).to_le_bytes());
                head.push(o[10]);
                crate::ogg::decode(&ogg_stream(&[&head, b"OpusTags\x00\x00\x00\x00\x00\x00\x00\x00"], &packets))
            }
            _ => None,
        };
        let label = if &a.entry == b"mp4a" && a.oti == 0x40 { String::from("AAC (patent-encumbered)") } else { name.clone() };
        m.aname = name;
        set_audio(m, decoded, &label);
        if m.duration_ms == 0 && m.rate > 0 {
            m.duration_ms = (m.pcm.len() as u64 / 2 / m.channels as u64) * 1000 / m.rate as u64;
        }
    }
    m.vcodec != VCodec::None || !m.pcm.is_empty()
}

/// The object type of an `esds` box (0x40 AAC, 0x69 / 0x6B MP3, 0x6C JPEG,
/// 0x20 MPEG-4 Part 2): ES descriptor (tag 3) -> decoder config (tag 4).
fn esds_object_type(b: &[u8]) -> u8 {
    fn len(b: &[u8], p: &mut usize) -> usize {
        let mut n = 0usize;
        for _ in 0..4 {
            let Some(&x) = b.get(*p) else { return 0 };
            *p += 1;
            n = n << 7 | (x & 0x7F) as usize;
            if x & 0x80 == 0 {
                break;
            }
        }
        n
    }
    let mut p = 4; // version + flags
    if b.get(p) != Some(&3) {
        return 0;
    }
    p += 1;
    let _ = len(b, &mut p);
    p += 2; // ES_ID
    let Some(&flags) = b.get(p) else { return 0 };
    p += 1;
    if flags & 0x80 != 0 {
        p += 2;
    }
    if flags & 0x40 != 0 {
        p += 1 + *b.get(p).unwrap_or(&0) as usize;
    }
    if flags & 0x20 != 0 {
        p += 2;
    }
    if b.get(p) != Some(&4) {
        return 0;
    }
    p += 1;
    let _ = len(b, &mut p);
    *b.get(p).unwrap_or(&0)
}

fn trak(d: &[u8], s: usize, e: usize, t: &mut Mp4Track) {
    boxes(d, s, e, |ty, s, e| match ty {
        b"mdia" | b"minf" | b"stbl" => trak(d, s, e, t),
        b"mdhd" if e >= s + 20 => {
            t.timescale = if d[s] == 1 { be32(d, s + 20) } else { be32(d, s + 12) };
        }
        // the first handler (mdia's); QuickTime puts a data handler in minf too
        b"hdlr" if e >= s + 12 && t.handler == [0; 4] => t.handler = d[s + 8..s + 12].try_into().unwrap(),
        b"stsd" if e >= s + 16 => {
            let entry = s + 8;
            let size = be32(d, entry) as usize;
            t.entry = d[entry + 4..entry + 8].try_into().unwrap();
            let body = entry + 8;
            let end = (entry + size).min(e);
            if &t.handler == b"vide" && body + 28 <= end {
                t.width = u16::from_be_bytes([d[body + 24], d[body + 25]]) as u32;
                t.height = u16::from_be_bytes([d[body + 26], d[body + 27]]) as u32;
                // child boxes after the 78-byte visual sample entry: esds (mp4v)
                if body + 78 <= end {
                    boxes(d, body + 78, end, |ty, s, e| {
                        if ty == b"esds" {
                            t.oti = esds_object_type(&d[s..e]);
                        }
                    });
                }
            } else if &t.handler == b"soun" && body + 28 <= end {
                t.channels = u16::from_be_bytes([d[body + 16], d[body + 17]]) as u32;
                t.bits = u16::from_be_bytes([d[body + 18], d[body + 19]]) as u32;
                t.rate = u16::from_be_bytes([d[body + 24], d[body + 25]]) as u32;
                // child boxes after the sound sample entry (28 bytes; QuickTime
                // version 1 adds 16, version 2 adds 36): esds, dOps
                let version = u16::from_be_bytes([d[body + 8], d[body + 9]]);
                let children = body
                    + 28
                    + match version {
                        1 => 16,
                        2 => 36,
                        _ => 0,
                    };
                boxes(d, children.min(end), end, |ty, s, e| {
                    if ty == b"esds" {
                        t.oti = esds_object_type(&d[s..e]);
                    }
                    if ty == b"dOps" {
                        t.dops = d[s..e].to_vec();
                    }
                });
            }
        }
        b"stts" if e >= s + 8 => {
            let n = be32(d, s + 4) as usize;
            for k in 0..n {
                let p = s + 8 + k * 8;
                if p + 8 > e {
                    break;
                }
                t.stts.push((be32(d, p), be32(d, p + 4)));
            }
        }
        b"stsc" if e >= s + 8 => {
            let n = be32(d, s + 4) as usize;
            for k in 0..n {
                let p = s + 8 + k * 12;
                if p + 12 > e {
                    break;
                }
                t.stsc.push((be32(d, p), be32(d, p + 4)));
            }
        }
        b"stsz" if e >= s + 12 => {
            let fixed = be32(d, s + 4);
            let n = be32(d, s + 8) as usize;
            for k in 0..n {
                if fixed != 0 {
                    t.sizes.push(fixed);
                } else {
                    let p = s + 12 + k * 4;
                    if p + 4 > e {
                        break;
                    }
                    t.sizes.push(be32(d, p));
                }
            }
        }
        b"stco" if e >= s + 8 => {
            let n = be32(d, s + 4) as usize;
            for k in 0..n {
                let p = s + 8 + k * 4;
                if p + 4 > e {
                    break;
                }
                t.offsets.push(be32(d, p) as u64);
            }
        }
        b"co64" if e >= s + 8 => {
            let n = be32(d, s + 4) as usize;
            for k in 0..n {
                let p = s + 8 + k * 8;
                if p + 8 > e {
                    break;
                }
                t.offsets.push(u64::from_be_bytes(d[p..p + 8].try_into().unwrap()));
            }
        }
        _ => {}
    });
}

/// Demux a file (Matroska / WebM or MP4 / QuickTime).
pub fn open(data: Vec<u8>) -> Option<Movie> {
    let mut m = Movie::empty(data);
    let d = &m.data;
    let ok = if d.len() >= 4 && d[..4] == [0x1A, 0x45, 0xDF, 0xA3] {
        mkv(&mut m)
    } else if d.len() >= 12 && (&d[4..8] == b"ftyp" || &d[4..8] == b"moov" || &d[4..8] == b"mdat" || &d[4..8] == b"wide") {
        mp4(&mut m)
    } else {
        false
    };
    if !ok {
        return None;
    }
    if m.vcodec == VCodec::Vp8 && (m.width == 0 || m.height == 0) {
        // the keyframe header carries the size
        if let Some((o, _, _)) = m.frames.first() {
            let f = &m.data[*o..];
            if f.len() >= 10 {
                m.width = (u16::from_le_bytes([f[6], f[7]]) & 0x3FFF) as usize;
                m.height = (u16::from_le_bytes([f[8], f[9]]) & 0x3FFF) as usize;
            }
        }
    }
    Some(m)
}
