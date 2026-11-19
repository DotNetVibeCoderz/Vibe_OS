//! Codebooks: building the Huffman tables at setup, and decoding with them.
#[allow(unused_imports)]
use alloc::{boxed::Box, vec, vec::Vec};

use crate::io::{Io, EOP};
use crate::VorbisError;

/// stb_vorbis's STB_VORBIS_FAST_HUFFMAN_LENGTH: codes up to this many bits are found with one
/// table look-up.
pub(crate) const FAST_HUFFMAN_LENGTH: u32 = 10;
pub(crate) const FAST_HUFFMAN_TABLE_SIZE: usize = 1 << FAST_HUFFMAN_LENGTH;
const FAST_HUFFMAN_TABLE_MASK: u32 = FAST_HUFFMAN_TABLE_SIZE as u32 - 1;
/// Where a codeword's length sits in its entry of the fast table, above its position (which is
/// under 32767).
const FAST_LENGTH_SHIFT: u32 = 16;

/// A codeword length meaning "this entry is not used".
pub(crate) const NO_CODE: u8 = 255;

pub(crate) struct Codebook {
    pub dimensions: i32,
    pub entries: i32,
    pub codeword_lengths: Vec<u8>,
    pub minimum_value: f32,
    pub delta_value: f32,
    pub value_bits: u8,
    pub lookup_type: u8,
    pub sequence_p: bool,
    pub sparse: bool,
    pub lookup_values: u32,
    pub multiplicands: Vec<f32>,
    /// Nothing for a sparse codebook once it is built.
    pub codewords: Option<Vec<u32>>,
    /// For each run of bits a codeword can begin with: -1, or the codeword's position in the
    /// tables with its length above it ([`FAST_LENGTH_SHIFT`]), so that the usual codeword is
    /// one look in one table. stb_vorbis keeps the position alone and looks the length up.
    pub fast_huffman: Box<[i32; FAST_HUFFMAN_TABLE_SIZE]>,
    pub sorted_codewords: Option<Vec<u32>>,
    pub sorted_values: Vec<i32>,
    pub sorted_entries: i32,
}

impl Codebook {
    pub fn new() -> Codebook {
        Codebook {
            dimensions: 0,
            entries: 0,
            codeword_lengths: Vec::new(),
            minimum_value: 0.0,
            delta_value: 0.0,
            value_bits: 0,
            lookup_type: 0,
            sequence_p: false,
            sparse: false,
            lookup_values: 0,
            multiplicands: Vec::new(),
            codewords: None,
            fast_huffman: Box::new([-1; FAST_HUFFMAN_TABLE_SIZE]),
            sorted_codewords: None,
            sorted_values: Vec::new(),
            sorted_entries: 0,
        }
    }

    fn length(&self, index: usize) -> u8 {
        self.codeword_lengths.get(index).copied().unwrap_or(NO_CODE)
    }

    /// The symbol a sorted position stands for; stb_vorbis keeps a -1 before the table so that
    /// "no symbol" stays "no symbol".
    fn sorted_value(&self, index: i32) -> i32 {
        if index < 0 {
            return -1;
        }
        self.sorted_values.get(index as usize).copied().unwrap_or(-1)
    }
}

pub(crate) fn bit_reverse(mut n: u32) -> u32 {
    n = ((n & 0xAAAA_AAAA) >> 1) | ((n & 0x5555_5555) << 1);
    n = ((n & 0xCCCC_CCCC) >> 2) | ((n & 0x3333_3333) << 2);
    n = ((n & 0xF0F0_F0F0) >> 4) | ((n & 0x0F0F_0F0F) << 4);
    n = ((n & 0xFF00_FF00) >> 8) | ((n & 0x00FF_00FF) << 8);
    (n >> 16) | (n << 16)
}

/// This is a weird definition of log2() for which log2(1) = 1, log2(2) = 2, log2(4) = 3
/// as required by the specification. Fast table lookup in stb_vorbis; the same values here.
pub(crate) fn ilog(n: i32) -> i32 {
    if n < 0 {
        0
    } else {
        32 - (n as u32).leading_zeros() as i32
    }
}

/// The spec's float32_unpack: a float stored as 21 bits of mantissa, 10 of exponent and a sign.
pub(crate) fn float32_unpack(x: u32) -> f32 {
    // from the specification
    let mantissa = x & 0x1f_ffff;
    let sign = x & 0x8000_0000;
    let exp = ((x & 0x7fe0_0000) >> 21) as i32;
    let res = if sign != 0 { -f64::from(mantissa) } else { f64::from(mantissa) };
    // ldexp((float) res, exp - 788): exp - 788 is -788..=235, so the power of two is a
    // normal double and the product exact.
    let scale = f64::from_bits(((exp - 788 + 1023) as u64) << 52);
    (f64::from(res as f32) * scale) as f32
}

/// zlib & jpeg huffman tables assume that the output symbols can either be arbitrarily
/// arranged, or have monotonically increasing frequencies--they rely on the lengths being
/// sorted; this makes for a very simple generation algorithm. vorbis allows a huffman table
/// with non-sorted lengths. This requires a more sophisticated construction, since symbols in
/// order do not map to huffman codes "in order".
fn add_entry(c: &mut Codebook, codewords: &mut [u32], huff_code: u32, symbol: usize, count: usize, len: u8, values: &mut [u32]) {
    if !c.sparse {
        codewords[symbol] = huff_code;
    } else {
        codewords[count] = huff_code;
        c.codeword_lengths[count] = len;
        values[count] = symbol as u32;
    }
}

fn compute_codewords(c: &mut Codebook, codewords: &mut [u32], len: &[u8], values: &mut [u32]) -> bool {
    let n = len.len();
    let mut m = 0usize;
    let mut available = [0u32; 32];
    // find the first entry
    let Some(k) = len.iter().position(|&length| length < NO_CODE) else { return true };
    // add to the list
    add_entry(c, codewords, 0, k, m, len[k], values);
    m += 1;
    // add all available leaves
    for i in 1..=usize::from(len[k]).min(31) {
        available[i] = 1u32 << (32 - i);
    }
    // note that the above code treats the first case specially, but it's really the same as
    // the following code, so they could probably be combined (except the initial code is 0,
    // and would use 0x80000000, which isn't representable (in C))
    for i in k + 1..n {
        let mut z = usize::from(len[i]);
        if z == usize::from(NO_CODE) {
            continue;
        }
        if z >= 32 {
            return false;
        }
        // find lowest available leaf (should always be earliest, which is what the
        // specification calls for)
        // note that this property, and the fact we can never have more than one free leaf at a
        // given level, isn't totally trivial to prove, but it seems true and the assert never
        // fires, so!
        while z > 0 && available[z] == 0 {
            z -= 1;
        }
        if z == 0 {
            return false;
        }
        let res = available[z];
        available[z] = 0;
        add_entry(c, codewords, bit_reverse(res), i, m, len[i], values);
        m += 1;
        // propagate availability up the tree
        if z != usize::from(len[i]) {
            let mut y = usize::from(len[i]);
            while y > z {
                available[y] = res.wrapping_add(1u32 << (32 - y));
                y -= 1;
            }
        }
    }
    true
}

/// accelerated huffman table allows fast O(1) match of all symbols of length <=
/// STB_VORBIS_FAST_HUFFMAN_LENGTH
fn compute_accelerated_huffman(c: &mut Codebook) {
    for slot in c.fast_huffman.iter_mut() {
        *slot = -1;
    }
    let mut len = if c.sparse { c.sorted_entries } else { c.entries };
    if len > 32767 {
        len = 32767; // largest possible value we can encode!
    }
    for i in 0..len as usize {
        let length = u32::from(c.length(i));
        if length <= FAST_HUFFMAN_LENGTH {
            let entry = i as i32 | (length as i32) << FAST_LENGTH_SHIFT;
            let mut z = if c.sparse { bit_reverse(c.sorted_codewords.as_ref().map_or(0, |sorted| sorted[i])) } else { c.codewords.as_ref().map_or(0, |codewords| codewords[i]) };
            // set table entries for all bit combinations in the higher bits
            while (z as usize) < FAST_HUFFMAN_TABLE_SIZE {
                c.fast_huffman[z as usize] = entry;
                z += 1 << length;
            }
        }
    }
}

fn include_in_sort(c: &Codebook, len: u8) -> bool {
    if c.sparse {
        return true;
    }
    if len == NO_CODE {
        return false;
    }
    u32::from(len) > FAST_HUFFMAN_LENGTH
}

/// if the fast table above doesn't work, we want to binary search them... need to reverse the
/// bits
fn compute_sorted_huffman(c: &mut Codebook, codewords: &[u32], lengths: &[u8], values: &[u32]) {
    let sorted_entries = c.sorted_entries as usize;
    let mut sorted = vec![0u32; sorted_entries + 1];
    // build a list of all the entries
    // OPTIMIZATION: don't include the short ones, since they'll be caught by FAST_HUFFMAN.
    // this is kind of a frivolous optimization--I don't see any performance improvement,
    // but it's like 4 extra lines of code, so.
    if !c.sparse {
        let mut k = 0;
        for i in 0..c.entries as usize {
            if include_in_sort(c, lengths[i]) && k < sorted_entries {
                sorted[k] = bit_reverse(codewords[i]);
                k += 1;
            }
        }
    } else {
        for i in 0..sorted_entries {
            sorted[i] = bit_reverse(codewords[i]);
        }
    }
    sorted[..sorted_entries].sort_unstable();
    sorted[sorted_entries] = 0xffff_ffff;

    let len = if c.sparse { sorted_entries } else { c.entries as usize };
    // now we need to indicate how they correspond; we could either
    //   #1: sort a different data structure that says who they correspond to
    //   #2: for each sorted entry, search the original list to find who corresponds
    //   #3: for each original entry, find the sorted entry
    // #1 requires extra storage, #2 is slow, #3 can use binary search!
    for i in 0..len {
        let huff_len = if c.sparse { lengths.get(values[i] as usize).copied().unwrap_or(NO_CODE) } else { lengths[i] };
        if include_in_sort(c, huff_len) {
            let code = bit_reverse(codewords[i]);
            let mut x = 0usize;
            let mut n = sorted_entries;
            while n > 1 {
                // invariant: sc[x] <= code < sc[x+n]
                let m = x + (n >> 1);
                if sorted[m] <= code {
                    x = m;
                    n -= n >> 1;
                } else {
                    n >>= 1;
                }
            }
            if c.sparse {
                c.sorted_values[x] = values[i] as i32;
                c.codeword_lengths[x] = huff_len;
            } else {
                c.sorted_values[x] = i as i32;
            }
        }
    }
    c.sorted_codewords = Some(sorted);
}

/// The number of values in a lattice codebook's value list (the spec's lookup1_values), with
/// the float steps stb_vorbis takes; -1 if it can't be right.
fn lookup1_values(entries: i32, dim: i32) -> i32 {
    let mut r = libm::floor(libm::exp((libm::log(f64::from(entries as f32)) as f32 / dim as f32) as f64)) as i32;
    if libm::floor(libm::pow(f64::from(r as f32 + 1.0), f64::from(dim))) as i32 <= entries {
        // (int) cast for MinGW warning; floor() to avoid _ftol() when non-CRT
        r += 1;
    }
    if libm::pow(f64::from(r as f32 + 1.0), f64::from(dim)) <= f64::from(entries) {
        return -1;
    }
    if libm::floor(libm::pow(f64::from(r as f32), f64::from(dim))) as i32 > entries {
        return -1;
    }
    r
}

/// Reads one codebook from the setup header. `last`: no other codebook follows it.
pub(crate) fn read_codebook(f: &mut Io, last: bool) -> Result<Codebook, VorbisError> {
    let mut c = Codebook::new();
    for sync in [0x42, 0x43, 0x56] {
        if f.get_bits(8) != sync {
            return Err(VorbisError::InvalidSetup);
        }
    }
    let x = f.get_bits(8);
    c.dimensions = ((f.get_bits(8) << 8) + x) as i32;
    let x = f.get_bits(8);
    let y = f.get_bits(8);
    c.entries = ((f.get_bits(8) << 16) + (y << 8) + x) as i32;
    let ordered = f.get_bits(1) != 0;
    c.sparse = if ordered { false } else { f.get_bits(1) != 0 };

    if c.dimensions == 0 && c.entries != 0 {
        return Err(VorbisError::InvalidSetup);
    }
    // (stb_vorbis can't allocate for a codebook with no entries, and says so.)
    if c.entries == 0 {
        return Err(VorbisError::OutOfMemory);
    }
    let entries = c.entries as usize;
    let mut lengths = vec![0u8; entries];
    let mut total = 0i32;

    if ordered {
        let mut current_entry = 0i32;
        let mut current_length = f.get_bits(5) as i32 + 1;
        while current_entry < c.entries {
            let limit = c.entries - current_entry;
            let n = f.get_bits(ilog(limit)) as i32;
            if current_length >= 32 {
                return Err(VorbisError::InvalidSetup);
            }
            if current_entry + n > c.entries {
                return Err(VorbisError::InvalidSetup);
            }
            lengths[current_entry as usize..(current_entry + n) as usize].fill(current_length as u8);
            current_entry += n;
            current_length += 1;
        }
    } else {
        for length in lengths.iter_mut() {
            let present = if c.sparse { f.get_bits(1) != 0 } else { true };
            if present {
                *length = f.get_bits(5) as u8 + 1;
                total += 1;
                if *length == 32 {
                    return Err(VorbisError::InvalidSetup);
                }
            } else {
                *length = NO_CODE;
            }
        }
    }

    if c.sparse && total >= c.entries >> 2 {
        // convert sparse items to non-sparse!
        c.sparse = false;
    }

    // compute the size of the sorted tables
    let sorted_count = if c.sparse { total } else { lengths.iter().filter(|&&length| u32::from(length) > FAST_HUFFMAN_LENGTH && length != NO_CODE).count() as i32 };
    c.sorted_entries = sorted_count;
    let sorted_entries = sorted_count as usize;

    let mut values: Vec<u32> = Vec::new();
    let mut codewords: Vec<u32>;
    if !c.sparse {
        c.codeword_lengths = lengths.clone();
        codewords = vec![0u32; entries];
    } else {
        c.codeword_lengths = vec![0u8; sorted_entries];
        codewords = vec![0u32; sorted_entries];
        values = vec![0u32; sorted_entries];
    }

    if !compute_codewords(&mut c, &mut codewords, &lengths, &mut values) {
        return Err(VorbisError::InvalidSetup);
    }

    if c.sorted_entries != 0 {
        // allocate an extra slot for sentinels
        c.sorted_values = vec![0i32; sorted_entries];
        compute_sorted_huffman(&mut c, &codewords, &lengths, &values);
    }

    if !c.sparse {
        c.codewords = Some(codewords);
    }

    compute_accelerated_huffman(&mut c);

    c.lookup_type = f.get_bits(4) as u8;
    if c.lookup_type > 2 {
        return Err(VorbisError::InvalidSetup);
    }
    if c.lookup_type > 0 {
        c.minimum_value = float32_unpack(f.get_bits(32));
        c.delta_value = float32_unpack(f.get_bits(32));
        c.value_bits = f.get_bits(4) as u8 + 1;
        c.sequence_p = f.get_bits(1) != 0;
        if c.lookup_type == 1 {
            let values = lookup1_values(c.entries, c.dimensions);
            if values < 0 {
                return Err(VorbisError::InvalidSetup);
            }
            c.lookup_values = values as u32;
        } else {
            c.lookup_values = (c.entries as u32).wrapping_mul(c.dimensions as u32);
        }
        if c.lookup_values == 0 {
            return Err(VorbisError::InvalidSetup);
        }
        let mut mults: Vec<u16> = Vec::new();
        for _ in 0..c.lookup_values {
            let q = f.get_bits(i32::from(c.value_bits));
            if f.valid_bits == EOP {
                // The packet has run out. stb_vorbis reads zeros from here on, for as many
                // values as the header claims (it can claim billions), and then for the rest
                // of the header, which can only fail: at the next codebook, which doesn't
                // start as one should, or after the last, at a floor of type 0.
                return Err(if last { VorbisError::FeatureNotSupported } else { VorbisError::InvalidSetup });
            }
            mults.push(q as u16);
        }

        if c.lookup_type == 1 {
            let sparse = c.sparse;
            // pre-expand the lookup1-style multiplicands, to avoid a divide in the inner loop
            if !(sparse && c.sorted_entries == 0) {
                let len = if sparse { sorted_entries } else { entries };
                let dimensions = c.dimensions as usize;
                c.multiplicands = vec![0.0f32; len * dimensions];
                let mut last = 0.0f32;
                for j in 0..len {
                    let z = if sparse { c.sorted_values[j] as u32 } else { j as u32 };
                    let mut div = 1u32;
                    for k in 0..dimensions {
                        let off = ((z / div) % c.lookup_values) as usize;
                        let val = f32::from(mults[off]) * c.delta_value + c.minimum_value + last;
                        c.multiplicands[j * dimensions + k] = val;
                        if c.sequence_p {
                            last = val;
                        }
                        if k + 1 < dimensions {
                            if div > u32::MAX / c.lookup_values {
                                return Err(VorbisError::InvalidSetup);
                            }
                            div *= c.lookup_values;
                        }
                    }
                }
                c.lookup_type = 2;
            }
        } else {
            let mut last = 0.0f32;
            c.multiplicands = Vec::with_capacity(mults.len());
            for &mult in &mults {
                let val = f32::from(mult) * c.delta_value + c.minimum_value + last;
                c.multiplicands.push(val);
                if c.sequence_p {
                    last = val;
                }
            }
        }
    }
    Ok(c)
}

#[inline(never)]
fn codebook_decode_scalar_raw(f: &mut Io, c: &Codebook) -> i32 {
    f.prep_huffman();

    if c.codewords.is_none() && c.sorted_codewords.is_none() {
        return -1;
    }

    // cases to use binary search: sorted_codewords && !c->codewords
    //                             sorted_codewords && c->entries > 8
    let binary = if c.entries > 8 { c.sorted_codewords.is_some() } else { c.codewords.is_none() };
    if binary {
        let Some(sorted) = c.sorted_codewords.as_ref() else { return -1 };
        // binary search
        let code = bit_reverse(f.acc);
        let mut x = 0usize;
        let mut n = c.sorted_entries as usize;
        while n > 1 {
            // invariant: sc[x] <= code < sc[x+n]
            let m = x + (n >> 1);
            if sorted[m] <= code {
                x = m;
                n -= n >> 1;
            } else {
                n >>= 1;
            }
        }
        // x is now the sorted index
        let x = if !c.sparse { c.sorted_value(x as i32) } else { x as i32 };
        // x is now the symbol index
        let len = if x < 0 { i32::from(NO_CODE) } else { i32::from(c.length(x as usize)) };
        if f.valid_bits >= len {
            f.acc = f.acc.checked_shr(len as u32).unwrap_or(0);
            f.valid_bits -= len;
            return x;
        }
        f.valid_bits = 0;
        return -1;
    }

    // if small, linear search
    let Some(codewords) = c.codewords.as_ref() else { return -1 };
    for i in 0..c.entries as usize {
        let length = c.length(i);
        if length == NO_CODE {
            continue;
        }
        let mask = 1u32.checked_shl(u32::from(length)).map_or(!0, |bit| bit - 1);
        if codewords[i] == (f.acc & mask) {
            if f.valid_bits >= i32::from(length) {
                f.acc = f.acc.checked_shr(u32::from(length)).unwrap_or(0);
                f.valid_bits -= i32::from(length);
                return i as i32;
            }
            f.valid_bits = 0;
            return -1;
        }
    }

    f.error(VorbisError::InvalidStream);
    f.valid_bits = 0;
    -1
}

/// stb_vorbis's DECODE_RAW: the next codeword's position in the codebook's tables (the symbol
/// itself for a codebook that isn't sparse), or -1.
#[inline(always)]
pub(crate) fn decode_raw(f: &mut Io, c: &Codebook) -> i32 {
    if f.valid_bits < FAST_HUFFMAN_LENGTH as i32 {
        f.prep_huffman();
    }
    // fast huffman table lookup
    let mut var = c.fast_huffman[(f.acc & FAST_HUFFMAN_TABLE_MASK) as usize];
    if var >= 0 {
        // (A length here is 10 at most.)
        let n = var >> FAST_LENGTH_SHIFT;
        var &= (1 << FAST_LENGTH_SHIFT) - 1;
        f.acc >>= n as u32 & 31;
        f.valid_bits -= n;
        if f.valid_bits < 0 {
            f.valid_bits = 0;
            var = -1;
        }
    } else {
        var = codebook_decode_scalar_raw(f, c);
    }
    var
}

/// stb_vorbis's DECODE: the next symbol, or -1.
#[inline]
pub(crate) fn decode(f: &mut Io, c: &Codebook) -> i32 {
    let var = decode_raw(f, c);
    if c.sparse {
        c.sorted_value(var)
    } else {
        var
    }
}

// CODEBOOK_ELEMENT_FAST is an optimization for the CODEBOOK_FLOATS case
// where we avoid one addition
fn element(c: &Codebook, off: i32) -> f32 {
    if off < 0 {
        return 0.0;
    }
    c.multiplicands.get(off as usize).copied().unwrap_or(0.0)
}

fn add(output: &mut [f32], index: i64, value: f32) {
    if index >= 0 {
        if let Some(slot) = output.get_mut(index as usize) {
            *slot += value;
        }
    }
}

fn codebook_decode_start(f: &mut Io, c: &Codebook) -> i32 {
    let mut z = -1;
    // type 0 is only legal in a scalar context
    if c.lookup_type == 0 {
        f.error(VorbisError::InvalidStream);
    } else {
        z = decode_raw(f, c);
        if z < 0 {
            // check for EOP
            if f.bytes_in_seg == 0 && f.last_seg {
                return z;
            }
            f.error(VorbisError::InvalidStream);
        }
    }
    z
}

/// The values of a codebook from `from` on, `count` of them, when they are all there.
#[inline(always)]
fn elements(c: &Codebook, from: i32, count: i32) -> Option<&[f32]> {
    let from = usize::try_from(from).ok()?;
    let count = usize::try_from(count).ok()?;
    c.multiplicands.get(from..from.checked_add(count)?)
}

/// Two of the channel buffers at once.
#[inline(always)]
fn two_buffers(buffers: &mut [Vec<f32>], first: usize, second: usize) -> Option<(&mut [f32], &mut [f32])> {
    if first < second {
        let (low, high) = buffers.split_at_mut(second);
        Some((low.get_mut(first)?, high.first_mut()?))
    } else if second < first {
        let (low, high) = buffers.split_at_mut(first);
        let (second, first) = (low.get_mut(second)?, high.first_mut()?);
        Some((first, second))
    } else {
        None
    }
}

fn codebook_decode(f: &mut Io, c: &Codebook, output: &mut [f32], offset: i64, len: i32) -> bool {
    let z = codebook_decode_start(f, c);
    if z < 0 {
        return false;
    }
    let len = len.min(c.dimensions);
    let z = z.wrapping_mul(c.dimensions);
    if !c.sequence_p {
        // The usual case, when every value and every place for it is there: the loop below
        // without its looking each of them up.
        let places = usize::try_from(offset).ok().and_then(|offset| output.get_mut(offset..)).unwrap_or_default();
        if let Some(values) = elements(c, z, len).filter(|values| values.len() <= places.len()) {
            let last = 0.0f32;
            for (place, &value) in places.iter_mut().zip(values) {
                *place += value + last;
            }
            return true;
        }
    }
    if c.sequence_p {
        let mut last = 0.0f32;
        for i in 0..len {
            let val = element(c, z.wrapping_add(i)) + last;
            add(output, offset + i64::from(i), val);
            last = val + c.minimum_value;
        }
    } else {
        let last = 0.0f32;
        for i in 0..len {
            add(output, offset + i64::from(i), element(c, z.wrapping_add(i)) + last);
        }
    }
    true
}

fn codebook_decode_step(f: &mut Io, c: &Codebook, output: &mut [f32], offset: i64, len: i32, step: i32) -> bool {
    let z = codebook_decode_start(f, c);
    let mut last = 0.0f32;
    if z < 0 {
        return false;
    }
    let len = len.min(c.dimensions);
    let z = z.wrapping_mul(c.dimensions);
    for i in 0..len {
        let val = element(c, z.wrapping_add(i)) + last;
        add(output, offset + i64::from(i) * i64::from(step), val);
        if c.sequence_p {
            last = val;
        }
    }
    true
}

/// Decodes into several channels at once, a value to each in turn (residue type 2).
/// `outputs[c]` is the channel buffer a channel of the residue writes to, if it is decoded.
#[allow(clippy::too_many_arguments)]
pub(crate) fn codebook_decode_deinterleave_repeat(f: &mut Io, c: &Codebook, buffers: &mut [Vec<f32>], outputs: &[Option<usize>], ch: i32, c_inter_p: &mut i32, p_inter_p: &mut i32, len: i32, mut total_decode: i32) -> bool {
    let mut c_inter = *c_inter_p;
    let mut p_inter = *p_inter_p;
    let mut effective = c.dimensions;

    // type 0 is only legal in a scalar context
    if c.lookup_type == 0 {
        return f.error(VorbisError::InvalidStream);
    }

    // Most sound there is: two channels, both of them wanted.
    if ch == 2 && !c.sequence_p && (c_inter == 0 || c_inter == 1) {
        if let [Some(first), Some(second), ..] = outputs {
            if let Some((first, second)) = two_buffers(buffers, *first, *second) {
                return deinterleave_two(f, c, first, second, c_inter_p, p_inter_p, len, total_decode);
            }
        }
    }

    while total_decode > 0 {
        let mut last = 0.0f32;
        let z = decode_raw(f, c);
        if z < 0 {
            if f.bytes_in_seg == 0 && f.last_seg {
                return false;
            }
            return f.error(VorbisError::InvalidStream);
        }

        // if this will take us off the end of the buffers, stop short!
        // we check by computing the length of the virtual interleaved
        // buffer (len*ch), our current offset within it (p_inter*ch)+(c_inter),
        // and the length we'll be using (effective)
        if c_inter + p_inter * ch + effective > len * ch {
            effective = len * ch - (p_inter * ch - c_inter);
        }

        let z = z.wrapping_mul(c.dimensions);
        for i in 0..effective {
            let val = element(c, z.wrapping_add(i)) + last;
            if let Some(Some(channel)) = outputs.get(c_inter as usize) {
                add(&mut buffers[*channel], i64::from(p_inter), val);
            }
            c_inter += 1;
            if c_inter == ch {
                c_inter = 0;
                p_inter += 1;
            }
            if c.sequence_p {
                last = val;
            }
        }

        // (If nothing was used, this goes on to the end of the packet, as in stb_vorbis.)
        total_decode -= effective;
    }
    *c_inter_p = c_inter;
    *p_inter_p = p_inter;
    true
}

/// `PAIRS` pairs of values (`VALUES` of them) added to as many places of two channels.
#[inline(always)]
fn add_pairs<const PAIRS: usize, const VALUES: usize>(first: &mut [f32], second: &mut [f32], values: &[f32], last: f32) {
    let (Ok(first), Ok(second), Ok(values)) = (<&mut [f32; PAIRS]>::try_from(first), <&mut [f32; PAIRS]>::try_from(second), <&[f32; VALUES]>::try_from(values)) else { return };
    for i in 0..PAIRS {
        first[i] += values[i * 2] + last;
        second[i] += values[i * 2 + 1] + last;
    }
}

/// Any number of pairs of values added to as many places of two channels.
#[inline(always)]
fn add_any_pairs(first: &mut [f32], second: &mut [f32], values: &[f32], last: f32) {
    for ((first, second), pair) in first.iter_mut().zip(second).zip(values.chunks_exact(2)) {
        *first += pair[0] + last;
        *second += pair[1] + last;
    }
}

/// A codeword of a codebook that doesn't have all its values, from the start of a pair on:
/// what there is of them is added, and nothing for the rest.
#[cold]
fn add_values_missing(c: &Codebook, z: i32, first: &mut [f32], second: &mut [f32], at: usize, last: f32) {
    for i in 0..c.dimensions {
        let val = element(c, z.wrapping_add(i)) + last;
        add(if i & 1 == 0 { &mut *first } else { &mut *second }, (at + i as usize / 2) as i64, val);
    }
}

/// The codewords of [`deinterleave_two`] that go whole into the two channels, each value of
/// the first channel followed by one of the second: from sample `at` on, for as long as there
/// is room before sample `limit`. `Err` is what `deinterleave_two` is to give at once.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn whole_codewords(f: &mut Io, c: &Codebook, first: &mut [f32], second: &mut [f32], at: &mut usize, total_decode: &mut i32, limit: usize, last: f32, add_values: impl Fn(&mut [f32], &mut [f32], &[f32], f32)) -> Result<(), bool> {
    let pairs = c.dimensions as usize / 2;
    while *total_decode > 0 && *at + pairs <= limit {
        let z = decode_raw(f, c);
        if z < 0 {
            if f.bytes_in_seg == 0 && f.last_seg {
                return Err(false);
            }
            return Err(f.error(VorbisError::InvalidStream));
        }
        let z = z.wrapping_mul(c.dimensions);
        *total_decode -= c.dimensions;
        match elements(c, z, c.dimensions) {
            Some(values) => add_values(&mut first[*at..*at + pairs], &mut second[*at..*at + pairs], values, last),
            None => add_values_missing(c, z, first, second, *at, last),
        }
        *at += pairs;
    }
    Ok(())
}

/// [`codebook_decode_deinterleave_repeat`] for two channels, both of them wanted, and a
/// codebook whose values don't build on each other: the same additions to the same places,
/// with what doesn't change from one codeword to the next found once.
#[allow(clippy::too_many_arguments)]
fn deinterleave_two(f: &mut Io, c: &Codebook, first: &mut [f32], second: &mut [f32], c_inter_p: &mut i32, p_inter_p: &mut i32, len: i32, mut total_decode: i32) -> bool {
    let mut c_inter = *c_inter_p;
    let mut p_inter = *p_inter_p;
    let mut effective = c.dimensions;
    // (Added to each value as stb_vorbis adds it: it makes a -0.0 a 0.0.)
    let last = 0.0f32;

    // Nearly all of it: from the start of a pair, codewords of a whole number of pairs, each
    // of which fits before `len` (so that stb_vorbis's "stop short" below has nothing to do).
    if c_inter == 0 && c.dimensions > 0 && c.dimensions & 1 == 0 && p_inter >= 0 && len >= 0 {
        let limit = (len as usize).min(first.len()).min(second.len());
        let mut at = p_inter as usize;
        let done = match c.dimensions {
            2 => whole_codewords(f, c, first, second, &mut at, &mut total_decode, limit, last, add_pairs::<1, 2>),
            4 => whole_codewords(f, c, first, second, &mut at, &mut total_decode, limit, last, add_pairs::<2, 4>),
            8 => whole_codewords(f, c, first, second, &mut at, &mut total_decode, limit, last, add_pairs::<4, 8>),
            _ => whole_codewords(f, c, first, second, &mut at, &mut total_decode, limit, last, add_any_pairs),
        };
        if let Err(result) = done {
            return result;
        }
        p_inter = at as i32;
    }

    // The rest, a value at a time.
    while total_decode > 0 {
        let z = decode_raw(f, c);
        if z < 0 {
            if f.bytes_in_seg == 0 && f.last_seg {
                return false;
            }
            return f.error(VorbisError::InvalidStream);
        }

        if c_inter + p_inter * 2 + effective > len * 2 {
            effective = len * 2 - (p_inter * 2 - c_inter);
        }

        let z = z.wrapping_mul(c.dimensions);
        for i in 0..effective {
            let val = element(c, z.wrapping_add(i)) + last;
            if c_inter == 0 {
                add(first, i64::from(p_inter), val);
                c_inter = 1;
            } else {
                add(second, i64::from(p_inter), val);
                c_inter = 0;
                p_inter += 1;
            }
        }
        total_decode -= effective;
    }
    *c_inter_p = c_inter;
    *p_inter_p = p_inter;
    true
}

/// `COUNT` values added to as many places of a channel.
#[inline(always)]
fn add_run<const COUNT: usize>(places: &mut [f32], values: &[f32], last: f32) {
    let (Ok(places), Ok(values)) = (<&mut [f32; COUNT]>::try_from(places), <&[f32; COUNT]>::try_from(values)) else { return };
    for i in 0..COUNT {
        places[i] += values[i] + last;
    }
}

/// Any number of values added to as many places of a channel.
#[inline(always)]
fn add_any_run(places: &mut [f32], values: &[f32], last: f32) {
    for (place, &value) in places.iter_mut().zip(values) {
        *place += value + last;
    }
}

/// A codeword of a codebook that doesn't have all its values: what there is of them is added,
/// and nothing for the rest.
#[cold]
fn add_run_missing(c: &Codebook, z: i32, target: &mut [f32], at: usize, last: f32) {
    for i in 0..c.dimensions {
        add(target, (at + i as usize) as i64, element(c, z.wrapping_add(i)) + last);
    }
}

/// The codewords of a residue of type 1 that go whole into the channel: from `offset` on,
/// while there are `n - k` values still to come that make a whole codeword, and room for it.
/// False is a packet that ended or a broken one, as [`codebook_decode`] gives it.
#[inline(always)]
fn whole_codewords_one(f: &mut Io, c: &Codebook, target: &mut [f32], offset: &mut i32, k: &mut i32, n: i32, add_values: impl Fn(&mut [f32], &[f32], f32)) -> bool {
    let size = c.dimensions as usize;
    // (Added to each value as stb_vorbis adds it: it makes a -0.0 a 0.0.)
    let last = 0.0f32;
    while n - *k >= c.dimensions && *offset as usize + size <= target.len() {
        let z = decode_raw(f, c);
        if z < 0 {
            // check for EOP
            if !(f.bytes_in_seg == 0 && f.last_seg) {
                f.error(VorbisError::InvalidStream);
            }
            return false;
        }
        let z = z.wrapping_mul(c.dimensions);
        let at = *offset as usize;
        match elements(c, z, c.dimensions) {
            Some(values) => add_values(&mut target[at..at + size], values, last),
            None => add_run_missing(c, z, target, at, last),
        }
        *k += c.dimensions;
        *offset += c.dimensions;
    }
    true
}

/// Residue types 0 and 1: `n` values of `target` from `offset` on.
pub(crate) fn residue_decode(f: &mut Io, book: &Codebook, target: &mut [f32], mut offset: i32, n: i32, rtype: u16) -> bool {
    if rtype == 0 {
        let step = n / book.dimensions.max(1);
        for k in 0..step {
            if !codebook_decode_step(f, book, target, i64::from(offset) + i64::from(k), n - offset - k, step) {
                return false;
            }
        }
    } else {
        let mut k = 0;
        // Nearly all of it: codewords that go whole into the channel. (What is left, if
        // anything, the loop below does as stb_vorbis does all of it.)
        if book.lookup_type != 0 && !book.sequence_p && book.dimensions > 0 && offset >= 0 {
            let whole = match book.dimensions {
                1 => whole_codewords_one(f, book, target, &mut offset, &mut k, n, add_run::<1>),
                2 => whole_codewords_one(f, book, target, &mut offset, &mut k, n, add_run::<2>),
                4 => whole_codewords_one(f, book, target, &mut offset, &mut k, n, add_run::<4>),
                8 => whole_codewords_one(f, book, target, &mut offset, &mut k, n, add_run::<8>),
                _ => whole_codewords_one(f, book, target, &mut offset, &mut k, n, add_any_run),
            };
            if !whole {
                return false;
            }
        }
        while k < n {
            if !codebook_decode(f, book, target, i64::from(offset), n - k) {
                return false;
            }
            k += book.dimensions.max(1);
            offset += book.dimensions;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_specifications_log() {
        let expected = [(0, 0), (1, 1), (2, 2), (3, 2), (4, 3), (7, 3), (8, 4), (255, 8), (256, 9), (1 << 14, 15), (1 << 24, 25), (i32::MAX, 31), (-1, 0)];
        for (n, log) in expected {
            assert_eq!(ilog(n), log, "{n}");
        }
    }

    #[test]
    fn packed_floats_and_reversed_bits() {
        assert_eq!(bit_reverse(1), 0x8000_0000);
        assert_eq!(bit_reverse(0x0000_00F0), 0x0F00_0000);
        // mantissa 1, exponent 788: 1.0; the sign bit; exponent 789 with mantissa 3: 6.0
        assert_eq!(float32_unpack(788 << 21 | 1), 1.0);
        assert_eq!(float32_unpack(0x8000_0000 | 788 << 21 | 1), -1.0);
        assert_eq!(float32_unpack(789 << 21 | 3), 6.0);
        assert_eq!(float32_unpack(768 << 21 | 0x10_0000), 1.0);
    }

    #[test]
    fn lattice_sizes() {
        assert_eq!(lookup1_values(81, 4), 3); // 3^4
        assert_eq!(lookup1_values(80, 4), 2);
        assert_eq!(lookup1_values(625, 4), 5);
        assert_eq!(lookup1_values(1, 1), 1);
    }
}
