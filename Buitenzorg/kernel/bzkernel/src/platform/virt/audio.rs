//! The AUDIO_* syscalls' device on the PC is an AC'97 card. Device-Tree boards
//! have none; sound there is virtio-snd (`crate::virtio::snd`, its own test
//! path). These are the "no AC'97" answers, so the syscalls report an absent
//! device on ARM64.

pub const SAMPLE_RATE: u32 = 48_000;

pub fn is_present() -> bool {
    false
}
pub fn volume() -> u32 {
    0
}
pub fn is_muted() -> bool {
    false
}
pub fn set_volume(_pct: u32) -> bool {
    false
}
pub fn play_tone(_freq: u32, _ms: u32) -> bool {
    false
}
pub fn play_pcm(_src: u64, _len: u64) -> bool {
    false
}
