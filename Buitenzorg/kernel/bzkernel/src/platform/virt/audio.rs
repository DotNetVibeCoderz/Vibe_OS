//! The AUDIO_* syscalls' device on Device-Tree boards. A PC has an AC'97
//! card (`platform/pc/audio.rs`); a Raspberry Pi 4 has its 3.5 mm headphone
//! jack, driven by PWM (`rpi_io`); other boards have none here (sound on QEMU
//! `virt` is virtio-snd, its own test path), so the syscalls report an absent
//! device.

pub const SAMPLE_RATE: u32 = 48_000;

#[cfg(target_arch = "aarch64")]
use super::rpi_io as jack;

pub fn is_present() -> bool {
    #[cfg(target_arch = "aarch64")]
    return jack::audio_present();
    #[cfg(not(target_arch = "aarch64"))]
    false
}

pub fn volume() -> u32 {
    #[cfg(target_arch = "aarch64")]
    if jack::audio_present() {
        return jack::audio_volume();
    }
    0
}

pub fn is_muted() -> bool {
    is_present() && volume() == 0
}

pub fn set_volume(pct: u32) -> bool {
    #[cfg(target_arch = "aarch64")]
    return jack::audio_set_volume(pct);
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = pct;
        false
    }
}

pub fn play_tone(freq: u32, ms: u32) -> bool {
    #[cfg(target_arch = "aarch64")]
    return jack::audio_present() && jack::audio_tone(freq, ms);
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = (freq, ms);
        false
    }
}

/// PCM from a caller buffer (validated by the syscall layer): copied with
/// volatile reads (never a slice over user memory), at most 4 MiB (~22 s).
pub fn play_pcm(src: u64, len: u64) -> bool {
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
