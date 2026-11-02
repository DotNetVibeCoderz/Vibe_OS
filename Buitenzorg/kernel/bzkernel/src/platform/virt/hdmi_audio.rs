//! HDMI audio on the Raspberry Pi 4's HDMI0 port (roadmap R6), after Linux's
//! `vc4_hdmi` (BCM2711 register map: `vc4_hdmi_regs.h`, `vc5_hdmi_hdmi0_fields`).
//!
//! The firmware has already set the display mode; this adds a 2-channel,
//! 48 kHz PCM stream: the 108 MHz audio clock gate in the DVP block, the MAI
//! (multichannel audio interface) sample clock, format, channel map and
//! packet configuration, N/CTS audio clock regeneration from the pixel clock,
//! and the audio InfoFrame in the HDMI packet RAM. Samples are written by the
//! CPU into `MAI_DATA` as IEC 958 sub-frames (what Linux's DMA feeds it:
//! `SNDRV_PCM_FMTBIT_IEC958_SUBFRAME_LE`) -- polled, no DMA.
//!
//! Every block is probed before use (QEMU `raspi4b` models none of it).

use core::sync::atomic::{AtomicU32, Ordering};

// HDMI0 blocks, physical (bus 0x7ef0_xxxx = CPU 0xfef0_xxxx)
const DVP_PHYS: u64 = 0xfef0_0000; // + 0x08 MISC_CONFIG (clock gates)
const HDMI_PHYS: u64 = 0xfef0_0700;
const PACKET_PHYS: u64 = 0xfef0_1b00;
const HD_PHYS: u64 = 0xfef2_0000;

// "hdmi" block
const HDMI_MAI_CHANNEL_MAP: u64 = 0x09c;
const HDMI_MAI_CONFIG: u64 = 0x0a0;
const HDMI_AUDIO_PACKET_CONFIG: u64 = 0x0b8;
const HDMI_RAM_PACKET_CONFIG: u64 = 0x0bc;
const HDMI_RAM_PACKET_STATUS: u64 = 0x0c4;
const HDMI_CRP_CFG: u64 = 0x0c8;
const HDMI_CTS_0: u64 = 0x0cc;
const HDMI_CTS_1: u64 = 0x0d0;
const HDMI_HOTPLUG: u64 = 0x1a8;
const HDMI_SCHEDULER_CONTROL: u64 = 0x0e0;
const SCHED_MODE_HDMI: u32 = 1 << 0;
const SCHED_HDMI_ACTIVE: u32 = 1 << 1;
// "hd" block
const HD_MAI_CTL: u64 = 0x010;
const HD_MAI_FMT: u64 = 0x018;
const HD_MAI_DATA: u64 = 0x01c;
const HD_MAI_SMP: u64 = 0x020;

const MAI_CTL_RESET: u32 = 1 << 0;
const MAI_CTL_ERRORF: u32 = 1 << 1;
const MAI_CTL_ERRORE: u32 = 1 << 2;
const MAI_CTL_ENABLE: u32 = 1 << 3;
const MAI_CTL_FLUSH: u32 = 1 << 9;
const MAI_CTL_FULL: u32 = 1 << 11;
const MAI_CTL_WHOLSMP: u32 = 1 << 12;
const MAI_CTL_CHALIGN: u32 = 1 << 13;
const MAI_CTL_DLATE: u32 = 1 << 15;

const AUDIO_PACKET_ID: u32 = 4; // InfoFrame type 0x84 - 0x80
const PACKET_STRIDE: u64 = 0x24;
const RAM_PACKET_ENABLE: u32 = 1 << 16;

const RATE: u32 = 48_000;
const AUDIO_CLOCK_HZ: u64 = 108_000_000; // clk-108M through the DVP gate

static HD: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static HDMI: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// Sounds reported so far (the first few are logged, for bring-up).
static REPORTS: AtomicU32 = AtomicU32::new(0);
/// IEC 958 frame counter (a block is 192 frames).
static FRAME: AtomicU32 = AtomicU32::new(0);

fn r(a: u64) -> u32 {
    unsafe { (a as *const u32).read_volatile() }
}
fn w(a: u64, v: u32) {
    unsafe { (a as *mut u32).write_volatile(v) }
}

fn map(phys: u64) -> Option<u64> {
    let va = crate::memory::map_mmio(phys & !0xFFF, 0x1000)?;
    Some(va + (phys & 0xFFF))
}

pub fn present() -> bool {
    HD.load(Ordering::Relaxed) != 0
}

/// Bring HDMI0 audio up. `pixel_hz` = the current pixel clock (for CTS).
pub fn init(pixel_hz: u64) -> bool {
    let (Some(dvp), Some(hdmi), Some(packet), Some(hd)) = (map(DVP_PHYS), map(HDMI_PHYS), map(PACKET_PHYS), map(HD_PHYS)) else {
        return false;
    };
    let probe = crate::arch::aarch64::exceptions::probe_read32;
    if probe(hdmi + HDMI_HOTPLUG).is_none() || probe(hd + HD_MAI_CTL).is_none() || probe(dvp + 0x08).is_none() {
        crate::println!("[hdmi-audio] no HDMI block answers: HDMI audio unavailable");
        return false;
    }
    let connected = r(hdmi + HDMI_HOTPLUG) & 1 != 0;
    // Audio travels only in HDMI mode (DVI has no data islands): switch the
    // scheduler to HDMI and wait for it to be active, as Linux does.
    let sched0 = r(hdmi + HDMI_SCHEDULER_CONTROL);
    if sched0 & SCHED_MODE_HDMI == 0 {
        w(hdmi + HDMI_SCHEDULER_CONTROL, sched0 | SCHED_MODE_HDMI);
    }
    let hz = crate::arch::cpu::counter_hz().max(1);
    let t0 = crate::arch::cpu::timestamp();
    while r(hdmi + HDMI_SCHEDULER_CONTROL) & SCHED_HDMI_ACTIVE == 0 && crate::arch::cpu::timestamp() - t0 < hz {}
    let sched1 = r(hdmi + HDMI_SCHEDULER_CONTROL);
    // the 108 MHz audio clock: DVP MISC_CONFIG bit 3 gates HDMI0's (set = off)
    let misc = r(dvp + 0x08);
    w(dvp + 0x08, misc & !(1 << 3));

    // startup: reset + flush the MAI FIFO
    w(hd + HD_MAI_CTL, MAI_CTL_RESET | MAI_CTL_FLUSH | MAI_CTL_DLATE | MAI_CTL_ERRORE | MAI_CTL_ERRORF);
    // sample clock: audio clock / rate = n / m (m = 1 for 108 MHz / 48 kHz)
    let n = (AUDIO_CLOCK_HZ / RATE as u64) as u32;
    w(hd + HD_MAI_SMP, n << 8);
    // 2 channels, whole samples, aligned, enabled
    w(hd + HD_MAI_CTL, (2 << 4) | MAI_CTL_WHOLSMP | MAI_CTL_CHALIGN | MAI_CTL_ENABLE);
    // format: 48 kHz (code 9), PCM (2)
    w(hd + HD_MAI_FMT, (9 << 8) | (2 << 16));
    // audio packets: zero data when flat / inactive, B frame id 8, channels 0+1
    w(hdmi + HDMI_AUDIO_PACKET_CONFIG, (1 << 29) | (1 << 24) | (8 << 10) | 0b11);
    w(hdmi + HDMI_MAI_CONFIG, (1 << 27) | (1 << 26) | 0b11);
    w(hdmi + HDMI_MAI_CHANNEL_MAP, (1 << 4) | 0); // channel i -> slot i
    // N/CTS: N = 128 * fs / 1000, CTS = pixel clock * N / (128 * fs). The
    // TMDS pixel clock is the display mode's: the firmware drives the EDID's
    // preferred mode (scaling our framebuffer), so take its clock from there --
    // the mailbox "pixel" clock is not it (81 MHz on a 1080p link).
    let display = display_info();
    let pixel_hz = match &display {
        Some((_, clock, _)) if *clock > 20_000_000 => *clock,
        _ => pixel_hz,
    };
    let nval = 128 * RATE / 1000;
    let cts = (pixel_hz * nval as u64 / (128 * RATE as u64)) as u32;
    w(hdmi + HDMI_CRP_CFG, (1 << 24) | nval);
    w(hdmi + HDMI_CTS_0, cts);
    w(hdmi + HDMI_CTS_1, cts);

    // audio InfoFrame: type 0x84, version 1, length 10; 2 channels (CC = 1),
    // coding / rate / size "refer to the stream header"
    let mut f = [0u8; 14];
    f[0] = 0x84;
    f[1] = 1;
    f[2] = 10;
    f[4] = 0x01; // CC: 2 channels
    let sum: u32 = f.iter().map(|&b| b as u32).sum();
    f[3] = (0x100 - (sum & 0xFF)) as u8;
    let cfg = r(hdmi + HDMI_RAM_PACKET_CONFIG);
    w(hdmi + HDMI_RAM_PACKET_CONFIG, (cfg & !(1 << AUDIO_PACKET_ID)) | RAM_PACKET_ENABLE);
    let mut reg = packet + PACKET_STRIDE * AUDIO_PACKET_ID as u64;
    let end = reg + PACKET_STRIDE;
    for i in (0..14).step_by(7) {
        w(reg, f[i] as u32 | (f[i + 1] as u32) << 8 | (f[i + 2] as u32) << 16);
        reg += 4;
        let g = |k: usize| *f.get(i + k).unwrap_or(&0) as u32;
        w(reg, g(3) | g(4) << 8 | g(5) << 16 | g(6) << 24);
        reg += 4;
    }
    while reg < end {
        w(reg, 0);
        reg += 4;
    }
    w(hdmi + HDMI_RAM_PACKET_CONFIG, r(hdmi + HDMI_RAM_PACKET_CONFIG) | RAM_PACKET_ENABLE | (1 << AUDIO_PACKET_ID));
    let t1 = crate::arch::cpu::timestamp();
    while r(hdmi + HDMI_RAM_PACKET_STATUS) & (1 << AUDIO_PACKET_ID) == 0 && crate::arch::cpu::timestamp() - t1 < hz / 10 {}
    crate::println!(
        "[hdmi-audio] scheduler {:#x} -> {:#x} (HDMI mode {}, active {}); audio InfoFrame {}",
        sched0,
        sched1,
        sched1 & SCHED_MODE_HDMI != 0,
        sched1 & SCHED_HDMI_ACTIVE != 0,
        if r(hdmi + HDMI_RAM_PACKET_STATUS) & (1 << AUDIO_PACKET_ID) != 0 { "sending" } else { "NOT sending" }
    );

    HD.store(hd, Ordering::SeqCst);
    HDMI.store(hdmi, Ordering::SeqCst);
    match display {
        Some((name, clock, audio)) => crate::println!(
            "[hdmi-audio] display '{}' (EDID): preferred pixel clock {} Hz, HDMI audio {}",
            name.trim(),
            clock,
            match audio { Some(true) => "supported", Some(false) => "NOT supported (no speakers)", None => "unknown" }
        ),
        None => crate::println!("[hdmi-audio] display EDID not available"),
    }
    crate::println!(
        "[hdmi-audio] HDMI0 {}: MAI_CTL {:#x}, SMP n={} , CTS {} (pixel {} Hz), packet status {:#x}, DVP misc {:#x} -> {:#x}",
        if connected { "connected" } else { "not connected (HDMI1 is not supported yet)" },
        r(hd + HD_MAI_CTL),
        n,
        cts,
        pixel_hz,
        r(hdmi + HDMI_RAM_PACKET_STATUS),
        misc,
        r(dvp + 0x08)
    );
    true
}

/// EDID block `n` of the connected display, through the firmware (mailbox
/// tag 0x00030020: block number, status, 128 bytes).
fn edid_block(n: u32) -> Option<[u8; 128]> {
    let mut v = [0u32; 34];
    v[0] = n;
    if !crate::arch::aarch64::rpi::firmware_property(0x0003_0020, &mut v) || v[1] != 0 {
        return None;
    }
    let mut b = [0u8; 128];
    for (i, w) in v[2..34].iter().enumerate() {
        b[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    Some(b)
}

/// What the display says about itself: (name, preferred pixel clock in Hz,
/// whether it takes HDMI audio -- the "basic audio" bit of a CEA extension).
pub fn display_info() -> Option<(alloc::string::String, u64, Option<bool>)> {
    let b0 = edid_block(0)?;
    if b0[0..8] != [0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0] {
        return None;
    }
    let pixel = (u16::from_le_bytes([b0[54], b0[55]]) as u64) * 10_000;
    let mut name = alloc::string::String::from("?");
    for d in 0..4 {
        let o = 54 + d * 18;
        if b0[o] == 0 && b0[o + 1] == 0 && b0[o + 3] == 0xFC {
            name = b0[o + 5..o + 18].iter().take_while(|&&c| c != 0x0A && c != 0).map(|&c| c as char).collect();
        }
    }
    let audio = if b0[126] > 0 {
        edid_block(1).filter(|b| b[0] == 0x02).map(|b| b[3] & 0x40 != 0)
    } else {
        Some(false) // no extension block: DVI-style display, no audio
    };
    Some((name, pixel, audio))
}

/// After a sound: log how it went on HDMI (the first three sounds only).
pub fn report(frames: usize, sent: usize) {
    let (hd, hdmi) = (HD.load(Ordering::Relaxed), HDMI.load(Ordering::Relaxed));
    if hd == 0 || REPORTS.fetch_add(1, Ordering::Relaxed) >= 3 {
        return;
    }
    crate::println!(
        "[hdmi-audio] sound: {} of {} frames into the MAI FIFO; MAI_CTL {:#x}, packet status {:#x}, packet config {:#x}, scheduler {:#x}, audio packet config {:#x}, CRP {:#x}, CTS {}",
        sent,
        frames,
        r(hd + HD_MAI_CTL),
        r(hdmi + HDMI_RAM_PACKET_STATUS),
        r(hdmi + HDMI_RAM_PACKET_CONFIG),
        r(hdmi + HDMI_SCHEDULER_CONTROL),
        r(hdmi + HDMI_AUDIO_PACKET_CONFIG),
        r(hdmi + HDMI_CRP_CFG),
        r(hdmi + HDMI_CTS_0)
    );
}

/// One IEC 958 sub-frame for a 16-bit sample: preamble (bits 0-3), audio in
/// bits 4-27 (16-bit sample at 12-27), channel-status bit 30, even parity 31.
fn subframe(sample: i16, preamble: u32, cbit: bool) -> u32 {
    let mut v = preamble | ((sample as u16 as u32) << 12) | if cbit { 1 << 30 } else { 0 };
    if (v >> 4).count_ones() % 2 == 1 {
        v |= 1 << 31;
    }
    v
}

/// Queue one stereo frame; false if the FIFO stayed full (no audio clock).
pub fn frame(l: i16, rr: i16) -> bool {
    let hd = HD.load(Ordering::Relaxed);
    if hd == 0 {
        return false;
    }
    let n = FRAME.fetch_add(1, Ordering::Relaxed) % 192;
    // channel status: consumer, PCM, sample rate 48 kHz = bit 25 set
    let cbit = n == 25;
    let pre_l = if n == 0 { 0x8 } else { 0x2 };
    for word in [subframe(l, pre_l, cbit), subframe(rr, 0x4, cbit)] {
        let t0 = crate::arch::cpu::timestamp();
        let hz = crate::arch::cpu::counter_hz().max(1);
        while r(hd + HD_MAI_CTL) & MAI_CTL_FULL != 0 {
            if crate::arch::cpu::timestamp() - t0 > hz / 200 {
                return false; // 5 ms: the FIFO is not draining
            }
        }
        w(hd + HD_MAI_DATA, word);
    }
    true
}
