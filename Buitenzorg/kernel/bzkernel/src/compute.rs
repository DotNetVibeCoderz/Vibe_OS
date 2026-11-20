//! Compute API: data-parallel work for the compositor and the AI subsystem.
//!
//! Two layers:
//!
//! * `saxpy` / `blend_buffers` -- small CPU kernels (v0.11 "Cahaya").
//! * **GPU compute** (`COMPUTE_OP`): buffers owned by a process and a GEMV
//!   (`y = W x`, f32) -- the operation that dominates LLM inference at batch 1.
//!   A GPU driver installs a [`Native`] backend (V3D on the Raspberry Pi 4);
//!   without one, a **kernel CPU reference** backend answers the same calls so
//!   programs and the self-test can exercise the path anywhere. INFO tells a
//!   program which it got (`kind` 0 = the reference: slower than doing the
//!   math in the program itself, so programs use it only when asked to).
//!
//! Lock order: `BUFS` is held only to look a buffer up; backends are called
//! without it (they take their own locks, e.g. the V3D GPU lock).

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

/// Which backend executed a dispatch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    /// The CPU.
    Cpu,
    /// A GPU compute backend.
    Gpu,
}

/// The backend behind `saxpy` / `blend_buffers` (always the CPU) or, when a
/// GPU compute backend is installed, `Gpu` for GEMV.
pub fn backend() -> Backend {
    if native().is_some() {
        Backend::Gpu
    } else {
        Backend::Cpu
    }
}

/// Elementwise SAXPY: `y[i] = a * x[i] + y[i]`.
pub fn saxpy(a: f32, x: &[f32], y: &mut [f32]) -> Backend {
    let n = x.len().min(y.len());
    for i in 0..n {
        y[i] = a * x[i] + y[i];
    }
    Backend::Cpu
}

/// Blend two pixel buffers `out = src*alpha + dst*(1-alpha)`.
pub fn blend_buffers(dst: &mut [u32], src: &[u32], alpha: u8) -> Backend {
    let n = dst.len().min(src.len());
    let a = alpha as u32;
    for i in 0..n {
        let s = src[i];
        let d = dst[i];
        let mix = |sh: u32| -> u32 {
            let sc = (s >> sh) & 0xFF;
            let dc = (d >> sh) & 0xFF;
            (sc * a + dc * (255 - a)) / 255
        };
        dst[i] = (mix(16) << 16) | (mix(8) << 8) | mix(0);
    }
    Backend::Cpu
}

/// Self-test / benchmark of the CPU kernels: SAXPY over `n` elements.
pub fn selftest(n: usize) -> (Backend, u64) {
    let x = vec![1.0f32; n];
    let mut y = vec![2.0f32; n];
    let backend = saxpy(3.0, &x, &mut y);
    let checksum = y.iter().map(|&v| v as u64).sum();
    (backend, checksum)
}

// ---- GPU compute -----------------------------------------------------------

/// A GPU compute backend (function table, installed at run time).
pub struct Native {
    pub name: &'static str,
    /// `bz_abi::compute_kind` value.
    pub kind: u64,
    /// GEMV needs rows / cols to be multiples of these (callers pad with zeros).
    pub row_align: u32,
    pub col_align: u32,
    pub alloc: fn(bytes: usize) -> Result<usize, &'static str>,
    pub free: fn(h: usize),
    pub write: fn(h: usize, off: usize, data: &[u8]) -> Result<(), &'static str>,
    pub read: fn(h: usize, off: usize, out: &mut [u8]) -> Result<(), &'static str>,
    /// `y[r] = sum_c W[r][c] x[c]`; returns the GPU time in microseconds.
    pub gemv: fn(w: usize, x: usize, y: usize, rows: u32, cols: u32) -> Result<u64, &'static str>,
}

static NATIVE: Mutex<Option<&'static Native>> = Mutex::new(None);
static ENABLED: AtomicBool = AtomicBool::new(true);
static GEMVS: AtomicU64 = AtomicU64::new(0);
static GEMV_US: AtomicU64 = AtomicU64::new(0);
static GEMV_MACS: AtomicU64 = AtomicU64::new(0);

/// The GPU backend new buffers go to (None = the kernel reference).
pub fn native() -> Option<&'static Native> {
    if !ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    *NATIVE.lock()
}

/// `compute gpu` / `compute ref`: whether new buffers use the GPU backend.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Install a GPU backend after it passed the GEMV self-test against the
/// reference. Returns the log line (`MILESTONE: COMPUTE GPU OK (...)` or why not).
pub fn install_native(n: &'static Native) -> String {
    match gemv_self_test(n) {
        Ok(msg) => {
            *NATIVE.lock() = Some(n);
            format!("MILESTONE: COMPUTE GPU OK ({}: {})", n.name, msg)
        }
        Err(e) => format!("[compute] {} self-test FAILED: {} -- GEMV stays on the CPU", n.name, e),
    }
}

/// A test matrix whose products and sums are exact in f32 (multiples of
/// 1/16 with small magnitudes), so any summation order gives the same bits.
fn test_case(rows: usize, cols: usize) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let w: Vec<f32> = (0..rows * cols).map(|i| (((i * 7 + i / cols * 3) % 13) as f32 - 6.0) * 0.25).collect();
    let x: Vec<f32> = (0..cols).map(|c| ((c % 5) as f32 - 2.0) * 0.25).collect();
    let mut y = vec![0f32; rows];
    for r in 0..rows {
        let mut s = 0f32;
        for c in 0..cols {
            s += w[r * cols + c] * x[c];
        }
        y[r] = s;
    }
    (w, x, y)
}

fn f32_bytes(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for f in v {
        b.extend_from_slice(&f.to_bits().to_le_bytes());
    }
    b
}

/// GEMV on `n` vs the reference result, twice (the second run must not see
/// stale caches), plus a timing of a larger matrix.
fn gemv_self_test(n: &Native) -> Result<String, String> {
    let (rows, cols) = (n.row_align.max(1) as usize * 2, n.col_align.max(1) as usize * 3);
    let (w, x, want) = test_case(rows, cols);
    let hw = (n.alloc)(w.len() * 4).map_err(String::from)?;
    let hx = (n.alloc)(x.len() * 4).map_err(String::from)?;
    let hy = (n.alloc)(rows * 4).map_err(String::from)?;
    let mut result = Ok(());
    let mut us = 0;
    for round in 0..2 {
        let x2: Vec<f32> = if round == 0 { x.clone() } else { x.iter().map(|v| -v).collect() };
        let r = (|| -> Result<u64, &'static str> {
            (n.write)(hw, 0, &f32_bytes(&w))?;
            (n.write)(hx, 0, &f32_bytes(&x2))?;
            (n.write)(hy, 0, &vec![0xEE; rows * 4])?;
            let t = (n.gemv)(hw, hx, hy, rows as u32, cols as u32)?;
            let mut out = vec![0u8; rows * 4];
            (n.read)(hy, 0, &mut out)?;
            for r in 0..rows {
                let got = f32::from_bits(u32::from_le_bytes([out[r * 4], out[r * 4 + 1], out[r * 4 + 2], out[r * 4 + 3]]));
                let exp = if round == 0 { want[r] } else { -want[r] };
                if got.to_bits() != exp.to_bits() {
                    crate::println!("[compute] row {}: got {:?}, want {:?} (round {})", r, got, exp, round);
                    return Err("wrong result");
                }
            }
            Ok(t)
        })();
        match r {
            Ok(t) => us += t,
            Err(e) => {
                result = Err(String::from(e));
                break;
            }
        }
    }
    for h in [hw, hx, hy] {
        (n.free)(h);
    }
    result?;
    if n.kind == bz_abi::compute_kind::REFERENCE {
        return Ok(format!("{}x{} GEMV exact twice ({} us)", rows, cols, us));
    }
    // a bigger GEMV for a speed figure (contents do not matter)
    let (br, bc) = (round_up(1024, n.row_align), round_up(1024, n.col_align));
    let speed = (|| -> Result<u64, &'static str> {
        let (bw, bx, by) = ((n.alloc)(br * bc * 4)?, (n.alloc)(bc * 4)?, (n.alloc)(br * 4)?);
        let t = (n.gemv)(bw, bx, by, br as u32, bc as u32);
        for h in [bw, bx, by] {
            (n.free)(h);
        }
        t
    })();
    Ok(match speed {
        Ok(t) => format!(
            "{}x{} GEMV exact twice ({} us); {}x{} in {} us = {} MFLOP/s",
            rows,
            cols,
            us,
            br,
            bc,
            t,
            (2 * br * bc) as u64 / t.max(1)
        ),
        Err(e) => format!("{}x{} GEMV exact twice ({} us); large GEMV failed: {}", rows, cols, us, e),
    })
}

fn round_up(v: usize, a: u32) -> usize {
    let a = a.max(1) as usize;
    v.div_ceil(a) * a
}

// ---- the kernel CPU reference backend ----------------------------------------

/// Total bytes the reference backend holds (it lives on the kernel heap).
const REF_BUDGET: usize = 16 << 20;
static REF_BUFS: Mutex<Vec<Option<Vec<u8>>>> = Mutex::new(Vec::new());
static REF_USED: AtomicU64 = AtomicU64::new(0);

fn ref_alloc(bytes: usize) -> Result<usize, &'static str> {
    if REF_USED.load(Ordering::Relaxed) as usize + bytes > REF_BUDGET {
        return Err("reference backend budget (16 MiB) exceeded");
    }
    REF_USED.fetch_add(bytes as u64, Ordering::Relaxed);
    let mut b = REF_BUFS.lock();
    b.push(Some(vec![0u8; bytes]));
    Ok(b.len())
}
fn ref_free(h: usize) {
    if let Some(v) = REF_BUFS.lock().get_mut(h.wrapping_sub(1)).and_then(|b| b.take()) {
        REF_USED.fetch_sub(v.len() as u64, Ordering::Relaxed);
    }
}
fn ref_write(h: usize, off: usize, data: &[u8]) -> Result<(), &'static str> {
    let mut b = REF_BUFS.lock();
    let v = b.get_mut(h.wrapping_sub(1)).and_then(|b| b.as_mut()).ok_or("no such buffer")?;
    v.get_mut(off..off + data.len()).ok_or("out of range")?.copy_from_slice(data);
    Ok(())
}
fn ref_read(h: usize, off: usize, out: &mut [u8]) -> Result<(), &'static str> {
    let b = REF_BUFS.lock();
    let v = b.get(h.wrapping_sub(1)).and_then(|b| b.as_ref()).ok_or("no such buffer")?;
    out.copy_from_slice(v.get(off..off + out.len()).ok_or("out of range")?);
    Ok(())
}
fn ref_gemv(w: usize, x: usize, y: usize, rows: u32, cols: u32) -> Result<u64, &'static str> {
    let t0 = crate::arch::cpu::nanos();
    let mut b = REF_BUFS.lock();
    let get = |b: &Vec<Option<Vec<u8>>>, h: usize| -> Result<Vec<f32>, &'static str> {
        let v = b.get(h.wrapping_sub(1)).and_then(|b| b.as_ref()).ok_or("no such buffer")?;
        Ok(v.chunks_exact(4).map(|c| f32::from_bits(u32::from_le_bytes([c[0], c[1], c[2], c[3]]))).collect())
    };
    let (wv, xv) = (get(&b, w)?, get(&b, x)?);
    let (rows, cols) = (rows as usize, cols as usize);
    let mut out = Vec::with_capacity(rows * 4);
    for r in 0..rows {
        let row = &wv[r * cols..r * cols + cols];
        let mut s = 0f32;
        for c in 0..cols {
            s += row[c] * xv[c];
        }
        out.extend_from_slice(&s.to_bits().to_le_bytes());
    }
    let yv = b.get_mut(y.wrapping_sub(1)).and_then(|b| b.as_mut()).ok_or("no such buffer")?;
    yv.get_mut(..out.len()).ok_or("out of range")?.copy_from_slice(&out);
    Ok((crate::arch::cpu::nanos() - t0) / 1000)
}

static REFERENCE: Native = Native {
    name: "kernel CPU (reference)",
    kind: bz_abi::compute_kind::REFERENCE,
    row_align: 1,
    col_align: 1,
    alloc: ref_alloc,
    free: ref_free,
    write: ref_write,
    read: ref_read,
    gemv: ref_gemv,
};

// ---- buffers owned by processes ---------------------------------------------

struct Buf {
    owner: u64,
    backend: &'static Native,
    h: usize,
    bytes: usize,
}

static BUFS: Mutex<Vec<Option<Buf>>> = Mutex::new(Vec::new());

fn current() -> &'static Native {
    native().unwrap_or(&REFERENCE)
}

fn lookup(owner: u64, id: u64) -> Option<(&'static Native, usize, usize)> {
    let b = BUFS.lock();
    let x = b.get((id as usize).wrapping_sub(1))?.as_ref()?;
    (x.owner == owner).then_some((x.backend, x.h, x.bytes))
}

/// Free every buffer of a process (it exited).
pub fn release(owner: u64) {
    let mine: Vec<Buf> = {
        let mut b = BUFS.lock();
        b.iter_mut().filter(|x| x.as_ref().is_some_and(|x| x.owner == owner)).filter_map(|x| x.take()).collect()
    };
    for x in mine {
        (x.backend.free)(x.h);
    }
}

pub fn alloc(owner: u64, bytes: usize) -> Result<u64, &'static str> {
    if bytes == 0 || bytes > 1 << 30 {
        return Err("bad size");
    }
    let n = current();
    let h = (n.alloc)(bytes)?;
    let mut b = BUFS.lock();
    let slot = Buf { owner, backend: n, h, bytes };
    if let Some(i) = b.iter().position(|x| x.is_none()) {
        b[i] = Some(slot);
        Ok(i as u64 + 1)
    } else {
        b.push(Some(slot));
        Ok(b.len() as u64)
    }
}

pub fn free(owner: u64, id: u64) -> bool {
    let x = {
        let mut b = BUFS.lock();
        match b.get_mut((id as usize).wrapping_sub(1)) {
            Some(s) if s.as_ref().is_some_and(|x| x.owner == owner) => s.take(),
            _ => None,
        }
    };
    match x {
        Some(x) => {
            (x.backend.free)(x.h);
            true
        }
        None => false,
    }
}

pub fn write(owner: u64, id: u64, off: usize, data: &[u8]) -> Result<(), &'static str> {
    let (n, h, bytes) = lookup(owner, id).ok_or("no such buffer")?;
    if off.checked_add(data.len()).is_none_or(|e| e > bytes) {
        return Err("out of range");
    }
    (n.write)(h, off, data)
}

pub fn read(owner: u64, id: u64, off: usize, out: &mut [u8]) -> Result<(), &'static str> {
    let (n, h, bytes) = lookup(owner, id).ok_or("no such buffer")?;
    if off.checked_add(out.len()).is_none_or(|e| e > bytes) {
        return Err("out of range");
    }
    (n.read)(h, off, out)
}

pub fn gemv(owner: u64, w: u64, x: u64, y: u64, rows: u32, cols: u32) -> Result<u64, &'static str> {
    let (nw, hw, bw) = lookup(owner, w).ok_or("no such buffer")?;
    let (nx, hx, bx) = lookup(owner, x).ok_or("no such buffer")?;
    let (ny, hy, by) = lookup(owner, y).ok_or("no such buffer")?;
    if !core::ptr::eq(nw, nx) || !core::ptr::eq(nw, ny) {
        return Err("buffers of different backends");
    }
    let (r, c) = (rows as usize, cols as usize);
    if rows == 0 || cols == 0 || rows % nw.row_align != 0 || cols % nw.col_align != 0 {
        return Err("rows / cols not aligned");
    }
    if bw < r * c * 4 || bx < c * 4 || by < r * 4 {
        return Err("buffer too small");
    }
    let t = (nw.gemv)(hw, hx, hy, rows, cols)?;
    GEMVS.fetch_add(1, Ordering::Relaxed);
    GEMV_US.fetch_add(t, Ordering::Relaxed);
    GEMV_MACS.fetch_add((r * c) as u64, Ordering::Relaxed);
    Ok(t)
}

/// The backend new buffers go to: (kind, row alignment, column alignment, name).
pub fn info() -> (u64, u32, u32, &'static str) {
    let n = current();
    (n.kind, n.row_align, n.col_align, n.name)
}

/// Lines for the shell's `compute` command.
pub fn report() -> Vec<String> {
    let (kind, ra, ca, name) = info();
    let installed = NATIVE.lock().map(|n| n.name);
    let mut v = vec![format!(
        "compute: {} (jenis {}, baris kelipatan {}, kolom kelipatan {})",
        name, kind, ra, ca
    )];
    match installed {
        Some(n) if !ENABLED.load(Ordering::Relaxed) => v.push(format!("  GPU {} terpasang tapi dimatikan ('compute gpu' menyalakan)", n)),
        Some(_) => {}
        None => v.push(String::from("  tidak ada backend GPU compute: GEMV memakai referensi CPU kernel (program memakai CPU-nya sendiri)")),
    }
    let (n, us, macs) = (GEMVS.load(Ordering::Relaxed), GEMV_US.load(Ordering::Relaxed), GEMV_MACS.load(Ordering::Relaxed));
    v.push(format!("  {} GEMV, {} ms total, {} MFLOP/s rata-rata", n, us / 1000, 2 * macs / us.max(1)));
    let live = BUFS.lock().iter().filter(|b| b.is_some()).count();
    v.push(format!("  {} buffer hidup", live));
    v
}

/// Boot check of the always-present path (the reference backend through the
/// same alloc / write / gemv / read calls programs use).
pub fn self_test() -> bool {
    let owner = u64::MAX - 7;
    let n = &REFERENCE;
    let r = gemv_self_test(n);
    release(owner);
    match r {
        Ok(msg) => {
            let gpu = NATIVE.lock().map(|n| n.name);
            crate::println!(
                "MILESTONE: COMPUTE OK (reference: {}; GPU backend: {})",
                msg,
                gpu.unwrap_or("none -- programs compute on their own CPU")
            );
            true
        }
        Err(e) => {
            crate::println!("[compute] reference self-test FAILED: {}", e);
            false
        }
    }
}
