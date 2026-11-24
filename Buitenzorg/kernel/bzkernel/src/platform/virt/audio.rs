//! The AUDIO_* syscalls' device on Device-Tree boards. A PC has an AC'97
//! card (`platform/pc/audio.rs`); a Raspberry Pi 4 has its 3.5 mm headphone
//! jack, driven by PWM (`rpi_io`); QEMU `virt` (and any board with a virtio
//! sound device) streams the mixer through virtio-snd (`virtio::snd`), and
//! records from its input stream. Without either the syscalls report an
//! absent device.

pub const SAMPLE_RATE: u32 = 48_000;

#[cfg(target_arch = "aarch64")]
use super::rpi_io as jack;

use core::sync::atomic::{AtomicU32, Ordering};

use crate::virtio::snd;

/// The virtio sound card's master volume (percent); it has no mixer of its own.
static VIRTIO_VOLUME: AtomicU32 = AtomicU32::new(80);

fn has_jack() -> bool {
    #[cfg(target_arch = "aarch64")]
    return jack::audio_present();
    #[cfg(not(target_arch = "aarch64"))]
    false
}

/// The Pi's jack / HDMI streamed by DMA (`pi_stream`), when that works.
fn pi_stream() -> bool {
    #[cfg(target_arch = "aarch64")]
    return super::pi_stream::available();
    #[cfg(not(target_arch = "aarch64"))]
    false
}

/// The mixer's stream (AUDIO_WRITE): virtio-snd, or the Pi's jack + HDMI by
/// DMA. Without `pi_stream` the Pi plays only whole sounds fed by the CPU.
pub fn can_stream() -> bool {
    pi_stream() || (!has_jack() && snd::available())
}

pub fn is_streaming() -> bool {
    #[cfg(target_arch = "aarch64")]
    if pi_stream() {
        return super::pi_stream::is_streaming();
    }
    snd::is_streaming()
}

pub fn stream_pump() {
    #[cfg(target_arch = "aarch64")]
    if pi_stream() {
        super::pi_stream::kick();
        return;
    }
    if !has_jack() {
        snd::pump();
    }
}

/// Recording: virtio-snd's input stream (the Pi has no microphone input).
pub fn can_capture() -> bool {
    snd::can_capture()
}
pub fn capture_start() -> bool {
    snd::capture_start()
}
pub fn capture_stop() {
    snd::capture_stop()
}
pub fn capture_pump() {
    snd::pump()
}
pub fn capture_read(out: &mut [i16]) -> usize {
    snd::capture_read(out)
}
pub fn capture_queued_ms() -> u64 {
    snd::capture_queued_ms()
}

/// A sound device of the platform's own (USB audio then only plays its test tone).
pub fn has_card() -> bool {
    true
}

pub fn is_present() -> bool {
    has_jack() || snd::available()
}

pub fn volume() -> u32 {
    #[cfg(target_arch = "aarch64")]
    if jack::audio_present() {
        return jack::audio_volume();
    }
    if snd::available() {
        return VIRTIO_VOLUME.load(Ordering::Relaxed);
    }
    0
}

pub fn is_muted() -> bool {
    is_present() && volume() == 0
}

pub fn set_volume(pct: u32) -> bool {
    #[cfg(target_arch = "aarch64")]
    if jack::audio_present() {
        return jack::audio_set_volume(pct);
    }
    if snd::available() {
        VIRTIO_VOLUME.store(pct.min(100), Ordering::Relaxed);
        return true;
    }
    false
}

/// A tone into the mixer (as program 0), streamed through virtio-snd.
fn mixer_tone(freq: u32, ms: u32) -> bool {
    let frames = (48_000u64 * ms.min(5000) as u64 / 1000) as usize;
    let mut bytes = alloc::vec::Vec::with_capacity(frames * 4);
    let step = ((freq as u64) << 16) / 48_000;
    let mut phase: u32 = 0;
    for f in 0..frames {
        let p = (phase >> 6) & 0x3FF;
        let tri = (if p < 512 { p as i32 - 256 } else { 767 - p as i32 }) * 32;
        // short fade in / out: no clicks
        let edge = (f.min(frames - f) as i32).min(480);
        let s = (tri * edge / 480) as i16;
        bytes.extend_from_slice(&s.to_le_bytes());
        bytes.extend_from_slice(&s.to_le_bytes());
        phase = phase.wrapping_add(step as u32);
    }
    let took = crate::mixer::write(0, &bytes, 0);
    stream_pump();
    took > 0
}

pub fn play_tone(freq: u32, ms: u32) -> bool {
    if pi_stream() {
        return mixer_tone(freq, ms);
    }
    #[cfg(target_arch = "aarch64")]
    if jack::audio_present() {
        return jack::audio_tone(freq, ms);
    }
    snd::available() && mixer_tone(freq, ms)
}

/// PCM from a caller buffer (validated by the syscall layer): copied with
/// volatile reads (never a slice over user memory), at most 4 MiB (~22 s).
pub fn play_pcm(src: u64, len: u64) -> bool {
    if pi_stream() || (!has_jack() && snd::available()) {
        let n = len.min(4 << 20) as usize & !3;
        let mut pcm = alloc::vec::Vec::with_capacity(n);
        let p = src as *const u8;
        for i in 0..n {
            pcm.push(unsafe { p.add(i).read_volatile() });
        }
        let took = crate::mixer::write(0, &pcm, 0);
        stream_pump();
        return took > 0;
    }
    #[cfg(target_arch = "aarch64")]
    {
        if !jack::audio_present() {
            return false;
        }
        let n = len.min(4 << 20) as usize & !3;
        let mut pcm = alloc::vec::Vec::with_capacity(n);
        let p = src as *const u8;
        for i in 0..n {
            pcm.push(unsafe { p.add(i).read_volatile() });
        }
        return jack::audio_play(&pcm);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = (src, len);
        false
    }
}
