//! Turning the decoder's float channels into 16-bit samples, with stb_vorbis's down-mix to
//! one or two channels.
#[allow(unused_imports)]
use alloc::{boxed::Box, vec, vec::Vec};

const PLAYBACK_MONO: i8 = 1;
const PLAYBACK_LEFT: i8 = 2;
const PLAYBACK_RIGHT: i8 = 4;

const L: i8 = PLAYBACK_LEFT | PLAYBACK_MONO;
const C: i8 = PLAYBACK_LEFT | PLAYBACK_RIGHT | PLAYBACK_MONO;
const R: i8 = PLAYBACK_RIGHT | PLAYBACK_MONO;

/// Where each channel of a 1 to 6 channel stream plays.
const CHANNEL_POSITION: [[i8; 6]; 7] = [[0, 0, 0, 0, 0, 0], [C, 0, 0, 0, 0, 0], [L, R, 0, 0, 0, 0], [L, C, R, 0, 0, 0], [L, R, L, R, 0, 0], [L, C, R, L, R, 0], [L, C, R, L, R, C]];

const BUFFER_SIZE: usize = 32;

/// stb_vorbis's FAST_SCALED_FLOAT_TO_INT with 15 bits: a float as a 16-bit sample, rounded by
/// adding a magic number and reading the float's bits.
#[inline]
pub(crate) fn float_to_i16(x: f32) -> i16 {
    const MAGIC: f32 = 1.5f32 * (1 << (23 - 15)) as f32 + 0.5f32 / (1 << 15) as f32;
    const ADDEND: i32 = ((150 - 15) << 23) + (1 << 22);
    let v = ((x + MAGIC).to_bits() as i32).wrapping_sub(ADDEND);
    // (stb_vorbis: if ((unsigned int) (v + 32768) > 65535) v = v < 0 ? -32768 : 32767; said
    // this way, a compiler can convert several samples at a time.)
    v.clamp(-32768, 32767) as i16
}

fn copy_samples(dest: &mut [i16], src: &[f32]) {
    for (out, &sample) in dest.iter_mut().zip(src) {
        *out = float_to_i16(sample);
    }
}

/// One output channel as the sum of the stream's channels that play there.
fn compute_samples(mask: i8, output: &mut [i16], num_c: usize, data: &[Vec<f32>], d_offset: usize, len: usize) {
    let mut buffer = [0.0f32; BUFFER_SIZE];
    let mut n = BUFFER_SIZE;
    let mut o = 0usize;
    while o < len {
        buffer.fill(0.0);
        if o + n > len {
            n = len - o;
        }
        for j in 0..num_c {
            if CHANNEL_POSITION[num_c][j] & mask != 0 {
                for (sum, &sample) in buffer[..n].iter_mut().zip(&data[j][d_offset + o..][..n]) {
                    *sum += sample;
                }
            }
        }
        copy_samples(&mut output[o..o + n], &buffer[..n]);
        o += BUFFER_SIZE;
    }
}

/// Stereo, interleaved, from a stream of up to six channels.
fn compute_stereo_samples(output: &mut [i16], num_c: usize, data: &[Vec<f32>], d_offset: usize, len: usize) {
    let mut buffer = [0.0f32; BUFFER_SIZE];
    let mut n = BUFFER_SIZE >> 1;
    // o is the offset in the source data
    let mut o = 0usize;
    while o < len {
        // o2 is the offset in the output data
        let o2 = o << 1;
        buffer.fill(0.0);
        if o + n > len {
            n = len - o;
        }
        for j in 0..num_c {
            let m = CHANNEL_POSITION[num_c][j] & (PLAYBACK_LEFT | PLAYBACK_RIGHT);
            if m == 0 {
                continue;
            }
            for (pair, &sample) in buffer.chunks_exact_mut(2).zip(&data[j][d_offset + o..][..n]) {
                if m & PLAYBACK_LEFT != 0 {
                    pair[0] += sample;
                }
                if m & PLAYBACK_RIGHT != 0 {
                    pair[1] += sample;
                }
            }
        }
        copy_samples(&mut output[o2..o2 + (n << 1)], &buffer[..n << 1]);
        o += BUFFER_SIZE >> 1;
    }
}

/// `samples` samples of each of `buffers`, from `b_offset` on, out of `data` from `d_offset`.
pub(crate) fn convert_samples_short(buffers: &mut [&mut [i16]], b_offset: usize, data_c: usize, data: &[Vec<f32>], d_offset: usize, samples: usize) {
    let buf_c = buffers.len();
    if buf_c != data_c && buf_c <= 2 && data_c <= 6 {
        const CHANNEL_SELECTOR: [[i8; 2]; 3] = [[0, 0], [PLAYBACK_MONO, 0], [PLAYBACK_LEFT, PLAYBACK_RIGHT]];
        for i in 0..buf_c {
            compute_samples(CHANNEL_SELECTOR[buf_c][i], &mut buffers[i][b_offset..], data_c, data, d_offset, samples);
        }
    } else {
        let limit = buf_c.min(data_c);
        for i in 0..limit {
            copy_samples(&mut buffers[i][b_offset..b_offset + samples], &data[i][d_offset..d_offset + samples]);
        }
        for buffer in buffers.iter_mut().skip(limit) {
            buffer[b_offset..b_offset + samples].fill(0);
        }
    }
}

/// `len` samples for `buf_c` interleaved channels into `buffer`.
pub(crate) fn convert_channels_short_interleaved(buf_c: usize, buffer: &mut [i16], data_c: usize, data: &[Vec<f32>], d_offset: usize, len: usize) {
    if buf_c == 1 && data_c != 1 && data_c <= 6 {
        // (stb_vorbis asserts that this is stereo, and without the assert writes two channels
        // into a buffer for one. One channel is mixed the way its other functions do it.)
        compute_samples(PLAYBACK_MONO, buffer, data_c, data, d_offset, len);
    } else if buf_c != data_c && buf_c <= 2 && data_c <= 6 {
        // (stb_vorbis does this once for each of the two channels.)
        for _ in 0..buf_c {
            compute_stereo_samples(buffer, data_c, data, d_offset, len);
        }
    } else {
        let limit = buf_c.min(data_c);
        let buffer = &mut buffer[..len * buf_c];
        match (buf_c, &data[..limit]) {
            // One channel and two, which is nearly all sound, each as one run over the samples.
            (1, [only]) => copy_samples(buffer, &only[d_offset..d_offset + len]),
            (2, [left, right]) => {
                let (left, right) = (&left[d_offset..d_offset + len], &right[d_offset..d_offset + len]);
                for (pair, (&left, &right)) in buffer.chunks_exact_mut(2).zip(left.iter().zip(right)) {
                    pair[0] = float_to_i16(left);
                    pair[1] = float_to_i16(right);
                }
            }
            (_, channels) => {
                if buf_c == 0 {
                    return;
                }
                for (j, frame) in buffer.chunks_exact_mut(buf_c).enumerate() {
                    let (decoded, silent) = frame.split_at_mut(limit);
                    for (out, channel) in decoded.iter_mut().zip(channels) {
                        *out = float_to_i16(channel[d_offset + j]);
                    }
                    silent.fill(0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_to_samples() {
        assert_eq!(float_to_i16(0.0), 0);
        assert_eq!(float_to_i16(0.5), 16384);
        assert_eq!(float_to_i16(-0.5), -16384);
        assert_eq!(float_to_i16(1.0), 32767); // clamped
        assert_eq!(float_to_i16(-1.0), -32768);
        assert_eq!(float_to_i16(7.5), 32767);
        assert_eq!(float_to_i16(-7.5), -32768);
        assert_eq!(float_to_i16(1.0 / 32768.0), 1);
    }

    #[test]
    fn down_mixing() {
        // Three channels (left, centre, right) to stereo: the centre goes to both sides.
        let data = vec![vec![0.25f32; 40], vec![0.125; 40], vec![-0.25; 40]];
        let mut stereo = [0i16; 80];
        convert_channels_short_interleaved(2, &mut stereo, 3, &data, 0, 40);
        assert!(stereo.chunks(2).all(|pair| pair == [12288, -4096]));
        // The same to mono: all three.
        let mut mono = [0i16; 40];
        convert_samples_short(&mut [&mut mono[..]], 0, 3, &data, 0, 40);
        assert!(mono.iter().all(|&sample| sample == 4096));
        // More output channels than the stream has: the extra ones are silent.
        let mut four = [1i16; 16];
        convert_channels_short_interleaved(4, &mut four, 3, &data, 0, 4);
        assert_eq!(four[..4], [8192, 4096, -8192, 0]);
    }
}
