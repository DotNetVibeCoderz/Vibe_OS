//! Decoding an audio packet: the floors, the residues, channel coupling, the inverse MDCT and
//! the overlap with the frame before (stb_vorbis's vorbis_decode_packet and what it calls).
#[allow(unused_imports)]
use alloc::{boxed::Box, vec, vec::Vec};

use crate::codebook::{codebook_decode_deinterleave_repeat, decode, ilog, residue_decode, Codebook};
use crate::io::{Io, EOP};
use crate::mdct::inverse_mdct;
use crate::setup::{Floor1, Residue};
use crate::tables::INVERSE_DB_TABLE;
use crate::{Vorbis, VorbisError};

fn predict_point(x: i32, x0: i32, x1: i32, y0: i32, y1: i32) -> i32 {
    let dy = y1 - y0;
    let adx = x1 - x0;
    // @OPTIMIZE: force int division to round in the right direction... is this necessary on x86?
    let err = dy.abs().wrapping_mul(x - x0);
    let off = if adx != 0 { err / adx } else { 0 };
    if dy < 0 {
        y0 - off
    } else {
        y0 + off
    }
}

#[inline]
fn draw_line(output: &mut [f32], x0: i32, y0: i32, mut x1: i32, y1: i32, n: i32) {
    let dy = y1 - y0;
    let adx = x1 - x0;
    let mut ady = dy.abs();
    let x = x0;
    let mut y = y0;
    let mut err = 0;
    let base = dy / adx;
    let sy = if dy < 0 { base - 1 } else { base + 1 };
    ady -= base.abs() * adx;
    if x1 > n {
        x1 = n;
    }
    if x < x1 {
        let Some((first, rest)) = output[x as usize..x1 as usize].split_first_mut() else { return };
        *first *= INVERSE_DB_TABLE[(y & 255) as usize];
        for value in rest {
            err += ady;
            // (Written as two choices and not as a branch, which would be a guess each time.)
            let over = err >= adx;
            err -= if over { adx } else { 0 };
            y += if over { sy } else { base };
            *value *= INVERSE_DB_TABLE[(y & 255) as usize];
        }
    }
}

/// The floor curve of a channel, multiplied into its spectrum.
fn do_floor(g: &Floor1, n: usize, target: &mut [f32], final_y: &[i16]) {
    let n2 = (n >> 1) as i32;
    let multiplier = i32::from(g.floor1_multiplier);
    let mut lx = 0i32;
    let mut ly = i32::from(final_y[0]) * multiplier;
    for q in 1..g.values as usize {
        let j = usize::from(g.sorted_order[q]);
        if final_y[j] >= 0 {
            let hy = i32::from(final_y[j]) * multiplier;
            let hx = i32::from(g.xlist[j]);
            if lx != hx {
                draw_line(target, lx, ly, hx, hy, n2);
            }
            lx = hx;
            ly = hy;
        }
    }
    if lx < n2 {
        // optimization of: draw_line(target, lx,ly, n,ly, n2);
        let level = INVERSE_DB_TABLE[(ly & 255) as usize];
        for value in &mut target[lx as usize..n2 as usize] {
            *value *= level;
        }
    }
}

/// n is 1/2 of the blocksize -- specification: "Correct per-vector decode length is [n]/2"
///
/// `residue_buffers[c]` is the channel buffer the residue's channel `c` is decoded into, or
/// nothing if that channel is not decoded.
#[allow(clippy::too_many_arguments)]
fn decode_residue(f: &mut Io, codebooks: &[Codebook], r: &Residue, rtype: u16, buffers: &mut [Vec<f32>], residue_buffers: &[Option<usize>], n: usize, do_not_decode: &[bool], channels: usize, part_classdata: &mut Vec<usize>) {
    let ch = residue_buffers.len() as i32;
    let classbook = &codebooks[usize::from(r.classbook)];
    let classwords = classbook.dimensions;
    let actual_size = if rtype == 2 { n as u32 * 2 } else { n as u32 };
    let limit_r_begin = r.begin.min(actual_size);
    let limit_r_end = r.end.min(actual_size);
    let n_read = (limit_r_end - limit_r_begin) as i32;
    let part_read = n_read / r.part_size as i32;
    let part_size = r.part_size as i32;
    // The classification of each part, as a row of the residue's classdata: a row of these for
    // each channel, one after another.
    let row_length = part_read.max(0) as usize;
    part_classdata.clear();
    part_classdata.resize(row_length * channels.max(1), 0);

    for i in 0..ch as usize {
        if !do_not_decode[i] {
            if let Some(channel) = residue_buffers[i] {
                buffers[channel][..n].fill(0.0);
            }
        }
    }

    if rtype == 2 && ch != 1 {
        if do_not_decode[..ch as usize].iter().all(|&skip| skip) {
            return;
        }
        for pass in 0..r.passes {
            let mut pcount = 0i32;
            let mut class_set = 0usize;
            // (stb_vorbis has this loop twice, for ch == 2 and ch > 2: they differ in how a
            // position is split into a channel and a sample.)
            let split = |z: i32| if ch == 2 { (z & 1, z >> 1) } else { (z % ch, z / ch) };
            if ch >= 2 {
                while pcount < part_read {
                    let z = (r.begin as i32).wrapping_add(pcount.wrapping_mul(part_size));
                    let (mut c_inter, mut p_inter) = split(z);
                    if pass == 0 {
                        let q = decode(f, classbook);
                        if q == EOP {
                            return;
                        }
                        part_classdata[class_set] = q as usize;
                    }
                    let mut i = 0;
                    while i < classwords && pcount < part_read {
                        let mut z = (r.begin as i32).wrapping_add(pcount.wrapping_mul(part_size));
                        let Some(row) = r.classdata.get(part_classdata[class_set]) else { return };
                        let c = usize::from(row[i as usize]);
                        let b = r.residue_books[c][pass];
                        if b >= 0 {
                            let book = &codebooks[b as usize];
                            // saves 1%
                            if !codebook_decode_deinterleave_repeat(f, book, buffers, residue_buffers, ch, &mut c_inter, &mut p_inter, n as i32, part_size) {
                                return;
                            }
                        } else {
                            z = z.wrapping_add(part_size);
                            (c_inter, p_inter) = split(z);
                        }
                        i += 1;
                        pcount += 1;
                    }
                    class_set += 1;
                }
            }
        }
        return;
    }

    for pass in 0..r.passes {
        let mut pcount = 0i32;
        let mut class_set = 0usize;
        while pcount < part_read {
            if pass == 0 {
                for j in 0..ch as usize {
                    if !do_not_decode[j] {
                        let temp = decode(f, classbook);
                        if temp == EOP {
                            return;
                        }
                        part_classdata[j * row_length + class_set] = temp as usize;
                    }
                }
            }
            let mut i = 0;
            while i < classwords && pcount < part_read {
                for j in 0..ch as usize {
                    if !do_not_decode[j] {
                        let Some(row) = r.classdata.get(part_classdata[j * row_length + class_set]) else { return };
                        let c = usize::from(row[i as usize]);
                        let b = r.residue_books[c][pass];
                        if b >= 0 {
                            let Some(channel) = residue_buffers[j] else { continue };
                            let offset = (r.begin as i32).wrapping_add(pcount.wrapping_mul(part_size));
                            let book = &codebooks[b as usize];
                            if !residue_decode(f, book, &mut buffers[channel], offset, part_size, rtype) {
                                return;
                            }
                        }
                    }
                }
                i += 1;
                pcount += 1;
            }
            class_set += 1;
        }
    }
}

/// Two different channels' buffers at once.
fn two_channels(buffers: &mut [Vec<f32>], a: usize, b: usize) -> (&mut [f32], &mut [f32]) {
    if a < b {
        let (low, high) = buffers.split_at_mut(b);
        (&mut low[a][..], &mut high[0][..])
    } else {
        let (low, high) = buffers.split_at_mut(a);
        (&mut high[0][..], &mut low[b][..])
    }
}

/// A packet's window: where the overlap with the previous frame starts and ends, and the same
/// for the next, with the packet's mode.
pub(crate) struct PacketStart {
    pub left_start: i32,
    // (stb_vorbis works this out too, and nothing of its reads it either.)
    #[allow(dead_code)]
    pub left_end: i32,
    pub right_start: i32,
    pub right_end: i32,
    pub mode: usize,
}

impl Vorbis<'_> {
    /// The start of the next audio packet: its mode and window.
    pub(crate) fn vorbis_decode_initial(&mut self) -> Option<PacketStart> {
        self.channel_buffer_start = 0;
        self.channel_buffer_end = 0;
        let f = &mut self.io;

        loop {
            if f.eof {
                return None;
            }
            if !f.maybe_start_packet() {
                return None;
            }
            // check packet type
            if f.get_bits(1) != 0 {
                if f.push_mode {
                    f.error(VorbisError::BadPacketType);
                    return None;
                }
                // (A header packet in the audio: skipped.)
                while f.get8_packet() != EOP {}
                continue;
            }
            break;
        }

        let mode_count = self.setup.modes.len() as i32;
        let i = f.get_bits(ilog(mode_count - 1)) as i32;
        if i == EOP {
            return None;
        }
        if i >= mode_count {
            return None;
        }
        let mode = i as usize;
        let m = self.setup.modes[mode];
        let (blocksize_0, blocksize_1) = (self.setup.blocksize[0] as i32, self.setup.blocksize[1] as i32);
        let (n, prev, next);
        if m.blockflag != 0 {
            n = blocksize_1;
            prev = f.get_bits(1);
            next = f.get_bits(1);
        } else {
            prev = 0;
            next = 0;
            n = blocksize_0;
        }

        // WINDOWING

        let window_center = n >> 1;
        let (left_start, left_end) = if m.blockflag != 0 && prev == 0 { ((n - blocksize_0) >> 2, (n + blocksize_0) >> 2) } else { (0, window_center) };
        let (right_start, right_end) = if m.blockflag != 0 && next == 0 { ((n * 3 - blocksize_0) >> 2, (n * 3 + blocksize_0) >> 2) } else { (window_center, n) };
        Some(PacketStart { left_start, left_end, right_start, right_end, mode })
    }

    /// The rest of a packet. Gives the length of the data in the channel buffers and where its
    /// left edge is, or nothing if the stream is broken.
    pub(crate) fn vorbis_decode_packet_rest(&mut self, start: &PacketStart) -> Option<(i32, i32)> {
        let Vorbis { io: f, setup, channel_buffers, final_y, mdct_scratch, residue_scratch, .. } = self;
        let m = setup.modes[start.mode];
        let n = setup.blocksize[usize::from(m.blockflag)];
        let map = &setup.mappings[usize::from(m.mapping)];
        let channels = setup.channels;
        let mut left_start = start.left_start;
        let (right_start, right_end) = (start.right_start, start.right_end);

        // FLOORS
        let n2 = n >> 1;
        let mut zero_channel = [false; crate::MAX_CHANNELS];

        for i in 0..channels {
            let s = usize::from(map.chan[i].mux);
            let g = &setup.floors[usize::from(map.submap_floor[s])];
            if f.get_bits(1) != 0 {
                let mut step2_flag = [false; 256];
                const RANGE_LIST: [i32; 4] = [256, 128, 86, 64];
                let range = RANGE_LIST[usize::from(g.floor1_multiplier - 1)];
                let mut offset = 2usize;
                let final_y = &mut final_y[i];
                final_y[0] = f.get_bits(ilog(range) - 1) as i16;
                final_y[1] = f.get_bits(ilog(range) - 1) as i16;
                for j in 0..usize::from(g.partitions) {
                    let pclass = usize::from(g.partition_class_list[j]);
                    let cdim = usize::from(g.class_dimensions[pclass]);
                    let cbits = i32::from(g.class_subclasses[pclass]);
                    let csub = (1 << cbits) - 1;
                    let mut cval = 0i32;
                    if cbits != 0 {
                        let c = &setup.codebooks[usize::from(g.class_masterbooks[pclass])];
                        cval = decode(f, c);
                    }
                    for _ in 0..cdim {
                        let book = g.subclass_books[pclass][(cval & csub) as usize];
                        cval >>= cbits;
                        if book >= 0 {
                            let c = &setup.codebooks[book as usize];
                            final_y[offset] = decode(f, c) as i16;
                        } else {
                            final_y[offset] = 0;
                        }
                        offset += 1;
                    }
                }
                if f.valid_bits == EOP {
                    // behavior according to spec
                    zero_channel[i] = true;
                    continue;
                }
                step2_flag[0] = true;
                step2_flag[1] = true;
                for j in 2..g.values as usize {
                    let low = usize::from(g.neighbors[j][0]);
                    let high = usize::from(g.neighbors[j][1]);
                    let pred = predict_point(i32::from(g.xlist[j]), i32::from(g.xlist[low]), i32::from(g.xlist[high]), i32::from(final_y[low]), i32::from(final_y[high]));
                    let val = i32::from(final_y[j]);
                    let highroom = range - pred;
                    let lowroom = pred;
                    let room = if highroom < lowroom { highroom * 2 } else { lowroom * 2 };
                    if val != 0 {
                        step2_flag[low] = true;
                        step2_flag[high] = true;
                        step2_flag[j] = true;
                        let y = if val >= room {
                            if highroom > lowroom {
                                val - lowroom + pred
                            } else {
                                pred - val + highroom - 1
                            }
                        } else if val & 1 != 0 {
                            pred - ((val + 1) >> 1)
                        } else {
                            pred + (val >> 1)
                        };
                        final_y[j] = y as i16;
                    } else {
                        step2_flag[j] = false;
                        final_y[j] = pred as i16;
                    }
                }

                // defer final floor computation until _after_ residue
                for j in 0..g.values as usize {
                    if !step2_flag[j] {
                        final_y[j] = -1;
                    }
                }
            } else {
                zero_channel[i] = true;
            }
            // So we just defer everything else to later

            // at this point we've decoded the floor into buffer
        }

        // re-enable coupled channels if necessary
        let really_zero_channel = zero_channel;
        for i in 0..usize::from(map.coupling_steps) {
            let (magnitude, angle) = (usize::from(map.chan[i].magnitude), usize::from(map.chan[i].angle));
            if !zero_channel[magnitude] || !zero_channel[angle] {
                zero_channel[magnitude] = false;
                zero_channel[angle] = false;
            }
        }

        // RESIDUE DECODE
        for i in 0..map.submaps {
            let mut residue_buffers = [None; crate::MAX_CHANNELS];
            let mut do_not_decode = [false; crate::MAX_CHANNELS];
            let mut ch = 0usize;
            for j in 0..channels {
                if map.chan[j].mux == i {
                    if zero_channel[j] {
                        do_not_decode[ch] = true;
                        residue_buffers[ch] = None;
                    } else {
                        do_not_decode[ch] = false;
                        residue_buffers[ch] = Some(j);
                    }
                    ch += 1;
                }
            }
            let r = usize::from(map.submap_residue[usize::from(i)]);
            decode_residue(f, &setup.codebooks, &setup.residues[r], setup.residue_types[r], channel_buffers, &residue_buffers[..ch], n2, &do_not_decode, channels, residue_scratch);
        }

        // INVERSE COUPLING
        for i in (0..usize::from(map.coupling_steps)).rev() {
            let (m, a) = two_channels(channel_buffers, usize::from(map.chan[i].magnitude), usize::from(map.chan[i].angle));
            // stb_vorbis's four cases, as two choices: which of the difference and the sum is
            // the new value, and which of the two channels it goes to. The other channel gets
            // the magnitude. (Choices and not branches, so that the samples can be done several
            // at a time.)
            //
            //     m > 0, a > 0:   m2 = m      a2 = m - a
            //     m > 0, a <= 0:  m2 = m + a  a2 = m
            //     m <= 0, a > 0:  m2 = m      a2 = m + a
            //     m <= 0, a <= 0: m2 = m - a  a2 = m
            for (m, a) in m[..n2].iter_mut().zip(&mut a[..n2]) {
                let (magnitude, angle) = (*m, *a);
                let other = if (magnitude > 0.0) == (angle > 0.0) { magnitude - angle } else { magnitude + angle };
                let to_angle = angle > 0.0;
                *m = if to_angle { magnitude } else { other };
                *a = if to_angle { other } else { magnitude };
            }
        }

        // finish decoding the floors
        for i in 0..channels {
            if really_zero_channel[i] {
                channel_buffers[i][..n2].fill(0.0);
            } else {
                let s = usize::from(map.chan[i].mux);
                let g = &setup.floors[usize::from(map.submap_floor[s])];
                do_floor(g, n, &mut channel_buffers[i], &final_y[i]);
            }
        }

        // INVERSE MDCT
        for buffer in channel_buffers.iter_mut().take(channels) {
            inverse_mdct(buffer, n, &setup.tables[usize::from(m.blockflag)], mdct_scratch);
        }

        // this shouldn't be necessary, unless we exited on an error
        // and want to flush to get to the next packet
        f.flush_packet();

        let n = n as i32;
        let n2 = n2 as i32;
        if f.first_decode {
            // assume we start so first non-discarded sample is sample 0
            // this isn't to spec, but spec would require us to read ahead
            // and decode the size of all current frames--could be done,
            // but presumably it's not a commonly used feature
            self.current_loc = 0u32.wrapping_sub(n2 as u32); // start of first frame is positioned for discard (NB this is an intentional unsigned overflow/wrap-around)
            // we might have to discard samples "from" the next frame too,
            // if we're lapping a large block then a small at the start?
            self.discard_samples_deferred = n - right_end;
            self.current_loc_valid = true;
            f.first_decode = false;
        } else if self.discard_samples_deferred != 0 {
            if self.discard_samples_deferred >= right_start - left_start {
                self.discard_samples_deferred -= right_start - left_start;
                left_start = right_start;
            } else {
                left_start += self.discard_samples_deferred;
                self.discard_samples_deferred = 0;
            }
        }

        // check if we have ogg information about the sample # for this packet
        if f.last_seg_which == f.end_seg_with_known_loc {
            // if we have a valid current loc, and this is final:
            if self.current_loc_valid && (f.page_flag & 4) != 0 {
                let current_end = f.known_loc_for_packet;
                // then let's infer the size of the (probably) short final frame
                if current_end < self.current_loc.wrapping_add((right_end - left_start) as u32) {
                    let mut len = if current_end < self.current_loc {
                        // negative truncation, that's impossible!
                        0
                    } else {
                        current_end.wrapping_sub(self.current_loc) as i32
                    };
                    len = len.wrapping_add(left_start); // this doesn't seem right, but has no ill effect on my test files
                    if len > right_end {
                        len = right_end; // this should never happen
                    }
                    self.current_loc = self.current_loc.wrapping_add(len as u32);
                    return Some((len, left_start));
                }
            }
            // otherwise, just set our sample loc
            // guess that the ogg granule pos refers to the _middle_ of the
            // last frame?
            // set f->current_loc to the position of left_start
            self.current_loc = f.known_loc_for_packet.wrapping_sub((n2 - left_start) as u32);
            self.current_loc_valid = true;
        }
        if self.current_loc_valid {
            self.current_loc = self.current_loc.wrapping_add((right_start - left_start) as u32);
        }

        Some((right_end, left_start))
    }

    /// Decodes the next packet: `(len, left, right)`.
    pub(crate) fn vorbis_decode_packet(&mut self) -> Option<(i32, i32, i32)> {
        let start = self.vorbis_decode_initial()?;
        let (len, left) = self.vorbis_decode_packet_rest(&start)?;
        Some((len, left, start.right_start))
    }

    /// Overlaps the frame just decoded with the one before, and keeps its right half for the
    /// next. Gives the number of samples now ready, from `left` on.
    pub(crate) fn vorbis_finish_frame(&mut self, len: i32, left: i32, mut right: i32) -> i32 {
        // we use right&left (the start of the right- and left-window sin()-regions)
        // to determine how much to return, rather than inferring from the rules
        // (same result, clearer code); 'left' indicates where our sin() window
        // starts, therefore where the previous window's right edge starts, and
        // therefore where to start mixing from the previous buffer. 'right'
        // indicates where our sin() ending-window starts, therefore that's where
        // we start saving, and where our returned-data ends.

        // mixin from previous window
        if self.previous_length != 0 {
            let n = self.previous_length;
            // (A window that is neither block's: nothing to mix with.)
            let w = if n << 1 == self.setup.blocksize[0] as i32 {
                &self.setup.tables[0].window
            } else if n << 1 == self.setup.blocksize[1] as i32 {
                &self.setup.tables[1].window
            } else {
                return 0;
            };
            let (n, left) = (n as usize, left.max(0) as usize);
            let w = &w[..n.min(w.len())];
            for (buffer, previous) in self.channel_buffers.iter_mut().zip(&self.previous_window) {
                // (buffer[left + j] = buffer[left + j] * w[j] + previous[j] * w[n - 1 - j],
                // for the n values from `left` on.)
                let buffer = buffer.get_mut(left..).unwrap_or_default();
                for ((value, &before), (&rising, &falling)) in buffer.iter_mut().zip(previous).zip(w.iter().zip(w.iter().rev())) {
                    let (now, earlier) = (*value * rising, before * falling);
                    // The sum of the two, said so that it doesn't depend on the compiler. A
                    // broken file can make both of them not-a-number, and an addition then
                    // gives whichever of the two the compiler happened to put first, which
                    // shows in the sign of what comes out. It is the earlier frame's that
                    // stb_vorbis built by gcc gives, so that one is taken outright (with a
                    // mask and not a branch, to leave the loop several samples at a time).
                    let sum = now + earlier;
                    let earlier_is_nan = u32::from(earlier.is_nan()).wrapping_neg();
                    *value = f32::from_bits((earlier.to_bits() & earlier_is_nan) | (sum.to_bits() & !earlier_is_nan));
                }
            }
        }

        let prev = self.previous_length;

        // last half of this data becomes previous window
        self.previous_length = len - right;

        // @OPTIMIZE: could avoid this copy by double-buffering the
        // output (flipping previous_window with channel_buffers), but
        // then previous_window would have to be 2x as large, and
        // channel_buffers couldn't be temp mem (although they're NOT
        // currently temp mem, they could be (unless we want to level
        // performance by spreading out the computation))
        for (previous, buffer) in self.previous_window.iter_mut().zip(&self.channel_buffers) {
            // (previous[j] = buffer[right + j], for the values from `right` up to `len`.)
            let kept = buffer.get(right.max(0) as usize..len.max(0) as usize).unwrap_or_default();
            let count = kept.len().min(previous.len());
            previous[..count].copy_from_slice(&kept[..count]);
        }

        if prev == 0 {
            // there was no previous packet, so this data isn't valid...
            // this isn't entirely true, only the would-have-overlapped data
            // isn't valid, but this seems to be what the spec requires
            return 0;
        }

        // truncate a short frame
        if len < right {
            right = len;
        }

        self.samples_output = self.samples_output.wrapping_add((right - left) as u32);

        right - left
    }

    pub(crate) fn vorbis_pump_first_frame(&mut self) -> bool {
        match self.vorbis_decode_packet() {
            Some((len, left, right)) => {
                self.vorbis_finish_frame(len, left, right);
                true
            }
            None => false,
        }
    }
}
