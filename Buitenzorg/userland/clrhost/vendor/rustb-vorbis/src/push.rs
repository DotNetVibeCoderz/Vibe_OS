//! Decoding a file that arrives a piece at a time: stb_vorbis's "push" API
//! (stb_vorbis_open_pushdata, stb_vorbis_decode_frame_pushdata, stb_vorbis_flush_pushdata).
#[allow(unused_imports)]
use alloc::{boxed::Box, vec, vec::Vec};

use crate::io::{Io, EOP, OGG_PAGE_HEADER};
use crate::seek::crc32_update;
use crate::{setup, Pi, Vorbis, VorbisError, VorbisInfo};

/// How many pages the search for a page follows at once (STB_VORBIS_PUSHDATA_CRC_COUNT).
const PUSHDATA_CRC_COUNT: usize = 4;

/// A page that the search is checking the checksum of (stb_vorbis's CRCscan).
#[derive(Clone, Copy, Default)]
struct CrcScan {
    /// the crc we're looking for
    goal_crc: u32,
    /// bytes left in packet
    bytes_left: i32,
    /// running crc
    crc_so_far: u32,
    /// bytes processed in _current_ chunk
    bytes_done: i32,
    /// granule pos encoded in page
    sample_loc: u32,
}

/// What a call to [`VorbisPush::decode_frame`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PushFrame {
    /// How many of the bytes handed over were used. Hand over the bytes after them next time.
    /// 0: nothing could be done with what there was, and more bytes are needed.
    pub bytes_used: usize,
    /// How many samples the frame gave for each channel ([`VorbisPush::frame`] has them). 0
    /// with bytes used: the decoder was finding its place, or the frame held no samples (the
    /// first one doesn't); keep going.
    pub samples: usize,
}

/// A decoder that is handed an Ogg Vorbis file a piece at a time, as the bytes arrive
/// (stb_vorbis's `stb_vorbis *` from `stb_vorbis_open_pushdata`).
///
/// ```no_run
/// use rustb_vorbis::{VorbisError, VorbisPush};
///
/// # fn read_more(_: &mut Vec<u8>) -> bool { false }
/// let mut bytes: Vec<u8> = Vec::new(); // what has arrived and has not been used yet
///
/// // The headers: hand over more until there are enough.
/// let mut vorbis = loop {
///     match VorbisPush::open(&bytes) {
///         Ok((vorbis, used)) => {
///             bytes.drain(..used);
///             break vorbis;
///         }
///         Err(VorbisError::NeedMoreData) => {
///             if !read_more(&mut bytes) {
///                 return; // the stream ended inside its headers
///             }
///         }
///         Err(error) => panic!("not Ogg Vorbis: {error}"),
///     }
/// };
///
/// // The sound, a frame at a time.
/// loop {
///     let step = vorbis.decode_frame(&bytes);
///     if step.bytes_used == 0 {
///         if !read_more(&mut bytes) {
///             break; // the end of the stream
///         }
///         continue;
///     }
///     bytes.drain(..step.bytes_used);
///     if step.samples > 0 {
///         let left: &[f32] = vorbis.frame(0);
///         // step.samples samples for each channel are ready
///         # let _ = left;
///     }
/// }
/// ```
pub struct VorbisPush {
    /// The decoder, with no bytes of the file between calls. Only ever empty if a call
    /// panicked half way.
    inner: Option<Vorbis<'static>>,
    /// The search for a page: -1 when not searching, or how many of `scan` are in use.
    page_crc_tests: i32,
    scan: [CrcScan; PUSHDATA_CRC_COUNT],
}

impl VorbisPush {
    /// Reads the headers from the start of a file (`stb_vorbis_open_pushdata`), and gives the
    /// decoder with how many of the bytes it used: hand over the bytes after those to
    /// [`VorbisPush::decode_frame`].
    ///
    /// [`VorbisError::NeedMoreData`]: the headers are not all there yet. Call again with more
    /// of the file, from its start again. (stb_vorbis's own advice: the headers are usually
    /// within the first few kilobytes, and can be much larger.)
    pub fn open(data: &[u8]) -> Result<(VorbisPush, usize), VorbisError> {
        VorbisPush::open_with_pi(data, Pi::default())
    }

    /// [`VorbisPush::open`], for the samples of one build of stb_vorbis or another.
    pub fn open_with_pi(data: &[u8], pi: Pi) -> Result<(VorbisPush, usize), VorbisError> {
        // (stb_vorbis takes the length as an int.)
        let data = &data[..data.len().min(i32::MAX as usize)];
        let mut io = Io::new(data);
        io.push_mode = true;
        match setup::start_decoder(&mut io, pi) {
            Ok(setup) => {
                let used = io.pos.min(data.len());
                let vorbis = Vorbis::assemble(io, setup).rebind(&[]);
                Ok((VorbisPush { inner: Some(vorbis), page_crc_tests: -1, scan: [CrcScan::default(); PUSHDATA_CRC_COUNT] }, used))
            }
            Err(error) => Err(if io.eof { VorbisError::NeedMoreData } else { error }),
        }
    }

    /// The decoder, which is always there unless a call panicked.
    fn inner(&self) -> Option<&Vorbis<'static>> {
        self.inner.as_ref()
    }

    /// `stb_vorbis_get_info`
    pub fn info(&self) -> VorbisInfo {
        self.inner().map_or(VorbisInfo { sample_rate: 0, channels: 0, max_frame_size: 0 }, |vorbis| vorbis.info())
    }

    /// The encoder's name from the comment header, as the file has it (usually UTF-8).
    pub fn vendor(&self) -> &[u8] {
        self.inner().map_or(&[], |vorbis| vorbis.vendor())
    }

    /// The comment header's "NAME=value" entries, as the file has them (usually UTF-8).
    pub fn comments(&self) -> &[Vec<u8>] {
        self.inner().map_or(&[], |vorbis| vorbis.comments())
    }

    /// The last error, which this forgets (`stb_vorbis_get_error`).
    pub fn take_error(&mut self) -> Option<VorbisError> {
        self.inner.as_mut().and_then(|vorbis| vorbis.take_error())
    }

    /// The sample the next frame starts at, when that is known
    /// (`stb_vorbis_get_sample_offset`). After [`VorbisPush::flush`] it is not known until the
    /// decoder has found a page.
    pub fn sample_offset(&self) -> Option<u32> {
        self.inner().and_then(|vorbis| vorbis.sample_offset())
    }

    /// A channel's samples of the frame the last [`VorbisPush::decode_frame`] decoded.
    pub fn frame(&self, channel: usize) -> &[f32] {
        self.inner().map_or(&[], |vorbis| vorbis.frame(channel))
    }

    /// Tells the decoder that the bytes it is handed next do not follow the ones before
    /// (`stb_vorbis_flush_pushdata`): after a seek in the file, or after lost data. It then
    /// looks for the start of a page, and decodes from there; the first frame after gives no
    /// samples, as at the start of a file.
    pub fn flush(&mut self) {
        if let Some(vorbis) = self.inner.as_mut() {
            flush_pushdata(vorbis, &mut self.page_crc_tests);
        }
    }

    /// Decodes the next frame from the bytes that follow the ones used so far
    /// (`stb_vorbis_decode_frame_pushdata`). See [`PushFrame`] for what it gives. What went
    /// wrong, if anything, is in [`VorbisPush::take_error`]: [`VorbisError::NeedMoreData`]
    /// when it used no bytes because there were not enough, and after any other error the
    /// decoder looks for the next page it can decode from.
    ///
    /// A frame is decoded only when the whole of its packet is in `data`: there is no need to
    /// hand over the file page by page, but whatever is handed over has to start where the
    /// bytes used so far ended.
    pub fn decode_frame(&mut self, data: &[u8]) -> PushFrame {
        // (stb_vorbis takes the length as an int.)
        let data = &data[..data.len().min(i32::MAX as usize)];
        let Some(vorbis) = self.inner.take() else {
            return PushFrame::default();
        };
        let mut vorbis = vorbis.rebind(data);
        let result = self.decode_frame_pushdata(&mut vorbis);
        self.inner = Some(vorbis.rebind(&[]));
        result
    }

    fn decode_frame_pushdata(&mut self, vorbis: &mut Vorbis) -> PushFrame {
        if self.page_crc_tests >= 0 {
            let used = self.vorbis_search_for_page_pushdata(vorbis);
            return PushFrame { bytes_used: used.max(0) as usize, samples: 0 };
        }

        vorbis.io.pos = 0;
        vorbis.io.error = None;

        // check that we have the entire packet in memory
        if !vorbis.io.is_whole_packet_present(vorbis.previous_length) {
            return PushFrame::default();
        }

        let Some((len, left, right)) = vorbis.vorbis_decode_packet() else {
            // save the actual error we encountered
            let error = vorbis.io.error;
            let skip_packet = match error {
                // flush and resynch
                Some(VorbisError::BadPacketType) => true,
                // did we just start decoding? then this is the continuation of a packet we
                // never saw the start of: just skip it
                Some(VorbisError::ContinuedPacketFlagInvalid) => vorbis.previous_length == 0,
                _ => false,
            };
            if skip_packet {
                vorbis.io.error = None;
                while vorbis.io.get8_packet() != EOP {
                    if vorbis.io.eof {
                        break;
                    }
                }
                return PushFrame { bytes_used: self.used(vorbis), samples: 0 };
            }
            // if we get an error while parsing, what to do?
            // well, it DEFINITELY won't work to continue from where we are!
            flush_pushdata(vorbis, &mut self.page_crc_tests);
            // restore the error that actually made us bail
            vorbis.io.error = error;
            return PushFrame { bytes_used: 1, samples: 0 };
        };

        // success!
        let len = vorbis.vorbis_finish_frame(len, left, right);
        vorbis.channel_buffer_start = left;
        vorbis.channel_buffer_end = left + len;
        PushFrame { bytes_used: self.used(vorbis), samples: len.max(0) as usize }
    }

    /// stb_vorbis's `f->stream - data`: how far into the bytes at hand the decoder is.
    fn used(&self, vorbis: &Vorbis) -> usize {
        vorbis.io.pos.min(vorbis.io.data.len())
    }

    /// Looks through the bytes at hand for a page whose checksum is right, following up to
    /// four candidates at once across calls, and gives how many bytes it is done with.
    fn vorbis_search_for_page_pushdata(&mut self, vorbis: &mut Vorbis) -> i32 {
        let data = vorbis.io.data;
        let mut data_len = data.len() as i32;
        let tests = |count: i32| count.clamp(0, PUSHDATA_CRC_COUNT as i32) as usize;

        for scan in &mut self.scan[..tests(self.page_crc_tests)] {
            scan.bytes_done = 0;
        }

        // if we have room for more scans, search for them first, because
        // they may cause us to stop early if their header is incomplete
        if self.page_crc_tests < PUSHDATA_CRC_COUNT as i32 {
            if data_len < 4 {
                return 0;
            }
            data_len -= 3; // need to look for 4-byte sequence, so don't miss
                           // one that straddles a boundary
            let mut i = 0i32;
            while i < data_len {
                let at = i as usize;
                if data[at] == 0x4f && data[at..at + 4] == OGG_PAGE_HEADER {
                    // make sure we have the whole page header
                    if i + 26 >= data_len || i + 27 + i32::from(data[at + 26]) >= data_len {
                        // only read up to this page start, so hopefully we'll
                        // have the whole page header start next time
                        data_len = i;
                        break;
                    }
                    // ok, we have it all; compute the length of the page
                    let segments = usize::from(data[at + 26]);
                    let mut len = 27 + segments as i32;
                    for &segment in &data[at + 27..at + 27 + segments] {
                        len += i32::from(segment);
                    }
                    // scan everything up to the embedded crc (which we must 0)
                    let mut crc = 0u32;
                    for &byte in &data[at..at + 22] {
                        crc = crc32_update(crc, byte);
                    }
                    // now process 4 0-bytes
                    for _ in 22..26 {
                        crc = crc32_update(crc, 0);
                    }
                    // len is the total number of bytes we need to scan
                    let n = tests(self.page_crc_tests);
                    self.page_crc_tests += 1;
                    let scan = &mut self.scan[n];
                    scan.bytes_left = len - 26;
                    scan.crc_so_far = crc;
                    scan.goal_crc = u32::from_le_bytes([data[at + 22], data[at + 23], data[at + 24], data[at + 25]]);
                    // if the last frame on a page is continued to the next, then
                    // we can't recover the sample_loc immediately
                    scan.sample_loc = if data[at + 27 + segments - 1] == 255 {
                        !0
                    } else {
                        u32::from_le_bytes([data[at + 6], data[at + 7], data[at + 8], data[at + 9]])
                    };
                    scan.bytes_done = i + 26;
                    if self.page_crc_tests == PUSHDATA_CRC_COUNT as i32 {
                        break;
                    }
                    // keep going if we still have room for more
                }
                i += 1;
            }
        }

        let mut i = 0usize;
        while i < tests(self.page_crc_tests) {
            let n = self.scan[i].bytes_done;
            let mut m = self.scan[i].bytes_left;
            if m > data_len - n {
                m = data_len - n;
            }
            // m is the bytes to scan in the current chunk
            let mut crc = self.scan[i].crc_so_far;
            for j in 0..m.max(0) {
                crc = crc32_update(crc, data[(n + j) as usize]);
            }
            self.scan[i].bytes_left -= m;
            self.scan[i].crc_so_far = crc;
            if self.scan[i].bytes_left == 0 {
                // does it match?
                if self.scan[i].crc_so_far == self.scan[i].goal_crc {
                    // Houston, we have page
                    data_len = n + m; // consumption amount is wherever that scan ended
                    self.page_crc_tests = -1; // drop out of page scan mode
                    vorbis.previous_length = 0; // decode-but-don't-output one frame
                    vorbis.io.next_seg = -1; // start a new page
                    vorbis.current_loc = self.scan[i].sample_loc; // set the current sample location
                                                                  // to the amount we'd have decoded had we decoded this page
                    vorbis.current_loc_valid = vorbis.current_loc != !0;
                    return data_len;
                }
                // delete entry
                self.page_crc_tests -= 1;
                self.scan[i] = self.scan[tests(self.page_crc_tests)];
            } else {
                i += 1;
            }
        }

        data_len
    }
}

/// `stb_vorbis_flush_pushdata`
fn flush_pushdata(vorbis: &mut Vorbis, page_crc_tests: &mut i32) {
    vorbis.previous_length = 0;
    *page_crc_tests = 0;
    vorbis.discard_samples_deferred = 0;
    vorbis.current_loc_valid = false;
    vorbis.io.first_decode = false;
    vorbis.samples_output = 0;
    vorbis.channel_buffer_start = 0;
    vorbis.channel_buffer_end = 0;
}
