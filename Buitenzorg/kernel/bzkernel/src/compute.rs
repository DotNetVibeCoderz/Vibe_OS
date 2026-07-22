//! Compute API (v0.11 "Cahaya"): a small parallel-compute abstraction in the
//! spirit of Vulkan compute / WebGPU, used by the compositor and (later) the AI
//! subsystem. Today it runs on the **CPU backend** (SIMD-friendly loops); the
//! same interface will back a GPU backend once a GPU driver lands, with a CPU
//! fallback when no GPU is present (requirements.md §7).

/// Which backend executed a dispatch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)] // Gpu is the future backend, wired once a GPU driver lands.
pub enum Backend {
    /// No GPU present: CPU fallback (current).
    Cpu,
    /// GPU compute (future).
    Gpu,
}

/// The active compute backend. A GPU driver would flip this to `Gpu`.
pub fn backend() -> Backend {
    Backend::Cpu
}

/// Elementwise SAXPY: `y[i] = a * x[i] + y[i]` — a canonical compute kernel.
/// Returns the backend used.
pub fn saxpy(a: f32, x: &[f32], y: &mut [f32]) -> Backend {
    let n = x.len().min(y.len());
    // CPU backend: a tight loop the autovectorizer can SIMD-ize.
    for i in 0..n {
        y[i] = a * x[i] + y[i];
    }
    backend()
}

/// Blend two RGBA-ish pixel buffers `out = src*alpha + dst*(1-alpha)` — the
/// kind of kernel a GPU-accelerated compositor offloads. CPU backend for now.
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
    backend()
}

/// Self-test / benchmark: run a SAXPY over `n` elements and return a checksum
/// plus the backend, so the boot demo can prove the compute API works.
pub fn selftest(n: usize) -> (Backend, u64) {
    use alloc::vec;
    let x = vec![1.0f32; n];
    let mut y = vec![2.0f32; n];
    let backend = saxpy(3.0, &x, &mut y);
    // Expected: every element = 3*1 + 2 = 5.
    let checksum = y.iter().map(|&v| v as u64).sum();
    (backend, checksum)
}
