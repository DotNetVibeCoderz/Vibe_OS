//! A local LLM: llama-architecture models in **GGUF** files (the llama.cpp
//! format on Hugging Face), run as `CLRHOST.ELF MODEL.GGU [prompt...]`.
//!
//! * GGUF v2/v3 reader: metadata, tensor table, F32 / F16 / Q8_0 / Q4_0
//!   weights (quantized blocks are expanded to f32 when loaded).
//! * Llama forward pass for one token at a time: RMSNorm, rotary position
//!   embedding on adjacent pairs, grouped-query attention over a KV cache,
//!   SwiGLU feed-forward, tied or separate output matrix.
//! * SentencePiece-style tokenizer from the GGUF vocabulary (characters, byte
//!   fallback `<0xXX>`, merges by score), greedy or temperature sampling.
//! * Matrix-vector products on the **GPU** when the kernel has a compute
//!   backend (`COMPUTE_OP` GEMV: V3D on the Pi 4), weights uploaded once;
//!   Q/K/V and gate/up are fused into one GEMV each. Otherwise on this CPU.
//!
//! Checked against a numpy reference of the same model (stories260K: the
//! prompt "Once upon a time" -> tokens 1 403 407 261 378, then ", there was
//! a little girl named Lily ...").

use crate::sys;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---- GGUF ---------------------------------------------------------------------

enum Val {
    Int(i64),
    F(f64),
    Str(Vec<u8>),
    Arr(Vec<Val>),
}

struct Tensor {
    dims: Vec<u64>,
    ty: u32,
    off: u64,
}

pub struct Gguf<'a> {
    data: &'a [u8],
    kv: BTreeMap<String, Val>,
    tensors: BTreeMap<String, Tensor>,
    base: usize,
}

struct Rd<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self.d.get(self.p..self.p + n).ok_or_else(|| String::from("file ends early"))?;
        self.p += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Result<u64, String> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }
    fn str(&mut self) -> Result<Vec<u8>, String> {
        let n = self.u64()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn val(&mut self, t: u32, depth: u32) -> Result<Val, String> {
        Ok(match t {
            0 => Val::Int(self.take(1)?[0] as i64),
            1 => Val::Int(self.take(1)?[0] as i8 as i64),
            2 => {
                let b = self.take(2)?;
                Val::Int(u16::from_le_bytes([b[0], b[1]]) as i64)
            }
            3 => {
                let b = self.take(2)?;
                Val::Int(i16::from_le_bytes([b[0], b[1]]) as i64)
            }
            4 => Val::Int(self.u32()? as i64),
            5 => Val::Int(self.u32()? as i32 as i64),
            6 => Val::F(f32::from_bits(self.u32()?) as f64),
            7 => Val::Int(self.take(1)?[0] as i64),
            8 => Val::Str(self.str()?),
            9 => {
                if depth > 2 {
                    return Err(String::from("arrays nested too deep"));
                }
                let et = self.u32()?;
                let n = self.u64()? as usize;
                if n > 1 << 22 {
                    return Err(String::from("array too long"));
                }
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(self.val(et, depth + 1)?);
                }
                Val::Arr(v)
            }
            10 | 11 => Val::Int(self.u64()? as i64),
            12 => Val::F(f64::from_bits(self.u64()?)),
            _ => {
                return Err(format!("unknown metadata type {}", t));
            }
        })
    }
}

impl<'a> Gguf<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Gguf<'a>, String> {
        if data.get(..4) != Some(b"GGUF") {
            return Err(String::from("not a GGUF file"));
        }
        let mut r = Rd { d: data, p: 4 };
        let ver = r.u32()?;
        if ver < 2 {
            return Err(format!("GGUF version {} (2 or 3 needed)", ver));
        }
        let nt = r.u64()? as usize;
        let nkv = r.u64()? as usize;
        let mut kv = BTreeMap::new();
        for _ in 0..nkv {
            let k = String::from_utf8_lossy(&r.str()?).into_owned();
            let t = r.u32()?;
            let v = r.val(t, 0)?;
            kv.insert(k, v);
        }
        let mut tensors = BTreeMap::new();
        for _ in 0..nt {
            let name = String::from_utf8_lossy(&r.str()?).into_owned();
            let nd = r.u32()? as usize;
            if nd > 4 {
                return Err(String::from("tensor with more than 4 dimensions"));
            }
            let mut dims = Vec::with_capacity(nd);
            for _ in 0..nd {
                dims.push(r.u64()?);
            }
            let ty = r.u32()?;
            let off = r.u64()?;
            tensors.insert(name, Tensor { dims, ty, off });
        }
        let align = match kv.get("general.alignment") {
            Some(Val::Int(a)) if *a > 0 => *a as usize,
            _ => 32,
        };
        let base = r.p.div_ceil(align) * align;
        Ok(Gguf { data, kv, tensors, base })
    }

    fn int(&self, k: &str) -> Option<i64> {
        match self.kv.get(k)? {
            Val::Int(v) => Some(*v),
            Val::F(f) => Some(*f as i64),
            _ => None,
        }
    }
    fn float(&self, k: &str) -> Option<f64> {
        match self.kv.get(k)? {
            Val::F(f) => Some(*f),
            Val::Int(v) => Some(*v as f64),
            _ => None,
        }
    }
    fn text(&self, k: &str) -> Option<&[u8]> {
        match self.kv.get(k)? {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A tensor as f32 values (quantized blocks expanded) and its dims.
    fn tensor(&self, name: &str) -> Result<(Vec<f32>, Vec<u64>), String> {
        let t = self.tensors.get(name).ok_or_else(|| format!("tensor {} missing", name))?;
        let n = t.dims.iter().product::<u64>() as usize;
        let at = self.base + t.off as usize;
        let need = match t.ty {
            0 => n * 4,
            1 => n * 2,
            8 => n / 32 * 34,
            2 => n / 32 * 18,
            other => return Err(format!("tensor {}: type {} not supported (F32, F16, Q8_0, Q4_0 are)", name, other)),
        };
        let raw = self.data.get(at..at + need).ok_or_else(|| format!("tensor {} past the end of the file", name))?;
        let mut v = Vec::with_capacity(n);
        match t.ty {
            0 => {
                for c in raw.chunks_exact(4) {
                    v.push(f32::from_le_bytes([c[0], c[1], c[2], c[3]]));
                }
            }
            1 => {
                for c in raw.chunks_exact(2) {
                    v.push(f16(u16::from_le_bytes([c[0], c[1]])));
                }
            }
            8 => {
                for b in raw.chunks_exact(34) {
                    let d = f16(u16::from_le_bytes([b[0], b[1]]));
                    for &q in &b[2..] {
                        v.push(q as i8 as f32 * d);
                    }
                }
            }
            _ => {
                for b in raw.chunks_exact(18) {
                    let d = f16(u16::from_le_bytes([b[0], b[1]]));
                    let qs = &b[2..];
                    for &q in qs {
                        v.push(((q & 15) as i32 - 8) as f32 * d);
                    }
                    for &q in qs {
                        v.push(((q >> 4) as i32 - 8) as f32 * d);
                    }
                }
            }
        }
        Ok((v, t.dims.clone()))
    }
}

/// IEEE half -> f32.
fn f16(h: u16) -> f32 {
    let s = ((h >> 15) as u32) << 31;
    let e = ((h >> 10) & 0x1F) as u32;
    let m = (h & 0x3FF) as u32;
    let bits = if e == 0 {
        if m == 0 {
            s
        } else {
            // subnormal: normalize
            let mut e2 = 127 - 15 + 1;
            let mut m2 = m;
            while m2 & 0x400 == 0 {
                m2 <<= 1;
                e2 -= 1;
            }
            s | (e2 << 23) | ((m2 & 0x3FF) << 13)
        }
    } else if e == 31 {
        s | 0x7F80_0000 | (m << 13)
    } else {
        s | ((e + 127 - 15) << 23) | (m << 13)
    };
    f32::from_bits(bits)
}

// ---- GPU compute (COMPUTE_OP) --------------------------------------------------

const COMPUTE_OP: u64 = 72;

/// A matrix on the GPU (padded to the backend's alignments).
struct GpuMat {
    h: u64,
    rows: usize,
    rows_p: usize,
    cols_p: usize,
}

pub struct Gpu {
    pub name: String,
    pub kind: u64,
    ra: usize,
    ca: usize,
    x: u64,
    y: u64,
    xbuf: Vec<f32>,
    pub us: u64,
    pub calls: u64,
}

fn bytes_of(v: &[f32]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 4) }
}

impl Gpu {
    /// The kernel's compute backend, if a program should use it (`force`:
    /// even the kernel's CPU reference, to test the path).
    pub fn open(force: bool, max_rows: usize, max_cols: usize) -> Option<Gpu> {
        let mut name = vec![0u8; 64];
        let r = sys::syscall(COMPUTE_OP, 0, name.as_mut_ptr() as u64, name.len() as u64);
        if sys::is_err(r) {
            return None;
        }
        let kind = r & 0xFFFF;
        if kind == 0 && !force {
            return None;
        }
        let (ra, ca) = (((r >> 16) & 0xFFFF).max(1) as usize, ((r >> 32) & 0xFFFF).max(1) as usize);
        let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        let name = String::from_utf8_lossy(&name[..end]).into_owned();
        let (rp, cp) = (max_rows.div_ceil(ra) * ra, max_cols.div_ceil(ca) * ca);
        let x = alloc_buf(cp * 4)?;
        let y = alloc_buf(rp * 4)?;
        Some(Gpu { name, kind, ra, ca, x, y, xbuf: vec![0.0; cp], us: 0, calls: 0 })
    }

    fn upload(&self, w: &[f32], rows: usize, cols: usize) -> Option<GpuMat> {
        let (rows_p, cols_p) = (rows.div_ceil(self.ra) * self.ra, cols.div_ceil(self.ca) * self.ca);
        let h = alloc_buf(rows_p * cols_p * 4)?;
        // padded rows, a block of rows per WRITE
        let block = (1 << 20) / (cols_p * 4).max(1);
        let block = block.max(1);
        let mut r0 = 0;
        while r0 < rows_p {
            let n = block.min(rows_p - r0);
            let mut tmp = vec![0f32; n * cols_p];
            for r in 0..n {
                if r0 + r < rows {
                    tmp[r * cols_p..r * cols_p + cols].copy_from_slice(&w[(r0 + r) * cols..(r0 + r + 1) * cols]);
                }
            }
            if !buf_write(h, r0 * cols_p * 4, bytes_of(&tmp)) {
                return None;
            }
            r0 += n;
        }
        Some(GpuMat { h, rows, rows_p, cols_p })
    }

    fn gemv(&mut self, m: &GpuMat, x: &[f32], out: &mut [f32]) -> bool {
        self.xbuf[..x.len()].copy_from_slice(x);
        for v in &mut self.xbuf[x.len()..m.cols_p] {
            *v = 0.0;
        }
        if !buf_write(self.x, 0, bytes_of(&self.xbuf[..m.cols_p])) {
            return false;
        }
        let r = sys::syscall(
            COMPUTE_OP,
            5 | m.h << 8,
            self.x | self.y << 32,
            m.rows_p as u64 | (m.cols_p as u64) << 32,
        );
        if sys::is_err(r) {
            return false;
        }
        self.us += r;
        self.calls += 1;
        let out = &mut out[..m.rows];
        let n = out.len() * 4;
        let r = sys::syscall(COMPUTE_OP, 3 | self.y << 8, out.as_mut_ptr() as u64, n as u64);
        !sys::is_err(r)
    }
}

fn alloc_buf(bytes: usize) -> Option<u64> {
    let r = sys::syscall(COMPUTE_OP, 1, bytes as u64, 0);
    (!sys::is_err(r)).then_some(r)
}

fn buf_write(h: u64, off: usize, data: &[u8]) -> bool {
    let r = sys::syscall(COMPUTE_OP, 2 | h << 8, data.as_ptr() as u64, (off as u64) << 32 | data.len() as u64);
    !sys::is_err(r)
}

// ---- the model ---------------------------------------------------------------

struct Layer {
    attn_norm: Vec<f32>,
    ffn_norm: Vec<f32>,
    wq: Vec<f32>,
    wk: Vec<f32>,
    wv: Vec<f32>,
    wo: Vec<f32>,
    w1: Vec<f32>, // gate
    w2: Vec<f32>, // down
    w3: Vec<f32>, // up
    g_qkv: Option<GpuMat>,
    g_o: Option<GpuMat>,
    g_13: Option<GpuMat>,
    g_2: Option<GpuMat>,
}

pub struct Model {
    pub dim: usize,
    pub hidden: usize,
    pub layers_n: usize,
    pub heads: usize,
    pub kv_heads: usize,
    pub vocab: usize,
    pub seq: usize,
    head_dim: usize,
    rot: usize,
    eps: f32,
    theta: f32,
    emb: Vec<f32>,
    out_norm: Vec<f32>,
    out_w: Option<Vec<f32>>, // None = tied to emb
    g_out: Option<GpuMat>,
    layers: Vec<Layer>,
    pub tokens: Vec<Vec<u8>>,
    scores: Vec<f32>,
    index: BTreeMap<Vec<u8>, u32>,
    pub bos: u32,
    pub eos: u32,
    #[allow(dead_code)]
    pub name: String,
    // state
    x: Vec<f32>,
    xb: Vec<f32>,
    xb2: Vec<f32>,
    qkv: Vec<f32>,
    hb: Vec<f32>,
    hb2: Vec<f32>,
    att: Vec<f32>,
    pub logits: Vec<f32>,
    kc: Vec<f32>,
    vc: Vec<f32>,
    pub gpu: Option<Gpu>,
}

fn rows_of(dims: &[u64]) -> usize {
    dims.get(1).copied().unwrap_or(1) as usize
}

impl Model {
    pub fn load(g: &Gguf, max_seq: usize) -> Result<Model, String> {
        let arch = g.text("general.architecture").unwrap_or(b"?");
        if arch != b"llama" {
            return Err(format!("architecture {} (only llama models)", String::from_utf8_lossy(arch)));
        }
        let int = |k: &str| g.int(k).ok_or_else(|| format!("{} missing", k));
        let dim = int("llama.embedding_length")? as usize;
        let hidden = int("llama.feed_forward_length")? as usize;
        let layers_n = int("llama.block_count")? as usize;
        let heads = int("llama.attention.head_count")? as usize;
        let kv_heads = g.int("llama.attention.head_count_kv").unwrap_or(heads as i64) as usize;
        let ctx = g.int("llama.context_length").unwrap_or(512) as usize;
        if heads == 0 || kv_heads == 0 || dim % heads != 0 || heads % kv_heads != 0 {
            return Err(String::from("inconsistent head counts"));
        }
        let head_dim = dim / heads;
        let rot = (g.int("llama.rope.dimension_count").unwrap_or(head_dim as i64) as usize).min(head_dim);
        let eps = g.float("llama.attention.layer_norm_rms_epsilon").unwrap_or(1e-5) as f32;
        let theta = g.float("llama.rope.freq_base").unwrap_or(10000.0) as f32;
        let tokens: Vec<Vec<u8>> = match g.kv.get("tokenizer.ggml.tokens") {
            Some(Val::Arr(a)) => a.iter().map(|v| if let Val::Str(s) = v { s.clone() } else { Vec::new() }).collect(),
            _ => return Err(String::from("no vocabulary")),
        };
        let scores: Vec<f32> = match g.kv.get("tokenizer.ggml.scores") {
            Some(Val::Arr(a)) => a.iter().map(|v| if let Val::F(f) = v { *f as f32 } else { 0.0 }).collect(),
            _ => vec![0.0; tokens.len()],
        };
        let mut index = BTreeMap::new();
        for (i, t) in tokens.iter().enumerate() {
            index.entry(t.clone()).or_insert(i as u32);
        }
        let (emb, ed) = g.tensor("token_embd.weight")?;
        let vocab = rows_of(&ed);
        let (out_norm, _) = g.tensor("output_norm.weight")?;
        let out_w = if g.tensors.contains_key("output.weight") { Some(g.tensor("output.weight")?.0) } else { None };
        let kvd = kv_heads * head_dim;
        let mut layers = Vec::with_capacity(layers_n);
        for l in 0..layers_n {
            let t = |n: &str| g.tensor(&format!("blk.{}.{}.weight", l, n)).map(|x| x.0);
            let layer = Layer {
                attn_norm: t("attn_norm")?,
                ffn_norm: t("ffn_norm")?,
                wq: t("attn_q")?,
                wk: t("attn_k")?,
                wv: t("attn_v")?,
                wo: t("attn_output")?,
                w1: t("ffn_gate")?,
                w2: t("ffn_down")?,
                w3: t("ffn_up")?,
                g_qkv: None,
                g_o: None,
                g_13: None,
                g_2: None,
            };
            if layer.wq.len() != dim * dim || layer.wk.len() != kvd * dim || layer.w1.len() != hidden * dim || layer.w2.len() != dim * hidden {
                return Err(format!("layer {}: unexpected weight shapes", l));
            }
            layers.push(layer);
        }
        let seq = ctx.min(max_seq).max(8);
        let name = g.text("general.name").map(|s| String::from_utf8_lossy(s).into_owned()).unwrap_or_default();
        Ok(Model {
            dim,
            hidden,
            layers_n,
            heads,
            kv_heads,
            vocab,
            seq,
            head_dim,
            rot,
            eps,
            theta,
            emb,
            out_norm,
            out_w,
            g_out: None,
            layers,
            scores,
            index,
            bos: g.int("tokenizer.ggml.bos_token_id").unwrap_or(1) as u32,
            eos: g.int("tokenizer.ggml.eos_token_id").unwrap_or(2) as u32,
            tokens,
            name,
            x: vec![0.0; dim],
            xb: vec![0.0; dim],
            xb2: vec![0.0; dim],
            qkv: vec![0.0; dim + 2 * kvd],
            hb: vec![0.0; 2 * hidden],
            hb2: vec![0.0; hidden],
            att: vec![0.0; seq],
            logits: vec![0.0; vocab],
            kc: vec![0.0; layers_n * seq * kvd],
            vc: vec![0.0; layers_n * seq * kvd],
            gpu: None,
        })
    }

    pub fn params(&self) -> usize {
        let kvd = self.kv_heads * self.head_dim;
        let per = 2 * self.dim + self.dim * self.dim * 2 + kvd * self.dim * 2 + 3 * self.hidden * self.dim;
        self.emb.len() + self.out_norm.len() + self.out_w.as_ref().map_or(0, |w| w.len()) + self.layers_n * per
    }

    /// Put every matrix on the GPU (Q/K/V and gate/up fused).
    pub fn attach_gpu(&mut self, force: bool) -> Result<(), String> {
        let kvd = self.kv_heads * self.head_dim;
        let max_rows = (self.dim + 2 * kvd).max(2 * self.hidden).max(self.vocab);
        let max_cols = self.dim.max(self.hidden);
        let gpu = Gpu::open(force, max_rows, max_cols).ok_or_else(|| String::from("no GPU compute backend"))?;
        let dim = self.dim;
        for l in &mut self.layers {
            let mut qkv = Vec::with_capacity((dim + 2 * kvd) * dim);
            qkv.extend_from_slice(&l.wq);
            qkv.extend_from_slice(&l.wk);
            qkv.extend_from_slice(&l.wv);
            l.g_qkv = Some(gpu.upload(&qkv, dim + 2 * kvd, dim).ok_or("GPU upload failed")?);
            l.g_o = Some(gpu.upload(&l.wo, dim, dim).ok_or("GPU upload failed")?);
            let mut w13 = Vec::with_capacity(2 * self.hidden * dim);
            w13.extend_from_slice(&l.w1);
            w13.extend_from_slice(&l.w3);
            l.g_13 = Some(gpu.upload(&w13, 2 * self.hidden, dim).ok_or("GPU upload failed")?);
            l.g_2 = Some(gpu.upload(&l.w2, dim, self.hidden).ok_or("GPU upload failed")?);
        }
        let ow = self.out_w.as_ref().unwrap_or(&self.emb);
        self.g_out = Some(gpu.upload(ow, self.vocab, dim).ok_or("GPU upload failed")?);
        self.gpu = Some(gpu);
        Ok(())
    }

    /// Clear the KV cache (a new conversation).
    pub fn reset(&mut self) {
        for v in self.kc.iter_mut().chain(self.vc.iter_mut()) {
            *v = 0.0;
        }
    }

    /// One token at position `pos` -> `self.logits`.
    pub fn forward(&mut self, token: u32, pos: usize) -> Result<(), String> {
        let (dim, hd, kvd) = (self.dim, self.head_dim, self.kv_heads * self.head_dim);
        if pos >= self.seq {
            return Err(String::from("context full"));
        }
        let t = (token as usize).min(self.vocab - 1);
        self.x.copy_from_slice(&self.emb[t * dim..(t + 1) * dim]);
        let group = self.heads / self.kv_heads;
        let scale = 1.0 / libm::sqrtf(hd as f32);
        for l in 0..self.layers_n {
            rmsnorm(&mut self.xb, &self.x, &self.layers[l].attn_norm, self.eps);
            // q k v
            let ly = &self.layers[l];
            match (&mut self.gpu, &ly.g_qkv) {
                (Some(g), Some(m)) => {
                    if !g.gemv(m, &self.xb, &mut self.qkv) {
                        return Err(String::from("GPU GEMV failed"));
                    }
                }
                _ => {
                    matvec(&mut self.qkv[..dim], &ly.wq, &self.xb);
                    matvec(&mut self.qkv[dim..dim + kvd], &ly.wk, &self.xb);
                    matvec(&mut self.qkv[dim + kvd..dim + 2 * kvd], &ly.wv, &self.xb);
                }
            }
            // rotary embedding (adjacent pairs) on q and k
            for i in (0..self.rot).step_by(2) {
                let freq = 1.0 / libm::powf(self.theta, i as f32 / hd as f32);
                let a = pos as f32 * freq;
                let (s, c) = (libm::sinf(a), libm::cosf(a));
                for h in 0..self.heads {
                    rotate(&mut self.qkv, h * hd + i, c, s);
                }
                for h in 0..self.kv_heads {
                    rotate(&mut self.qkv, dim + h * hd + i, c, s);
                }
            }
            let base = l * self.seq * kvd + pos * kvd;
            self.kc[base..base + kvd].copy_from_slice(&self.qkv[dim..dim + kvd]);
            self.vc[base..base + kvd].copy_from_slice(&self.qkv[dim + kvd..dim + 2 * kvd]);
            // attention per head
            for h in 0..self.heads {
                let kh = h / group;
                let q = &self.qkv[h * hd..(h + 1) * hd];
                let mut mx = f32::MIN;
                for p in 0..=pos {
                    let k = &self.kc[l * self.seq * kvd + p * kvd + kh * hd..][..hd];
                    let mut s = 0.0;
                    for i in 0..hd {
                        s += q[i] * k[i];
                    }
                    s *= scale;
                    self.att[p] = s;
                    if s > mx {
                        mx = s;
                    }
                }
                let mut sum = 0.0;
                for p in 0..=pos {
                    let e = libm::expf(self.att[p] - mx);
                    self.att[p] = e;
                    sum += e;
                }
                let out = &mut self.xb2[h * hd..(h + 1) * hd];
                for v in out.iter_mut() {
                    *v = 0.0;
                }
                for p in 0..=pos {
                    let a = self.att[p] / sum;
                    let v = &self.vc[l * self.seq * kvd + p * kvd + kh * hd..][..hd];
                    for i in 0..hd {
                        out[i] += a * v[i];
                    }
                }
            }
            // output projection + residual
            let ly = &self.layers[l];
            match (&mut self.gpu, &ly.g_o) {
                (Some(g), Some(m)) => {
                    if !g.gemv(m, &self.xb2, &mut self.xb) {
                        return Err(String::from("GPU GEMV failed"));
                    }
                }
                _ => matvec(&mut self.xb, &ly.wo, &self.xb2),
            }
            for i in 0..dim {
                self.x[i] += self.xb[i];
            }
            // feed-forward
            rmsnorm(&mut self.xb, &self.x, &self.layers[l].ffn_norm, self.eps);
            let hidden = self.hidden;
            let ly = &self.layers[l];
            match (&mut self.gpu, &ly.g_13) {
                (Some(g), Some(m)) => {
                    if !g.gemv(m, &self.xb, &mut self.hb) {
                        return Err(String::from("GPU GEMV failed"));
                    }
                }
                _ => {
                    let (a, b) = self.hb.split_at_mut(hidden);
                    matvec(a, &ly.w1, &self.xb);
                    matvec(&mut b[..hidden], &ly.w3, &self.xb);
                }
            }
            for i in 0..hidden {
                let gt = self.hb[i];
                self.hb2[i] = gt / (1.0 + libm::expf(-gt)) * self.hb[hidden + i];
            }
            match (&mut self.gpu, &ly.g_2) {
                (Some(g), Some(m)) => {
                    if !g.gemv(m, &self.hb2, &mut self.xb) {
                        return Err(String::from("GPU GEMV failed"));
                    }
                }
                _ => matvec(&mut self.xb, &ly.w2, &self.hb2),
            }
            for i in 0..dim {
                self.x[i] += self.xb[i];
            }
        }
        rmsnorm(&mut self.xb, &self.x, &self.out_norm, self.eps);
        match (&mut self.gpu, &self.g_out) {
            (Some(g), Some(m)) => {
                if !g.gemv(m, &self.xb, &mut self.logits) {
                    return Err(String::from("GPU GEMV failed"));
                }
            }
            _ => {
                let w = self.out_w.as_ref().unwrap_or(&self.emb);
                matvec(&mut self.logits, w, &self.xb);
            }
        }
        Ok(())
    }

    // ---- tokenizer ----

    pub fn encode(&self, text: &str, bos: bool) -> Vec<u32> {
        let mut toks = Vec::new();
        if bos {
            toks.push(self.bos);
        }
        // SentencePiece: a leading space, spaces as U+2581
        let mut s = String::from(" ");
        s.push_str(text);
        let s = s.replace(' ', "\u{2581}");
        let mut seq: Vec<u32> = Vec::new();
        let mut buf = [0u8; 4];
        for ch in s.chars() {
            let b = ch.encode_utf8(&mut buf).as_bytes();
            match self.index.get(b) {
                Some(&id) => seq.push(id),
                None => {
                    for byte in b {
                        let name = format!("<0x{:02X}>", byte);
                        seq.push(self.index.get(name.as_bytes()).copied().unwrap_or(0));
                    }
                }
            }
        }
        loop {
            let mut best = (f32::MIN, usize::MAX, 0u32);
            for i in 0..seq.len().saturating_sub(1) {
                let mut m = self.tokens[seq[i] as usize].clone();
                m.extend_from_slice(&self.tokens[seq[i + 1] as usize]);
                if let Some(&id) = self.index.get(&m) {
                    let sc = self.scores.get(id as usize).copied().unwrap_or(0.0);
                    if sc > best.0 {
                        best = (sc, i, id);
                    }
                }
            }
            if best.1 == usize::MAX {
                break;
            }
            seq[best.1] = best.2;
            seq.remove(best.1 + 1);
        }
        toks.extend(seq);
        toks
    }

    pub fn decode(&self, t: u32) -> Vec<u8> {
        let s = self.tokens.get(t as usize).map(|v| v.as_slice()).unwrap_or(b"");
        if s.len() == 6 && s.starts_with(b"<0x") && s[5] == b'>' {
            if let Ok(v) = u8::from_str_radix(core::str::from_utf8(&s[3..5]).unwrap_or("00"), 16) {
                return vec![v];
            }
        }
        let mut out = Vec::with_capacity(s.len());
        let mut i = 0;
        while i < s.len() {
            if s[i..].starts_with("\u{2581}".as_bytes()) {
                out.push(b' ');
                i += 3;
            } else {
                out.push(s[i]);
                i += 1;
            }
        }
        out
    }
}

fn rmsnorm(out: &mut [f32], x: &[f32], w: &[f32], eps: f32) {
    let mut ss = 0.0;
    for v in x {
        ss += v * v;
    }
    let k = 1.0 / libm::sqrtf(ss / x.len() as f32 + eps);
    for i in 0..x.len() {
        out[i] = x[i] * k * w[i];
    }
}

fn rotate(v: &mut [f32], i: usize, c: f32, s: f32) {
    let (a, b) = (v[i], v[i + 1]);
    v[i] = a * c - b * s;
    v[i + 1] = a * s + b * c;
}

/// out[r] = sum_c w[r][c] x[c]
fn matvec(out: &mut [f32], w: &[f32], x: &[f32]) {
    let n = x.len();
    for (r, o) in out.iter_mut().enumerate() {
        let row = &w[r * n..(r + 1) * n];
        let mut s = 0.0;
        for c in 0..n {
            s += row[c] * x[c];
        }
        *o = s;
    }
}

// ---- sampling + generation --------------------------------------------------------

fn argmax(v: &[f32]) -> u32 {
    let mut best = 0;
    for i in 1..v.len() {
        if v[i] > v[best] {
            best = i;
        }
    }
    best as u32
}

struct Rng(u64);
impl Rng {
    fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn sample(logits: &[f32], temp: f32, rng: &mut Rng) -> u32 {
    if temp <= 0.0 {
        return argmax(logits);
    }
    let mx = logits.iter().cloned().fold(f32::MIN, f32::max);
    let mut p: Vec<f32> = logits.iter().map(|&l| libm::expf((l - mx) / temp)).collect();
    let sum: f32 = p.iter().sum();
    let mut r = rng.next_f32() * sum;
    for (i, v) in p.iter_mut().enumerate() {
        r -= *v;
        if r <= 0.0 {
            return i as u32;
        }
    }
    (logits.len() - 1) as u32
}

/// A generation in progress, one token per [`Stream::next`] -- what the app
/// natives (`aiapi.rs`) and the script functions use to show text as it
/// arrives instead of after the whole run.
pub struct Stream {
    toks: Vec<u32>,
    tok: u32,
    pos: usize,
    steps: usize,
    temp: f32,
    rng: Rng,
    pub generated: usize,
    pub prompt_tokens: usize,
    pub t0: u64,
    pub done: bool,
}

impl Stream {
    /// Reset the model's cache and tokenize `prompt` (with BOS).
    pub fn begin(m: &mut Model, prompt: &str, steps: usize, temp: f32, seed: u64) -> Stream {
        m.reset();
        let toks = m.encode(prompt, true);
        let tok = toks[0];
        Stream { prompt_tokens: toks.len(), toks, tok, pos: 0, steps, temp, rng: Rng(seed | 1), generated: 0, t0: now_ms(), done: false }
    }

    /// Run the model up to the next generated token and return its text (may
    /// be empty for a partial UTF-8 byte); None once the run is over (`steps`
    /// tokens, end of sequence or the context is full).
    pub fn next(&mut self, m: &mut Model) -> Result<Option<Vec<u8>>, String> {
        while !self.done {
            if self.generated >= self.steps || self.pos >= m.seq {
                self.done = true;
                break;
            }
            m.forward(self.tok, self.pos)?;
            self.pos += 1;
            if self.pos < self.toks.len() {
                self.tok = self.toks[self.pos];
                continue;
            }
            self.tok = sample(&m.logits, self.temp, &mut self.rng);
            if self.tok == m.eos {
                self.done = true;
                break;
            }
            self.generated += 1;
            return Ok(Some(m.decode(self.tok)));
        }
        Ok(None)
    }

    pub fn elapsed_ms(&self) -> u64 {
        now_ms() - self.t0
    }
}

/// Generate up to `steps` tokens after `prompt`; `emit` gets each piece of
/// text as it is produced. Returns (generated tokens, prompt tokens, ms).
pub fn generate(m: &mut Model, prompt: &str, steps: usize, temp: f32, seed: u64, emit: &mut dyn FnMut(&[u8])) -> Result<(Vec<u32>, usize, u64), String> {
    let mut st = Stream::begin(m, prompt, steps, temp, seed);
    let mut out = Vec::new();
    while let Some(piece) = st.next(m)? {
        out.push(st.tok);
        emit(&piece);
    }
    Ok((out, st.prompt_tokens, st.elapsed_ms()))
}

fn now_ms() -> u64 {
    sys::syscall(crate::threads::CLOCK_NANOS, 0, 0, 0) / 1_000_000
}

// ---- the program -------------------------------------------------------------------

/// `.gguf` / `.ggu` files are models: `CLRHOST.ELF MODEL [prompt words] [--steps N]
/// [--temp T] [--gpu | --cpu | --gpu-force] [--selftest]`.
pub fn is_model(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.ends_with(".gguf") || p.ends_with(".ggu")
}

pub fn run(path: &str, args: Vec<String>) -> ! {
    let mut steps = 64usize;
    let mut temp = 0.0f32;
    let mut mode = "auto";
    let mut selftest = false;
    let mut words: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--steps" if i + 1 < args.len() => {
                steps = args[i + 1].parse().unwrap_or(steps);
                i += 1;
            }
            "--temp" if i + 1 < args.len() => {
                temp = parse_f32(&args[i + 1]).unwrap_or(0.0);
                i += 1;
            }
            "--gpu" => mode = "gpu",
            "--gpu-force" => mode = "force",
            "--cpu" => mode = "cpu",
            "--selftest" => selftest = true,
            w => words.push(w),
        }
        i += 1;
    }
    let Some(bytes) = sys::read_file(path) else {
        sys::print(&format!("[llm] {}: file not found\n", path));
        sys::exit(3);
    };
    let g = match Gguf::parse(&bytes) {
        Ok(g) => g,
        Err(e) => {
            sys::print(&format!("[llm] {}: {}\n", path, e));
            sys::exit(4);
        }
    };
    let mut m = match Model::load(&g, 512) {
        Ok(m) => m,
        Err(e) => {
            sys::print(&format!("[llm] {}: {}\n", path, e));
            sys::exit(5);
        }
    };
    drop(g);
    sys::print(&format!(
        "[llm] {}: llama, {} layers, dim {}, {} heads ({} KV), vocab {}, {} parameters\n",
        path,
        m.layers_n,
        m.dim,
        m.heads,
        m.kv_heads,
        m.vocab,
        m.params()
    ));
    if selftest {
        let ok = self_test(&mut m);
        sys::exit(if ok { 0 } else { 1 });
    }
    if mode != "cpu" {
        match m.attach_gpu(mode == "force") {
            Ok(()) => sys::print(&format!("[llm] matrices on the GPU: {}\n", m.gpu.as_ref().unwrap().name)),
            Err(e) if mode == "gpu" || mode == "force" => sys::print(&format!("[llm] GPU: {} -- using the CPU\n", e)),
            Err(_) => {}
        }
    }
    let prompt = if words.is_empty() { String::from("Once upon a time") } else { words.join(" ") };
    sys::print(&prompt);
    let r = generate(&mut m, &prompt, steps, temp, now_ms(), &mut |b| sys::print(&String::from_utf8_lossy(b)));
    match r {
        Ok((out, _, ms)) => {
            sys::print(&format!(
                "\n[llm] {} tokens in {} ms ({} tok/s, {})\n",
                out.len(),
                ms,
                out.len() as u64 * 1000 / ms.max(1),
                m.gpu.as_ref().map_or(String::from("CPU"), |g| format!("GPU {}", g.name))
            ));
            sys::exit(0)
        }
        Err(e) => {
            sys::print(&format!("\n[llm] {}\n", e));
            sys::exit(1)
        }
    }
}

fn parse_f32(s: &str) -> Option<f32> {
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    let mut v = int.parse::<i32>().ok()? as f32;
    let mut k = 0.1;
    for c in frac.chars() {
        v += c.to_digit(10)? as f32 * k;
        k /= 10.0;
    }
    Some(v)
}

/// Boot check: tokenizer + greedy text against the numpy reference, then
/// the same generation with every matrix product through COMPUTE_OP (the GPU
/// backend, or the kernel's CPU reference when there is no GPU): the tokens
/// must match.
fn self_test(m: &mut Model) -> bool {
    let toks = m.encode("Once upon a time", true);
    let want_toks = [1u32, 403, 407, 261, 378];
    let tok_ok = m.vocab != 512 || toks == want_toks;
    let cpu = generate(m, "Once upon a time", 24, 0.0, 1, &mut |_| {});
    let Ok((cpu_out, _, cpu_ms)) = cpu else {
        sys::print("[llm] self-test: CPU generation failed\n");
        return false;
    };
    let text: Vec<u8> = cpu_out.iter().flat_map(|&t| m.decode(t)).collect();
    let text = String::from_utf8_lossy(&text).into_owned();
    let want = ", there was a little girl named Lily.";
    let text_ok = m.vocab != 512 || text.starts_with(want);
    sys::print(&format!("[llm] CPU: \"Once upon a time{}\" ({} ms)\n", text, cpu_ms));
    let gpu = m.attach_gpu(true).and_then(|()| generate(m, "Once upon a time", 24, 0.0, 1, &mut |_| {}));
    let (gpu_ok, gpu_note) = match gpu {
        Ok((out, _, ms)) => {
            let g = m.gpu.as_ref().unwrap();
            let same = out == cpu_out;
            (
                same,
                format!(
                    "{} ({}): {} GEMV, {} ms, tokens {}",
                    g.name,
                    if g.kind == 0 { "kernel reference" } else { "GPU" },
                    g.calls,
                    ms,
                    if same { "identical" } else { "DIFFERENT" }
                ),
            )
        }
        Err(e) => (false, format!("offload failed: {}", e)),
    };
    let ok = tok_ok && text_ok && gpu_ok;
    if ok {
        sys::print(&format!(
            "MILESTONE: LLM OK ({} parameters, 24 tokens greedy in {} ms on the CPU; COMPUTE_OP {})\n",
            m.params(),
            cpu_ms,
            gpu_note
        ));
    } else {
        sys::print(&format!(
            "[llm] self-test FAILED: tokenizer {} {:?}, text {}, offload {}\n",
            if tok_ok { "ok" } else { "WRONG" },
            toks,
            if text_ok { "ok" } else { "WRONG" },
            gpu_note
        ));
    }
    ok
}
