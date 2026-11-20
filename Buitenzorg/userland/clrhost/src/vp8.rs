//! VP8 video decoder (RFC 6386): a Rust port of the RFC's reference decoder
//! "dixie" (Attachment One; Copyright (c) 2010, 2011 Google Inc., BSD-style
//! licence -- see `vendor/NOTICE-vp8.md`), keyframes and inter frames: the
//! boolean decoder, frame / segment / loop-filter / quantizer / reference /
//! entropy headers, per-macroblock modes and motion vectors (near-MV search,
//! split MVs), coefficient tokens, the inverse WHT and DCT, intra prediction
//! (16x16, 8x8 chroma, 4x4 sub-blocks), inter prediction (six-tap or bilinear
//! sub-pixel filters, emulated edges), the normal and simple loop filters, and
//! the last / golden / altref reference frames with entropy save / restore.
//!
//! Frames are kept with a 16-pixel border (8 for chroma) like dixie, because
//! intra prediction reads its out-of-frame neighbours (127 / 129) from there.
//! Same arithmetic as the C, including the 16-bit wrap of stored coefficients.

use alloc::vec;
use alloc::vec::Vec;

use crate::vp8_tables::*;

const BORDER: usize = 16;

// modes
const DC_PRED: u8 = 0;
const V_PRED: u8 = 1;
const H_PRED: u8 = 2;
const TM_PRED: u8 = 3;
const B_PRED: u8 = 4;
const NEARESTMV: u8 = 5;
const NEARMV: u8 = 6;
const ZEROMV: u8 = 7;
const NEWMV: u8 = 8;
const SPLITMV: u8 = 9;
// 4x4 modes
const B_DC_PRED: u8 = 0;
const B_TM_PRED: u8 = 1;
const B_VE_PRED: u8 = 2;
const B_HE_PRED: u8 = 3;
const B_LD_PRED: u8 = 4;
const B_RD_PRED: u8 = 5;
const B_VR_PRED: u8 = 6;
const B_VL_PRED: u8 = 7;
const B_HD_PRED: u8 = 8;
const B_HU_PRED: u8 = 9;
const LEFT4X4: u8 = 10;
const ABOVE4X4: u8 = 11;
const ZERO4X4: u8 = 12;
const NEW4X4: u8 = 13;

// reference frames
const CURRENT: usize = 0;
const LAST: usize = 1;
const GOLDEN: usize = 2;
const ALTREF: usize = 3;

const KF_Y_MODE_PROBS: [u8; 4] = [145, 156, 163, 128];
const KF_UV_MODE_PROBS: [u8; 3] = [142, 114, 183];
const DEFAULT_Y_MODE_PROBS: [u8; 4] = [112, 86, 140, 37];
const DEFAULT_UV_MODE_PROBS: [u8; 3] = [162, 101, 204];
const DEFAULT_B_MODE_PROBS: [u8; 9] = [120, 90, 79, 133, 87, 85, 80, 111, 151];
const MV_COUNTS_TO_PROBS: [[u8; 4]; 6] = [[7, 1, 1, 143], [14, 18, 14, 107], [135, 64, 57, 68], [60, 56, 128, 65], [159, 134, 128, 34], [234, 188, 128, 28]];
const SPLIT_MV_PROBS: [u8; 3] = [110, 111, 150];
const SUBMV_REF_PROBS2: [[u8; 3]; 5] = [[147, 136, 18], [106, 145, 1], [179, 121, 1], [223, 1, 34], [208, 1, 1]];
const MV_PARTITIONS: [[u8; 16]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1],
    [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1],
    [0, 0, 1, 1, 0, 0, 1, 1, 2, 2, 3, 3, 2, 2, 3, 3],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
];

// trees: leaves are negated values (-0 == 0 stops too, as in the C `> 0` test)
const KF_Y_MODE_TREE: [i32; 8] = [-(B_PRED as i32), 2, 4, 6, -(DC_PRED as i32), -(V_PRED as i32), -(H_PRED as i32), -(TM_PRED as i32)];
const Y_MODE_TREE: [i32; 8] = [-(DC_PRED as i32), 2, 4, 6, -(V_PRED as i32), -(H_PRED as i32), -(TM_PRED as i32), -(B_PRED as i32)];
const UV_MODE_TREE: [i32; 6] = [-(DC_PRED as i32), 2, -(V_PRED as i32), 4, -(H_PRED as i32), -(TM_PRED as i32)];
const B_MODE_TREE: [i32; 18] = [
    -(B_DC_PRED as i32),
    2,
    -(B_TM_PRED as i32),
    4,
    -(B_VE_PRED as i32),
    6,
    8,
    12,
    -(B_HE_PRED as i32),
    10,
    -(B_RD_PRED as i32),
    -(B_VR_PRED as i32),
    -(B_LD_PRED as i32),
    14,
    -(B_VL_PRED as i32),
    16,
    -(B_HD_PRED as i32),
    -(B_HU_PRED as i32),
];
const SMALL_MV_TREE: [i32; 14] = [2, 8, 4, 6, 0, -1, -2, -3, 10, 12, -4, -5, -6, -7];
const MV_REF_TREE: [i32; 8] = [-(ZEROMV as i32), 2, -(NEARESTMV as i32), 4, -(NEARMV as i32), 6, -(NEWMV as i32), -(SPLITMV as i32)];
const SUBMV_REF_TREE: [i32; 6] = [-(LEFT4X4 as i32), 2, -(ABOVE4X4 as i32), 4, -(ZERO4X4 as i32), -(NEW4X4 as i32)];
const SPLIT_MV_TREE: [i32; 6] = [-3, 2, -2, 4, 0, -1];

const SIXTAP: [[i32; 6]; 8] = [
    [0, 0, 128, 0, 0, 0],
    [0, -6, 123, 12, -1, 0],
    [2, -11, 108, 36, -8, 1],
    [0, -9, 93, 50, -6, 0],
    [3, -16, 77, 77, -16, 3],
    [0, -6, 50, 93, -9, 0],
    [1, -8, 36, 108, -11, 2],
    [0, -1, 12, 123, -6, 0],
];
const BILINEAR: [[i32; 6]; 8] = [
    [0, 0, 128, 0, 0, 0],
    [0, 0, 112, 16, 0, 0],
    [0, 0, 96, 32, 0, 0],
    [0, 0, 80, 48, 0, 0],
    [0, 0, 64, 64, 0, 0],
    [0, 0, 48, 80, 0, 0],
    [0, 0, 32, 96, 0, 0],
    [0, 0, 16, 112, 0, 0],
];

const LEFT_CTX_INDEX: [usize; 25] = [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8];
const ABOVE_CTX_INDEX: [usize; 25] = [0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3, 4, 5, 4, 5, 6, 7, 6, 7, 8];
const BANDS: [usize; 16] = [0, 1, 2, 3, 6, 4, 5, 6, 6, 6, 6, 6, 6, 6, 6, 7];
const ZIGZAG: [usize; 16] = [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];
const CAT_MIN: [i32; 6] = [5, 7, 11, 19, 35, 67];
const CAT_LEN: [i32; 6] = [0, 1, 2, 3, 4, 10];
const CAT_PROBS: [[u8; 11]; 6] = [
    [159, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [145, 165, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [140, 148, 173, 0, 0, 0, 0, 0, 0, 0, 0],
    [135, 140, 155, 176, 0, 0, 0, 0, 0, 0, 0],
    [130, 134, 141, 157, 180, 0, 0, 0, 0, 0, 0],
    [129, 130, 133, 140, 153, 177, 196, 230, 243, 254, 254],
];

// ---- boolean decoder ---------------------------------------------------------

#[derive(Clone, Default)]
struct Bool {
    data: Vec<u8>,
    pos: usize,
    range: u32,
    value: u32,
    bit_count: i32,
}

impl Bool {
    fn new(d: &[u8]) -> Bool {
        let mut b = Bool { data: d.to_vec(), pos: 0, range: 255, value: 0, bit_count: 0 };
        if d.len() >= 2 {
            b.value = (d[0] as u32) << 8 | d[1] as u32;
            b.pos = 2;
        } else {
            b.pos = d.len();
        }
        b
    }

    #[inline]
    fn get(&mut self, prob: u8) -> bool {
        let split = 1 + (((self.range - 1) * prob as u32) >> 8);
        let big = split << 8;
        let bit = if self.value >= big {
            self.range -= split;
            self.value -= big;
            true
        } else {
            self.range = split;
            false
        };
        while self.range < 128 {
            self.value <<= 1;
            self.range <<= 1;
            self.bit_count += 1;
            if self.bit_count == 8 {
                self.bit_count = 0;
                if self.pos < self.data.len() {
                    self.value |= self.data[self.pos] as u32;
                    self.pos += 1;
                }
            }
        }
        bit
    }

    fn bit(&mut self) -> bool {
        self.get(128)
    }

    fn uint(&mut self, bits: u32) -> i32 {
        let mut z = 0i32;
        for b in (0..bits).rev() {
            z |= (self.bit() as i32) << b;
        }
        z
    }

    fn int(&mut self, bits: u32) -> i32 {
        let z = self.uint(bits);
        if self.bit() {
            -z
        } else {
            z
        }
    }

    fn maybe_int(&mut self, bits: u32) -> i32 {
        if self.bit() {
            self.int(bits)
        } else {
            0
        }
    }

    fn tree(&mut self, t: &[i32], p: &[u8]) -> u8 {
        let mut i = 0i32;
        loop {
            i = t[(i + self.get(p[(i >> 1) as usize]) as i32) as usize];
            if i <= 0 {
                return (-i) as u8;
            }
        }
    }
}

// ---- state -------------------------------------------------------------------

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Mv {
    x: i16,
    y: i16,
}

impl Mv {
    fn is_zero(self) -> bool {
        self.x == 0 && self.y == 0
    }
}

#[derive(Clone, Copy, Default)]
struct MbInfo {
    y_mode: u8,
    uv_mode: u8,
    segment_id: u8,
    ref_frame: u8,
    skip_coeff: bool,
    need_mc_border: bool,
    partitioning: u8,
    mv: Mv,
    eob_mask: u32,
    mvs: [Mv; 16],
    modes: [u8; 16],
}

#[derive(Clone, Copy, Default)]
struct Segment {
    enabled: bool,
    update_data: bool,
    update_map: bool,
    abs: bool,
    tree_probs: [u8; 3],
    lf_level: [i32; 4],
    quant_idx: [i32; 4],
}

#[derive(Clone, Copy, Default)]
struct LoopFilter {
    use_simple: bool,
    level: i32,
    sharpness: i32,
    delta_enabled: bool,
    ref_delta: [i32; 4],
    mode_delta: [i32; 4],
}

#[derive(Clone, Copy, Default)]
struct Quant {
    q_index: i32,
    delta_update: bool,
    y1_dc: i32,
    y2_dc: i32,
    y2_ac: i32,
    uv_dc: i32,
    uv_ac: i32,
}

#[derive(Clone, Copy, Default)]
struct RefHdr {
    refresh_last: bool,
    refresh_gf: bool,
    refresh_arf: bool,
    copy_gf: i32,
    copy_arf: i32,
    sign_bias: [bool; 4],
    refresh_entropy: bool,
}

#[derive(Clone)]
struct Entropy {
    coeff: [u8; 1056],
    mv: [[u8; 19]; 2],
    skip_enabled: bool,
    skip_prob: u8,
    y_mode: [u8; 4],
    uv_mode: [u8; 3],
    prob_inter: u8,
    prob_last: u8,
    prob_gf: u8,
}

impl Default for Entropy {
    fn default() -> Entropy {
        Entropy { coeff: [0; 1056], mv: [[0; 19]; 2], skip_enabled: false, skip_prob: 0, y_mode: [0; 4], uv_mode: [0; 3], prob_inter: 0, prob_last: 0, prob_gf: 0 }
    }
}

#[derive(Clone, Copy, Default)]
struct Dequant {
    quant_idx: i32,
    // [Y1, UV, Y2][DC, AC]
    f: [[i32; 2]; 3],
}

/// A frame with borders: planes and their strides; `origin` = index of (0, 0).
#[derive(Clone, Default)]
pub struct Image {
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    pub ys: usize,
    pub uvs: usize,
    pub yo: usize,
    pub uvo: usize,
}

impl Image {
    fn new(mb_cols: usize, mb_rows: usize) -> Image {
        let w = mb_cols * 16 + BORDER * 2;
        let h = mb_rows * 16 + BORDER * 2;
        let (cw, ch) = (w / 2, h / 2);
        Image { y: vec![0; w * h], u: vec![0; cw * ch], v: vec![0; cw * ch], ys: w, uvs: cw, yo: BORDER * w + BORDER, uvo: BORDER / 2 * cw + BORDER / 2 }
    }
}

pub struct Vp8 {
    pub width: usize,
    pub height: usize,
    pub key: bool,
    pub shown: bool,
    version: u32,
    mb_rows: usize,
    mb_cols: usize,
    seg: Segment,
    lf: LoopFilter,
    quant: Quant,
    refh: RefHdr,
    ent: Entropy,
    saved: Entropy,
    mbi: Vec<MbInfo>,
    above_ctx: Vec<[u8; 9]>,
    dqf: [Dequant; 4],
    frames: [Image; 4],
    refcnt: [u32; 4],
    refs: [Option<usize>; 4],
    parts: Vec<Bool>,
    coeffs: Vec<i16>,
    emul: Vec<u8>,
    frame_count: u64,
}

impl Default for Vp8 {
    fn default() -> Self {
        Self::new()
    }
}

impl Vp8 {
    pub fn new() -> Vp8 {
        let mut dqf = [Dequant::default(); 4];
        for d in dqf.iter_mut() {
            d.quant_idx = -1;
        }
        Vp8 {
            width: 0,
            height: 0,
            key: false,
            shown: false,
            version: 0,
            mb_rows: 0,
            mb_cols: 0,
            seg: Segment::default(),
            lf: LoopFilter::default(),
            quant: Quant::default(),
            refh: RefHdr::default(),
            ent: Entropy::default(),
            saved: Entropy::default(),
            mbi: Vec::new(),
            above_ctx: Vec::new(),
            dqf,
            frames: [Image::default(), Image::default(), Image::default(), Image::default()],
            refcnt: [0; 4],
            refs: [None; 4],
            parts: Vec::new(),
            coeffs: Vec::new(),
            emul: Vec::new(),
            frame_count: 0,
        }
    }

    /// Index of macroblock info (row, col); row / col may be -1 (the border).
    #[inline]
    fn mi(&self, row: isize, col: isize) -> usize {
        (1 + (row + 1) * (self.mb_cols as isize + 1) + col) as usize
    }

    /// The decoded picture (after `decode`): the current frame.
    pub fn image(&self) -> Option<&Image> {
        self.refs[CURRENT].map(|i| &self.frames[i])
    }

    /// Decode one compressed frame. Err for a corrupt or unsupported one.
    pub fn decode(&mut self, data: &[u8]) -> Result<(), &'static str> {
        if data.len() < 10 {
            return Err("vp8: frame too short");
        }
        let raw = data[0] as u32 | (data[1] as u32) << 8 | (data[2] as u32) << 16;
        let key = raw & 1 == 0;
        let version = (raw >> 1) & 3;
        let experimental = (raw >> 3) & 1 != 0;
        let shown = (raw >> 4) & 1 != 0;
        let part0 = (raw >> 5) as usize;
        if data.len() <= part0 + if key { 10 } else { 3 } {
            return Err("vp8: truncated frame");
        }
        if experimental {
            return Err("vp8: experimental bitstream");
        }
        let mut size_updated = false;
        let mut off = 3;
        if key {
            if data[3] != 0x9d || data[4] != 0x01 || data[5] != 0x2a {
                return Err("vp8: bad keyframe start code");
            }
            let r = data[6] as u32 | (data[7] as u32) << 8 | (data[8] as u32) << 16 | (data[9] as u32) << 24;
            let (w, h) = ((r & 0x3FFF) as usize, ((r >> 16) & 0x3FFF) as usize);
            if w == 0 || h == 0 {
                return Err("vp8: zero size");
            }
            if w != self.width || h != self.height || self.mbi.is_empty() {
                size_updated = true;
            }
            self.width = w;
            self.height = h;
            off = 10;
            self.mb_cols = w.div_ceil(16);
            self.mb_rows = h.div_ceil(16);
        } else if self.mbi.is_empty() || self.refs[LAST].is_none() {
            return Err("vp8: inter frame before a keyframe");
        }
        self.key = key;
        self.shown = shown;
        self.version = version;
        let rest = &data[off..];
        let mut b = Bool::new(&rest[..part0]);
        if key && b.uint(2) != 0 {
            return Err("vp8: reserved colour space / clamping bits");
        }
        // segmentation
        if key {
            self.seg = Segment::default();
        }
        self.seg.enabled = b.bit();
        if self.seg.enabled {
            self.seg.update_map = b.bit();
            self.seg.update_data = b.bit();
            if self.seg.update_data {
                self.seg.abs = b.bit();
                for i in 0..4 {
                    self.seg.quant_idx[i] = b.maybe_int(7);
                }
                for i in 0..4 {
                    self.seg.lf_level[i] = b.maybe_int(6);
                }
            }
            if self.seg.update_map {
                for i in 0..3 {
                    self.seg.tree_probs[i] = if b.bit() { b.uint(8) as u8 } else { 255 };
                }
            }
        } else {
            self.seg.update_map = false;
            self.seg.update_data = false;
        }
        // loop filter
        if key {
            self.lf = LoopFilter::default();
        }
        self.lf.use_simple = b.bit();
        self.lf.level = b.uint(6);
        self.lf.sharpness = b.uint(3);
        self.lf.delta_enabled = b.bit();
        if self.lf.delta_enabled && b.bit() {
            for i in 0..4 {
                self.lf.ref_delta[i] = b.maybe_int(6);
            }
            for i in 0..4 {
                self.lf.mode_delta[i] = b.maybe_int(6);
            }
        }
        // token partitions
        let parts = 1usize << b.uint(2);
        let mut sz = rest.len() - part0;
        let mut p = part0;
        if sz < 3 * (parts - 1) {
            return Err("vp8: truncated partition sizes");
        }
        let sizes_at = p;
        sz -= 3 * (parts - 1);
        p += 3 * (parts - 1);
        self.parts.clear();
        for i in 0..parts {
            let len = if i < parts - 1 {
                let s = sizes_at + 3 * i;
                (rest[s] as usize) | (rest[s + 1] as usize) << 8 | (rest[s + 2] as usize) << 16
            } else {
                sz
            };
            if sz < len {
                return Err("vp8: truncated partition");
            }
            self.parts.push(Bool::new(&rest[p..p + len]));
            p += len;
            sz -= len;
        }
        // quantizer
        let last_q = self.quant.q_index;
        self.quant.q_index = b.uint(7);
        let mut upd = last_q != self.quant.q_index;
        self.quant.y1_dc = b.maybe_int(4);
        upd |= self.quant.y1_dc != 0;
        self.quant.y2_dc = b.maybe_int(4);
        upd |= self.quant.y2_dc != 0;
        self.quant.y2_ac = b.maybe_int(4);
        upd |= self.quant.y2_ac != 0;
        self.quant.uv_dc = b.maybe_int(4);
        upd |= self.quant.uv_dc != 0;
        self.quant.uv_ac = b.maybe_int(4);
        upd |= self.quant.uv_ac != 0;
        self.quant.delta_update = upd;
        // references
        let r = &mut self.refh;
        r.refresh_gf = if key { true } else { b.bit() };
        r.refresh_arf = if key { true } else { b.bit() };
        r.copy_gf = if key || r.refresh_gf { 0 } else { b.uint(2) };
        r.copy_arf = if key || r.refresh_arf { 0 } else { b.uint(2) };
        r.sign_bias[GOLDEN] = if key { false } else { b.bit() };
        r.sign_bias[ALTREF] = if key { false } else { b.bit() };
        r.refresh_entropy = b.bit();
        r.refresh_last = if key { true } else { b.bit() };
        // entropy
        if key {
            self.ent.coeff = DEFAULT_COEFF_PROBS;
            for i in 0..2 {
                self.ent.mv[i].copy_from_slice(&DEFAULT_MV_PROBS[i * 19..i * 19 + 19]);
            }
            self.ent.y_mode = DEFAULT_Y_MODE_PROBS;
            self.ent.uv_mode = DEFAULT_UV_MODE_PROBS;
        }
        let saved = !self.refh.refresh_entropy;
        if saved {
            self.saved = self.ent.clone();
        }
        for i in 0..1056 {
            if b.get(COEFF_UPDATE_PROBS[i]) {
                self.ent.coeff[i] = b.uint(8) as u8;
            }
        }
        self.ent.skip_enabled = b.bit();
        if self.ent.skip_enabled {
            self.ent.skip_prob = b.uint(8) as u8;
        }
        if !key {
            self.ent.prob_inter = b.uint(8) as u8;
            self.ent.prob_last = b.uint(8) as u8;
            self.ent.prob_gf = b.uint(8) as u8;
            if b.bit() {
                for i in 0..4 {
                    self.ent.y_mode[i] = b.uint(8) as u8;
                }
            }
            if b.bit() {
                for i in 0..3 {
                    self.ent.uv_mode[i] = b.uint(8) as u8;
                }
            }
            for i in 0..2 {
                for j in 0..19 {
                    if b.get(MV_UPDATE_PROBS[i * 19 + j]) {
                        let x = b.uint(7);
                        self.ent.mv[i][j] = if x != 0 { (x << 1) as u8 } else { 1 };
                    }
                }
            }
        }
        // storage
        if size_updated {
            self.mbi = vec![MbInfo::default(); (self.mb_cols + 1) * (self.mb_rows + 1) + 1];
            self.above_ctx = vec![[0; 9]; self.mb_cols];
            for i in 0..4 {
                self.frames[i] = Image::new(self.mb_cols, self.mb_rows);
                self.refcnt[i] = 0;
                self.refs[i] = None;
            }
            self.coeffs = vec![0; self.mb_cols * 25 * 16];
        }
        // a free frame to predict into
        if let Some(c) = self.refs[CURRENT] {
            self.refcnt[c] -= 1;
        }
        let free = (0..4).find(|&i| self.refcnt[i] == 0).ok_or("vp8: no free frame")?;
        self.refcnt[free] = 1;
        self.refs[CURRENT] = Some(free);
        self.dequant_init();

        for row in 0..self.mb_rows {
            let part = row % parts;
            self.modemv_row(&mut b, row);
            self.tokens_row(part, row);
            self.predict_row(row);
            if self.lf.level != 0 && row > 0 {
                self.loopfilter_row(row - 1);
            }
        }
        if self.lf.level != 0 {
            self.loopfilter_row(self.mb_rows - 1);
        }
        self.frame_count += 1;
        if saved {
            self.ent = self.saved.clone();
        }
        // reference updates
        let rh = self.refh;
        if rh.copy_arf == 1 {
            self.set_ref(ALTREF, LAST);
        } else if rh.copy_arf == 2 {
            self.set_ref(ALTREF, GOLDEN);
        }
        if rh.copy_gf == 1 {
            self.set_ref(GOLDEN, LAST);
        } else if rh.copy_gf == 2 {
            self.set_ref(GOLDEN, ALTREF);
        }
        if rh.refresh_gf {
            self.set_ref(GOLDEN, CURRENT);
        }
        if rh.refresh_arf {
            self.set_ref(ALTREF, CURRENT);
        }
        if rh.refresh_last {
            self.set_ref(LAST, CURRENT);
        }
        Ok(())
    }

    fn set_ref(&mut self, to: usize, from: usize) {
        if let Some(i) = self.refs[to] {
            self.refcnt[i] -= 1;
        }
        self.refs[to] = self.refs[from];
        if let Some(i) = self.refs[to] {
            self.refcnt[i] += 1;
        }
    }

    fn dequant_init(&mut self) {
        let n = if self.seg.enabled { 4 } else { 1 };
        for i in 0..n {
            let mut q = self.quant.q_index;
            if self.seg.enabled {
                q = if !self.seg.abs { q + self.seg.quant_idx[i] } else { self.seg.quant_idx[i] };
            }
            let d = &mut self.dqf[i];
            if d.quant_idx != q || self.quant.delta_update {
                let dc = |q: i32| DC_Q[q.clamp(0, 127) as usize];
                let ac = |q: i32| AC_Q[q.clamp(0, 127) as usize];
                d.f[0][0] = dc(q + self.quant.y1_dc);
                d.f[0][1] = ac(q);
                d.f[1][0] = dc(q + self.quant.uv_dc);
                d.f[1][1] = ac(q + self.quant.uv_ac);
                d.f[2][0] = dc(q + self.quant.y2_dc) * 2;
                d.f[2][1] = ac(q + self.quant.y2_ac) * 155 / 100;
                if d.f[2][1] < 8 {
                    d.f[2][1] = 8;
                }
                if d.f[1][0] > 132 {
                    d.f[1][0] = 132;
                }
                d.quant_idx = q;
            }
        }
    }

    // ---- modes and motion vectors ---------------------------------------------

    fn modemv_row(&mut self, b: &mut Bool, row: usize) {
        let cols = self.mb_cols;
        // eighth-pel MV bounds with a 1 MB border
        let mut to_left = -(1i32 << 7);
        let mut to_right = (cols as i32) << 7;
        let to_top = -(((row + 1) as i32) << 7);
        let to_bottom = ((self.mb_rows - row) as i32) << 7;
        for col in 0..cols {
            let me = self.mi(row as isize, col as isize);
            if self.seg.update_map {
                let tp = self.seg.tree_probs;
                self.mbi[me].segment_id = if b.get(tp[0]) { 2 + b.get(tp[2]) as u8 } else { b.get(tp[1]) as u8 };
            }
            if self.ent.skip_enabled {
                self.mbi[me].skip_coeff = b.get(self.ent.skip_prob);
            } else {
                // dixie leaves the flag as it was; a frame without the skip
                // flag codes every macroblock's tokens
                self.mbi[me].skip_coeff = false;
            }
            if self.key {
                if !self.seg.update_map {
                    self.mbi[me].segment_id = 0;
                }
                self.decode_kf_mode(b, row, col);
            } else {
                if b.get(self.ent.prob_inter) {
                    self.decode_mvs(b, row, col, [to_left, to_right, to_top, to_bottom]);
                } else {
                    self.decode_intra_mode(b, me);
                }
                to_left -= 16 << 3;
                to_right -= 16 << 3;
            }
        }
    }

    fn above_block_mode(&self, me: usize, above: usize, b: usize) -> u8 {
        if b < 4 {
            let a = &self.mbi[above];
            return match a.y_mode {
                DC_PRED => B_DC_PRED,
                V_PRED => B_VE_PRED,
                H_PRED => B_HE_PRED,
                TM_PRED => B_TM_PRED,
                B_PRED => a.modes[b + 12],
                _ => B_DC_PRED,
            };
        }
        self.mbi[me].modes[b - 4]
    }

    fn left_block_mode(&self, me: usize, left: usize, b: usize) -> u8 {
        if b & 3 == 0 {
            let l = &self.mbi[left];
            return match l.y_mode {
                DC_PRED => B_DC_PRED,
                V_PRED => B_VE_PRED,
                H_PRED => B_HE_PRED,
                TM_PRED => B_TM_PRED,
                B_PRED => l.modes[b + 3],
                _ => B_DC_PRED,
            };
        }
        self.mbi[me].modes[b - 1]
    }

    fn decode_kf_mode(&mut self, b: &mut Bool, row: usize, col: usize) {
        let me = self.mi(row as isize, col as isize);
        let above = self.mi(row as isize - 1, col as isize);
        let left = me - 1;
        let y_mode = b.tree(&KF_Y_MODE_TREE, &KF_Y_MODE_PROBS);
        if y_mode == B_PRED {
            // the contexts read y_mode of this block: set it first (dixie sets it after,
            // but only reads `this->split.modes` for b >= 4 / b & 3 != 0)
            for i in 0..16 {
                let a = self.above_block_mode(me, above, i) as usize;
                let l = self.left_block_mode(me, left, i) as usize;
                let probs = &KF_B_MODE_PROBS[(a * 10 + l) * 9..(a * 10 + l) * 9 + 9];
                self.mbi[me].modes[i] = b.tree(&B_MODE_TREE, probs);
            }
        }
        let uv_mode = b.tree(&UV_MODE_TREE, &KF_UV_MODE_PROBS);
        let m = &mut self.mbi[me];
        m.y_mode = y_mode;
        m.uv_mode = uv_mode;
        m.mv = Mv::default();
        m.ref_frame = 0;
    }

    fn decode_intra_mode(&mut self, b: &mut Bool, me: usize) {
        let y_mode = b.tree(&Y_MODE_TREE, &self.ent.y_mode);
        if y_mode == B_PRED {
            for i in 0..16 {
                self.mbi[me].modes[i] = b.tree(&B_MODE_TREE, &DEFAULT_B_MODE_PROBS);
            }
        }
        let uv_mode = b.tree(&UV_MODE_TREE, &self.ent.uv_mode);
        let m = &mut self.mbi[me];
        m.y_mode = y_mode;
        m.uv_mode = uv_mode;
        m.mv = Mv::default();
        m.ref_frame = CURRENT as u8;
    }

    fn read_mv_component(b: &mut Bool, mvc: &[u8; 19]) -> i16 {
        const IS_SHORT: usize = 0;
        const SIGN: usize = 1;
        const SHORT: usize = 2;
        const BITS: usize = SHORT + 8 - 1;
        const LONG_WIDTH: usize = 10;
        let mut x: i32 = 0;
        if b.get(mvc[IS_SHORT]) {
            for i in 0..3 {
                x += (b.get(mvc[BITS + i]) as i32) << i;
            }
            for i in (4..LONG_WIDTH).rev() {
                x += (b.get(mvc[BITS + i]) as i32) << i;
            }
            if x & 0xFFF0 == 0 || b.get(mvc[BITS + 3]) {
                x += 8;
            }
        } else {
            x = b.tree(&SMALL_MV_TREE, &mvc[SHORT..SHORT + 7]) as i32;
        }
        if x != 0 && b.get(mvc[SIGN]) {
            x = -x;
        }
        (x << 1) as i16
    }

    fn read_mv(&self, b: &mut Bool) -> Mv {
        let y = Self::read_mv_component(b, &self.ent.mv[0]);
        let x = Self::read_mv_component(b, &self.ent.mv[1]);
        Mv { x, y }
    }

    fn mv_bias(&self, other: usize, ref_frame: u8, mv: &mut Mv) {
        let sb = &self.refh.sign_bias;
        if sb[self.mbi[other].ref_frame as usize] ^ sb[ref_frame as usize] {
            mv.x = mv.x.wrapping_neg();
            mv.y = mv.y.wrapping_neg();
        }
    }

    /// near_mvs[0] = best (shares the slot with zero-zero), [1] nearest, [2] near.
    fn find_near_mvs(&self, me: usize, left: usize, above: usize) -> ([Mv; 4], [i32; 4]) {
        let aboveleft = above - 1;
        let this_ref = self.mbi[me].ref_frame;
        let mut near = [Mv::default(); 4];
        let mut cnt = [0i32; 4];
        let mut mvi = 0usize; // index of the last MV stored (0 = none)
        let mut cntx = 0usize;
        let a = &self.mbi[above];
        if a.ref_frame as usize != CURRENT {
            if !a.mv.is_zero() {
                mvi += 1;
                near[mvi] = a.mv;
                let mut m = near[mvi];
                self.mv_bias(above, this_ref, &mut m);
                near[mvi] = m;
                cntx += 1;
            }
            cnt[cntx] += 2;
        }
        let l = &self.mbi[left];
        if l.ref_frame as usize != CURRENT {
            if !l.mv.is_zero() {
                let mut m = l.mv;
                self.mv_bias(left, this_ref, &mut m);
                if m != near[mvi] {
                    mvi += 1;
                    near[mvi] = m;
                    cntx += 1;
                }
                cnt[cntx] += 2;
            } else {
                cnt[0] += 2;
            }
        }
        let al = &self.mbi[aboveleft];
        if al.ref_frame as usize != CURRENT {
            if !al.mv.is_zero() {
                let mut m = al.mv;
                self.mv_bias(aboveleft, this_ref, &mut m);
                if m != near[mvi] {
                    mvi += 1;
                    near[mvi] = m;
                    cntx += 1;
                }
                cnt[cntx] += 1;
            } else {
                cnt[0] += 1;
            }
        }
        // three distinct MVs: merge above-left with nearest when equal
        if cnt[3] != 0 && near[mvi] == near[1] {
            cnt[1] += 1;
        }
        cnt[3] = ((self.mbi[above].y_mode == SPLITMV) as i32 + (self.mbi[left].y_mode == SPLITMV) as i32) * 2 + (self.mbi[aboveleft].y_mode == SPLITMV) as i32;
        if cnt[2] > cnt[1] {
            cnt.swap(1, 2);
            near.swap(1, 2);
        }
        if cnt[1] >= cnt[0] {
            near[0] = near[1];
        }
        (near, cnt)
    }

    fn left_block_mv(&self, me: usize, left: usize, b: usize) -> Mv {
        if b & 3 == 0 {
            let l = &self.mbi[left];
            if l.y_mode == SPLITMV {
                return l.mvs[b + 3];
            }
            return l.mv;
        }
        self.mbi[me].mvs[b - 1]
    }

    fn above_block_mv(&self, me: usize, above: usize, b: usize) -> Mv {
        if b < 4 {
            let a = &self.mbi[above];
            if a.y_mode == SPLITMV {
                return a.mvs[b + 12];
            }
            return a.mv;
        }
        self.mbi[me].mvs[b - 4]
    }

    fn decode_split_mv(&mut self, b: &mut Bool, me: usize, left: usize, above: usize, best: Mv) {
        let pid = b.tree(&SPLIT_MV_TREE, &SPLIT_MV_PROBS) as usize;
        let part = MV_PARTITIONS[pid];
        self.mbi[me].partitioning = pid as u8;
        let mut mask = 0u32;
        let mut j = 0u8;
        while mask < 65535 {
            let mut k = 0usize;
            while part[k] != j {
                k += 1;
            }
            let lmv = self.left_block_mv(me, left, k);
            let amv = self.above_block_mv(me, above, k);
            let lez = lmv.is_zero();
            let aez = amv.is_zero();
            let lea = lmv == amv;
            let ctx = if lea && lez {
                4
            } else if lea {
                3
            } else if aez {
                2
            } else if lez {
                1
            } else {
                0
            };
            let mode = b.tree(&SUBMV_REF_TREE, &SUBMV_REF_PROBS2[ctx]);
            let mv = match mode {
                LEFT4X4 => lmv,
                ABOVE4X4 => amv,
                ZERO4X4 => Mv::default(),
                _ => {
                    let d = self.read_mv(b);
                    Mv { x: d.x.wrapping_add(best.x), y: d.y.wrapping_add(best.y) }
                }
            };
            for kk in k..16 {
                if part[kk] == j {
                    self.mbi[me].mvs[kk] = mv;
                    mask |= 1 << kk;
                }
            }
            j += 1;
        }
    }

    fn decode_mvs(&mut self, b: &mut Bool, row: usize, col: usize, bounds: [i32; 4]) {
        let me = self.mi(row as isize, col as isize);
        let above = self.mi(row as isize - 1, col as isize);
        let left = me - 1;
        let [to_left, to_right, to_top, to_bottom] = bounds;
        let clamp = |m: Mv| -> Mv {
            let mut x = m.x as i32;
            let mut y = m.y as i32;
            x = if (m.x as i32) < to_left { to_left } else { x };
            x = if (m.x as i32) > to_right { to_right } else { x };
            y = if (m.y as i32) < to_top { to_top } else { y };
            y = if (m.y as i32) > to_bottom { to_bottom } else { y };
            Mv { x: x as i16, y: y as i16 }
        };
        self.mbi[me].ref_frame = if b.get(self.ent.prob_last) { 2 + b.get(self.ent.prob_gf) as u8 } else { 1 };
        let (near, cnt) = self.find_near_mvs(me, left, above);
        let probs = [MV_COUNTS_TO_PROBS[cnt[0] as usize][0], MV_COUNTS_TO_PROBS[cnt[1] as usize][1], MV_COUNTS_TO_PROBS[cnt[2] as usize][2], MV_COUNTS_TO_PROBS[cnt[3] as usize][3]];
        let y_mode = b.tree(&MV_REF_TREE, &probs);
        self.mbi[me].y_mode = y_mode;
        self.mbi[me].uv_mode = y_mode;
        self.mbi[me].need_mc_border = false;
        let x = (-to_left - 128) >> 3;
        let y = (-to_top - 128) >> 3;
        let w = (self.mb_cols * 16) as i32;
        let h = (self.mb_rows * 16) as i32;
        match y_mode {
            NEARESTMV => self.mbi[me].mv = clamp(near[1]),
            NEARMV => self.mbi[me].mv = clamp(near[2]),
            ZEROMV => {
                self.mbi[me].mv = Mv::default();
                return;
            }
            NEWMV => {
                let best = clamp(near[0]);
                let d = self.read_mv(b);
                self.mbi[me].mv = Mv { x: d.x.wrapping_add(best.x), y: d.y.wrapping_add(best.y) };
            }
            _ => {
                // SPLITMV
                let best = clamp(near[0]);
                self.decode_split_mv(b, me, left, above, best);
                self.mbi[me].mv = self.mbi[me].mvs[15];
                let mut chroma = [(0i32, 0i32); 4];
                let mut border = false;
                for blk in 0..16 {
                    let m = self.mbi[me].mvs[blk];
                    let c = &mut chroma[((blk >> 1) & 1) + ((blk >> 2) & 2)];
                    c.0 += m.x as i32;
                    c.1 += m.y as i32;
                    if need_mc_border(m, x + (blk as i32 & 3) * 4, y + (blk as i32 & !3), 4, w, h) {
                        border = true;
                        break;
                    }
                }
                if !border {
                    for (k, c) in chroma.iter_mut().enumerate() {
                        c.0 += 4 + 8 * (c.0 >> 31);
                        c.1 += 4 + 8 * (c.1 >> 31);
                        c.0 /= 4;
                        c.1 /= 4;
                        let m = Mv { x: c.0 as i16, y: c.1 as i16 };
                        if need_mc_border(m, x + (k as i32 & 1) * 8, y + (k as i32 >> 1) * 8, 16, w, h) {
                            border = true;
                            break;
                        }
                    }
                }
                self.mbi[me].need_mc_border = border;
                return;
            }
        }
        if need_mc_border(self.mbi[me].mv, x, y, 16, w, h) {
            self.mbi[me].need_mc_border = true;
        }
    }

    // ---- tokens ---------------------------------------------------------------

    fn tokens_row(&mut self, part: usize, row: usize) {
        let cols = self.mb_cols;
        if row == 0 {
            for a in self.above_ctx.iter_mut() {
                *a = [0; 9];
            }
        }
        let mut left = [0u8; 9];
        let mut bool = core::mem::take(&mut self.parts[part]);
        for col in 0..cols {
            let me = self.mi(row as isize, col as isize);
            let co = col * 400;
            self.coeffs[co..co + 400].fill(0);
            let m = self.mbi[me];
            if m.skip_coeff {
                // keep the Y2 context when this mode has no Y2 block
                let has_y2 = m.y_mode != B_PRED && m.y_mode != SPLITMV;
                let (l8, a8) = (left[8], self.above_ctx[col][8]);
                left = [0; 9];
                self.above_ctx[col] = [0; 9];
                if !has_y2 {
                    left[8] = l8;
                    self.above_ctx[col][8] = a8;
                }
                self.mbi[me].eob_mask = 0;
            } else {
                let dq = self.dqf[m.segment_id as usize].f;
                let mut above = self.above_ctx[col];
                let mask = decode_mb_tokens(&mut bool, &mut left, &mut above, &mut self.coeffs[co..co + 400], m.y_mode, &self.ent.coeff, &dq);
                self.above_ctx[col] = above;
                self.mbi[me].eob_mask = mask;
            }
        }
        self.parts[part] = bool;
    }

    // ---- prediction -------------------------------------------------------------

    fn predict_row(&mut self, row: usize) {
        let cur = self.refs[CURRENT].unwrap();
        let mut img = core::mem::take(&mut self.frames[cur]);
        let (ys, uvs) = (img.ys, img.uvs);
        let mut y = img.yo + ys * row * 16;
        let mut uv = img.uvo + uvs * row * 8;
        let first = self.mbi[self.mi(row as isize, 0)];
        fixup_left(&mut img.y, y, 16, ys, row, first.y_mode);
        fixup_left(&mut img.u, uv, 8, uvs, row, first.uv_mode);
        fixup_left(&mut img.v, uv, 8, uvs, row, first.uv_mode);
        if row == 0 {
            img.y[y - ys - 1] = 127;
        }
        for col in 0..self.mb_cols {
            let me = self.mi(row as isize, col as isize);
            let m = self.mbi[me];
            if row == 0 {
                fixup_above(&mut img.y, y, 16, ys, col, m.y_mode);
                fixup_above(&mut img.u, uv, 8, uvs, col, m.uv_mode);
                fixup_above(&mut img.v, uv, 8, uvs, col, m.uv_mode);
            }
            let co = col * 400;
            if m.y_mode <= B_PRED {
                predict_intra_luma(&mut img.y, y, ys, &m, &mut self.coeffs[co..co + 400]);
                predict_intra_chroma(&mut img.u, &mut img.v, uv, uvs, &m, &self.coeffs[co..co + 400]);
            } else {
                if m.y_mode != SPLITMV {
                    fixup_dc_coeffs(&mut self.coeffs[co..co + 400]);
                }
                self.predict_inter(&mut img, cur, y, uv, row, col, &m, co);
            }
            y += 16;
            uv += 8;
        }
        // extend the last row by four pixels for the next row's above-right
        let at = y + 15 * ys;
        let v = img.y[at - 1];
        img.y[at..at + 4].fill(v);
        self.frames[cur] = img;
    }

    #[allow(clippy::too_many_arguments)]
    fn predict_inter(&mut self, img: &mut Image, _cur: usize, y0: usize, uv0: usize, row: usize, col: usize, m: &MbInfo, co: usize) {
        let full_pixel = self.version == 3;
        let filters = if self.version != 0 { &BILINEAR } else { &SIXTAP };
        let mut chroma = [Mv::default(); 4];
        if m.y_mode != SPLITMV {
            let mut x = m.mv.x as i32;
            let mut yy = m.mv.y as i32;
            x = (x + 1 + (x >> 31) * 2) / 2;
            yy = (yy + 1 + (yy >> 31) * 2) / 2;
            let mut uvmv = Mv { x: x as i16, y: yy as i16 };
            if full_pixel {
                uvmv.x &= !7;
                uvmv.y &= !7;
            }
            chroma = [uvmv; 4];
        } else {
            for (k, b) in [0usize, 2, 8, 10].iter().enumerate() {
                chroma[k] = chroma_split_mv(m, *b, full_pixel);
            }
        }
        let rf = self.refs[m.ref_frame as usize].unwrap_or(self.refs[CURRENT].unwrap());
        let reference = core::mem::take(&mut self.frames[rf]);
        // a missing reference (a broken stream) predicts from a copy of this frame
        let selfref;
        let refimg: &Image = if rf == self.refs[CURRENT].unwrap() {
            selfref = img.clone();
            &selfref
        } else {
            &reference
        };
        let (w, h) = ((self.mb_cols * 16) as i32, (self.mb_rows * 16) as i32);
        let (ys, uvs) = (img.ys, img.uvs);
        if self.emul.len() < 9 * ys.max(64) {
            self.emul = vec![0; 9 * ys.max(64)];
        }
        // luma
        let (mut bx, mut by) = ((col * 16) as i32, (row * 16) as i32);
        let mut out = y0;
        for blk in 0..16 {
            let mv = if m.y_mode != SPLITMV { m.mv } else { m.mvs[blk] };
            let mut pred = [0u8; 16];
            predict_block(&refimg.y, refimg.yo, ys, bx, by, mv, filters, m.need_mc_border, w, h, &mut pred, &mut self.emul);
            let c: [i16; 16] = self.coeffs[co + blk * 16..co + blk * 16 + 16].try_into().unwrap();
            idct_add_pred(&mut img.y, out, ys, &pred, &c);
            bx += 4;
            out += 4;
            if blk & 3 == 3 {
                bx -= 16;
                by += 4;
                out += 4 * ys - 16;
            }
        }
        // chroma
        let (w2, h2) = (w >> 1, h >> 1);
        let (mut bx, mut by) = ((col * 8) as i32, (row * 8) as i32);
        let mut out = uv0;
        for k in 0..4 {
            let mut pred = [0u8; 16];
            predict_block(&refimg.u, refimg.uvo, uvs, bx, by, chroma[k], filters, m.need_mc_border, w2, h2, &mut pred, &mut self.emul);
            let c: [i16; 16] = self.coeffs[co + (16 + k) * 16..co + (16 + k) * 16 + 16].try_into().unwrap();
            idct_add_pred(&mut img.u, out, uvs, &pred, &c);
            predict_block(&refimg.v, refimg.uvo, uvs, bx, by, chroma[k], filters, m.need_mc_border, w2, h2, &mut pred, &mut self.emul);
            let c: [i16; 16] = self.coeffs[co + (20 + k) * 16..co + (20 + k) * 16 + 16].try_into().unwrap();
            idct_add_pred(&mut img.v, out, uvs, &pred, &c);
            bx += 4;
            out += 4;
            if k & 1 == 1 {
                bx -= 8;
                by += 4;
                out += 4 * uvs - 8;
            }
        }
        self.frames[rf] = reference;
    }

    // ---- loop filter -------------------------------------------------------------

    fn filter_params(&self, m: &MbInfo) -> (i32, i32, i32) {
        let mut lvl = self.lf.level;
        if self.seg.enabled {
            lvl = if !self.seg.abs { lvl + self.seg.lf_level[m.segment_id as usize] } else { self.seg.lf_level[m.segment_id as usize] };
        }
        lvl = lvl.clamp(0, 63);
        if self.lf.delta_enabled {
            lvl += self.lf.ref_delta[m.ref_frame as usize];
            if m.ref_frame as usize == CURRENT {
                if m.y_mode == B_PRED {
                    lvl += self.lf.mode_delta[0];
                }
            } else if m.y_mode == ZEROMV {
                lvl += self.lf.mode_delta[1];
            } else if m.y_mode == SPLITMV {
                lvl += self.lf.mode_delta[3];
            } else {
                lvl += self.lf.mode_delta[2];
            }
        }
        lvl = lvl.clamp(0, 63);
        let mut interior = lvl;
        if self.lf.sharpness != 0 {
            interior >>= if self.lf.sharpness > 4 { 2 } else { 1 };
            if interior > 9 - self.lf.sharpness {
                interior = 9 - self.lf.sharpness;
            }
        }
        if interior < 1 {
            interior = 1;
        }
        let mut hev = (lvl >= 15) as i32;
        if lvl >= 40 {
            hev += 1;
        }
        if lvl >= 20 && !self.key {
            hev += 1;
        }
        (lvl, interior, hev)
    }

    fn loopfilter_row(&mut self, row: usize) {
        let cur = self.refs[CURRENT].unwrap();
        let mut img = core::mem::take(&mut self.frames[cur]);
        let (ys, uvs) = (img.ys as isize, img.uvs as isize);
        let mut y = img.yo + img.ys * row * 16;
        let mut uv = img.uvo + img.uvs * row * 8;
        for col in 0..self.mb_cols {
            let m = self.mbi[self.mi(row as isize, col as isize)];
            let (edge, interior, hev) = self.filter_params(&m);
            if edge != 0 {
                let sub = m.eob_mask != 0 || m.y_mode == SPLITMV || m.y_mode == B_PRED;
                if self.lf.use_simple {
                    let mb_limit = (edge + 2) * 2 + interior;
                    let b_limit = edge * 2 + interior;
                    if col > 0 {
                        simple_edge(&mut img.y, y, 1, ys, mb_limit);
                    }
                    if sub {
                        for k in [4usize, 8, 12] {
                            simple_edge(&mut img.y, y + k, 1, ys, b_limit);
                        }
                    }
                    if row > 0 {
                        simple_edge(&mut img.y, y, ys, 1, mb_limit);
                    }
                    if sub {
                        for k in [4usize, 8, 12] {
                            simple_edge(&mut img.y, y + k * img.ys, ys, 1, b_limit);
                        }
                    }
                } else {
                    if col > 0 {
                        mb_edge(&mut img.y, y, 1, ys, edge + 2, interior, hev, 16);
                        mb_edge(&mut img.u, uv, 1, uvs, edge + 2, interior, hev, 8);
                        mb_edge(&mut img.v, uv, 1, uvs, edge + 2, interior, hev, 8);
                    }
                    if sub {
                        for k in [4usize, 8, 12] {
                            sub_edge(&mut img.y, y + k, 1, ys, edge, interior, hev, 16);
                        }
                        sub_edge(&mut img.u, uv + 4, 1, uvs, edge, interior, hev, 8);
                        sub_edge(&mut img.v, uv + 4, 1, uvs, edge, interior, hev, 8);
                    }
                    if row > 0 {
                        mb_edge(&mut img.y, y, ys, 1, edge + 2, interior, hev, 16);
                        mb_edge(&mut img.u, uv, uvs, 1, edge + 2, interior, hev, 8);
                        mb_edge(&mut img.v, uv, uvs, 1, edge + 2, interior, hev, 8);
                    }
                    if sub {
                        for k in [4usize, 8, 12] {
                            sub_edge(&mut img.y, y + k * img.ys, ys, 1, edge, interior, hev, 16);
                        }
                        sub_edge(&mut img.u, uv + 4 * img.uvs, uvs, 1, edge, interior, hev, 8);
                        sub_edge(&mut img.v, uv + 4 * img.uvs, uvs, 1, edge, interior, hev, 8);
                    }
                }
            }
            y += 16;
            uv += 8;
        }
        self.frames[cur] = img;
    }
}

fn need_mc_border(mv: Mv, l: i32, t: i32, b_w: i32, w: i32, h: i32) -> bool {
    let l = l + (mv.x as i32 >> 3);
    let t = t + (mv.y as i32 >> 3);
    let r = w - (l + b_w);
    let b = h - (t + b_w);
    (l >> 1) < 2 || (r >> 1) < 3 || (t >> 1) < 2 || (b >> 1) < 3
}

fn chroma_split_mv(m: &MbInfo, b: usize, full_pixel: bool) -> Mv {
    let sum = |f: fn(&Mv) -> i32| -> i32 {
        let mut t = f(&m.mvs[b]) + f(&m.mvs[b + 1]) + f(&m.mvs[b + 4]) + f(&m.mvs[b + 5]);
        if t < 0 {
            t -= 4;
        } else {
            t += 4;
        }
        t / 8
    };
    let mut mv = Mv { x: sum(|v| v.x as i32) as i16, y: sum(|v| v.y as i32) as i16 };
    if full_pixel {
        mv.x &= !7;
        mv.y &= !7;
    }
    mv
}

/// The 4x4 prediction of a block at plane position (x, y) moved by `mv`
/// (eighth pixels), with edge emulation when it reaches outside the w x h area.
#[allow(clippy::too_many_arguments)]
fn predict_block(plane: &[u8], origin: usize, stride: usize, x: i32, y: i32, mv: Mv, filters: &[[i32; 6]; 8], border: bool, w: i32, h: i32, out: &mut [u8; 16], emul: &mut [u8]) {
    let px = x + (mv.x as i32 >> 3);
    let py = y + (mv.y as i32 >> 3);
    let (mx, my) = ((mv.x & 7) as usize, (mv.y & 7) as usize);
    // a 9x9 source window around the block (2 left / above, 3 right / below)
    let mut src = [0u8; 81];
    // dixie emulates the edge only for macroblocks flagged `need_mc_border`;
    // reading clamped coordinates whenever the window leaves the frame is the
    // same picture (edge pixels repeated) and never reads past the buffer
    let _ = border;
    let emulate = px < 2 || px + 3 + 3 >= w || py < 2 || py + 3 + 3 >= h;
    for r in 0..9 {
        for c in 0..9 {
            let (sx, sy) = (px - 2 + c, py - 2 + r);
            let (sx, sy) = if emulate { (sx.clamp(0, w - 1), sy.clamp(0, h - 1)) } else { (sx, sy) };
            src[(r * 9 + c) as usize] = plane[(origin as isize + sy as isize * stride as isize + sx as isize) as usize];
        }
    }
    let _ = emul;
    if mx == 0 && my == 0 {
        for r in 0..4 {
            for c in 0..4 {
                out[r * 4 + c] = src[(r + 2) * 9 + c + 2];
            }
        }
        return;
    }
    // horizontal pass over 9 rows, then vertical
    let fh = &filters[mx];
    let fv = &filters[my];
    let mut temp = [0u8; 36];
    for r in 0..9 {
        for c in 0..4 {
            let s = &src[r * 9 + c..];
            let t = s[0] as i32 * fh[0] + s[1] as i32 * fh[1] + s[2] as i32 * fh[2] + s[3] as i32 * fh[3] + s[4] as i32 * fh[4] + s[5] as i32 * fh[5] + 64;
            temp[r * 4 + c] = (t >> 7).clamp(0, 255) as u8;
        }
    }
    for r in 0..4 {
        for c in 0..4 {
            let t = temp[r * 4 + c] as i32 * fv[0]
                + temp[(r + 1) * 4 + c] as i32 * fv[1]
                + temp[(r + 2) * 4 + c] as i32 * fv[2]
                + temp[(r + 3) * 4 + c] as i32 * fv[3]
                + temp[(r + 4) * 4 + c] as i32 * fv[4]
                + temp[(r + 5) * 4 + c] as i32 * fv[5]
                + 64;
            out[r * 4 + c] = (t >> 7).clamp(0, 255) as u8;
        }
    }
}

// ---- tokens -----------------------------------------------------------------------

fn decode_mb_tokens(b: &mut Bool, left: &mut [u8; 9], above: &mut [u8; 9], tokens: &mut [i16], mode: u8, probs: &[u8; 1056], dq: &[[i32; 2]; 3]) -> u32 {
    let mut eob_mask = 0u32;
    let (mut i, mut stop, mut ty, mut dqi, mut tok) = if mode != B_PRED && mode != SPLITMV { (24usize, 24usize, 1usize, 2usize, 24 * 16usize) } else { (0, 16, 3, 0, 0) };
    loop {
        // one block
        let t = left[LEFT_CTX_INDEX[i]] + above[ABOVE_CTX_INDEX[i]];
        let first = if ty == 0 { 1usize } else { 0 };
        let mut c = first;
        let mut ctx = t as usize;
        let mut check_eob = true;
        let base = ty * 8 * 3 * 11;
        loop {
            let p = &probs[base + BANDS[c] * 33 + ctx * 11..];
            if check_eob && !b.get(p[0]) {
                break;
            }
            if !b.get(p[1]) {
                // zero token: no EOB check next
                ctx = 0;
                check_eob = false;
                if c < 15 {
                    c += 1;
                    continue;
                }
                break; // malformed: finish with c = 15
            }
            let v: i32;
            if !b.get(p[2]) {
                v = 1;
                ctx = 1;
            } else {
                ctx = 2;
                if !b.get(p[3]) {
                    v = if !b.get(p[4]) {
                        2
                    } else if !b.get(p[5]) {
                        3
                    } else {
                        4
                    };
                } else {
                    let cat = if !b.get(p[6]) {
                        if !b.get(p[7]) {
                            0
                        } else {
                            1
                        }
                    } else if !b.get(p[8]) {
                        if !b.get(p[9]) {
                            2
                        } else {
                            3
                        }
                    } else if !b.get(p[10]) {
                        4
                    } else {
                        5
                    };
                    let mut val = CAT_MIN[cat];
                    let mut bit = CAT_LEN[cat];
                    while bit >= 0 {
                        val += (b.get(CAT_PROBS[cat][bit as usize]) as i32) << bit;
                        bit -= 1;
                    }
                    v = val;
                }
            }
            let signed = if b.bit() { -v } else { v };
            let q = dq[dqi][(c != 0) as usize];
            tokens[tok + ZIGZAG[c]] = (signed.wrapping_mul(q)) as i16;
            check_eob = true;
            if c < 15 {
                c += 1;
                continue;
            }
            break; // c stays 15
        }
        eob_mask |= ((c > 1) as u32) << i;
        let nz = (c != first) as u8;
        eob_mask |= (nz as u32) << 31;
        left[LEFT_CTX_INDEX[i]] = nz;
        above[ABOVE_CTX_INDEX[i]] = nz;
        tok += 16;
        i += 1;
        if i < stop {
            continue;
        }
        if i == 25 {
            ty = 0;
            i = 0;
            stop = 16;
            tok = 0;
            dqi = 0;
            continue;
        }
        if i == 16 {
            ty = 2;
            stop = 24;
            dqi = 1;
            continue;
        }
        return eob_mask;
    }
}

// ---- transforms ---------------------------------------------------------------------

fn walsh(input: &[i16], output: &mut [i16; 16]) {
    let mut t = [0i32; 16];
    for i in 0..4 {
        let (a1, b1, c1, d1) = (
            input[i] as i32 + input[12 + i] as i32,
            input[4 + i] as i32 + input[8 + i] as i32,
            input[4 + i] as i32 - input[8 + i] as i32,
            input[i] as i32 - input[12 + i] as i32,
        );
        t[i] = (a1 + b1) as i16 as i32;
        t[4 + i] = (c1 + d1) as i16 as i32;
        t[8 + i] = (a1 - b1) as i16 as i32;
        t[12 + i] = (d1 - c1) as i16 as i32;
    }
    for i in 0..4 {
        let r = &t[i * 4..i * 4 + 4];
        let (a1, b1, c1, d1) = (r[0] + r[3], r[1] + r[2], r[1] - r[2], r[0] - r[3]);
        let (a2, b2, c2, d2) = (a1 + b1, c1 + d1, a1 - b1, d1 - c1);
        output[i * 4] = ((a2 + 3) >> 3) as i16;
        output[i * 4 + 1] = ((b2 + 3) >> 3) as i16;
        output[i * 4 + 2] = ((c2 + 3) >> 3) as i16;
        output[i * 4 + 3] = ((d2 + 3) >> 3) as i16;
    }
}

const COS: i32 = 20091;
const SIN: i32 = 35468;

/// recon = predict + IDCT(coeffs), predict given as 16 bytes.
fn idct_add_pred(recon: &mut [u8], at: usize, stride: usize, predict: &[u8; 16], coeffs: &[i16; 16]) {
    let mut tmp = [0i16; 16];
    for i in 0..4 {
        let ip = |k: usize| coeffs[i + k] as i32;
        let a1 = ip(0) + ip(8);
        let b1 = ip(0) - ip(8);
        let temp1 = (ip(4) * SIN) >> 16;
        let temp2 = ip(12) + ((ip(12) * COS) >> 16);
        let c1 = temp1 - temp2;
        let temp1 = ip(4) + ((ip(4) * COS) >> 16);
        let temp2 = (ip(12) * SIN) >> 16;
        let d1 = temp1 + temp2;
        tmp[i] = (a1 + d1) as i16;
        tmp[12 + i] = (a1 - d1) as i16;
        tmp[4 + i] = (b1 + c1) as i16;
        tmp[8 + i] = (b1 - c1) as i16;
    }
    for i in 0..4 {
        let c = |k: usize| tmp[i * 4 + k] as i32;
        let a1 = c(0) + c(2);
        let b1 = c(0) - c(2);
        let temp1 = (c(1) * SIN) >> 16;
        let temp2 = c(3) + ((c(3) * COS) >> 16);
        let c1 = temp1 - temp2;
        let temp1 = c(1) + ((c(1) * COS) >> 16);
        let temp2 = (c(3) * SIN) >> 16;
        let d1 = temp1 + temp2;
        let row = at + i * stride;
        let p = &predict[i * 4..i * 4 + 4];
        recon[row] = (p[0] as i32 + ((a1 + d1 + 4) >> 3)).clamp(0, 255) as u8;
        recon[row + 3] = (p[3] as i32 + ((a1 - d1 + 4) >> 3)).clamp(0, 255) as u8;
        recon[row + 1] = (p[1] as i32 + ((b1 + c1 + 4) >> 3)).clamp(0, 255) as u8;
        recon[row + 2] = (p[2] as i32 + ((b1 - c1 + 4) >> 3)).clamp(0, 255) as u8;
    }
}

/// In place: the block at `at` is both the prediction and the output.
fn idct_add_in_place(buf: &mut [u8], at: usize, stride: usize, coeffs: &[i16]) {
    let mut p = [0u8; 16];
    for r in 0..4 {
        p[r * 4..r * 4 + 4].copy_from_slice(&buf[at + r * stride..at + r * stride + 4]);
    }
    let c: [i16; 16] = coeffs[..16].try_into().unwrap();
    idct_add_pred(buf, at, stride, &p, &c);
}

fn fixup_dc_coeffs(coeffs: &mut [i16]) {
    let mut y2 = [0i16; 16];
    walsh(&coeffs[24 * 16..25 * 16], &mut y2);
    for i in 0..16 {
        coeffs[i * 16] = y2[i];
    }
}

// ---- intra prediction --------------------------------------------------------------

fn fixup_left(p: &mut [u8], at: usize, width: usize, stride: usize, row: usize, mode: u8) {
    if mode == DC_PRED && row > 0 {
        for i in 0..width {
            p[at - 1 + i * stride] = p[at - stride + i];
        }
    } else {
        let mut l = at - 1 - stride;
        for _ in 0..width + 1 {
            p[l] = 129;
            l += stride;
        }
    }
}

fn fixup_above(p: &mut [u8], at: usize, width: usize, stride: usize, col: usize, mode: u8) {
    let above = at - stride;
    if mode == DC_PRED && col > 0 {
        for i in 0..width {
            p[above + i] = p[at - 1 + i * stride];
        }
    } else {
        p[above - 1..above + width].fill(127);
    }
    p[above + width..above + width + 4].fill(127);
}

fn predict_dc(p: &mut [u8], at: usize, stride: usize, n: usize) {
    let mut dc = 0u32;
    for i in 0..n {
        dc += p[at - 1 + i * stride] as u32 + p[at - stride + i] as u32;
    }
    let shift = match n {
        16 => 5,
        8 => 4,
        _ => 3,
    };
    let v = ((dc + (1 << (shift - 1))) >> shift) as u8;
    for i in 0..n {
        p[at + i * stride..at + i * stride + n].fill(v);
    }
}

fn predict_v(p: &mut [u8], at: usize, stride: usize, n: usize) {
    for i in 0..n {
        let (src, dst) = (at - stride, at + i * stride);
        for j in 0..n {
            p[dst + j] = p[src + j];
        }
    }
}

fn predict_h(p: &mut [u8], at: usize, stride: usize, n: usize) {
    for i in 0..n {
        let v = p[at + i * stride - 1];
        p[at + i * stride..at + i * stride + n].fill(v);
    }
}

fn predict_tm(p: &mut [u8], at: usize, stride: usize, n: usize) {
    let tl = p[at - stride - 1] as i32;
    for j in 0..n {
        let l = p[at + j * stride - 1] as i32;
        for i in 0..n {
            let a = p[at - stride + i] as i32;
            p[at + j * stride + i] = (l + a - tl).clamp(0, 255) as u8;
        }
    }
}

/// 4x4 sub-block predictions (RFC 6386 section 12.3), reading the frame around `at`.
fn predict_4x4(p: &mut [u8], at: usize, s: usize, mode: u8) {
    let a = |p: &[u8], k: isize| p[(at as isize - s as isize + k) as usize] as i32; // above row, k = -1 .. 7
    let l = |p: &[u8], k: usize| p[at - 1 + k * s] as i32; // left column, k = 0 .. 3
    let mut out = [0i32; 16];
    match mode {
        B_DC_PRED => {
            let mut dc = 0;
            for k in 0..4 {
                dc += a(p, k) + l(p, k as usize);
            }
            out = [(dc + 4) >> 3; 16];
        }
        B_TM_PRED => {
            let tl = a(p, -1);
            for r in 0..4 {
                for c in 0..4 {
                    out[r * 4 + c] = (l(p, r) + a(p, c as isize) - tl).clamp(0, 255);
                }
            }
        }
        B_VE_PRED => {
            for c in 0..4 {
                let v = (a(p, c - 1) + 2 * a(p, c) + a(p, c + 1) + 2) >> 2;
                for r in 0..4 {
                    out[r * 4 + c as usize] = v;
                }
            }
        }
        B_HE_PRED => {
            let rows = [
                (a(p, -1) + 2 * l(p, 0) + l(p, 1) + 2) >> 2,
                (l(p, 0) + 2 * l(p, 1) + l(p, 2) + 2) >> 2,
                (l(p, 1) + 2 * l(p, 2) + l(p, 3) + 2) >> 2,
                (l(p, 2) + 2 * l(p, 3) + l(p, 3) + 2) >> 2,
            ];
            for r in 0..4 {
                for c in 0..4 {
                    out[r * 4 + c] = rows[r];
                }
            }
        }
        B_LD_PRED => {
            let e = |k: isize| (a(p, k) + 2 * a(p, k + 1) + a(p, (k + 2).min(7)) + 2) >> 2;
            let v = [e(0), e(1), e(2), e(3), e(4), e(5), (a(p, 6) + 2 * a(p, 7) + a(p, 7) + 2) >> 2];
            for r in 0..4 {
                for c in 0..4 {
                    out[r * 4 + c] = v[r + c];
                }
            }
        }
        B_RD_PRED => {
            // edge: l3 l2 l1 l0 tl a0 a1 a2 a3
            let e = [l(p, 3), l(p, 2), l(p, 1), l(p, 0), a(p, -1), a(p, 0), a(p, 1), a(p, 2), a(p, 3)];
            let f = |k: usize| (e[k] + 2 * e[k + 1] + e[k + 2] + 2) >> 2;
            for r in 0..4 {
                for c in 0..4 {
                    out[r * 4 + c] = f(3 - r + c);
                }
            }
        }
        B_VR_PRED => {
            let (tl, l0, l1, l2) = (a(p, -1), l(p, 0), l(p, 1), l(p, 2));
            let av = |k: isize| a(p, k);
            out[0] = (tl + av(0) + 1) >> 1;
            out[1] = (av(0) + av(1) + 1) >> 1;
            out[2] = (av(1) + av(2) + 1) >> 1;
            out[3] = (av(2) + av(3) + 1) >> 1;
            out[4] = (l0 + 2 * tl + av(0) + 2) >> 2;
            out[5] = (tl + 2 * av(0) + av(1) + 2) >> 2;
            out[6] = (av(0) + 2 * av(1) + av(2) + 2) >> 2;
            out[7] = (av(1) + 2 * av(2) + av(3) + 2) >> 2;
            out[8] = (l1 + 2 * l0 + tl + 2) >> 2;
            out[9] = out[0];
            out[10] = out[1];
            out[11] = out[2];
            out[12] = (l2 + 2 * l1 + l0 + 2) >> 2;
            out[13] = out[4];
            out[14] = out[5];
            out[15] = out[6];
        }
        B_VL_PRED => {
            let av = |k: isize| a(p, k);
            out[0] = (av(0) + av(1) + 1) >> 1;
            out[1] = (av(1) + av(2) + 1) >> 1;
            out[2] = (av(2) + av(3) + 1) >> 1;
            out[3] = (av(3) + av(4) + 1) >> 1;
            out[4] = (av(0) + 2 * av(1) + av(2) + 2) >> 2;
            out[5] = (av(1) + 2 * av(2) + av(3) + 2) >> 2;
            out[6] = (av(2) + 2 * av(3) + av(4) + 2) >> 2;
            out[7] = (av(3) + 2 * av(4) + av(5) + 2) >> 2;
            out[8] = out[1];
            out[9] = out[2];
            out[10] = out[3];
            out[11] = (av(4) + 2 * av(5) + av(6) + 2) >> 2;
            out[12] = out[5];
            out[13] = out[6];
            out[14] = out[7];
            out[15] = (av(5) + 2 * av(6) + av(7) + 2) >> 2;
        }
        B_HD_PRED => {
            let (tl, l0, l1, l2, l3) = (a(p, -1), l(p, 0), l(p, 1), l(p, 2), l(p, 3));
            let av = |k: isize| a(p, k);
            out[0] = (l0 + tl + 1) >> 1;
            out[1] = (l0 + 2 * tl + av(0) + 2) >> 2;
            out[2] = (tl + 2 * av(0) + av(1) + 2) >> 2;
            out[3] = (av(0) + 2 * av(1) + av(2) + 2) >> 2;
            out[4] = (l1 + l0 + 1) >> 1;
            out[5] = (l1 + 2 * l0 + tl + 2) >> 2;
            out[6] = out[0];
            out[7] = out[1];
            out[8] = (l2 + l1 + 1) >> 1;
            out[9] = (l2 + 2 * l1 + l0 + 2) >> 2;
            out[10] = out[4];
            out[11] = out[5];
            out[12] = (l3 + l2 + 1) >> 1;
            out[13] = (l3 + 2 * l2 + l1 + 2) >> 2;
            out[14] = out[8];
            out[15] = out[9];
        }
        _ => {
            // B_HU_PRED
            let (l0, l1, l2, l3) = (l(p, 0), l(p, 1), l(p, 2), l(p, 3));
            out[0] = (l0 + l1 + 1) >> 1;
            out[1] = (l0 + 2 * l1 + l2 + 2) >> 2;
            out[2] = (l1 + l2 + 1) >> 1;
            out[3] = (l1 + 2 * l2 + l3 + 2) >> 2;
            out[4] = out[2];
            out[5] = out[3];
            out[6] = (l2 + l3 + 1) >> 1;
            out[7] = (l2 + 2 * l3 + l3 + 2) >> 2;
            out[8] = out[6];
            out[9] = out[7];
            out[10] = l3;
            out[11] = l3;
            out[12] = l3;
            out[13] = l3;
            out[14] = l3;
            out[15] = l3;
        }
    }
    for r in 0..4 {
        for c in 0..4 {
            p[at + r * s + c] = out[r * 4 + c] as u8;
        }
    }
}

fn predict_intra_luma(p: &mut [u8], at: usize, s: usize, m: &MbInfo, coeffs: &mut [i16]) {
    if m.y_mode == B_PRED {
        // copy the four pixels above-right of sub-block 3 to above-right of 7, 11, 15
        let src = at + 16 - s;
        let v: [u8; 4] = p[src..src + 4].try_into().unwrap();
        for k in [4usize, 8, 12] {
            let d = src + k * s;
            p[d..d + 4].copy_from_slice(&v);
        }
        let mut row = at;
        for i in 0..16 {
            let b = row + (i & 3) * 4;
            predict_4x4(p, b, s, m.modes[i]);
            idct_add_in_place(p, b, s, &coeffs[i * 16..]);
            if i & 3 == 3 {
                row += s * 4;
            }
        }
    } else {
        match m.y_mode {
            DC_PRED => predict_dc(p, at, s, 16),
            V_PRED => predict_v(p, at, s, 16),
            H_PRED => predict_h(p, at, s, 16),
            _ => predict_tm(p, at, s, 16),
        }
        fixup_dc_coeffs(coeffs);
        let mut b = at;
        for i in 0..16 {
            idct_add_in_place(p, b, s, &coeffs[i * 16..]);
            b += 4;
            if i & 3 == 3 {
                b += s * 4 - 16;
            }
        }
    }
}

fn predict_intra_chroma(pu: &mut [u8], pv: &mut [u8], at: usize, s: usize, m: &MbInfo, coeffs: &[i16]) {
    for (plane, first) in [(pu, 16usize), (pv, 20usize)] {
        match m.uv_mode {
            DC_PRED => predict_dc(plane, at, s, 8),
            V_PRED => predict_v(plane, at, s, 8),
            H_PRED => predict_h(plane, at, s, 8),
            _ => predict_tm(plane, at, s, 8),
        }
        let mut b = at;
        for i in 0..4 {
            idct_add_in_place(plane, b, s, &coeffs[(first + i) * 16..]);
            b += 4;
            if i & 1 == 1 {
                b += s * 4 - 8;
            }
        }
    }
}

// ---- loop filter helpers ---------------------------------------------------------------

#[inline]
fn px(p: &[u8], at: usize, k: isize) -> i32 {
    p[(at as isize + k) as usize] as i32
}

#[inline]
fn set(p: &mut [u8], at: usize, k: isize, v: i32) {
    p[(at as isize + k) as usize] = v.clamp(0, 255) as u8;
}

fn s8(x: i32) -> i32 {
    x.clamp(-128, 127)
}

fn simple_threshold(p: &[u8], at: usize, st: isize, limit: i32) -> bool {
    ((px(p, at, -st) - px(p, at, 0)).abs() * 2 + ((px(p, at, -2 * st) - px(p, at, st)).abs() >> 1)) <= limit
}

fn normal_threshold(p: &[u8], at: usize, st: isize, e: i32, i: i32) -> bool {
    let (p3, p2, p1, p0) = (px(p, at, -4 * st), px(p, at, -3 * st), px(p, at, -2 * st), px(p, at, -st));
    let (q0, q1, q2, q3) = (px(p, at, 0), px(p, at, st), px(p, at, 2 * st), px(p, at, 3 * st));
    simple_threshold(p, at, st, 2 * e + i)
        && (p3 - p2).abs() <= i
        && (p2 - p1).abs() <= i
        && (p1 - p0).abs() <= i
        && (q3 - q2).abs() <= i
        && (q2 - q1).abs() <= i
        && (q1 - q0).abs() <= i
}

fn hev(p: &[u8], at: usize, st: isize, t: i32) -> bool {
    (px(p, at, -2 * st) - px(p, at, -st)).abs() > t || (px(p, at, st) - px(p, at, 0)).abs() > t
}

fn filter_common(p: &mut [u8], at: usize, st: isize, outer: bool) {
    let (p1, p0, q0, q1) = (px(p, at, -2 * st), px(p, at, -st), px(p, at, 0), px(p, at, st));
    let mut a = 3 * (q0 - p0);
    if outer {
        a += s8(p1 - q1);
    }
    a = s8(a);
    let f1 = (if a + 4 > 127 { 127 } else { a + 4 }) >> 3;
    let f2 = (if a + 3 > 127 { 127 } else { a + 3 }) >> 3;
    set(p, at, -st, p0 + f2);
    set(p, at, 0, q0 - f1);
    if !outer {
        let a = (f1 + 1) >> 1;
        set(p, at, -2 * st, p1 + a);
        set(p, at, st, q1 - a);
    }
}

fn filter_mb(p: &mut [u8], at: usize, st: isize) {
    let (p2, p1, p0, q0, q1, q2) = (px(p, at, -3 * st), px(p, at, -2 * st), px(p, at, -st), px(p, at, 0), px(p, at, st), px(p, at, 2 * st));
    let w = s8(s8(p1 - q1) + 3 * (q0 - p0));
    let a = (27 * w + 63) >> 7;
    set(p, at, -st, p0 + a);
    set(p, at, 0, q0 - a);
    let a = (18 * w + 63) >> 7;
    set(p, at, -2 * st, p1 + a);
    set(p, at, st, q1 - a);
    let a = (9 * w + 63) >> 7;
    set(p, at, -3 * st, p2 + a);
    set(p, at, 2 * st, q2 - a);
}

/// Filter across an edge: `st` = step across the edge, `along` = step along it.
#[allow(clippy::too_many_arguments)]
fn mb_edge(p: &mut [u8], at: usize, st: isize, along: isize, e: i32, i: i32, t: i32, n: usize) {
    let mut a = at as isize;
    for _ in 0..n {
        let u = a as usize;
        if normal_threshold(p, u, st, e, i) {
            if hev(p, u, st, t) {
                filter_common(p, u, st, true);
            } else {
                filter_mb(p, u, st);
            }
        }
        a += along;
    }
}

#[allow(clippy::too_many_arguments)]
fn sub_edge(p: &mut [u8], at: usize, st: isize, along: isize, e: i32, i: i32, t: i32, n: usize) {
    let mut a = at as isize;
    for _ in 0..n {
        let u = a as usize;
        if normal_threshold(p, u, st, e, i) {
            let h = hev(p, u, st, t);
            filter_common(p, u, st, h);
        }
        a += along;
    }
}

fn simple_edge(p: &mut [u8], at: usize, st: isize, along: isize, limit: i32) {
    let mut a = at as isize;
    for _ in 0..16 {
        let u = a as usize;
        if simple_threshold(p, u, st, limit) {
            filter_common(p, u, st, true);
        }
        a += along;
    }
}

/// The current frame's visible area as 0xAARRGGBB, scaled (nearest) to dw x dh.
pub fn to_rgb(img: &Image, w: usize, h: usize, out: &mut [i32], dw: usize, dh: usize) {
    for y in 0..dh {
        let sy = y * h / dh;
        for x in 0..dw {
            let sx = x * w / dw;
            let yy = img.y[img.yo + sy * img.ys + sx] as i32;
            let u = img.u[img.uvo + (sy / 2) * img.uvs + sx / 2] as i32 - 128;
            let v = img.v[img.uvo + (sy / 2) * img.uvs + sx / 2] as i32 - 128;
            // BT.601 limited range, fixed point
            let c = (yy - 16) * 298;
            let r = ((c + 409 * v + 128) >> 8).clamp(0, 255);
            let g = ((c - 100 * u - 208 * v + 128) >> 8).clamp(0, 255);
            let b = ((c + 516 * u + 128) >> 8).clamp(0, 255);
            out[y * dw + x] = (0xFF00_0000u32 | (r as u32) << 16 | (g as u32) << 8 | b as u32) as i32;
        }
    }
}
