//! The byte stream, Ogg pages and packets, and the bit reader (stb_vorbis's get8 .. prep_huffman).

use crate::VorbisError;
#[allow(unused_imports)]
use alloc::{boxed::Box, vec, vec::Vec};

/// "No more bits in this packet" (stb_vorbis's EOP and INVALID_BITS).
pub(crate) const EOP: i32 = -1;

/// "OggS": what every Ogg page starts with.
pub(crate) const OGG_PAGE_HEADER: [u8; 4] = [0x4f, 0x67, 0x67, 0x53];

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ProbedPage {
    pub page_start: u32,
    pub page_end: u32,
    pub last_decoded_sample: u32,
}

/// What a decoder reads a file on disk through: anything that can be read and searched, and
/// (so that a decoder stays something a thread can be handed) sent and shared.
///
/// (no_std build: a small trait of its own instead of std's `Read + Seek`.)
pub trait Stream {
    /// Bytes read into `buf` (0 = the end); Err = it could not be read.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()>;
    /// Move to `pos` from the stream's start; the new position, or None.
    fn seek_to(&mut self, pos: u64) -> Option<u64>;
}

/// A file read as it is needed: stb_vorbis's `FILE *f`, with C's `fgetc`, `fread`, `fseek`
/// and `ftell` over it.
pub(crate) struct FileIo<'a> {
    file: Box<dyn Stream + 'a>,
    /// Where the Vorbis data starts in the file (f_start).
    start: u32,
    /// The length the stream was opened with (stream_len): the rest of the file, or what
    /// the caller said.
    stream_len: u32,
    /// The position in the file, as `ftell` gives it.
    pos: u64,
    /// Bytes of the file from `buffer_start` on, read ahead.
    buffer: Vec<u8>,
    buffer_start: u64,
    /// Where the file itself is, when that is known.
    file_pos: Option<u64>,
}

/// How much of a file is read at a time.
const FILE_BUFFER: usize = 16 * 1024;

impl<'a> FileIo<'a> {
    pub fn new(file: Box<dyn Stream + 'a>, start: u32, stream_len: u32) -> FileIo<'a> {
        FileIo { file, start, stream_len, pos: u64::from(start), buffer: Vec::new(), buffer_start: 0, file_pos: None }
    }

    /// The bytes read ahead from the position on, without reading any more of the file.
    #[inline]
    fn buffered(&self) -> &[u8] {
        let from = self.pos.checked_sub(self.buffer_start).and_then(|from| usize::try_from(from).ok());
        from.and_then(|from| self.buffer.get(from..)).unwrap_or_default()
    }

    /// The bytes of the buffer from the position on; reads more of the file if there are none.
    /// Nothing at the end of the file (or where the file can't be read).
    fn ahead(&mut self) -> &[u8] {
        if self.buffered().is_empty() {
            self.buffer.clear();
            self.buffer_start = self.pos;
            if self.file_pos != Some(self.pos) {
                self.file_pos = self.file.seek_to(self.pos);
            }
            if self.file_pos == Some(self.pos) {
                self.buffer.resize(FILE_BUFFER, 0);
                let mut filled = 0;
                while filled < FILE_BUFFER {
                    match self.file.read(&mut self.buffer[filled..]) {
                        Ok(0) => break,
                        Ok(n) => filled += n,
                        Err(_) => {
                            // (As C's stdio: what could not be read is the end of the file.)
                            self.file_pos = None;
                            break;
                        }
                    }
                }
                self.buffer.truncate(filled);
                if let Some(file_pos) = self.file_pos.as_mut() {
                    *file_pos += filled as u64;
                }
            }
        }
        &self.buffer[(self.pos - self.buffer_start) as usize..]
    }

    /// `fgetc`
    #[inline]
    fn get8(&mut self) -> Option<u8> {
        let byte = match self.buffered().first() {
            Some(&byte) => byte,
            None => self.ahead().first().copied()?,
        };
        self.pos += 1;
        Some(byte)
    }

    /// `fread(out, n, 1, f) == 1`: all of `out`, or as much as the file has and "failed".
    ///
    /// Reading nothing "fails" too: C's `fread` of one item of no bytes gives 0 items. So a
    /// page with no segments, which stb_vorbis reads from memory, is the end of the file to
    /// it when it reads from disk, and it is the same here.
    fn getn(&mut self, out: &mut [u8]) -> bool {
        if out.is_empty() {
            return false;
        }
        let mut done = 0;
        while done < out.len() {
            let ahead = self.ahead();
            if ahead.is_empty() {
                return false;
            }
            let n = ahead.len().min(out.len() - done);
            out[done..done + n].copy_from_slice(&ahead[..n]);
            done += n;
            self.pos += n as u64;
        }
        true
    }

    /// `fseek(f, ftell(f) + n, SEEK_SET)`, which fails and stays put before the file's start.
    fn skip(&mut self, n: i64) {
        if let Some(pos) = (self.pos as i64).checked_add(n).filter(|&pos| pos >= 0) {
            self.pos = pos as u64;
        }
    }
}

/// Everything about where the decoder is in the file.
pub(crate) struct Io<'a> {
    /// The whole file, or in push mode the bytes the caller handed over this time. Empty when
    /// the file is read from disk (`file`).
    pub data: &'a [u8],
    /// The file, when it is read from disk as it is needed.
    pub file: Option<FileIo<'a>>,
    /// Offset of the next byte. A skip may put it past the end.
    pub pos: usize,
    pub eof: bool,
    pub error: Option<VorbisError>,
    /// The decoder is given the file a piece at a time (stb_vorbis's push_mode).
    pub push_mode: bool,

    pub p_first: ProbedPage,
    pub first_decode: bool,

    // The current page.
    pub last_page: u32,
    pub segment_count: i32,
    pub segments: [u8; 255],
    pub page_flag: u8,
    pub next_seg: i32,
    pub last_seg: bool,
    /// The segment number of the last segment of the packet being read.
    pub last_seg_which: i32,
    pub end_seg_with_known_loc: i32,
    pub known_loc_for_packet: u32,

    // The current packet and its bits.
    pub bytes_in_seg: u8,
    pub acc: u32,
    pub valid_bits: i32,
    pub packet_bytes: i32,
}

impl<'a> Io<'a> {
    pub fn new(data: &'a [u8]) -> Io<'a> {
        Io {
            data,
            file: None,
            pos: 0,
            eof: false,
            error: None,
            push_mode: false,
            p_first: ProbedPage::default(),
            first_decode: false,
            last_page: 0,
            segment_count: 0,
            segments: [0; 255],
            page_flag: 0,
            next_seg: 0,
            last_seg: false,
            last_seg_which: 0,
            end_seg_with_known_loc: 0,
            known_loc_for_packet: 0,
            bytes_in_seg: 0,
            acc: 0,
            valid_bits: 0,
            packet_bytes: 0,
        }
    }

    /// Records an error and gives stb_vorbis's "failed" (0).
    pub fn error(&mut self, e: VorbisError) -> bool {
        self.error = Some(e);
        false
    }

    pub fn stream_len(&self) -> u32 {
        match &self.file {
            Some(file) => file.stream_len,
            None => self.data.len() as u32,
        }
    }

    #[inline]
    pub fn get8(&mut self) -> u8 {
        match self.data.get(self.pos) {
            Some(&byte) => {
                self.pos += 1;
                byte
            }
            None => self.get8_slow(),
        }
    }

    /// The end of the bytes in memory: the end of the file, or a file on disk to read from.
    #[inline(never)]
    fn get8_slow(&mut self) -> u8 {
        if let Some(byte) = self.file.as_mut().and_then(|file| file.get8()) {
            return byte;
        }
        self.eof = true;
        0
    }

    pub fn get32(&mut self) -> u32 {
        let mut x = u32::from(self.get8());
        x += u32::from(self.get8()) << 8;
        x += u32::from(self.get8()) << 16;
        x += u32::from(self.get8()) << 24;
        x
    }

    /// Fills `out` with the next bytes, or fails (and it is the end of the file) if there
    /// aren't that many. From memory a failure reads nothing; from a file on disk it reads
    /// what there is, as C's `fread` does.
    pub fn getn(&mut self, out: &mut [u8]) -> bool {
        if let Some(file) = self.file.as_mut() {
            if file.getn(out) {
                return true;
            }
            self.eof = true;
            return false;
        }
        let end = self.pos.checked_add(out.len()).filter(|&end| end <= self.data.len());
        match end {
            Some(end) => {
                out.copy_from_slice(&self.data[self.pos..end]);
                self.pos = end;
                true
            }
            None => {
                self.eof = true;
                false
            }
        }
    }

    pub fn skip(&mut self, n: i64) {
        if let Some(file) = self.file.as_mut() {
            file.skip(n);
            return;
        }
        self.pos = (self.pos as i64).saturating_add(n).max(0) as usize;
        if self.pos >= self.data.len() {
            self.eof = true;
        }
    }

    pub fn set_file_offset(&mut self, loc: u32) -> bool {
        if self.push_mode {
            return false;
        }
        self.eof = false;
        if let Some(file) = self.file.as_mut() {
            // (C's fseek takes any place in the first 2 GB, past the end of the file too;
            // reading there is then the end of the file.)
            let mut loc = loc;
            if loc.checked_add(file.start).is_none() || loc >= 0x8000_0000 {
                loc = 0x7fff_ffff;
                self.eof = true;
            } else {
                loc += file.start;
            }
            file.pos = u64::from(loc);
            return true;
        }
        if loc as usize >= self.data.len() {
            self.pos = self.data.len();
            self.eof = true;
            false
        } else {
            self.pos = loc as usize;
            true
        }
    }

    pub fn file_offset(&self) -> u32 {
        if self.push_mode {
            return 0;
        }
        if let Some(file) = &self.file {
            return file.pos.wrapping_sub(u64::from(file.start)) as u32;
        }
        self.pos as u32
    }

    /// The same state over other bytes: push mode's `f->stream = data` at each call, which in
    /// Rust is another lifetime and so another value.
    pub fn rebind<'b>(self, data: &'b [u8]) -> Io<'b> {
        Io {
            data,
            // (Push mode never reads from a file on disk.)
            file: None,
            pos: self.pos,
            eof: self.eof,
            error: self.error,
            push_mode: self.push_mode,
            p_first: self.p_first,
            first_decode: self.first_decode,
            last_page: self.last_page,
            segment_count: self.segment_count,
            segments: self.segments,
            page_flag: self.page_flag,
            next_seg: self.next_seg,
            last_seg: self.last_seg,
            last_seg_which: self.last_seg_which,
            end_seg_with_known_loc: self.end_seg_with_known_loc,
            known_loc_for_packet: self.known_loc_for_packet,
            bytes_in_seg: self.bytes_in_seg,
            acc: self.acc,
            valid_bits: self.valid_bits,
            packet_bytes: self.packet_bytes,
        }
    }

    /// Push mode: whether the bytes at hand hold the whole of the next packet
    /// (is_whole_packet_present). `previous_length`: the decoder's, 0 when it is resynching.
    pub fn is_whole_packet_present(&mut self, previous_length: i32) -> bool {
        // make sure that we have the packet available before continuing...
        // this requires a full ogg parse, but we know we can fetch from f->stream

        // instead of coding this out explicitly, we could save the current read state,
        // read the next packet with get8() until end-of-packet, check f->eof, then
        // reset the state? but that would be slower, esp. since we'd have over 256 bytes
        // of state to restore (primarily the page segment table)

        let end = self.data.len();
        let mut s = self.next_seg;
        let mut first = true;
        let mut p = self.pos;
        if s != -1 {
            // if we're not at the start of a page
            while s < self.segment_count {
                let segment = self.segments.get(s as usize).copied().unwrap_or(0);
                p += usize::from(segment);
                if segment < 255 {
                    break;
                }
                s += 1;
            }
            // either this continues, or it ends it...
            if s == self.segment_count {
                s = -1; // set 'crosses page' flag
            }
            if p > end {
                return self.error(VorbisError::NeedMoreData);
            }
            first = false;
        }
        while s == -1 {
            // check that we have the page header ready
            if p + 26 >= end {
                return self.error(VorbisError::NeedMoreData);
            }
            // validate the page
            if self.data[p..p + 4] != OGG_PAGE_HEADER {
                return self.error(VorbisError::InvalidStream);
            }
            if self.data[p + 4] != 0 {
                return self.error(VorbisError::InvalidStream);
            }
            if first {
                // the first segment must NOT have 'continued_packet', later ones MUST
                if previous_length != 0 && self.data[p + 5] & 1 != 0 {
                    return self.error(VorbisError::InvalidStream);
                }
                // if no previous length, we're resynching, so we can come in on a continued-packet,
                // which we'll just drop
            } else if self.data[p + 5] & 1 == 0 {
                return self.error(VorbisError::InvalidStream);
            }
            let n = usize::from(self.data[p + 26]); // segment counts
            let q = p + 27; // q points to segment table
            p = q + n; // advance past header
            // make sure we've read the segment table
            if p > end {
                return self.error(VorbisError::NeedMoreData);
            }
            let mut ended = false;
            for k in 0..n {
                p += usize::from(self.data[q + k]);
                if self.data[q + k] < 255 {
                    ended = true;
                    break;
                }
            }
            if ended {
                s = 0;
            }
            if p > end {
                return self.error(VorbisError::NeedMoreData);
            }
            first = false;
        }
        true
    }

    fn capture_pattern(&mut self) -> bool {
        self.get8() == 0x4f && self.get8() == 0x67 && self.get8() == 0x67 && self.get8() == 0x53
    }

    pub fn start_page_no_capturepattern(&mut self) -> bool {
        if self.first_decode && !self.push_mode {
            self.p_first.page_start = self.file_offset().wrapping_sub(4);
        }
        // stream structure version
        if self.get8() != 0 {
            return self.error(VorbisError::InvalidStreamStructureVersion);
        }
        // header flag
        self.page_flag = self.get8();
        // absolute granule position
        let loc0 = self.get32();
        let loc1 = self.get32();
        // stream serial number -- vorbis doesn't care, although ogg does
        self.get32();
        // page sequence number
        self.last_page = self.get32();
        // CRC32
        self.get32();
        // page_segments
        self.segment_count = i32::from(self.get8());
        let count = self.segment_count as usize;
        let mut segments = self.segments;
        let read = self.getn(&mut segments[..count]);
        self.segments = segments;
        if !read {
            return self.error(VorbisError::UnexpectedEof);
        }
        // assume we _don't_ know the sample position of any segments
        self.end_seg_with_known_loc = -2;
        if loc0 != !0 || loc1 != !0 {
            // determine which packet is the last one that will complete
            if let Some(i) = self.segments[..count].iter().rposition(|&segment| segment < 255) {
                // 'i' is now the index of the _last_ segment of a packet that ends
                self.end_seg_with_known_loc = i as i32;
                self.known_loc_for_packet = loc0;
            }
        }
        if self.first_decode {
            let mut len = 0u32;
            for &segment in &self.segments[..count] {
                len += u32::from(segment);
            }
            len += 27 + count as u32;
            self.p_first.page_end = self.p_first.page_start.wrapping_add(len);
            self.p_first.last_decoded_sample = loc0;
        }
        self.next_seg = 0;
        true
    }

    pub fn start_page(&mut self) -> bool {
        if !self.capture_pattern() {
            return self.error(VorbisError::MissingCapturePattern);
        }
        self.start_page_no_capturepattern()
    }

    pub fn start_packet(&mut self) -> bool {
        while self.next_seg == -1 {
            if !self.start_page() {
                return false;
            }
            if self.page_flag & 1 != 0 {
                return self.error(VorbisError::ContinuedPacketFlagInvalid);
            }
        }
        self.last_seg = false;
        self.valid_bits = 0;
        self.packet_bytes = 0;
        self.bytes_in_seg = 0;
        true
    }

    pub fn maybe_start_packet(&mut self) -> bool {
        if self.next_seg == -1 {
            let x = self.get8();
            if self.eof {
                return false; // EOF at page boundary is not an error!
            }
            if x != 0x4f || self.get8() != 0x67 || self.get8() != 0x67 || self.get8() != 0x53 {
                return self.error(VorbisError::MissingCapturePattern);
            }
            if !self.start_page_no_capturepattern() {
                return false;
            }
            if self.page_flag & 1 != 0 {
                // set up enough state that we can read this packet if we want,
                // e.g. during recovery
                self.last_seg = false;
                self.bytes_in_seg = 0;
                return self.error(VorbisError::ContinuedPacketFlagInvalid);
            }
        }
        self.start_packet()
    }

    pub fn next_segment(&mut self) -> i32 {
        if self.last_seg {
            return 0;
        }
        if self.next_seg == -1 {
            self.last_seg_which = self.segment_count - 1; // in case start_page fails
            if !self.start_page() {
                self.last_seg = true;
                return 0;
            }
            if self.page_flag & 1 == 0 {
                self.error(VorbisError::ContinuedPacketFlagInvalid);
                return 0;
            }
        }
        let len = i32::from(self.segments.get(self.next_seg as usize).copied().unwrap_or(0));
        self.next_seg += 1;
        if len < 255 {
            self.last_seg = true;
            self.last_seg_which = self.next_seg - 1;
        }
        if self.next_seg >= self.segment_count {
            self.next_seg = -1;
        }
        self.bytes_in_seg = len as u8;
        len
    }

    pub fn get8_packet_raw(&mut self) -> i32 {
        if self.bytes_in_seg == 0 {
            if self.last_seg || self.next_segment() == 0 {
                return EOP;
            }
        }
        self.bytes_in_seg = self.bytes_in_seg.wrapping_sub(1);
        self.packet_bytes += 1;
        i32::from(self.get8())
    }

    pub fn get8_packet(&mut self) -> i32 {
        let x = self.get8_packet_raw();
        self.valid_bits = 0;
        x
    }

    pub fn get32_packet(&mut self) -> i32 {
        let mut x = self.get8_packet() as u32;
        x = x.wrapping_add((self.get8_packet() << 8) as u32);
        x = x.wrapping_add((self.get8_packet() << 16) as u32);
        x = x.wrapping_add((self.get8_packet() as u32) << 24);
        x as i32
    }

    pub fn flush_packet(&mut self) {
        while self.get8_packet_raw() != EOP {}
    }

    // @OPTIMIZE: this is the secondary bit decoder, so it's probably not as important
    // as the huffman decoder?
    pub fn get_bits(&mut self, n: i32) -> u32 {
        if self.valid_bits < 0 {
            return 0;
        }
        if self.valid_bits < n {
            if n > 24 {
                // the accumulator technique below would not work correctly in this case
                let mut z = self.get_bits(24);
                z = z.wrapping_add(self.get_bits(n - 24) << 24);
                return z;
            }
            if self.valid_bits == 0 {
                self.acc = 0;
            }
            while self.valid_bits < n {
                let z = self.get8_packet_raw();
                if z == EOP {
                    self.valid_bits = EOP;
                    return 0;
                }
                self.acc = self.acc.wrapping_add((z as u32) << self.valid_bits);
                self.valid_bits += 8;
            }
        }
        let z = self.acc & 1u32.checked_shl(n as u32).map_or(!0, |bit| bit - 1);
        self.acc = self.acc.checked_shr(n as u32).unwrap_or(0);
        self.valid_bits -= n;
        z
    }

    // @OPTIMIZE: primary accumulator for huffman
    // expand the buffer to as many bits as possible without reading off end of packet
    // it might be nice to allow f->valid_bits and f->acc to be stored in registers,
    // e.g. cache them locally and decode locally
    #[inline(never)]
    pub fn prep_huffman(&mut self) {
        if self.valid_bits <= 24 {
            if self.valid_bits == 0 {
                self.acc = 0;
            }
            // The usual case, when the bytes wanted are all in this segment and in memory (or,
            // of a file on disk, read ahead): the loop below, without its asking about the
            // segment for each byte.
            if self.valid_bits >= 0 {
                let count = (32 - self.valid_bits as usize) / 8;
                if usize::from(self.bytes_in_seg) >= count {
                    let bytes = match &self.file {
                        None => self.data.get(self.pos..).and_then(|rest| rest.get(..count)),
                        Some(file) => file.buffered().get(..count),
                    };
                    if let Some(bytes) = bytes {
                        let (mut acc, mut valid_bits) = (self.acc, self.valid_bits);
                        for &byte in bytes {
                            acc = acc.wrapping_add(u32::from(byte) << valid_bits);
                            valid_bits += 8;
                        }
                        (self.acc, self.valid_bits) = (acc, valid_bits);
                        self.bytes_in_seg -= count as u8;
                        self.packet_bytes += count as i32;
                        match self.file.as_mut() {
                            None => self.pos += count,
                            Some(file) => file.pos += count as u64,
                        }
                        return;
                    }
                }
            }
            loop {
                if self.last_seg && self.bytes_in_seg == 0 {
                    return;
                }
                let z = self.get8_packet_raw();
                if z == EOP {
                    return;
                }
                self.acc = self.acc.wrapping_add((z as u32) << self.valid_bits);
                self.valid_bits += 8;
                if self.valid_bits > 24 {
                    break;
                }
            }
        }
    }
}
