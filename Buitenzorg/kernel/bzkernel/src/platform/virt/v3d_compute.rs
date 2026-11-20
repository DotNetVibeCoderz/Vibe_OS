//! GPU compute on the Raspberry Pi 4's V3D: the `compute` layer's GEMV runs
//! on the 8 QPUs through the compute-shader dispatcher (CSD), the way
//! py-videocore6 drives it -- one job of 8 batches x 16 lanes, every thread
//! finding its rows from its QPU number. The kernel comes from
//! `scripts/v3d-compute.py` (`v3d_compute_kernels.rs`).
//!
//! Lock order: `v3d::GPU` -> `STATE` (like the 3D backend). Caches:
//! L2T flushed + slice caches invalidated before a job; after it, the TMU
//! write combiner and L2T are cleaned (Linux `v3d_clean_caches`) and the CPU
//! invalidates its copy before reading results.

use super::v3d::{self, r, w, Buffer, Gpu, GPU};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

const CSD_STATUS: u64 = 0x900;
const CSD_CFG0: u64 = 0x904;
const L2TCACTL_L2TFLS: u32 = 1 << 0;
const L2TCACTL_FLM_CLEAN: u32 = 2 << 1;
const L2TCACTL_TMUWCF: u32 = 1 << 8;
/// QPU threads the kernel assumes (8 batches, one per QPU).
const THREADS: u32 = 8;

struct State {
    code: Buffer,
    unif: Buffer,
    bufs: Vec<Option<Buffer>>,
}

// the buffers are only touched under the GPU lock
unsafe impl Send for State {}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut Gpu, &mut State) -> Result<T, &'static str>) -> Result<T, &'static str> {
    let mut g = GPU.lock();
    let gpu = g.as_mut().ok_or("no V3D")?;
    let mut out = Vec::new();
    if !gpu.bring_up(&mut out) {
        return Err("the V3D MMU did not come up");
    }
    let mut s = STATE.lock();
    if s.is_none() {
        let words = &super::v3d_compute_kernels::GEMV;
        let code = gpu.alloc((words.len() * 8).div_ceil(4096)).ok_or("out of GPU memory")?;
        for (k, wd) in words.iter().enumerate() {
            code.write(k * 8, &wd.to_le_bytes());
        }
        code.sync();
        let unif = gpu.alloc(1).ok_or("out of GPU memory")?;
        *s = Some(State { code, unif, bufs: Vec::new() });
    }
    f(gpu, s.as_mut().unwrap())
}

fn alloc(bytes: usize) -> Result<usize, &'static str> {
    with(|gpu, st| {
        let b = gpu.alloc(bytes.div_ceil(4096).max(1)).ok_or("out of GPU memory")?;
        if let Some(i) = st.bufs.iter().position(|x| x.is_none()) {
            st.bufs[i] = Some(b);
            Ok(i + 1)
        } else {
            st.bufs.push(Some(b));
            Ok(st.bufs.len())
        }
    })
}

fn free(h: usize) {
    let _ = with(|gpu, st| {
        if let Some(b) = st.bufs.get_mut(h.wrapping_sub(1)).and_then(|b| b.take()) {
            gpu.free(b);
        }
        Ok(())
    });
}

fn buf(st: &State, h: usize) -> Result<&Buffer, &'static str> {
    st.bufs.get(h.wrapping_sub(1)).and_then(|b| b.as_ref()).ok_or("no such buffer")
}

fn write(h: usize, off: usize, data: &[u8]) -> Result<(), &'static str> {
    with(|_, st| {
        let b = buf(st, h)?;
        if off + data.len() > b.frames.len() * 4096 {
            return Err("out of range");
        }
        b.write_bytes(off, data);
        b.sync_range(off, data.len());
        Ok(())
    })
}

fn read(h: usize, off: usize, out: &mut [u8]) -> Result<(), &'static str> {
    with(|_, st| {
        let b = buf(st, h)?;
        if off + out.len() > b.frames.len() * 4096 {
            return Err("out of range");
        }
        b.sync_range(off, out.len());
        b.read_bytes(off, out);
        Ok(())
    })
}

/// Wait (bounded) until `bits` of L2TCACTL read 0.
fn wait_l2t(core: u64, bits: u32) -> bool {
    v3d::wait_us(100_000, || r(core, v3d::CTL_L2TCACTL) & bits == 0)
}

fn gemv(wh: usize, xh: usize, yh: usize, rows: u32, cols: u32) -> Result<u64, &'static str> {
    if rows % (16 * THREADS) != 0 || cols % 64 != 0 {
        return Err("rows / cols not aligned");
    }
    with(|gpu, st| {
        let (wv, xv, yv) = (buf(st, wh)?.va, buf(st, xh)?.va, buf(st, yh)?.va);
        let stride = cols * 4;
        let unif = [rows / (16 * THREADS), cols / 64, wv, xv, yv, stride, 16 * stride, (THREADS - 1) * 16 * stride, THREADS * 64, 64];
        st.unif.write_words(0, &unif);
        st.unif.sync_range(0, unif.len() * 4);
        let core = gpu.core;
        // invalidate: flush L2T (the GPU's texture / TMU cache), slice caches
        wait_l2t(core, L2TCACTL_L2TFLS);
        w(core, v3d::CTL_L2TCACTL, L2TCACTL_L2TFLS);
        w(core, v3d::CTL_SLCACTL, 0x0F0F_0F0F);
        w(core, v3d::CTL_INT_CLR, 0xFF);
        let cfg = [
            1 << 16,
            1 << 16,
            1 << 16,
            ((256 - 1) << 12) | (16 << 8) | 16, // 16 workgroups per supergroup, 16 invocations
            THREADS - 1,                        // batches - 1
            st.code.va,
            st.unif.va,
        ];
        let t0 = crate::arch::cpu::timestamp();
        for (i, v) in cfg.iter().enumerate().skip(1) {
            w(core, CSD_CFG0 + 4 * i as u64, *v);
        }
        w(core, CSD_CFG0, cfg[0]); // CFG0 starts the dispatch
        let done = v3d::wait_us(2_000_000, || r(core, v3d::CTL_INT_STS) & (v3d::INT_CSDDONE | v3d::INT_GMPV) != 0);
        let us = v3d::elapsed_us(t0);
        let sts = r(core, v3d::CTL_INT_STS);
        if !done || sts & v3d::INT_GMPV != 0 {
            let e = format!(
                "CSD job {}: INT_STS {:#x} CSD_STATUS {:#x} MMU VIO {:#x}; {}",
                if done { "FAULTED" } else { "TIMED OUT (2 s)" },
                sts,
                r(core, CSD_STATUS),
                r(gpu.hub, v3d::MMU_VIO_ADDR),
                gpu.debug_regs()
            );
            crate::println!("[v3d-compute] {}", e);
            *LAST_ERROR.lock() = e;
            gpu.reset();
            return Err("V3D compute job failed");
        }
        w(core, v3d::CTL_INT_CLR, 0xFF);
        // clean: TMU write combiner, then L2T (results reach memory)
        wait_l2t(core, L2TCACTL_L2TFLS);
        w(core, v3d::CTL_L2TCACTL, L2TCACTL_TMUWCF);
        wait_l2t(core, L2TCACTL_TMUWCF);
        w(core, v3d::CTL_L2TCACTL, L2TCACTL_L2TFLS | L2TCACTL_FLM_CLEAN);
        wait_l2t(core, L2TCACTL_L2TFLS);
        Ok(us)
    })
}

static LAST_ERROR: Mutex<String> = Mutex::new(String::new());

static NATIVE: crate::compute::Native = crate::compute::Native {
    name: "V3D 4.2 QPU",
    kind: bz_abi::compute_kind::V3D,
    row_align: 16 * THREADS,
    col_align: 64,
    alloc,
    free,
    write,
    read,
    gemv,
};

/// Install the V3D compute backend (after `RPI4 V3D OK`): needs 8 QPUs (the
/// kernel's thread layout) and must pass the compute layer's GEMV self-test.
pub fn install() -> String {
    let qpus = {
        let g = GPU.lock();
        let Some(gpu) = g.as_ref() else { return String::from("[v3d-compute] no V3D: GEMV stays on the CPU") };
        let id1 = r(gpu.core, v3d::CTL_IDENT1);
        ((id1 >> 4) & 0xF) * ((id1 >> 8) & 0xF)
    };
    if qpus != THREADS {
        return format!("[v3d-compute] {} QPUs (the GEMV kernel is laid out for {}): GEMV stays on the CPU", qpus, THREADS);
    }
    crate::compute::install_native(&NATIVE)
}

/// The last failed job, for `gpu`.
pub fn last_error() -> String {
    LAST_ERROR.lock().clone()
}
