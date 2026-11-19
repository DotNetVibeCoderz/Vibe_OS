//! The inverse MDCT (stb_vorbis's inverse_mdct and its helpers), and the tables it needs.
//!
//! The following is stb_vorbis's hand-optimised version of the algorithm from "The use of
//! multirate filter banks for coding of high quality digital audio" (Sporer, Brandenburg,
//! Edler), with every float operation in the same order, so the samples come out the same.
#[allow(unused_imports)]
use alloc::{boxed::Box, vec, vec::Vec};

use crate::codebook::{bit_reverse, ilog};
use crate::Pi;

/// stb_vorbis's own M_PI: `3.14159265358979323846264f`.
const PI: f32 = core::f32::consts::PI;

pub(crate) struct BlockTables {
    pub a: Vec<f32>,
    pub b: Vec<f32>,
    pub c: Vec<f32>,
    pub window: Vec<f32>,
    pub bit_reverse: Vec<u16>,
}

fn square(x: f32) -> f32 {
    x * x
}

impl BlockTables {
    /// stb_vorbis's init_blocksize for a block of `n` samples.
    pub fn new(n: usize, pi: Pi) -> BlockTables {
        let (n2, n4, n8) = (n >> 1, n >> 2, n >> 3);
        // The cosine and sine of `x * M_PI / n` and of `x * M_PI / n / 2` as the C library works
        // them out: in doubles where M_PI is one; in floats where it is stb_vorbis's own, which C
        // makes doubles for cos and sin, and C++ gives to the float cos and sin.
        let pi_over_n = |x: usize| x as f32 * PI / n as f32;
        let pi_over_2n = |x: usize| x as f32 * PI / n as f32 / 2.0;
        let cos_sin = |double: f64, float: f32| match pi {
            Pi::Double => (libm::cos(double) as f32, libm::sin(double) as f32),
            Pi::Float => (libm::cos(f64::from(float)) as f32, libm::sin(f64::from(float)) as f32),
            Pi::FloatCpp => (libm::cosf(float), libm::sinf(float)),
        };
        let cos_sin_pi_over_n = |x: usize| cos_sin(x as f64 * core::f64::consts::PI / n as f64, pi_over_n(x));
        let cos_sin_pi_over_2n = |x: usize| cos_sin(x as f64 * core::f64::consts::PI / n as f64 / 2.0, pi_over_2n(x));
        let pi_as_double = match pi {
            Pi::Double => core::f64::consts::PI,
            Pi::Float | Pi::FloatCpp => f64::from(PI),
        };
        // compute_twiddle_factors
        let (mut a, mut b, mut c) = (vec![0.0f32; n2], vec![0.0f32; n2], vec![0.0f32; n4]);
        let mut k2 = 0usize;
        for k in 0..n4 {
            let (cos, sin) = cos_sin_pi_over_n(4 * k);
            a[k2] = cos;
            a[k2 + 1] = -sin;
            let (cos, sin) = cos_sin_pi_over_2n(k2 + 1);
            b[k2] = cos * 0.5;
            b[k2 + 1] = sin * 0.5;
            k2 += 2;
        }
        k2 = 0;
        for _ in 0..n8 {
            let (cos, sin) = cos_sin_pi_over_n(2 * (k2 + 1));
            c[k2] = cos;
            c[k2 + 1] = -sin;
            k2 += 2;
        }
        // compute_window
        let mut window = vec![0.0f32; n2];
        for (i, w) in window.iter_mut().enumerate() {
            let inner = libm::sin((i as f64 + 0.5) / n2 as f64 * 0.5 * pi_as_double) as f32;
            *w = libm::sin(0.5 * pi_as_double * f64::from(square(inner))) as f32;
        }
        // compute_bitreverse
        let ld = ilog(n as i32) - 1; // ilog is off-by-one from normal definitions
        let mut reverse = vec![0u16; n8];
        for (i, r) in reverse.iter_mut().enumerate() {
            *r = ((bit_reverse(i as u32) >> (32 - ld + 3)) << 2) as u16;
        }
        BlockTables { a, b, c, window, bit_reverse: reverse }
    }
}

/// One of the algorithm's butterflies, on a pair of values from each of two places.
#[inline(always)]
fn butterfly(e0: &mut f32, e0m1: &mut f32, e2: &mut f32, e2m1: &mut f32, a0: f32, a1: f32) {
    let k00_20 = *e0 - *e2;
    let k01_21 = *e0m1 - *e2m1;
    *e0 += *e2;
    *e0m1 += *e2m1;
    *e2 = k00_20 * a0 - k01_21 * a1;
    *e2m1 = k01_21 * a0 + k00_20 * a1;
}

/// Four butterflies between the top eight values of two places, with four pairs of factors.
/// (stb_vorbis's pointers `e0` and `e2` are the last value of `hi` and of `lo`, and step down
/// by two.)
#[inline(always)]
fn butterflies(hi: &mut [f32; 8], lo: &mut [f32; 8], factors: [(f32, f32); 4]) {
    let [h0, h1, h2, h3, h4, h5, h6, h7] = hi;
    let [l0, l1, l2, l3, l4, l5, l6, l7] = lo;
    butterfly(h7, h6, l7, l6, factors[0].0, factors[0].1);
    butterfly(h5, h4, l5, l4, factors[1].0, factors[1].1);
    butterfly(h3, h2, l3, l2, factors[2].0, factors[2].1);
    butterfly(h1, h0, l1, l0, factors[3].0, factors[3].1);
}

/// A slice's last eight values.
#[inline(always)]
fn last8(values: &mut [f32]) -> Option<&mut [f32; 8]> {
    values.last_chunk_mut::<8>()
}

// the following were split out into separate functions while optimizing;
// they could be pushed back up but eh. __forceinline showed no change;
// they're probably already being inlined.

/// stb_vorbis's imdct_step3_iter0_loop (with `k1` 8) and imdct_step3_inner_r_loop: butterflies
/// between the upper half of `region` and the lower, from their tops down, taking a pair of
/// factors every `k1` values of `a`.
///
/// (In stb_vorbis these work down from `e + i_off` and `e + i_off - k_off`; the values they
/// reach are always the `k_off` below each, which is `region` cut in two.)
fn imdct_step3_inner_r_loop(region: &mut [f32], a: &[f32], k1: usize) {
    let half = region.len() / 2;
    let (lo, hi) = region.split_at_mut(half);
    for ((hi, lo), a) in hi.rchunks_exact_mut(8).zip(lo.rchunks_exact_mut(8)).zip(a.chunks_exact(k1 * 4)) {
        let (Ok(hi), Ok(lo)) = (<&mut [f32; 8]>::try_from(hi), <&mut [f32; 8]>::try_from(lo)) else { return };
        let factors = [(a[0], a[1]), (a[k1], a[k1 + 1]), (a[k1 * 2], a[k1 * 2 + 1]), (a[k1 * 3], a[k1 * 3 + 1])];
        butterflies(hi, lo, factors);
    }
}

/// stb_vorbis's imdct_step3_inner_s_loop: the same four butterflies, with the same factors,
/// in each stretch of `k0` values of `region` from its top down: between the last eight values
/// of the stretch and the eight that end half a stretch below. (The lowest stretch may be
/// cut short just below those.)
fn imdct_step3_inner_s_loop(region: &mut [f32], k0: usize, a: &[f32], a_off: usize) {
    let factors = [(a[0], a[1]), (a[a_off], a[a_off + 1]), (a[a_off * 2], a[a_off * 2 + 1]), (a[a_off * 3], a[a_off * 3 + 1])];
    let half = k0 / 2;
    for stretch in region.rchunks_mut(k0) {
        let Some(split) = stretch.len().checked_sub(half) else { return };
        let (lo, hi) = stretch.split_at_mut(split);
        let (Some(hi), Some(lo)) = (last8(hi), last8(lo)) else { return };
        butterflies(hi, lo, factors);
    }
}

/// On eight values, of which stb_vorbis's `z` points at the last.
#[inline(always)]
fn iter_54(z: &mut [f32; 8]) {
    // (z[7 - k] is stb_vorbis's z[-k].)
    let k00 = z[7] - z[3];
    let y0 = z[7] + z[3];
    let y2 = z[5] + z[1];
    let k22 = z[5] - z[1];

    z[7] = y0 + y2; // z0 + z4 + z2 + z6
    z[5] = y0 - y2; // z0 + z4 - z2 - z6

    // done with y0,y2

    let k33 = z[4] - z[0];

    z[3] = k00 + k33; // z0 - z4 + z3 - z7
    z[1] = k00 - k33; // z0 - z4 - z3 + z7

    // done with k33

    let k11 = z[6] - z[2];
    let y1 = z[6] + z[2];
    let y3 = z[4] + z[0];

    z[6] = y1 + y3; // z1 + z5 + z3 + z7
    z[4] = y1 - y3; // z1 + z5 - z3 - z7
    z[2] = k11 - k22; // z1 - z5 + z2 - z6
    z[0] = k11 + k22; // z1 - z5 - z2 + z6
}

/// stb_vorbis's imdct_step3_inner_s_loop_ld654, over `region` sixteen values at a time from
/// its top down.
fn imdct_step3_inner_s_loop_ld654(region: &mut [f32], a: &[f32], base_n: usize) {
    let a_off = base_n >> 3;
    let a2 = a[a_off];
    for z in region.rchunks_exact_mut(16) {
        let Ok(z) = <&mut [f32; 16]>::try_from(z) else { return };
        // (z[15 - k] is stb_vorbis's z[-k].)
        let k00 = z[15] - z[7];
        let k11 = z[14] - z[6];
        let l00 = z[13] - z[5];
        let l11 = z[12] - z[4];
        z[15] += z[7];
        z[14] += z[6];
        z[13] += z[5];
        z[12] += z[4];
        z[7] = k00;
        z[6] = k11;
        z[5] = (l00 + l11) * a2;
        z[4] = (l11 - l00) * a2;

        let k00 = z[11] - z[3];
        let k11 = z[10] - z[2];
        let l00 = z[9] - z[1];
        let l11 = z[8] - z[0];
        z[11] += z[3];
        z[10] += z[2];
        z[9] += z[1];
        z[8] += z[0];
        z[3] = k11;
        z[2] = -k00;
        z[1] = (l11 - l00) * a2;
        z[0] = (l00 + l11) * -a2;

        let (low, high) = z.split_at_mut(8);
        let (Ok(low), Ok(high)) = (<&mut [f32; 8]>::try_from(low), <&mut [f32; 8]>::try_from(high)) else { return };
        iter_54(high);
        iter_54(low);
    }
}

/// The inverse MDCT of the first `n / 2` values of `buffer`, giving `n` samples in place.
/// `buf2` is scratch space of at least `n / 2` values.
///
/// The loops are stb_vorbis's, with its pointers that step through the buffers written as
/// the stretches of the buffers they step through, so that the compiler can see that no
/// value is reached for outside them. Every value is worked out by the same operations in
/// the same order.
pub(crate) fn inverse_mdct(buffer: &mut [f32], n: usize, tables: &BlockTables, buf2: &mut [f32]) {
    let (n2, n4, n8) = (n >> 1, n >> 2, n >> 3);
    let a = &tables.a[..n2];
    let buffer = &mut buffer[..n];
    let buf2 = &mut buf2[..n2];

    // IMDCT algorithm from "The use of multirate filter banks for coding of high quality digital audio"
    // See notes about bugs in that paper in less-optimal implementation 'inverse_mdct_old' in stb_vorbis.

    // kernel from paper

    // merged:
    //   copy and reflect spectral data
    //   step 0

    // note that it turns out that the items added together during
    // this step are, in fact, being added to themselves (as reflected
    // by step 0). inexplicable inefficiency! this became obvious
    // once I combined the passes.

    // so there's a missing 'times 2' here (for adding X to itself).
    // this propagates through linearly to the end, where the numbers
    // are 1/2 too small, and need to be compensated for.
    {
        let (low, high) = buf2.split_at_mut(n4);
        let (a_low, a_high) = a.split_at(n4);
        // (d steps down through the upper half of buf2 as e steps up through the buffer.)
        for ((d, e), aa) in high.rchunks_exact_mut(2).zip(buffer[..n2].chunks_exact(4)).zip(a_low.chunks_exact(2)) {
            d[1] = e[0] * aa[0] - e[2] * aa[1];
            d[0] = e[0] * aa[1] + e[2] * aa[0];
        }
        // (and on down through its lower half as e steps back down.)
        for ((d, e), aa) in low.rchunks_exact_mut(2).zip(buffer[..n2].rchunks_exact(4)).zip(a_high.chunks_exact(2)) {
            d[1] = -e[3] * aa[0] - -e[1] * aa[1];
            d[0] = -e[3] * aa[1] + -e[1] * aa[0];
        }
    }

    // now we use symbolic names for these, so that we can
    // possibly swap their meaning as we change which operations
    // are in place

    let u = buffer;
    let v = buf2;

    // step 2    (paper output is w, now u)
    // this could be in place, but the data ends up in the wrong
    // place... _somebody_'s got to swap it, so this is nominated
    {
        let (v1, v0) = v.split_at(n4);
        let (u1, u0) = u[..n2].split_at_mut(n4);
        let stretches = u0.chunks_exact_mut(4).zip(u1.chunks_exact_mut(4)).zip(v0.chunks_exact(4).zip(v1.chunks_exact(4)));
        for (((d0, d1), (e0, e1)), aa) in stretches.zip(a.rchunks_exact(8)) {
            let v41_21 = e0[1] - e1[1];
            let v40_20 = e0[0] - e1[0];
            d0[1] = e0[1] + e1[1];
            d0[0] = e0[0] + e1[0];
            d1[1] = v41_21 * aa[4] - v40_20 * aa[5];
            d1[0] = v40_20 * aa[4] + v41_21 * aa[5];

            let v41_21 = e0[3] - e1[3];
            let v40_20 = e0[2] - e1[2];
            d0[3] = e0[3] + e1[3];
            d0[2] = e0[2] + e1[2];
            d1[3] = v41_21 * aa[0] - v40_20 * aa[1];
            d1[2] = v40_20 * aa[0] + v41_21 * aa[1];
        }
    }

    // step 3
    let ld = ilog(n as i32) - 1; // ilog is off-by-one from normal definitions

    // optimized step 3:

    // the original step3 loop can be nested r inside s or s inside r;
    // it's written originally as s inside r, but this is dumb when r
    // iterates many times, and s few. So I have two copies of it and
    // switch between them halfway.

    // this is iteration 0 of step 3
    imdct_step3_inner_r_loop(&mut u[n4..n2], a, 8);
    imdct_step3_inner_r_loop(&mut u[..n4], a, 8);

    // this is iteration 1 of step 3
    for region in u[..n2].rchunks_exact_mut(n8) {
        imdct_step3_inner_r_loop(region, a, 16);
    }

    let mut l = 2i32;
    while l < (ld - 3) >> 1 {
        let k0 = n >> (l + 2);
        let lim = 1usize << (l + 1);
        for region in u[..n2].rchunks_exact_mut(k0).take(lim) {
            imdct_step3_inner_r_loop(region, a, 1 << (l + 3));
        }
        l += 1;
    }

    while l < ld - 6 {
        let k0 = n >> (l + 2);
        let k1 = 1usize << (l + 3);
        let rlim = n >> (l + 6);
        let lim = 1usize << (l + 1);
        let mut a0 = 0usize;
        let mut top = n2;
        for _ in 0..rlim {
            // (lim stretches of k0 values, the highest ending at `top`; of the lowest, only as
            // far down as the loop reaches.)
            let reach = (lim - 1) * k0 + k0 / 2 + 8;
            let Some(region) = top.checked_sub(reach).and_then(|bottom| u.get_mut(bottom..top)) else { break };
            imdct_step3_inner_s_loop(region, k0, &a[a0..], k1);
            a0 += k1 * 4;
            top = top.wrapping_sub(8);
        }
        l += 1;
    }

    // iterations with count:
    //   ld-6,-5,-4 all interleaved together
    //       the big win comes from getting rid of needless flops
    //         due to the constants on pass 5 & 4 being all 1 and 0;
    //       combining them to be simultaneous to improve cache made little difference
    imdct_step3_inner_s_loop_ld654(&mut u[..n2], a, n);

    // output is u

    // step 4, 5, and 6
    // cannot be in-place because of step 5
    {
        // weirdly, I'd have thought reading sequentially and writing
        // erratically would have been better than vice-versa, but in
        // fact that's not what my testing showed. (That is, with
        // j = bitreverse(i), do you read i and write j, or read j and write i.)
        let (v0, v1) = v.split_at_mut(n4);
        for ((d0, d1), bitrev) in v0.rchunks_exact_mut(4).zip(v1.rchunks_exact_mut(4)).zip(tables.bit_reverse.chunks_exact(2)) {
            let k4 = usize::from(bitrev[0]);
            let Some(from) = u.get(k4..k4 + 4) else { return };
            d1[3] = from[0];
            d1[2] = from[1];
            d0[3] = from[2];
            d0[2] = from[3];

            let k4 = usize::from(bitrev[1]);
            let Some(from) = u.get(k4..k4 + 4) else { return };
            d1[1] = from[0];
            d1[0] = from[1];
            d0[1] = from[2];
            d0[0] = from[3];
        }
    }
    // (paper output is u, now v)

    // data must be in buf2

    // step 7   (paper output is v, now v)
    // this is now in place
    {
        let (front, back) = v.split_at_mut(n4);
        for ((d, e), c) in front.chunks_exact_mut(4).zip(back.rchunks_exact_mut(4)).zip(tables.c.chunks_exact(4)) {
            let a02 = d[0] - e[2];
            let a11 = d[1] + e[3];

            let b0 = c[1] * a02 + c[0] * a11;
            let b1 = c[1] * a11 - c[0] * a02;

            let b2 = d[0] + e[2];
            let b3 = d[1] - e[3];

            d[0] = b2 + b0;
            d[1] = b3 + b1;
            e[2] = b2 - b0;
            e[3] = b1 - b3;

            let a02 = d[2] - e[0];
            let a11 = d[3] + e[1];

            let b0 = c[3] * a02 + c[2] * a11;
            let b1 = c[3] * a11 - c[2] * a02;

            let b2 = d[2] + e[0];
            let b3 = d[3] - e[1];

            d[2] = b2 + b0;
            d[3] = b3 + b1;
            e[0] = b2 - b0;
            e[1] = b1 - b3;
        }
    }

    // data must be in buf2

    // step 8+decode   (paper output is X, now buffer)
    // this generates pairs of data a la 8 and pushes them directly through
    // the decode kernel (pushing rather than pulling) to avoid having
    // to make another pass later

    // this cannot POSSIBLY be in place, so we refer to the buffers directly
    {
        let (first, second) = u.split_at_mut(n2);
        let (q0, q1) = first.split_at_mut(n4);
        let (q2, q3) = second.split_at_mut(n4);
        let outputs = q0.chunks_exact_mut(4).zip(q1.rchunks_exact_mut(4)).zip(q2.chunks_exact_mut(4).zip(q3.rchunks_exact_mut(4)));
        for (((d0, d1), (d2, d3)), (e, b)) in outputs.zip(v.rchunks_exact(8).zip(tables.b[..n2].rchunks_exact(8))) {
            let p3 = e[6] * b[7] - e[7] * b[6];
            let p2 = -e[6] * b[6] - e[7] * b[7];

            d0[0] = p3;
            d1[3] = -p3;
            d2[0] = p2;
            d3[3] = p2;

            let p1 = e[4] * b[5] - e[5] * b[4];
            let p0 = -e[4] * b[4] - e[5] * b[5];

            d0[1] = p1;
            d1[2] = -p1;
            d2[1] = p0;
            d3[2] = p0;

            let p3 = e[2] * b[3] - e[3] * b[2];
            let p2 = -e[2] * b[2] - e[3] * b[3];

            d0[2] = p3;
            d1[1] = -p3;
            d2[2] = p2;
            d3[1] = p2;

            let p1 = e[0] * b[1] - e[1] * b[0];
            let p0 = -e[0] * b[0] - e[1] * b[1];

            d0[3] = p1;
            d1[0] = -p1;
            d2[3] = p0;
            d3[0] = p0;
        }
    }
}
