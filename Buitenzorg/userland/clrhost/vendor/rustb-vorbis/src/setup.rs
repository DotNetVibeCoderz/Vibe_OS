//! The three Vorbis headers (stb_vorbis's start_decoder): identification, comments, and the
//! setup header with its codebooks, floors, residues, mappings and modes.
#[allow(unused_imports)]
use alloc::{boxed::Box, vec, vec::Vec};

use crate::codebook::{ilog, read_codebook, Codebook};
use crate::io::{Io, EOP};
use crate::mdct::BlockTables;
use crate::{Pi, VorbisError};

const VORBIS_PACKET_ID: i32 = 1;
const VORBIS_PACKET_COMMENT: i32 = 3;
const VORBIS_PACKET_SETUP: i32 = 5;

/// Floor type 1: a curve through points, drawn as line segments.
pub(crate) struct Floor1 {
    pub partitions: u8,
    pub partition_class_list: [u8; 32], // varies
    pub class_dimensions: [u8; 16],     // varies
    pub class_subclasses: [u8; 16],     // varies
    pub class_masterbooks: [u8; 16],    // varies
    pub subclass_books: [[i16; 8]; 16], // varies
    pub xlist: [u16; 31 * 8 + 2],       // varies
    pub sorted_order: [u8; 31 * 8 + 2],
    pub neighbors: [[u8; 2]; 31 * 8 + 2],
    pub floor1_multiplier: u8,
    pub rangebits: u8,
    pub values: i32,
}

pub(crate) struct Residue {
    pub begin: u32,
    pub end: u32,
    pub part_size: u32,
    // (Read from the file; decoding goes by classdata, made from it.)
    #[allow(dead_code)]
    pub classifications: u8,
    pub classbook: u8,
    pub classdata: Vec<Vec<u8>>,
    pub residue_books: Vec<[i16; 8]>,
    /// How many of the eight passes there is anything to decode in: the last one that any
    /// classification has a codebook for, and at least the first, which reads the
    /// classifications. stb_vorbis goes through all eight, finding nothing to do in the rest.
    pub passes: usize,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct MappingChannel {
    pub magnitude: u8,
    pub angle: u8,
    pub mux: u8,
}

pub(crate) struct Mapping {
    pub coupling_steps: u16,
    pub chan: Vec<MappingChannel>,
    pub submaps: u8,
    pub submap_floor: [u8; 15],   // varies
    pub submap_residue: [u8; 15], // varies
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Mode {
    pub blockflag: u8,
    pub mapping: u8,
}

/// What the headers say, and the tables made from it.
pub(crate) struct Setup {
    pub sample_rate: u32,
    pub channels: usize,
    pub vendor: Vec<u8>,
    pub comments: Vec<Vec<u8>>,
    pub blocksize: [usize; 2],
    pub codebooks: Vec<Codebook>,
    pub floors: Vec<Floor1>,
    pub residue_types: Vec<u16>,
    pub residues: Vec<Residue>,
    pub mappings: Vec<Mapping>,
    pub modes: Vec<Mode>,
    pub tables: [BlockTables; 2],
    /// The most points any floor has.
    pub longest_floorlist: usize,
    pub first_audio_page_offset: u32,
}

fn vorbis_validate(data: &[u8]) -> bool {
    data == b"vorbis"
}

/// only run while parsing the header (3 times)
fn neighbors(x: &[u16], n: usize) -> (u8, u8) {
    let mut low = -1i32;
    let mut high = 65536i32;
    let (mut plow, mut phigh) = (0u8, 0u8);
    for i in 0..n {
        let xi = i32::from(x[i]);
        if xi > low && xi < i32::from(x[n]) {
            plow = i as u8;
            low = xi;
        }
        if xi < high && xi > i32::from(x[n]) {
            phigh = i as u8;
            high = xi;
        }
    }
    (plow, phigh)
}

/// A length-prefixed string of the comment header.
///
/// stb_vorbis allocates the length the header gives, whatever it is, and past the end of the
/// packet fills it with bytes of 255. This keeps the bytes the packet has, and is in the same
/// place in the file afterwards.
fn read_comment(f: &mut Io) -> Result<Vec<u8>, VorbisError> {
    let len = f.get32_packet();
    // (stb_vorbis's allocation of len + 1 bytes fails.)
    if len < 0 || len == i32::MAX {
        return Err(VorbisError::OutOfMemory);
    }
    let mut text = Vec::new();
    for _ in 0..len {
        let byte = f.get8_packet();
        if byte == EOP {
            if f.last_seg {
                // The packet is over: every byte from here on would be the same.
                break;
            }
            continue;
        }
        text.push(byte as u8);
    }
    Ok(text)
}

fn read_floor(f: &mut Io, codebook_count: i32) -> Result<Floor1, VorbisError> {
    let mut g = Floor1 {
        partitions: 0,
        partition_class_list: [0; 32],
        class_dimensions: [0; 16],
        class_subclasses: [0; 16],
        class_masterbooks: [0; 16],
        subclass_books: [[0; 8]; 16],
        xlist: [0; 250],
        sorted_order: [0; 250],
        neighbors: [[0; 2]; 250],
        floor1_multiplier: 0,
        rangebits: 0,
        values: 0,
    };
    let mut max_class = -1i32;
    g.partitions = f.get_bits(5) as u8;
    for j in 0..usize::from(g.partitions) {
        g.partition_class_list[j] = f.get_bits(4) as u8;
        max_class = max_class.max(i32::from(g.partition_class_list[j]));
    }
    for j in 0..=max_class {
        let j = j as usize;
        g.class_dimensions[j] = f.get_bits(3) as u8 + 1;
        g.class_subclasses[j] = f.get_bits(2) as u8;
        if g.class_subclasses[j] != 0 {
            g.class_masterbooks[j] = f.get_bits(8) as u8;
            if i32::from(g.class_masterbooks[j]) >= codebook_count {
                return Err(VorbisError::InvalidSetup);
            }
        }
        for k in 0..(1usize << g.class_subclasses[j]) {
            g.subclass_books[j][k] = f.get_bits(8) as i16 - 1;
            if i32::from(g.subclass_books[j][k]) >= codebook_count {
                return Err(VorbisError::InvalidSetup);
            }
        }
    }
    g.floor1_multiplier = f.get_bits(2) as u8 + 1;
    g.rangebits = f.get_bits(4) as u8;
    g.xlist[0] = 0;
    g.xlist[1] = (1u32 << g.rangebits) as u16;
    g.values = 2;
    for j in 0..usize::from(g.partitions) {
        let c = usize::from(g.partition_class_list[j]);
        for _ in 0..g.class_dimensions[c] {
            g.xlist[g.values as usize] = f.get_bits(i32::from(g.rangebits)) as u16;
            g.values += 1;
        }
    }
    // precompute the sorting
    let values = g.values as usize;
    let mut points: Vec<(u16, u16)> = (0..values).map(|j| (g.xlist[j], j as u16)).collect();
    points.sort_by_key(|point| point.0);
    if points.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(VorbisError::InvalidSetup);
    }
    for (j, point) in points.iter().enumerate() {
        g.sorted_order[j] = point.1 as u8;
    }
    // precompute the neighbors
    for j in 2..values {
        let (low, high) = neighbors(&g.xlist, j);
        g.neighbors[j] = [low, high];
    }
    Ok(g)
}

fn read_residue(f: &mut Io, codebooks: &[Codebook]) -> Result<(u16, Residue), VorbisError> {
    let codebook_count = codebooks.len() as i32;
    let mut residue_cascade = [0u8; 64];
    let residue_type = f.get_bits(16) as u16;
    if residue_type > 2 {
        return Err(VorbisError::InvalidSetup);
    }
    let begin = f.get_bits(24);
    let end = f.get_bits(24);
    if end < begin {
        return Err(VorbisError::InvalidSetup);
    }
    let part_size = f.get_bits(24) + 1;
    let classifications = f.get_bits(6) as u8 + 1;
    let classbook = f.get_bits(8) as u8;
    if i32::from(classbook) >= codebook_count {
        return Err(VorbisError::InvalidSetup);
    }
    for cascade in residue_cascade.iter_mut().take(usize::from(classifications)) {
        let mut high_bits = 0u8;
        let low_bits = f.get_bits(3) as u8;
        if f.get_bits(1) != 0 {
            high_bits = f.get_bits(5) as u8;
        }
        *cascade = high_bits.wrapping_mul(8).wrapping_add(low_bits);
    }
    let mut residue_books = vec![[0i16; 8]; usize::from(classifications)];
    for (j, books) in residue_books.iter_mut().enumerate() {
        for (k, book) in books.iter_mut().enumerate() {
            if residue_cascade[j] & (1 << k) != 0 {
                *book = f.get_bits(8) as i16;
                if i32::from(*book) >= codebook_count {
                    return Err(VorbisError::InvalidSetup);
                }
            } else {
                *book = -1;
            }
        }
    }
    // precompute the classifications[] array to avoid inner-loop mod/divide
    // call it 'classdata' since we already have r->classifications
    let book = &codebooks[usize::from(classbook)];
    let classwords = book.dimensions.max(0) as usize;
    let mut classdata = Vec::with_capacity(book.entries.max(0) as usize);
    for j in 0..book.entries.max(0) {
        let mut temp = j;
        let mut row = vec![0u8; classwords];
        for k in (0..classwords).rev() {
            row[k] = (temp % i32::from(classifications)) as u8;
            temp /= i32::from(classifications);
        }
        classdata.push(row);
    }
    let passes = (1..8).rev().find(|&pass| residue_books.iter().any(|books| books[pass] >= 0)).map_or(1, |last| last + 1);
    Ok((residue_type, Residue { begin, end, part_size, classifications, classbook, classdata, residue_books, passes }))
}

fn read_mapping(f: &mut Io, channels: usize, floor_count: usize, residue_count: usize) -> Result<Mapping, VorbisError> {
    let mapping_type = f.get_bits(16);
    if mapping_type != 0 {
        return Err(VorbisError::InvalidSetup);
    }
    let mut m = Mapping { coupling_steps: 0, chan: vec![MappingChannel::default(); channels], submaps: 1, submap_floor: [0; 15], submap_residue: [0; 15] };
    if f.get_bits(1) != 0 {
        m.submaps = f.get_bits(4) as u8 + 1;
    } else {
        m.submaps = 1;
    }
    if f.get_bits(1) != 0 {
        m.coupling_steps = f.get_bits(8) as u16 + 1;
        if usize::from(m.coupling_steps) > channels {
            return Err(VorbisError::InvalidSetup);
        }
        for k in 0..usize::from(m.coupling_steps) {
            m.chan[k].magnitude = f.get_bits(ilog(channels as i32 - 1)) as u8;
            m.chan[k].angle = f.get_bits(ilog(channels as i32 - 1)) as u8;
            if usize::from(m.chan[k].magnitude) >= channels || usize::from(m.chan[k].angle) >= channels || m.chan[k].magnitude == m.chan[k].angle {
                return Err(VorbisError::InvalidSetup);
            }
        }
    } else {
        m.coupling_steps = 0;
    }

    // reserved field
    if f.get_bits(2) != 0 {
        return Err(VorbisError::InvalidSetup);
    }
    if m.submaps > 1 {
        for j in 0..channels {
            m.chan[j].mux = f.get_bits(4) as u8;
            if m.chan[j].mux >= m.submaps {
                return Err(VorbisError::InvalidSetup);
            }
        }
    } else {
        // @SPECIFICATION: this case is missing from the spec
        for j in 0..channels {
            m.chan[j].mux = 0;
        }
    }
    for j in 0..usize::from(m.submaps) {
        f.get_bits(8); // discard
        m.submap_floor[j] = f.get_bits(8) as u8;
        m.submap_residue[j] = f.get_bits(8) as u8;
        if usize::from(m.submap_floor[j]) >= floor_count || usize::from(m.submap_residue[j]) >= residue_count {
            return Err(VorbisError::InvalidSetup);
        }
    }
    Ok(m)
}

/// Reads the three headers, leaving the stream at the first audio packet.
pub(crate) fn start_decoder(f: &mut Io, pi: Pi) -> Result<Setup, VorbisError> {
    // first page, first packet
    f.first_decode = true;

    if !f.start_page() {
        return Err(f.error.unwrap_or(VorbisError::MissingCapturePattern));
    }
    // validate page flag
    if f.page_flag & 2 == 0 || f.page_flag & 4 != 0 || f.page_flag & 1 != 0 {
        return Err(VorbisError::InvalidFirstPage);
    }
    // check for expected packet length
    if f.segment_count != 1 {
        return Err(VorbisError::InvalidFirstPage);
    }
    if f.segments[0] != 30 {
        // check for the Ogg skeleton fishead identifying header to refine our error
        let mut header = [0u8; 6];
        if f.segments[0] == 64 && f.getn(&mut header) && &header == b"fishea" && f.get8() == b'd' && f.get8() == 0 {
            return Err(VorbisError::OggSkeletonNotSupported);
        }
        return Err(VorbisError::InvalidFirstPage);
    }

    // read packet
    // check packet header
    if i32::from(f.get8()) != VORBIS_PACKET_ID {
        return Err(VorbisError::InvalidFirstPage);
    }
    let mut header = [0u8; 6];
    if !f.getn(&mut header) {
        return Err(VorbisError::UnexpectedEof);
    }
    if !vorbis_validate(&header) {
        return Err(VorbisError::InvalidFirstPage);
    }
    // vorbis_version
    if f.get32() != 0 {
        return Err(VorbisError::InvalidFirstPage);
    }
    let channels = usize::from(f.get8());
    if channels == 0 {
        return Err(VorbisError::InvalidFirstPage);
    }
    if channels > crate::MAX_CHANNELS {
        return Err(VorbisError::TooManyChannels);
    }
    let sample_rate = f.get32();
    if sample_rate == 0 {
        return Err(VorbisError::InvalidFirstPage);
    }
    f.get32(); // bitrate_maximum
    f.get32(); // bitrate_nominal
    f.get32(); // bitrate_minimum
    let x = f.get8();
    let (log0, log1) = (x & 15, x >> 4);
    let blocksize = [1usize << log0, 1usize << log1];
    if !(6..=13).contains(&log0) || !(6..=13).contains(&log1) || log0 > log1 {
        return Err(VorbisError::InvalidSetup);
    }

    // framing_flag
    let x = f.get8();
    if x & 1 == 0 {
        return Err(VorbisError::InvalidFirstPage);
    }

    // second packet!
    if !f.start_page() || !f.start_packet() || f.next_segment() == 0 {
        return Err(f.error.unwrap_or(VorbisError::UnexpectedEof));
    }

    if f.get8_packet() != VORBIS_PACKET_COMMENT {
        return Err(VorbisError::InvalidSetup);
    }
    let header: Vec<u8> = (0..6).map(|_| f.get8_packet() as u8).collect();
    if !vorbis_validate(&header) {
        return Err(VorbisError::InvalidSetup);
    }
    // file vendor
    let vendor = read_comment(f)?;
    // user comments
    let comment_list_length = f.get32_packet();
    // (stb_vorbis's size for the list no longer fits its int: it fails to allocate, or
    // allocates too little.)
    if comment_list_length >= 1 << 28 {
        return Err(VorbisError::OutOfMemory);
    }
    let mut comments = Vec::new();
    for _ in 0..comment_list_length.max(0) {
        comments.push(read_comment(f)?);
    }

    // framing_flag
    let x = f.get8_packet();
    if x & 1 == 0 {
        return Err(VorbisError::InvalidSetup);
    }

    f.skip(i64::from(f.bytes_in_seg));
    f.bytes_in_seg = 0;

    loop {
        let len = f.next_segment();
        f.skip(i64::from(len));
        f.bytes_in_seg = 0;
        if len == 0 {
            break;
        }
    }

    // third packet!
    if !f.start_packet() {
        return Err(f.error.unwrap_or(VorbisError::UnexpectedEof));
    }

    if f.push_mode && !f.is_whole_packet_present(0) {
        // convert error in ogg header to write type
        if f.error == Some(VorbisError::InvalidStream) {
            f.error = Some(VorbisError::InvalidSetup);
        }
        return Err(f.error.unwrap_or(VorbisError::InvalidSetup));
    }

    if f.get8_packet() != VORBIS_PACKET_SETUP {
        return Err(VorbisError::InvalidSetup);
    }
    let header: Vec<u8> = (0..6).map(|_| f.get8_packet() as u8).collect();
    if !vorbis_validate(&header) {
        return Err(VorbisError::InvalidSetup);
    }

    // codebooks
    let codebook_count = f.get_bits(8) as usize + 1;
    let mut codebooks = Vec::with_capacity(codebook_count);
    for i in 0..codebook_count {
        codebooks.push(read_codebook(f, i + 1 == codebook_count)?);
    }

    // time domain transfers (notused)
    let x = (f.get_bits(6) as u8).wrapping_add(1);
    for _ in 0..x {
        if f.get_bits(16) != 0 {
            return Err(VorbisError::InvalidSetup);
        }
    }

    // Floors
    let floor_count = f.get_bits(6) as usize + 1;
    let mut floors = Vec::with_capacity(floor_count);
    let mut longest_floorlist = 0usize;
    for _ in 0..floor_count {
        let floor_type = f.get_bits(16);
        if floor_type > 1 {
            return Err(VorbisError::InvalidSetup);
        }
        if floor_type == 0 {
            // (Floor 0 has not been written by an encoder since 2001; stb_vorbis reads its
            // header and stops.)
            return Err(VorbisError::FeatureNotSupported);
        }
        let floor = read_floor(f, codebook_count as i32)?;
        longest_floorlist = longest_floorlist.max(floor.values as usize);
        floors.push(floor);
    }

    // Residue
    let residue_count = f.get_bits(6) as usize + 1;
    let mut residue_types = Vec::with_capacity(residue_count);
    let mut residues = Vec::with_capacity(residue_count);
    for _ in 0..residue_count {
        let (residue_type, residue) = read_residue(f, &codebooks)?;
        residue_types.push(residue_type);
        residues.push(residue);
    }

    let mapping_count = f.get_bits(6) as usize + 1;
    let mut mappings = Vec::with_capacity(mapping_count);
    for _ in 0..mapping_count {
        mappings.push(read_mapping(f, channels, floor_count, residue_count)?);
    }

    // Modes
    let mode_count = f.get_bits(6) as usize + 1;
    let mut modes = Vec::with_capacity(mode_count);
    for _ in 0..mode_count {
        let blockflag = f.get_bits(1) as u8;
        let windowtype = f.get_bits(16);
        let transformtype = f.get_bits(16);
        let mapping = f.get_bits(8) as u8;
        if windowtype != 0 || transformtype != 0 || usize::from(mapping) >= mapping_count {
            return Err(VorbisError::InvalidSetup);
        }
        modes.push(Mode { blockflag, mapping });
    }

    f.flush_packet();

    let tables = [BlockTables::new(blocksize[0], pi), BlockTables::new(blocksize[1], pi)];

    let first_audio_page_offset = if f.next_seg == -1 {
        f.file_offset()
    } else {
        0
    };

    Ok(Setup { sample_rate, channels, vendor, comments, blocksize, codebooks, floors, residue_types, residues, mappings, modes, tables, longest_floorlist, first_audio_page_offset })
}
