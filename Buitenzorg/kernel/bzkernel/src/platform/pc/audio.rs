//! Intel AC'97 audio driver (v0.16 "Panen": OS audio subsystem).
//!
//! Drives QEMU's `AC97` sound card: enumerates it on the PCI bus, cold-resets
//! the codec, programs the mixer (master + PCM-out volume), and plays 16-bit
//! signed stereo PCM at 48 kHz through the Native Audio Bus-Master DMA engine
//! using a buffer-descriptor list (BDL). Speaker output only for now; mic
//! capture (the NABM PCM-in box) is a straightforward follow-up.
//!
//! Registers follow the AC'97 spec: BAR0 = NAM (Native Audio Mixer, I/O),
//! BAR1 = NABM (Native Audio Bus Master, I/O). The PCM-out channel "box" lives
//! at NABM offset 0x10.

use crate::{memory, pci};
use spin::Mutex;
use x86_64::instructions::port::Port;

// --- NAM (mixer) register offsets ---
const NAM_RESET: u16 = 0x00;
const NAM_MASTER_VOL: u16 = 0x02;
const NAM_PCM_OUT_VOL: u16 = 0x18;

// --- NABM (bus master) register offsets ---
const PO_BDBAR: u16 = 0x10; // u32: BDL physical base
const PO_CIV: u16 = 0x14; // u8:  current index value (RO)
const PO_LVI: u16 = 0x15; // u8:  last valid index
const PO_SR: u16 = 0x16; // u16: status
const PO_PICB: u16 = 0x18; // u16: position in current buffer (RO)
const PO_CR: u16 = 0x1B; // u8:  control
const GLOB_CNT: u16 = 0x2C; // u32: global control

// PO_CR control bits.
const CR_RPBM: u8 = 0x01; // run/pause bus master
const CR_RR: u8 = 0x02; // reset registers
// PO_SR status bits.
const SR_DCH: u16 = 0x01; // DMA controller halted

const NUM_BUFFERS: usize = 8;
const FRAME_BYTES: usize = 4096;
/// 16-bit samples (words) per DMA buffer.
const WORDS_PER_BUF: usize = FRAME_BYTES / 2;
pub const SAMPLE_RATE: u32 = 48_000;

struct Ac97 {
    nam: u16,
    nabm: u16,
    bdl_phys: u64,
    bdl_virt: u64,                    // *mut u32 as address (Send-safe)
    bufs: [(u64, u64); NUM_BUFFERS],  // (phys, virt-as-address)
    volume: u32,                      // 0..=100
    muted: bool,
    reset_caps: u16,                  // NAM reset-register readback (capabilities)
    mixer_ok: bool,                   // volume write/readback verified
}

// Only accessed behind the global Mutex; the addresses are plain integers.
unsafe impl Send for Ac97 {}

static DEVICE: Mutex<Option<Ac97>> = Mutex::new(None);

#[inline]
fn outb(port: u16, val: u8) {
    unsafe { Port::<u8>::new(port).write(val) };
}
#[inline]
fn outw(port: u16, val: u16) {
    unsafe { Port::<u16>::new(port).write(val) };
}
#[inline]
fn outl(port: u16, val: u32) {
    unsafe { Port::<u32>::new(port).write(val) };
}
#[inline]
fn inb(port: u16) -> u8 {
    unsafe { Port::<u8>::new(port).read() }
}
#[inline]
fn inw(port: u16) -> u16 {
    unsafe { Port::<u16>::new(port).read() }
}

/// Encode a 0..=100 volume percentage into an AC'97 stereo attenuation value
/// (0 = loudest, 0x3F per channel = quietest; bit 15 mutes).
fn vol_encode(pct: u32, muted: bool) -> u16 {
    if muted || pct == 0 {
        return 0x8000; // mute bit
    }
    let p = if pct > 100 { 100 } else { pct };
    let att = ((100 - p) * 0x3F / 100) as u16; // per-channel attenuation
    (att << 8) | att
}

/// Probe the PCI bus for an AC'97 controller and initialize it. Called once at
/// boot from `kernel_main`. Returns true if a device was brought up.
pub fn init() -> bool {
    let devices = pci::scan();
    // AC'97: class 0x04 (multimedia), subclass 0x01 (audio device). QEMU's model
    // is Intel 82801AA (vendor 0x8086, device 0x2415).
    let dev = devices.into_iter().find(|d| {
        (d.class == 0x04 && d.subclass == 0x01)
            || (d.vendor_id == 0x8086 && d.device_id == 0x2415)
    });
    let Some(dev) = dev else {
        crate::println!("[audio] no AC'97 controller found on PCI");
        return false;
    };
    dev.enable_io_and_bus_master();
    let nam = dev.io_bar(0);
    let nabm = dev.io_bar(1);
    if nam == 0 || nabm == 0 {
        crate::println!("[audio] AC'97 has no I/O BARs (nam={nam:#x} nabm={nabm:#x})");
        return false;
    }

    // Cold-reset the link + codec, then reset the PCM-out DMA engine.
    outl(nabm + GLOB_CNT, 0x0000_0002);
    outw(nam + NAM_RESET, 0x0001);
    let reset_caps = inw(nam + NAM_RESET);
    outb(nabm + PO_CR, CR_RR);
    for _ in 0..100_000 {
        if inb(nabm + PO_CR) & CR_RR == 0 {
            break;
        }
    }

    // Program the mixer to full volume and verify the NAM I/O path by writing a
    // known attenuation and reading it back.
    outw(nam + NAM_MASTER_VOL, 0x0000);
    outw(nam + NAM_PCM_OUT_VOL, 0x0000);
    outw(nam + NAM_MASTER_VOL, 0x0808);
    let mixer_ok = inw(nam + NAM_MASTER_VOL) == 0x0808;
    outw(nam + NAM_MASTER_VOL, 0x0000);

    // Allocate the BDL frame + audio buffer pool for DMA.
    let Some((bdl_phys, bdl_virt)) = memory::alloc_dma_frame() else {
        crate::println!("[audio] failed to allocate BDL frame");
        return false;
    };
    let mut bufs = [(0u64, 0u64); NUM_BUFFERS];
    for slot in bufs.iter_mut() {
        match memory::alloc_dma_frame() {
            Some((p, v)) => *slot = (p, v as u64),
            None => {
                crate::println!("[audio] failed to allocate audio buffer");
                return false;
            }
        }
    }

    let ac = Ac97 {
        nam,
        nabm,
        bdl_phys,
        bdl_virt: bdl_virt as u64,
        bufs,
        volume: 80,
        muted: false,
        reset_caps,
        mixer_ok,
    };
    // Apply the default volume.
    outw(nam + NAM_MASTER_VOL, vol_encode(ac.volume, ac.muted));
    *DEVICE.lock() = Some(ac);

    crate::println!(
        "[audio] AC'97 up: NAM={nam:#06x} NABM={nabm:#06x} caps={reset_caps:#06x} mixer_ok={mixer_ok} rate={SAMPLE_RATE}"
    );
    true
}

/// Is a sound card present and initialized?
pub fn is_present() -> bool {
    DEVICE.lock().is_some()
}

/// Was the mixer write/readback check successful (proves the NAM path works)?
pub fn mixer_ok() -> bool {
    DEVICE.lock().as_ref().map(|d| d.mixer_ok).unwrap_or(false)
}

/// Current master volume percentage.
pub fn volume() -> u32 {
    DEVICE.lock().as_ref().map(|d| d.volume).unwrap_or(0)
}

/// Is output currently muted?
pub fn is_muted() -> bool {
    DEVICE.lock().as_ref().map(|d| d.muted).unwrap_or(false)
}

/// NAM reset-register capabilities readback (nonzero on a real/QEMU codec).
pub fn caps() -> u16 {
    DEVICE.lock().as_ref().map(|d| d.reset_caps).unwrap_or(0)
}

/// Set the master output volume (0..=100). Non-zero un-mutes.
pub fn set_volume(pct: u32) -> bool {
    let mut guard = DEVICE.lock();
    let Some(d) = guard.as_mut() else { return false };
    d.volume = if pct > 100 { 100 } else { pct };
    if d.volume > 0 {
        d.muted = false;
    }
    outw(d.nam + NAM_MASTER_VOL, vol_encode(d.volume, d.muted));
    true
}

/// Toggle mute; returns the new muted state. (Audio-settings UI API.)
#[allow(dead_code)]
pub fn toggle_mute() -> bool {
    let mut guard = DEVICE.lock();
    let Some(d) = guard.as_mut() else { return false };
    d.muted = !d.muted;
    outw(d.nam + NAM_MASTER_VOL, vol_encode(d.volume, d.muted));
    d.muted
}

/// Program the BDL over the first `used` buffers (each fully filled) and start
/// the DMA engine. Returns the PCM-out current-index value observed shortly
/// after starting (advances as buffers are consumed).
fn start_playback(d: &Ac97, used: usize) -> u8 {
    // Each BDL entry is 2 u32 words: [buffer phys addr][ctrl | length_in_words].
    let bdl = d.bdl_virt as *mut u32;
    for i in 0..used {
        let (phys, _) = d.bufs[i];
        unsafe {
            bdl.add(i * 2).write_volatile(phys as u32);
            // length = number of 16-bit samples in the buffer.
            bdl.add(i * 2 + 1).write_volatile(WORDS_PER_BUF as u32);
        }
    }
    // Point the engine at the BDL and mark the last valid index.
    outl(d.nabm + PO_BDBAR, d.bdl_phys as u32);
    outb(d.nabm + PO_LVI, (used as u8).saturating_sub(1));
    // Clear any stale status, then run.
    outw(d.nabm + PO_SR, inw(d.nabm + PO_SR));
    outb(d.nabm + PO_CR, CR_RPBM);
    // Let the DMA engine tick, then sample the current index for verification.
    for _ in 0..200_000 {
        core::hint::spin_loop();
    }
    inb(d.nabm + PO_CIV)
}

/// Fill `count` stereo 16-bit frames of a sine wave at `freq` into buffer `i`,
/// returning the running phase for continuation across buffers.
fn fill_tone(buf_virt: u64, count: usize, freq: u32, phase0: u32) -> u32 {
    // Fixed-point phase accumulator: step = freq * 65536 / sample_rate.
    let step = ((freq as u64) << 16) / SAMPLE_RATE as u64;
    let mut phase = phase0;
    let out = buf_virt as *mut i16;
    for n in 0..count {
        // Triangle-approximated sine from the top phase bits (cheap, no FPU).
        let s = sine_fx(phase);
        unsafe {
            out.add(n * 2).write_volatile(s); // left
            out.add(n * 2 + 1).write_volatile(s); // right
        }
        phase = phase.wrapping_add(step as u32);
    }
    phase
}

/// Integer sine: input phase 0..=0xFFFF maps one period; output -8192..=8192.
fn sine_fx(phase: u32) -> i16 {
    // Quarter-wave parabola approximation (Bhaskara-like), plenty for a beep.
    let x = (phase & 0xFFFF) as i32; // 0..65535
    // Map to -32768..32767 signed angle.
    let a = x - 32768;
    // y = A * (1 - (a/32768)^2)-style triangle→parabola blend, scaled to ~8192.
    let sq = ((a as i64 * a as i64) >> 15) as i32; // ~a^2/32768, 0..32768
    let tri = 32768 - sq; // parabola peak
    // Sign by half-period.
    let v = if (phase & 0x8000) != 0 { -tri } else { tri };
    (v / 4) as i16 // scale into a comfortable amplitude
}

/// Play a generated sine tone of `freq` Hz for `ms` milliseconds.
pub fn play_tone(freq: u32, ms: u32) -> bool {
    let guard = DEVICE.lock();
    let Some(d) = guard.as_ref() else { return false };
    let total_frames = (SAMPLE_RATE as u64 * ms as u64 / 1000) as usize;
    let frames_per_buf = WORDS_PER_BUF / 2; // stereo pairs per buffer
    let mut phase = 0u32;
    let mut remaining = total_frames;
    let mut used = 0usize;
    for i in 0..NUM_BUFFERS {
        if remaining == 0 {
            break;
        }
        let cnt = core::cmp::min(frames_per_buf, remaining);
        phase = fill_tone(d.bufs[i].1, cnt, freq, phase);
        // Zero the tail if the last buffer is partial.
        if cnt < frames_per_buf {
            let out = d.bufs[i].1 as *mut i16;
            for n in (cnt * 2)..(frames_per_buf * 2) {
                unsafe { out.add(n).write_volatile(0) };
            }
        }
        remaining -= cnt;
        used += 1;
    }
    if used == 0 {
        used = 1;
    }
    let civ = start_playback(d, used);
    crate::println!("[audio] tone {freq}Hz {ms}ms: {used} buffer(s), CIV={civ}");
    true
}

/// Play interleaved 16-bit stereo PCM from a caller buffer (read per-sample to
/// avoid aliasing a user region). `len_bytes` is capped to the DMA pool size.
pub fn play_pcm(src_ptr: u64, len_bytes: u64) -> bool {
    let guard = DEVICE.lock();
    let Some(d) = guard.as_ref() else { return false };
    let cap = NUM_BUFFERS * FRAME_BYTES;
    let n = core::cmp::min(len_bytes as usize, cap);
    let words = n / 2;
    let src = src_ptr as *const i16;
    let mut used = 0usize;
    for i in 0..NUM_BUFFERS {
        let start_word = i * WORDS_PER_BUF;
        if start_word >= words {
            break;
        }
        let out = d.bufs[i].1 as *mut i16;
        for w in 0..WORDS_PER_BUF {
            let gw = start_word + w;
            let v = if gw < words {
                unsafe { core::ptr::read_volatile(src.add(gw)) }
            } else {
                0
            };
            unsafe { out.add(w).write_volatile(v) };
        }
        used += 1;
    }
    if used == 0 {
        return false;
    }
    let civ = start_playback(d, used);
    crate::println!("[audio] pcm {n} bytes: {used} buffer(s), CIV={civ}");
    true
}

/// Is the PCM-out DMA engine currently running (not halted)? (UI meter API.)
#[allow(dead_code)]
pub fn is_playing() -> bool {
    let guard = DEVICE.lock();
    let Some(d) = guard.as_ref() else { return false };
    inw(d.nabm + PO_SR) & SR_DCH == 0
}
