//! An Ogg Vorbis decoder: a port to Rust of Sean Barrett's
//! [stb_vorbis](https://github.com/nothings/stb) (version 1.22), function for function.
//!
//! It keeps stb_vorbis's behaviour, order of float operations included, so it gives the same
//! samples as the C library does, to the bit. It has no dependencies and no `unsafe`.
//!
//! ```no_run
//! let file = std::fs::read("sound.ogg").unwrap();
//! let mut vorbis = rustb_vorbis::Vorbis::open(&file).unwrap();
//! let info = vorbis.info();
//! let mut samples = vec![0i16; 4096 * info.channels];
//! loop {
//!     let frames = vorbis.get_samples_short_interleaved(info.channels, &mut samples);
//!     if frames == 0 {
//!         break;
//!     }
//!     // frames * info.channels samples are ready
//! }
//! ```
//!
//! Both of stb_vorbis's APIs are here:
//!
//! - [`Vorbis`], its "pull" API: opening, the stream's information and comments, decoding by
//!   frame or by any number of samples as floats or 16-bit integers (with its down-mix to mono
//!   and stereo), seeking, and the stream's length. The file is either held in memory
//!   ([`Vorbis::open`]) or read from disk as it is decoded ([`Vorbis::open_file_section`]).
//! - [`VorbisPush`], its "push" API: the decoder is handed the file a piece at a time, as the
//!   bytes arrive, and gives a frame whenever it has the whole of one.
//!
//! Like stb_vorbis, it does not decode floor 0 (found in old files, from before 2004), reads
//! only the first logical stream of a file, and does not check page checksums while decoding.
//!
//! stb_vorbis's tables depend a little on how it is built, and so do its samples in their last
//! bits: [`Pi`] chooses which build to be the same as.
//!
//! Where stb_vorbis would read or write outside its buffers on a broken file, this stops or
//! reads zeros instead; on files that are valid the two agree.
#![no_std]
#![forbid(unsafe_code)]
#![allow(clippy::needless_range_loop, clippy::collapsible_if, clippy::manual_range_contains, clippy::manual_rotate, clippy::implicit_saturating_sub, clippy::too_many_arguments)]


extern crate alloc;
#[allow(unused_imports)]
use alloc::{boxed::Box, vec, vec::Vec};
// The code follows stb_vorbis's line for line, and these are its shapes.

mod codebook;
mod decode;
mod io;
mod mdct;
mod push;
mod samples;
mod seek;
mod setup;
mod tables;

pub use io::Stream;
use io::{FileIo, Io, ProbedPage};
use setup::Setup;

pub use push::{PushFrame, VorbisPush};

/// The most channels a stream may have (STB_VORBIS_MAX_CHANNELS).
pub const MAX_CHANNELS: usize = 16;

/// Why a file could not be opened, decoded or searched: stb_vorbis's STBVorbisError.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VorbisError {
    /// Not a real error: [`VorbisPush`] needs more of the file than it was handed.
    NeedMoreData = 1,
    /// Can't mix the push and pull APIs. (Here they are two types, so this can't happen.)
    InvalidApiMixing = 2,
    /// Not enough memory.
    OutOfMemory = 3,
    /// Uses floor 0.
    FeatureNotSupported = 4,
    /// More than [`MAX_CHANNELS`] channels.
    TooManyChannels = 5,
    /// The file could not be opened (or, handed over in memory, is larger than 2 GB).
    FileOpenFailure = 6,
    /// Can't seek in a file of unknown length.
    SeekWithoutLength = 7,
    /// The file is truncated.
    UnexpectedEof = 10,
    /// Seek past the end of the file.
    SeekInvalid = 11,

    // decoding errors (corrupt/invalid stream) -- you probably don't care about the exact
    // details of these

    // vorbis errors:
    InvalidSetup = 20,
    InvalidStream = 21,

    // ogg errors:
    MissingCapturePattern = 30,
    InvalidStreamStructureVersion = 31,
    ContinuedPacketFlagInvalid = 32,
    IncorrectStreamSerialNumber = 33,
    InvalidFirstPage = 34,
    BadPacketType = 35,
    CantFindLastPage = 36,
    SeekFailed = 37,
    OggSkeletonNotSupported = 38,
}

impl VorbisError {
    /// stb_vorbis's number for the error.
    pub fn code(self) -> i32 {
        self as i32
    }
}

impl core::fmt::Display for VorbisError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let text = match self {
            VorbisError::NeedMoreData => "more data is needed",
            VorbisError::InvalidApiMixing => "the push and pull APIs can't be mixed",
            VorbisError::OutOfMemory => "not enough memory",
            VorbisError::FeatureNotSupported => "the stream uses floor 0, which is not supported",
            VorbisError::TooManyChannels => "the stream has too many channels",
            VorbisError::FileOpenFailure => "the file could not be opened",
            VorbisError::SeekWithoutLength => "can't seek in a stream of unknown length",
            VorbisError::UnexpectedEof => "the file ends too early",
            VorbisError::SeekInvalid => "seek past the end of the stream",
            VorbisError::InvalidSetup => "the Vorbis setup header is invalid",
            VorbisError::InvalidStream => "the Vorbis stream is invalid",
            VorbisError::MissingCapturePattern => "an Ogg page does not start where one should",
            VorbisError::InvalidStreamStructureVersion => "unknown Ogg stream structure version",
            VorbisError::ContinuedPacketFlagInvalid => "an Ogg page's continued-packet flag is wrong",
            VorbisError::IncorrectStreamSerialNumber => "an Ogg page has another stream's serial number",
            VorbisError::InvalidFirstPage => "the first Ogg page is not a Vorbis identification header",
            VorbisError::BadPacketType => "a packet is not an audio packet",
            VorbisError::CantFindLastPage => "the last Ogg page could not be found",
            VorbisError::SeekFailed => "the seek failed",
            VorbisError::OggSkeletonNotSupported => "Ogg Skeleton streams are not supported",
        };
        f.write_str(text)
    }
}


/// Which stb_vorbis to give the samples of. Its tables are made with the C library's `M_PI`,
/// and where the library's headers don't have that, with a constant of its own, which is a
/// `float`: the samples then differ in their last bits (and now and then a 16-bit sample by
/// one).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pi {
    /// `M_PI` from `<math.h>`, a `double`: stb_vorbis as gcc and clang build it (Linux, macOS,
    /// MinGW), as C or as C++.
    #[default]
    Double,
    /// stb_vorbis's own `3.14159265358979323846264f`, in a C program: as Microsoft's compiler
    /// builds it, unless `_USE_MATH_DEFINES` is set.
    Float,
    /// stb_vorbis's own `3.14159265358979323846264f`, in a C++ program (the file included in,
    /// or compiled as, C++): the angles of some tables are then floats, and C++ takes their
    /// `cos` and `sin` with the float functions (`cosf`, `sinf`) where C takes the double ones.
    /// The float functions are the system's, so this is stb_vorbis as built on the system this
    /// runs on.
    FloatCpp,
}

/// What a stream is (stb_vorbis_info, without its memory figures).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VorbisInfo {
    pub sample_rate: u32,
    pub channels: usize,
    /// The most samples a frame can give.
    pub max_frame_size: usize,
}

/// A decoder over an Ogg Vorbis file held in memory (stb_vorbis's `stb_vorbis *` from
/// `stb_vorbis_open_memory`).
pub struct Vorbis<'a> {
    pub(crate) io: Io<'a>,
    pub(crate) setup: Setup,
    pub(crate) p_last: ProbedPage,
    pub(crate) total_samples: u32,

    // decode buffer
    pub(crate) channel_buffers: Vec<Vec<f32>>,
    pub(crate) previous_window: Vec<Vec<f32>>,
    pub(crate) previous_length: i32,
    pub(crate) final_y: Vec<Vec<i16>>,
    pub(crate) mdct_scratch: Vec<f32>,
    /// The classifications of the parts of the residue being decoded, a row for each channel:
    /// kept from one packet to the next so that a packet allocates nothing.
    pub(crate) residue_scratch: Vec<usize>,

    /// sample location of next frame to decode
    pub(crate) current_loc: u32,
    pub(crate) current_loc_valid: bool,
    pub(crate) discard_samples_deferred: i32,
    pub(crate) samples_output: u32,

    /// The samples of the current frame not handed out yet: this range of each channel buffer.
    pub(crate) channel_buffer_start: i32,
    pub(crate) channel_buffer_end: i32,
}

impl<'a> Vorbis<'a> {
    /// Opens a whole Ogg Vorbis file (`stb_vorbis_open_memory`): reads its headers and gets
    /// ready to decode from its start.
    pub fn open(data: &'a [u8]) -> Result<Vorbis<'a>, VorbisError> {
        Vorbis::open_with_pi(data, Pi::default())
    }

    /// [`Vorbis::open`], for the samples of one build of stb_vorbis or the other.
    pub fn open_with_pi(data: &'a [u8], pi: Pi) -> Result<Vorbis<'a>, VorbisError> {
        // (stb_vorbis takes the length as an int, and keeps places in the file in 32 bits.)
        if data.len() > i32::MAX as usize {
            return Err(VorbisError::FileOpenFailure);
        }
        let mut io = Io::new(data);
        let setup = setup::start_decoder(&mut io, pi)?;
        let mut vorbis = Vorbis::assemble(io, setup);
        vorbis.vorbis_pump_first_frame();
        Ok(vorbis)
    }

    /// Opens an Ogg Vorbis stream that is a part of a file on disk
    /// (`stb_vorbis_open_file_section`): it starts where `file` is now and is `length` bytes
    /// long. The length is what looking for the end of the stream goes by (its length in
    /// samples, and seeking); decoding goes on for as long as there are pages.
    pub fn open_file_section<R: Stream + 'a>(file: R, length: u32) -> Result<Vorbis<'a>, VorbisError> {
        Vorbis::open_file_section_with_pi(file, length, Pi::default())
    }

    /// [`Vorbis::open_file_section`], for the samples of one build of stb_vorbis or another.
    pub fn open_file_section_with_pi<R: Stream + 'a>(file: R, length: u32, pi: Pi) -> Result<Vorbis<'a>, VorbisError> {
        // the stream starts at the reader's position 0
        let mut io = Io::new(&[]);
        io.file = Some(FileIo::new(Box::new(file), 0, length));
        let setup = setup::start_decoder(&mut io, pi)?;
        let mut vorbis = Vorbis::assemble(io, setup);
        vorbis.vorbis_pump_first_frame();
        Ok(vorbis)
    }

    /// The decoder for headers that have been read, with its buffers.
    pub(crate) fn assemble(io: Io<'a>, setup: Setup) -> Vorbis<'a> {
        let block = setup.blocksize[1];
        Vorbis {
            io,
            p_last: ProbedPage::default(),
            total_samples: 0,
            channel_buffers: vec![vec![0.0; block]; setup.channels],
            previous_window: vec![vec![0.0; block / 2]; setup.channels],
            previous_length: 0,
            final_y: vec![vec![0; setup.longest_floorlist.max(2)]; setup.channels],
            mdct_scratch: vec![0.0; block / 2],
            residue_scratch: Vec::new(),
            current_loc: 0,
            current_loc_valid: false,
            discard_samples_deferred: 0,
            samples_output: 0,
            channel_buffer_start: 0,
            channel_buffer_end: 0,
            setup,
        }
    }

    /// The same decoder over other bytes (see `Io::rebind`).
    pub(crate) fn rebind<'b>(self, data: &'b [u8]) -> Vorbis<'b> {
        Vorbis {
            io: self.io.rebind(data),
            setup: self.setup,
            p_last: self.p_last,
            total_samples: self.total_samples,
            channel_buffers: self.channel_buffers,
            previous_window: self.previous_window,
            previous_length: self.previous_length,
            final_y: self.final_y,
            mdct_scratch: self.mdct_scratch,
            residue_scratch: self.residue_scratch,
            current_loc: self.current_loc,
            current_loc_valid: self.current_loc_valid,
            discard_samples_deferred: self.discard_samples_deferred,
            samples_output: self.samples_output,
            channel_buffer_start: self.channel_buffer_start,
            channel_buffer_end: self.channel_buffer_end,
        }
    }

    /// `stb_vorbis_get_info`
    pub fn info(&self) -> VorbisInfo {
        VorbisInfo { sample_rate: self.setup.sample_rate, channels: self.setup.channels, max_frame_size: self.setup.blocksize[1] >> 1 }
    }

    /// The encoder's name from the comment header, as the file has it (usually UTF-8).
    pub fn vendor(&self) -> &[u8] {
        &self.setup.vendor
    }

    /// The comment header's "NAME=value" entries, as the file has them (usually UTF-8).
    pub fn comments(&self) -> &[Vec<u8>] {
        &self.setup.comments
    }

    /// The last error, which this forgets (`stb_vorbis_get_error`).
    pub fn take_error(&mut self) -> Option<VorbisError> {
        self.io.error.take()
    }

    /// The sample the next frame starts at, when that is known (`stb_vorbis_get_sample_offset`).
    pub fn sample_offset(&self) -> Option<u32> {
        self.current_loc_valid.then_some(self.current_loc)
    }

    /// Where in the file the decoder is (`stb_vorbis_get_file_offset`).
    pub fn file_offset(&self) -> u32 {
        self.io.file_offset()
    }

    /// Decodes the next frame (`stb_vorbis_get_frame_float`) and gives the number of samples
    /// in it for each channel; [`Vorbis::frame`] has them. 0 at the end of the stream; a frame
    /// can also hold no samples (the first one does), and the decoder then goes on.
    pub fn get_frame_float(&mut self) -> usize {
        let Some((len, left, right)) = self.vorbis_decode_packet() else {
            self.channel_buffer_start = 0;
            self.channel_buffer_end = 0;
            return 0;
        };
        let len = self.vorbis_finish_frame(len, left, right);
        self.channel_buffer_start = left;
        self.channel_buffer_end = left + len;
        len.max(0) as usize
    }

    /// A channel's samples of the current frame that have not been handed out yet.
    pub fn frame(&self, channel: usize) -> &[f32] {
        let (start, end) = self.frame_range();
        self.channel_buffers.get(channel).map_or(&[], |buffer| &buffer[start..end])
    }

    /// The part of the channel buffers that holds samples not handed out yet.
    fn frame_range(&self) -> (usize, usize) {
        let limit = self.setup.blocksize[1];
        let start = (self.channel_buffer_start.max(0) as usize).min(limit);
        let end = (self.channel_buffer_end.max(0) as usize).clamp(start, limit);
        (start, end)
    }

    /// How many samples of the current frame are still to hand out.
    fn frame_left(&self) -> usize {
        let (start, end) = self.frame_range();
        end - start
    }

    /// Decodes the next frame into 16-bit samples, a slice for each channel wanted
    /// (`stb_vorbis_get_frame_short`): at most as many samples as the shortest slice holds.
    /// Gives the number of samples written to each.
    pub fn get_frame_short(&mut self, buffers: &mut [&mut [i16]]) -> usize {
        let room = buffers.iter().map(|buffer| buffer.len()).min().unwrap_or(0);
        let len = self.get_frame_float().min(room);
        if len != 0 {
            let start = self.frame_range().0;
            samples::convert_samples_short(buffers, 0, self.setup.channels, &self.channel_buffers, start, len);
        }
        len
    }

    /// Decodes the next frame into interleaved 16-bit samples for `num_c` channels
    /// (`stb_vorbis_get_frame_short_interleaved`). Gives the number of samples per channel.
    pub fn get_frame_short_interleaved(&mut self, num_c: usize, buffer: &mut [i16]) -> usize {
        if num_c == 0 {
            return 0;
        }
        if num_c == 1 {
            return self.get_frame_short(&mut [buffer]);
        }
        let mut len = self.get_frame_float();
        if len != 0 {
            if len * num_c > buffer.len() {
                len = buffer.len() / num_c;
            }
            let start = self.frame_range().0;
            samples::convert_channels_short_interleaved(num_c, buffer, self.setup.channels, &self.channel_buffers, start, len);
        }
        len
    }

    /// Fills `buffer` with interleaved 16-bit samples for `channels` channels, decoding as
    /// many frames as that takes (`stb_vorbis_get_samples_short_interleaved`). Gives the
    /// number of samples per channel; less than asked at the end of the stream.
    pub fn get_samples_short_interleaved(&mut self, channels: usize, buffer: &mut [i16]) -> usize {
        if channels == 0 {
            return 0;
        }
        let len = buffer.len() / channels;
        let mut n = 0usize;
        while n < len {
            let k = self.frame_left().min(len - n);
            if k != 0 {
                let start = self.frame_range().0;
                samples::convert_channels_short_interleaved(channels, &mut buffer[n * channels..], self.setup.channels, &self.channel_buffers, start, k);
            }
            n += k;
            self.channel_buffer_start += k as i32;
            if n == len {
                break;
            }
            if !self.next_frame() {
                break;
            }
        }
        n
    }

    /// Fills a slice of 16-bit samples for each channel wanted (`stb_vorbis_get_samples_short`).
    /// Gives the number of samples written to each.
    pub fn get_samples_short(&mut self, buffers: &mut [&mut [i16]]) -> usize {
        let len = buffers.iter().map(|buffer| buffer.len()).min().unwrap_or(0);
        let mut n = 0usize;
        while n < len {
            let k = self.frame_left().min(len - n);
            if k != 0 {
                let start = self.frame_range().0;
                samples::convert_samples_short(buffers, n, self.setup.channels, &self.channel_buffers, start, k);
            }
            n += k;
            self.channel_buffer_start += k as i32;
            if n == len {
                break;
            }
            if !self.next_frame() {
                break;
            }
        }
        n
    }

    /// Fills `buffer` with interleaved float samples for `channels` channels
    /// (`stb_vorbis_get_samples_float_interleaved`); channels the stream doesn't have are
    /// silent, and ones it has beyond `channels` are left out. Gives the number of samples per
    /// channel.
    pub fn get_samples_float_interleaved(&mut self, channels: usize, buffer: &mut [f32]) -> usize {
        if channels == 0 {
            return 0;
        }
        let len = buffer.len() / channels;
        let z = self.setup.channels.min(channels);
        let mut n = 0usize;
        let mut at = 0usize;
        while n < len {
            let k = self.frame_left().min(len - n);
            let start = self.frame_range().0;
            for j in 0..k {
                for i in 0..z {
                    buffer[at] = self.channel_buffers[i][start + j];
                    at += 1;
                }
                for _ in z..channels {
                    buffer[at] = 0.0;
                    at += 1;
                }
            }
            n += k;
            self.channel_buffer_start += k as i32;
            if n == len {
                break;
            }
            if !self.next_frame() {
                break;
            }
        }
        n
    }

    /// Fills a slice of float samples for each channel wanted (`stb_vorbis_get_samples_float`).
    /// Gives the number of samples written to each.
    pub fn get_samples_float(&mut self, buffers: &mut [&mut [f32]]) -> usize {
        let num_samples = buffers.iter().map(|buffer| buffer.len()).min().unwrap_or(0);
        let z = self.setup.channels.min(buffers.len());
        let mut n = 0usize;
        while n < num_samples {
            let k = self.frame_left().min(num_samples - n);
            if k != 0 {
                let start = self.frame_range().0;
                for (i, buffer) in buffers.iter_mut().enumerate() {
                    if i < z {
                        buffer[n..n + k].copy_from_slice(&self.channel_buffers[i][start..start + k]);
                    } else {
                        buffer[n..n + k].fill(0.0);
                    }
                }
            }
            n += k;
            self.channel_buffer_start += k as i32;
            if n == num_samples {
                break;
            }
            if !self.next_frame() {
                break;
            }
        }
        n
    }

    /// stb_vorbis's `if (!stb_vorbis_get_frame_float(...)) break;`: decodes a frame and says
    /// whether to go on. A frame without samples ends the loop there too.
    fn next_frame(&mut self) -> bool {
        self.get_frame_float() != 0
    }
}

/// Decodes a whole file to interleaved 16-bit samples (`stb_vorbis_decode_memory`):
/// `(channels, sample rate, samples)`.
pub fn decode_memory(data: &[u8]) -> Result<(usize, u32, Vec<i16>), VorbisError> {
    decode_memory_with_pi(data, Pi::default())
}

/// [`decode_memory`], for the samples of one build of stb_vorbis or the other.
pub fn decode_memory_with_pi(data: &[u8], pi: Pi) -> Result<(usize, u32, Vec<i16>), VorbisError> {
    Ok(decode_all(Vorbis::open_with_pi(data, pi)?))
}

/// Everything a decoder has, from where it is to the end.
fn decode_all(mut vorbis: Vorbis) -> (usize, u32, Vec<i16>) {
    let info = vorbis.info();
    let limit = info.channels * 4096;
    let mut samples = vec![0i16; limit];
    let mut offset = 0usize;
    loop {
        let n = vorbis.get_frame_short_interleaved(info.channels, &mut samples[offset..]);
        if n == 0 {
            break;
        }
        offset += n * info.channels;
        if offset + limit > samples.len() {
            samples.resize(samples.len() * 2, 0);
        }
    }
    samples.truncate(offset);
    (info.channels, info.sample_rate, samples)
}
