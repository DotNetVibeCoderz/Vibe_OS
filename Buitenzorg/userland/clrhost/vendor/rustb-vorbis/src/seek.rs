//! Finding pages, the stream's length and seeking (stb_vorbis's vorbis_find_page ..
//! stb_vorbis_stream_length_in_seconds).
#[allow(unused_imports)]
use alloc::{boxed::Box, vec, vec::Vec};

use crate::codebook::ilog;
use crate::decode::PacketStart;
use crate::io::{ProbedPage, OGG_PAGE_HEADER};
use crate::{Vorbis, VorbisError};

const CRC32_POLY: u32 = 0x04c1_1db7; // from spec

const CRC_TABLE: [u32; 256] = crc32_init();

const fn crc32_init() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut s = (i as u32) << 24;
        let mut j = 0;
        while j < 8 {
            s = (s << 1) ^ (if s >= (1u32 << 31) { CRC32_POLY } else { 0 });
            j += 1;
        }
        table[i] = s;
        i += 1;
    }
    table
}

#[inline]
pub(crate) fn crc32_update(crc: u32, byte: u8) -> u32 {
    (crc << 8) ^ CRC_TABLE[usize::from(byte) ^ (crc >> 24) as usize]
}

/// What `vorbis_find_page` found: where the page ends and whether it is the stream's last.
struct FoundPage {
    end: u32,
    last: bool,
}

/// How `seek_to_sample_coarse`'s search ended.
enum Search {
    /// The decoder is at the start of the packet to decode from.
    AtPacket,
    /// The seek is over, with this result.
    Over(bool),
}

impl Vorbis<'_> {
    /// Looks from here on for a page whose checksum is right, and stops at its start.
    fn vorbis_find_page(&mut self) -> Option<FoundPage> {
        let f = &mut self.io;
        loop {
            if f.eof {
                return None;
            }
            let n = f.get8();
            if n == 0x4f {
                // page header candidate
                let retry_loc = f.file_offset();
                // check if we're off the end of a file_section stream
                if retry_loc.wrapping_sub(25) > f.stream_len() {
                    return None;
                }
                // check the rest of the header
                let mut i = 1;
                while i < 4 {
                    if f.get8() != OGG_PAGE_HEADER[i] {
                        break;
                    }
                    i += 1;
                }
                if f.eof {
                    return None;
                }
                if i == 4 {
                    let mut header = [0u8; 27];
                    header[..4].copy_from_slice(&OGG_PAGE_HEADER);
                    for byte in &mut header[4..] {
                        *byte = f.get8();
                    }
                    if f.eof {
                        return None;
                    }
                    if header[4] == 0 {
                        let goal = u32::from_le_bytes([header[22], header[23], header[24], header[25]]);
                        header[22..26].fill(0);
                        let mut crc = 0u32;
                        for &byte in &header {
                            crc = crc32_update(crc, byte);
                        }
                        let mut len = 0u32;
                        for _ in 0..header[26] {
                            let s = f.get8();
                            crc = crc32_update(crc, s);
                            len += u32::from(s);
                        }
                        if len != 0 && f.eof {
                            return None;
                        }
                        for _ in 0..len {
                            crc = crc32_update(crc, f.get8());
                        }
                        if crc == goal {
                            // we could now check that it's either got the last
                            // page flag set, OR it's followed by the capture
                            // pattern, but I guess TECHNICALLY you could have
                            // a file with garbage between each ogg page and recover
                            // from it automatically? So even though that paranoia
                            // might decrease the chance of an invalid decode by
                            // another 2^32, not worth it since it would hose those
                            // invalid-but-useful files?
                            let found = FoundPage { end: f.file_offset(), last: header[5] & 0x04 != 0 };
                            f.set_file_offset(retry_loc.wrapping_sub(1));
                            return Some(found);
                        }
                    }
                }
                // not a valid page, so rewind and look for next one
                f.set_file_offset(retry_loc);
            }
        }
    }

    /// Reads the header of the page here, and stays at its start.
    ///
    /// given a sufficiently large block of seekable data, the decoder can
    /// find where the page containing a given sample lies
    fn get_seek_page_info(&mut self) -> Option<ProbedPage> {
        let f = &mut self.io;
        let page_start = f.file_offset();

        // record where the page starts
        // parse the header
        // (stb_vorbis goes on with whatever its buffers held when the file ends here.)
        let mut header = [0u8; 27];
        if !f.getn(&mut header) {
            return None;
        }
        if header[..4] != OGG_PAGE_HEADER {
            return None;
        }
        let mut lacing = [0u8; 255];
        let lacing = &mut lacing[..usize::from(header[26])];
        // (A page with no segments has nothing to read. From a file on disk that counts as
        // failing, as it does in C, but there is nothing missing and stb_vorbis goes on.)
        if !f.getn(lacing) && !lacing.is_empty() {
            return None;
        }

        // determine the length of the payload
        let len: u32 = lacing.iter().map(|&segment| u32::from(segment)).sum();

        // this is the last page of the stream
        let page = ProbedPage {
            page_start,
            // figure out where the page ends
            page_end: page_start.wrapping_add(27 + u32::from(header[26]) + len),
            // read the last-decoded sample out of the data
            last_decoded_sample: u32::from_le_bytes([header[6], header[7], header[8], header[9]]),
        };

        // restore file state to where we were
        f.set_file_offset(page_start);
        Some(page)
    }

    /// rarely used function to seek back to the preceding page while finding the
    /// start of a packet
    fn go_to_page_before(&mut self, limit_offset: u32) -> bool {
        // now we want to seek back 64K from the limit
        let first = self.setup.first_audio_page_offset;
        let previous_safe = if limit_offset >= 65536 && limit_offset - 65536 >= first { limit_offset - 65536 } else { first };

        self.io.set_file_offset(previous_safe);

        while let Some(found) = self.vorbis_find_page() {
            if found.end >= limit_offset && self.io.file_offset() < limit_offset {
                return true;
            }
            self.io.set_file_offset(found.end);
        }

        false
    }

    /// implements the search logic for finding a page and starting decoding. if
    /// the function succeeds, current_loc_valid will be true and current_loc will
    /// be less than or equal to the provided sample number (the closer the
    /// better).
    fn seek_to_sample_coarse(&mut self, sample_number: u32) -> bool {
        // find the last page and validate the target sample
        let stream_length = self.stream_length_in_samples();
        if stream_length == 0 {
            return self.io.error(VorbisError::SeekWithoutLength);
        }
        if sample_number > stream_length {
            return self.io.error(VorbisError::SeekInvalid);
        }

        match self.seek_to_page(sample_number) {
            Some(Search::AtPacket) => {
                // prepare to start decoding
                if !self.vorbis_pump_first_frame() {
                    return false;
                }
                if self.current_loc > sample_number {
                    return self.io.error(VorbisError::SeekFailed);
                }
                true
            }
            Some(Search::Over(result)) => result,
            None => {
                // try to restore the file to a valid state
                self.seek_start_inner();
                self.io.error(VorbisError::SeekFailed)
            }
        }
    }

    /// The search itself; nothing is stb_vorbis's `goto error`.
    fn seek_to_page(&mut self, sample_number: u32) -> Option<Search> {
        let mut offset = 0.0f64;
        let mut bytes_per_sample = 0.0f64;
        let mut probe = 0;

        // this is the maximum difference between the window-center (which is the
        // actual granule position value), and the right-start (which the spec
        // indicates should be the granule position (give or take one)).
        let padding = ((self.setup.blocksize[1] as i32 - self.setup.blocksize[0] as i32) >> 2) as u32;
        let last_sample_limit = if sample_number < padding { 0 } else { sample_number - padding };

        let mut left = self.io.p_first;
        while left.last_decoded_sample == !0 {
            // (possibly) discard first page
            self.io.set_file_offset(left.page_end);
            left = self.get_seek_page_info()?;
        }

        let mut right = self.p_last;
        let mut mid = ProbedPage::default();

        // starting from the start is handled differently
        if last_sample_limit <= left.last_decoded_sample {
            let result = if self.seek_start_inner() {
                if self.current_loc > sample_number {
                    self.io.error(VorbisError::SeekFailed)
                } else {
                    true
                }
            } else {
                false
            };
            return Some(Search::Over(result));
        }

        while left.page_end != right.page_start {
            // (stb_vorbis asserts this.)
            if left.page_end >= right.page_start {
                return None;
            }
            // search range in bytes
            let delta = right.page_start - left.page_end;
            if delta <= 65536 {
                // there's only 64K left to search - handle it linearly
                self.io.set_file_offset(left.page_end);
            } else {
                if probe < 2 {
                    if probe == 0 {
                        // first probe (interpolate)
                        let data_bytes = f64::from(right.page_end.wrapping_sub(left.page_start));
                        bytes_per_sample = data_bytes / f64::from(right.last_decoded_sample);
                        offset = f64::from(left.page_start) + bytes_per_sample * f64::from(last_sample_limit.wrapping_sub(left.last_decoded_sample));
                    } else {
                        // second probe (try to bound the other side)
                        let mut error = (f64::from(last_sample_limit) - f64::from(mid.last_decoded_sample)) * bytes_per_sample;
                        if error >= 0.0 && error < 8000.0 {
                            error = 8000.0;
                        }
                        if error < 0.0 && error > -8000.0 {
                            error = -8000.0;
                        }
                        offset += error * 2.0;
                    }

                    // ensure the offset is valid
                    if offset < f64::from(left.page_end) {
                        offset = f64::from(left.page_end);
                    }
                    if offset > f64::from(right.page_start - 65536) {
                        offset = f64::from(right.page_start - 65536);
                    }

                    self.io.set_file_offset(offset as u32);
                } else {
                    // binary search for large ranges (offset by 32K to ensure
                    // we don't hit the right page)
                    self.io.set_file_offset(left.page_end.wrapping_add(delta / 2).wrapping_sub(32768));
                }

                self.vorbis_find_page()?;
            }

            loop {
                mid = self.get_seek_page_info()?;
                if mid.last_decoded_sample != !0 {
                    break;
                }
                // (untested) the first packet in the page is partial
                self.io.set_file_offset(mid.page_end);
                // (stb_vorbis asserts this.)
                if mid.page_start >= right.page_start {
                    return None;
                }
            }

            // if we've just found the last page again then we're in a tricky file,
            // and we're close enough (if it wasn't an interpolation probe).
            if mid.page_start == right.page_start {
                if probe >= 2 || delta <= 65536 {
                    break;
                }
            } else if last_sample_limit < mid.last_decoded_sample {
                right = mid;
            } else {
                left = mid;
            }

            probe += 1;
        }

        // seek back to start of the last packet
        let mut page_start = left.page_start;
        self.io.set_file_offset(page_start);
        if !self.io.start_page() {
            return Some(Search::Over(self.io.error(VorbisError::SeekFailed)));
        }
        let mut end_pos = self.io.end_seg_with_known_loc;
        // (stb_vorbis asserts this.)
        if end_pos < 0 {
            return None;
        }

        let start_seg_with_known_loc = loop {
            let mut i = end_pos;
            while i > 0 {
                if self.io.segments[(i - 1) as usize] != 255 {
                    break;
                }
                i -= 1;
            }

            if i > 0 || self.io.page_flag & 1 == 0 {
                break i;
            }

            // (untested) the final packet begins on an earlier page
            if !self.go_to_page_before(page_start) {
                return None;
            }

            page_start = self.io.file_offset();
            if !self.io.start_page() {
                return None;
            }
            end_pos = self.io.segment_count - 1;
        };

        // prepare to start decoding
        self.current_loc_valid = false;
        self.io.last_seg = false;
        self.io.valid_bits = 0;
        self.io.packet_bytes = 0;
        self.io.bytes_in_seg = 0;
        self.previous_length = 0;
        self.io.next_seg = start_seg_with_known_loc;

        for i in 0..start_seg_with_known_loc.max(0) as usize {
            self.io.skip(i64::from(self.io.segments[i]));
        }

        Some(Search::AtPacket)
    }

    /// the same as vorbis_decode_initial, but without advancing
    fn peek_decode_initial(&mut self) -> Option<PacketStart> {
        let start = self.vorbis_decode_initial()?;

        // either 1 or 2 bytes were read, figure out which so we can rewind
        let mut bits_read = 1 + ilog(self.setup.modes.len() as i32 - 1);
        if self.setup.modes[start.mode].blockflag != 0 {
            bits_read += 2;
        }
        let bytes_read = (bits_read + 7) / 8;

        let f = &mut self.io;
        f.bytes_in_seg = f.bytes_in_seg.wrapping_add(bytes_read as u8);
        f.packet_bytes -= bytes_read;
        f.skip(-i64::from(bytes_read));
        if f.next_seg == -1 {
            f.next_seg = f.segment_count - 1;
        } else {
            f.next_seg -= 1;
        }
        f.valid_bits = 0;

        Some(start)
    }

    fn seek_frame_inner(&mut self, sample_number: u32) -> bool {
        // fast page-level search
        if !self.seek_to_sample_coarse(sample_number) {
            return false;
        }

        let max_frame_samples = ((self.setup.blocksize[1] as i32 * 3 - self.setup.blocksize[0] as i32) >> 2) as u32;
        while self.current_loc < sample_number {
            let Some(start) = self.peek_decode_initial() else {
                return self.io.error(VorbisError::SeekFailed);
            };
            // calculate the number of samples returned by the next frame
            let frame_samples = (start.right_start - start.left_start) as u32;
            if self.current_loc.wrapping_add(frame_samples) > sample_number {
                return true; // the next frame will contain the sample
            } else if self.current_loc.wrapping_add(frame_samples).wrapping_add(max_frame_samples) > sample_number {
                // there's a chance the frame after this could contain the sample
                self.vorbis_pump_first_frame();
            } else {
                // this frame is too early to be relevant
                self.current_loc = self.current_loc.wrapping_add(frame_samples);
                self.previous_length = 0;
                self.io.maybe_start_packet();
                self.io.flush_packet();
            }
        }
        // the seek is valid
        if self.current_loc != sample_number {
            return self.io.error(VorbisError::SeekFailed);
        }
        true
    }

    fn seek_inner(&mut self, sample_number: u32) -> bool {
        if !self.seek_frame_inner(sample_number) {
            return false;
        }

        if sample_number != self.current_loc {
            let frame_start = self.current_loc;
            self.get_frame_float();
            self.channel_buffer_start = self.channel_buffer_start.wrapping_add(sample_number.wrapping_sub(frame_start) as i32);
        }

        true
    }

    fn seek_start_inner(&mut self) -> bool {
        self.io.set_file_offset(self.setup.first_audio_page_offset);
        self.previous_length = 0;
        self.io.first_decode = true;
        self.io.next_seg = -1;
        self.vorbis_pump_first_frame()
    }

    /// stb_vorbis's 1 or 0 as a result: the error is the one the decoder holds
    /// ([`Vorbis::take_error`] still gives it).
    fn outcome(&self, ok: bool) -> Result<(), VorbisError> {
        if ok {
            Ok(())
        } else {
            Err(self.io.error.unwrap_or(VorbisError::SeekFailed))
        }
    }

    /// Seeks to the frame holding a sample (`stb_vorbis_seek_frame`): the next frame decoded
    /// has it, though not as its first sample unless that is where it falls.
    /// [`Vorbis::sample_offset`] says where the frame starts.
    pub fn seek_frame(&mut self, sample_number: u32) -> Result<(), VorbisError> {
        let ok = self.seek_frame_inner(sample_number);
        self.outcome(ok)
    }

    /// Seeks to a sample (`stb_vorbis_seek`): the next sample handed out is that one.
    pub fn seek(&mut self, sample_number: u32) -> Result<(), VorbisError> {
        let ok = self.seek_inner(sample_number);
        self.outcome(ok)
    }

    /// Goes back to the start of the stream (`stb_vorbis_seek_start`).
    pub fn seek_start(&mut self) -> Result<(), VorbisError> {
        let ok = self.seek_start_inner();
        self.outcome(ok)
    }

    /// How many samples each channel has in all (`stb_vorbis_stream_length_in_samples`), found
    /// from the file's last page the first time this is asked. 0 if that can't be found.
    pub fn stream_length_in_samples(&mut self) -> u32 {
        if self.total_samples == 0 {
            // first, store the current decode position so we can restore it
            let restore_offset = self.io.file_offset();
            self.find_total_samples();
            self.io.set_file_offset(restore_offset);
        }
        if self.total_samples == 0xffff_ffff {
            0
        } else {
            self.total_samples
        }
    }

    fn find_total_samples(&mut self) {
        // now we want to seek back 64K from the end (the last page must
        // be at most a little less than 64K, but let's allow a little slop)
        let stream_len = self.io.stream_len();
        let first = self.setup.first_audio_page_offset;
        let previous_safe = if stream_len >= 65536 && stream_len - 65536 >= first { stream_len - 65536 } else { first };

        self.io.set_file_offset(previous_safe);
        // previous_safe is now our candidate 'earliest known place that seeking
        // to will lead to the final page'

        let Some(mut found) = self.vorbis_find_page() else {
            // if we can't find a page, we're hosed!
            self.io.error = Some(VorbisError::CantFindLastPage);
            self.total_samples = 0xffff_ffff;
            return;
        };

        // check if there are more pages
        let mut last_page_loc = self.io.file_offset();

        // stop when the last_page flag is set, not when we reach eof;
        // this allows us to stop short of a 'file_section' end without
        // explicitly checking the length of the section
        while !found.last {
            self.io.set_file_offset(found.end);
            match self.vorbis_find_page() {
                Some(next) => found = next,
                // the last page we found didn't have the 'last page' flag
                // set. whoops!
                None => break,
            }
            last_page_loc = self.io.file_offset();
        }

        self.io.set_file_offset(last_page_loc);

        // parse the header
        self.io.getn(&mut [0u8; 6]);
        // extract the absolute granule position
        let mut lo = self.io.get32();
        let hi = self.io.get32();
        if lo == 0xffff_ffff && hi == 0xffff_ffff {
            self.io.error = Some(VorbisError::CantFindLastPage);
            self.total_samples = 0xffff_ffff;
            return;
        }
        if hi != 0 {
            lo = 0xffff_fffe; // saturate
        }
        self.total_samples = lo;

        self.p_last.page_start = last_page_loc;
        self.p_last.page_end = found.end;
        self.p_last.last_decoded_sample = lo;
    }

    /// The stream's length in seconds (`stb_vorbis_stream_length_in_seconds`).
    pub fn stream_length_in_seconds(&mut self) -> f32 {
        self.stream_length_in_samples() as f32 / self.setup.sample_rate as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ogg_checksum() {
        // An empty last page of stream 1, as libogg writes it.
        let mut page = vec![0x4f, 0x67, 0x67, 0x53, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut crc = 0u32;
        for &byte in &page {
            crc = crc32_update(crc, byte);
        }
        // The same sum bit by bit.
        let mut slow = 0u32;
        for &byte in &page {
            slow ^= u32::from(byte) << 24;
            for _ in 0..8 {
                slow = if slow & 0x8000_0000 != 0 { (slow << 1) ^ CRC32_POLY } else { slow << 1 };
            }
        }
        assert_eq!(crc, slow);
        page[22..26].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(CRC_TABLE[1], CRC32_POLY);
    }
}
