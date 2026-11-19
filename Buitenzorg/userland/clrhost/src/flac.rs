//! A FLAC decoder (no_std): STREAMINFO, frames with CONSTANT / VERBATIM /
//! FIXED / LPC subframes, Rice-coded residuals (incl. escapes), wasted bits,
//! and the stereo decorrelations (left/side, side/right, mid/side). Output is
//! 16-bit little-endian PCM, interleaved, at most two channels (more are
//! mixed down to the first two). CRCs are not checked: a broken frame is
//! skipped by looking for the next frame sync.

use alloc::vec;
use alloc::vec::Vec;

pub struct Info {
    pub rate: u32,
    pub channels: u32,
    pub bits: u32,
    pub frames: u64,
}

struct Bits<'a> {
    d: &'a [u8],
    pos: usize, // bit position
}

impl<'a> Bits<'a> {
    fn new(d: &'a [u8], byte: usize) -> Self {
        Bits { d, pos: byte * 8 }
    }
    fn bit(&mut self) -> Option<u32> {
        let byte = *self.d.get(self.pos / 8)?;
        let b = (byte >> (7 - self.pos % 8)) & 1;
        self.pos += 1;
        Some(b as u32)
    }
    fn read(&mut self, n: u32) -> Option<u64> {
        let mut v = 0u64;
        let mut left = n;
        while left > 0 {
            if self.pos % 8 == 0 && left >= 8 {
                v = v << 8 | *self.d.get(self.pos / 8)? as u64;
                self.pos += 8;
                left -= 8;
            } else {
                v = v << 1 | self.bit()? as u64;
                left -= 1;
            }
        }
        Some(v)
    }
    fn signed(&mut self, n: u32) -> Option<i64> {
        if n == 0 {
            return Some(0);
        }
        let v = self.read(n)?;
        Some(((v << (64 - n)) as i64) >> (64 - n))
    }
    fn unary(&mut self) -> Option<u32> {
        let mut n = 0;
        while self.bit()? == 0 {
            n += 1;
            if n > 1 << 20 {
                return None;
            }
        }
        Some(n)
    }
    fn align(&mut self) {
        self.pos = (self.pos + 7) & !7;
    }
    fn byte_pos(&self) -> usize {
        self.pos / 8
    }
}

/// STREAMINFO and where the first frame starts.
pub fn info(d: &[u8]) -> Option<(Info, usize)> {
    if d.len() < 42 || &d[..4] != b"fLaC" {
        return None;
    }
    let mut p = 4;
    let mut info = None;
    loop {
        let h = *d.get(p)?;
        let len = (*d.get(p + 1)? as usize) << 16 | (*d.get(p + 2)? as usize) << 8 | *d.get(p + 3)? as usize;
        let body = p + 4;
        if h & 0x7F == 0 && len >= 34 {
            let mut b = Bits::new(d, body + 10);
            let rate = b.read(20)? as u32;
            let channels = b.read(3)? as u32 + 1;
            let bits = b.read(5)? as u32 + 1;
            let frames = b.read(36)?;
            info = Some(Info { rate, channels, bits, frames });
        }
        p = body + len;
        if h & 0x80 != 0 {
            break;
        }
        if p >= d.len() {
            return None;
        }
    }
    Some((info?, p))
}

fn block_size(code: u32, b: &mut Bits) -> Option<usize> {
    Some(match code {
        1 => 192,
        2..=5 => 576 << (code - 2),
        6 => b.read(8)? as usize + 1,
        7 => b.read(16)? as usize + 1,
        8..=15 => 256 << (code - 8),
        _ => return None,
    })
}

/// One subframe into `out` (block_size samples at `bps` bits).
fn subframe(b: &mut Bits, bps: u32, n: usize, out: &mut [i64]) -> Option<()> {
    if b.bit()? != 0 {
        return None;
    }
    let kind = b.read(6)? as u32;
    let mut wasted = 0;
    if b.bit()? == 1 {
        wasted = b.unary()? + 1;
    }
    let bps = bps.checked_sub(wasted)?;
    match kind {
        0 => {
            let v = b.signed(bps)?;
            out[..n].iter_mut().for_each(|s| *s = v);
        }
        1 => {
            for s in out[..n].iter_mut() {
                *s = b.signed(bps)?;
            }
        }
        8..=12 => {
            let order = (kind - 8) as usize;
            if order > n {
                return None;
            }
            for s in out[..order].iter_mut() {
                *s = b.signed(bps)?;
            }
            residual(b, n, order, out)?;
            for i in order..n {
                let p = match order {
                    0 => 0,
                    1 => out[i - 1],
                    2 => 2 * out[i - 1] - out[i - 2],
                    3 => 3 * out[i - 1] - 3 * out[i - 2] + out[i - 3],
                    _ => 4 * out[i - 1] - 6 * out[i - 2] + 4 * out[i - 3] - out[i - 4],
                };
                out[i] += p;
            }
        }
        32..=63 => {
            let order = (kind - 31) as usize;
            if order > n {
                return None;
            }
            for s in out[..order].iter_mut() {
                *s = b.signed(bps)?;
            }
            let precision = b.read(4)? as u32 + 1;
            if precision == 16 {
                return None;
            }
            let shift = b.signed(5)?;
            if shift < 0 {
                return None;
            }
            let mut coefs = [0i64; 32];
            for c in coefs[..order].iter_mut() {
                *c = b.signed(precision)?;
            }
            residual(b, n, order, out)?;
            for i in order..n {
                let mut sum: i64 = 0;
                for (j, c) in coefs[..order].iter().enumerate() {
                    sum += c * out[i - 1 - j];
                }
                out[i] += sum >> shift;
            }
        }
        _ => return None,
    }
    if wasted > 0 {
        out[..n].iter_mut().for_each(|s| *s <<= wasted);
    }
    Some(())
}

/// Rice-coded residual for samples order..n (added to `out`).
fn residual(b: &mut Bits, n: usize, order: usize, out: &mut [i64]) -> Option<()> {
    let method = b.read(2)? as u32;
    let (pbits, escape) = match method {
        0 => (4, 15),
        1 => (5, 31),
        _ => return None,
    };
    let porder = b.read(4)? as u32;
    let parts = 1usize << porder;
    let per = n >> porder;
    let mut i = order;
    for p in 0..parts {
        let count = if p == 0 { per.checked_sub(order)? } else { per };
        let param = b.read(pbits)? as u32;
        if param == escape {
            let raw = b.read(5)? as u32;
            for _ in 0..count {
                out[i] = b.signed(raw)?;
                i += 1;
            }
        } else {
            for _ in 0..count {
                let q = b.unary()? as u64;
                let u = q << param | b.read(param)?;
                out[i] = ((u >> 1) as i64) ^ -((u & 1) as i64);
                i += 1;
            }
        }
    }
    Some(())
}

/// Decode one frame at `p`: appends 16-bit PCM to `pcm`; returns the next frame's offset.
fn frame(d: &[u8], p: usize, si: &Info, pcm: &mut Vec<u8>, bufs: &mut [Vec<i64>; 8]) -> Option<usize> {
    let mut b = Bits::new(d, p);
    if b.read(15)? != 0x7FFC {
        return None;
    }
    let _blocking = b.bit()?;
    let bs_code = b.read(4)? as u32;
    let rate_code = b.read(4)? as u32;
    let chan = b.read(4)? as u32;
    let size_code = b.read(3)? as u32;
    b.read(1)?;
    // the UTF-8-like coded frame / sample number
    let first = b.read(8)? as u32;
    let extra = (first as u8).leading_ones().saturating_sub(1);
    for _ in 0..extra {
        b.read(8)?;
    }
    let n = block_size(bs_code, &mut b)?;
    match rate_code {
        12 => {
            b.read(8)?;
        }
        13 | 14 => {
            b.read(16)?;
        }
        _ => {}
    }
    b.read(8)?; // CRC-8
    let bps = match size_code {
        0 => si.bits,
        1 => 8,
        2 => 12,
        4 => 16,
        5 => 20,
        6 => 24,
        7 => 32,
        _ => return None,
    };
    let channels = match chan {
        0..=7 => chan + 1,
        8..=10 => 2,
        _ => return None,
    } as usize;
    for (c, buf) in bufs.iter_mut().enumerate().take(channels) {
        if buf.len() < n {
            buf.resize(n, 0);
        }
        // the side channel carries one more bit
        let extra = match (chan, c) {
            (8, 1) | (9, 0) | (10, 1) => 1,
            _ => 0,
        };
        subframe(&mut b, bps + extra, n, buf)?;
    }
    for i in 0..n {
        match chan {
            8 => bufs[1][i] = bufs[0][i] - bufs[1][i],
            9 => bufs[0][i] += bufs[1][i],
            10 => {
                let side = bufs[1][i];
                let mid = bufs[0][i] << 1 | (side & 1);
                bufs[0][i] = (mid + side) >> 1;
                bufs[1][i] = (mid - side) >> 1;
            }
            _ => {}
        }
    }
    let out_ch = channels.min(2);
    for i in 0..n {
        for c in 0..out_ch {
            let v = bufs[c][i];
            let s = if bps > 16 { v >> (bps - 16) } else { v << (16 - bps) };
            let s = s.clamp(-32768, 32767) as i16;
            pcm.extend_from_slice(&s.to_le_bytes());
        }
    }
    b.align();
    b.read(16)?; // CRC-16
    Some(b.byte_pos())
}

/// The whole stream as 16-bit PCM: (info with channels <= 2, bytes).
pub fn decode(d: &[u8]) -> Option<(Info, Vec<u8>)> {
    let (si, mut p) = info(d)?;
    let out_ch = si.channels.min(2) as usize;
    let mut pcm = Vec::with_capacity((si.frames as usize).min(64 << 20) * out_ch * 2);
    let mut bufs: [Vec<i64>; 8] = [vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![]];
    while p + 2 < d.len() {
        match frame(d, p, &si, &mut pcm, &mut bufs) {
            Some(next) if next > p => p = next,
            _ => {
                // resynchronise on the next frame header (0xFFF8 / 0xFFF9)
                p += 1;
                while p + 1 < d.len() && !(d[p] == 0xFF && (d[p + 1] & 0xFE) == 0xF8) {
                    p += 1;
                }
            }
        }
    }
    let max = (si.frames as usize) * out_ch * 2;
    if si.frames > 0 && pcm.len() > max {
        pcm.truncate(max);
    }
    Some((Info { rate: si.rate, channels: out_ch as u32, bits: 16, frames: (pcm.len() / (out_ch * 2)) as u64 }, pcm))
}
