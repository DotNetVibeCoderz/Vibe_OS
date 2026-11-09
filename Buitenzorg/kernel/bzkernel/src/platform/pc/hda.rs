//! Intel High Definition Audio (the sound of nearly every PC since 2004):
//! the controller (PCI class 04/03) and its codec, polled, output only.
//!
//! Codec commands go through the Immediate Command registers (no CORB/RIRB
//! rings). The driver walks the codec's widget graph from every output pin the
//! BIOS marked as connected (speaker, headphone, line out) back to a DAC,
//! selects that path, unmutes it at 0 dB, powers it up, turns on the external
//! amplifier (EAPD) and routes every DAC to one output stream: 48 kHz,
//! 16-bit stereo. Volume scales the samples.
//!
//! The stream DMA is cyclic -- it never stops by itself -- so a sound fills
//! part of the buffer, the rest is silence, and `poll` (the desktop loop)
//! stops the stream once the sound is over.

use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use spin::Mutex;

use crate::pci::PciDevice;

const GCAP: usize = 0x00;
const GCTL: usize = 0x08;
const STATESTS: usize = 0x0E;
const ICOI: usize = 0x60;
const ICII: usize = 0x64;
const ICIS: usize = 0x68;
const SD_CTL: usize = 0x00;
const SD_LPIB: usize = 0x04;
const SD_STS: usize = 0x03;
const SD_CBL: usize = 0x08;
const SD_LVI: usize = 0x0C;
const SD_FMT: usize = 0x12;
const SD_BDPL: usize = 0x18;
const SD_BDPU: usize = 0x1C;

/// 48 kHz, 16 bits, 2 channels (stream format and converter format alike).
const FORMAT: u16 = 0x0011;
const STREAM_TAG: u32 = 1;
const PAGES: usize = 16;
/// A sound uses at most this many pages; the rest stays silent, so the
/// cyclic DMA plays silence while `poll` gets round to stopping it.
const SOUND_PAGES: usize = 12;
pub const FRAME_BYTES: usize = 4096;

const W_OUTPUT: u32 = 0;
const W_MIXER: u32 = 2;
const W_SELECTOR: u32 = 3;
const W_PIN: u32 = 4;

struct Hda {
    regs: *mut u8,
    sd: usize,
    codec: u32,
    bdl: (u64, *mut u8),
    pages: Vec<(u64, *mut u8)>,
    volume: u32,
    muted: bool,
    outputs: usize,
    stop_at: u64,
    running: bool,
    /// The mixer streams through the cyclic pages; `wpage` = the next page to
    /// refill once the link position has passed it.
    streaming: bool,
    wpage: usize,
    idle: usize,
}

// The registers and DMA pages belong to this driver instance.
unsafe impl Send for Hda {}

static DEVICE: Mutex<Option<Hda>> = Mutex::new(None);

impl Hda {
    fn r8(&self, o: usize) -> u8 {
        unsafe { read_volatile(self.regs.add(o)) }
    }
    fn w8(&self, o: usize, v: u8) {
        unsafe { write_volatile(self.regs.add(o), v) }
    }
    fn r16(&self, o: usize) -> u16 {
        unsafe { read_volatile(self.regs.add(o) as *const u16) }
    }
    fn w16(&self, o: usize, v: u16) {
        unsafe { write_volatile(self.regs.add(o) as *mut u16, v) }
    }
    fn r32(&self, o: usize) -> u32 {
        unsafe { read_volatile(self.regs.add(o) as *const u32) }
    }
    fn w32(&self, o: usize, v: u32) {
        unsafe { write_volatile(self.regs.add(o) as *mut u32, v) }
    }

    /// One codec verb through the Immediate Command interface.
    fn cmd(&self, nid: u32, verb: u32) -> Option<u32> {
        let word = self.codec << 28 | (nid & 0x7F) << 20 | (verb & 0xF_FFFF);
        for _ in 0..1_000_000 {
            if self.r16(ICIS) & 1 == 0 {
                break;
            }
            core::hint::spin_loop();
        }
        self.w16(ICIS, 0b10); // clear "result valid"
        self.w32(ICOI, word);
        self.w16(ICIS, 0b01); // busy: send it
        for _ in 0..1_000_000 {
            let s = self.r16(ICIS);
            if s & 0b11 == 0b10 {
                return Some(self.r32(ICII));
            }
            core::hint::spin_loop();
        }
        None
    }

    /// GET_PARAMETER.
    fn param(&self, nid: u32, id: u32) -> u32 {
        self.cmd(nid, 0xF00 << 8 | id).unwrap_or(0)
    }

    /// The node ids a widget's input can be connected to (short or long form).
    fn connections(&self, nid: u32) -> Vec<u32> {
        let len = self.param(nid, 0x0E);
        let long = len & 0x80 != 0;
        let n = (len & 0x7F) as usize;
        let per = if long { 2 } else { 4 };
        let mut out = Vec::new();
        let mut i = 0;
        while i < n {
            let v = self.cmd(nid, 0xF02 << 8 | i as u32).unwrap_or(0);
            for k in 0..per {
                if i + k >= n {
                    break;
                }
                let e = if long { (v >> (16 * k)) & 0xFFFF } else { (v >> (8 * k)) & 0xFF };
                out.push(e & if long { 0x7FFF } else { 0x7F });
            }
            i += per;
        }
        out
    }

    fn widget_type(&self, nid: u32) -> u32 {
        (self.param(nid, 0x09) >> 20) & 0xF
    }

    /// Unmute an amplifier at 0 dB (its offset): `output` or input `index`.
    fn unmute(&self, nid: u32, output: bool, index: u32) {
        let caps = self.param(nid, if output { 0x12 } else { 0x0D });
        let gain = caps & 0x7F;
        let which = if output { 1 << 15 } else { 1 << 14 | index << 8 };
        let _ = self.cmd(nid, 0x3 << 16 | which | 3 << 12 | gain);
    }

    /// A path from `nid` back to a DAC, depth first (at most 5 hops).
    fn find_dac(&self, nid: u32, depth: u32, path: &mut Vec<(u32, usize)>) -> bool {
        if depth > 5 {
            return false;
        }
        for (i, c) in self.connections(nid).into_iter().enumerate() {
            match self.widget_type(c) {
                W_OUTPUT => {
                    path.push((nid, i));
                    path.push((c, 0));
                    return true;
                }
                W_MIXER | W_SELECTOR => {
                    path.push((nid, i));
                    if self.find_dac(c, depth + 1, path) {
                        return true;
                    }
                    path.pop();
                }
                _ => {}
            }
        }
        false
    }

    /// Set up every connected output pin. Returns how many play.
    fn configure(&self, afg: u32) -> usize {
        let _ = self.cmd(afg, 0x705 << 8); // D0
        let sub = self.param(afg, 0x04);
        let (start, count) = ((sub >> 16) & 0xFF, sub & 0xFF);
        let mut outputs = 0;
        let mut dacs: Vec<u32> = Vec::new();
        for nid in start..start + count {
            if self.widget_type(nid) != W_PIN || self.param(nid, 0x0C) & (1 << 4) == 0 {
                continue; // not an output-capable pin
            }
            let config = self.cmd(nid, 0xF1C << 8).unwrap_or(0);
            let device = (config >> 20) & 0xF;
            if config >> 30 == 1 || !matches!(device, 0 | 1 | 2) {
                continue; // no physical connection, or not line out/speaker/headphone
            }
            let mut path = Vec::new();
            if !self.find_dac(nid, 0, &mut path) {
                continue;
            }
            for &(node, input) in &path {
                let _ = self.cmd(node, 0x705 << 8); // D0
                let kind = self.widget_type(node);
                if kind == W_MIXER {
                    self.unmute(node, false, input as u32);
                } else if kind != W_OUTPUT && self.connections(node).len() > 1 {
                    let _ = self.cmd(node, 0x701 << 8 | input as u32); // connection select
                }
                self.unmute(node, true, 0);
            }
            let dac = path.last().map(|p| p.0).unwrap_or(0);
            if !dacs.contains(&dac) {
                dacs.push(dac);
            }
            let hp = if device == 2 { 0x80 } else { 0 };
            let _ = self.cmd(nid, 0x707 << 8 | 0x40 | hp); // pin: output enable (+ headphone amp)
            if self.param(nid, 0x0C) & (1 << 16) != 0 {
                let _ = self.cmd(nid, 0x70C << 8 | 0x02); // EAPD: external amplifier on
            }
            crate::println!(
                "[hda] output pin {:#04x} ({}) via {} widget(s) to DAC {:#04x}",
                nid,
                ["line out", "speaker", "headphone"][device as usize],
                path.len(),
                dac
            );
            outputs += 1;
        }
        for &dac in &dacs {
            let _ = self.cmd(dac, 0x706 << 8 | STREAM_TAG << 4); // stream tag, channel 0
            let _ = self.cmd(dac, 0x2 << 16 | FORMAT as u32);
        }
        outputs
    }

    fn stop(&mut self) {
        let ctl = self.r8(self.sd + SD_CTL);
        self.w8(self.sd + SD_CTL, ctl & !0x02);
        self.running = false;
    }

    /// Play the first `used` pages (the rest is silence) for `ms`.
    fn start(&mut self, used: usize, ms: u64) {
        self.stop();
        for (phys, va) in self.pages.iter().skip(used) {
            let _ = phys;
            unsafe { core::ptr::write_bytes(*va, 0, FRAME_BYTES) };
        }
        self.w8(self.sd + SD_STS, 0x1C);
        self.w8(self.sd + SD_CTL, self.r8(self.sd + SD_CTL) | 0x02); // RUN
        self.running = true;
        let hz = crate::arch::cpu::counter_hz().max(1);
        self.stop_at = crate::arch::cpu::timestamp() + (ms + 30) * hz / 1000;
    }

    fn scale(&self, s: i16) -> i16 {
        if self.muted {
            0
        } else {
            (s as i32 * self.volume as i32 / 100) as i16
        }
    }
}

fn bring_up(dev: &PciDevice) -> Result<Hda, &'static str> {
    let bar = dev.mem_bar(0).ok_or("no memory BAR")?;
    dev.enable_mem_and_bus_master();
    // Intel: traffic class 0 for the stream DMA (TCSEL, PCI config 0x44)
    if dev.vendor_id == 0x8086 {
        let v = dev.read32(0x44);
        crate::pci::write_config_u32(dev.bus, dev.slot, dev.function, 0x44, v & !0x07);
    }
    let va = crate::memory::map_mmio(bar, 0x4000).ok_or("cannot map the registers")?;
    let mut h = Hda {
        regs: va as *mut u8,
        sd: 0,
        codec: 0,
        bdl: crate::memory::alloc_dma_frame().ok_or("out of memory")?,
        pages: Vec::new(),
        volume: 80,
        muted: false,
        outputs: 0,
        stop_at: 0,
        running: false,
        streaming: false,
        wpage: 0,
        idle: 0,
    };
    // controller reset: CRST low, then high
    h.w32(GCTL, h.r32(GCTL) & !1);
    let mut ok = false;
    for _ in 0..2_000_000 {
        if h.r32(GCTL) & 1 == 0 {
            ok = true;
            break;
        }
    }
    if !ok {
        return Err("controller did not enter reset");
    }
    h.w32(GCTL, h.r32(GCTL) | 1);
    ok = false;
    for _ in 0..2_000_000 {
        if h.r32(GCTL) & 1 != 0 {
            ok = true;
            break;
        }
    }
    if !ok {
        return Err("controller did not leave reset");
    }
    // codecs announce themselves within 521 us of the reset
    let t0 = crate::arch::cpu::timestamp();
    let hz = crate::arch::cpu::counter_hz().max(1_000_000_000);
    while crate::arch::cpu::timestamp().wrapping_sub(t0) < hz / 100 {
        core::hint::spin_loop();
    }
    let codecs = h.r16(STATESTS) & 0x7FFF;
    if codecs == 0 {
        return Err("no codec on the link");
    }
    let gcap = h.r16(GCAP);
    let iss = ((gcap >> 8) & 0xF) as usize;
    let oss = ((gcap >> 12) & 0xF) as usize;
    if oss == 0 {
        return Err("no output stream");
    }
    h.sd = 0x80 + iss * 0x20; // the first output stream descriptor

    // the first codec with an audio function group that has an output path
    for cad in 0..15u32 {
        if codecs & (1 << cad) == 0 {
            continue;
        }
        h.codec = cad;
        let vendor = h.param(0, 0x00);
        let sub = h.param(0, 0x04);
        for nid in ((sub >> 16) & 0xFF)..((sub >> 16) & 0xFF) + (sub & 0xFF) {
            if h.param(nid, 0x05) & 0xFF == 0x01 {
                crate::println!("[hda] codec {} ({:04x}:{:04x}): audio function group {:#04x}", cad, vendor >> 16, vendor & 0xFFFF, nid);
                h.outputs = h.configure(nid);
                if h.outputs > 0 {
                    break;
                }
            }
        }
        if h.outputs > 0 {
            break;
        }
    }
    if h.outputs == 0 {
        return Err("no codec output path (speaker, headphone or line out)");
    }

    // the stream: reset it, then a cyclic buffer of PAGES pages
    let sd = h.sd;
    h.w8(sd + SD_CTL, h.r8(sd + SD_CTL) | 1);
    for _ in 0..1_000_000 {
        if h.r8(sd + SD_CTL) & 1 != 0 {
            break;
        }
    }
    h.w8(sd + SD_CTL, h.r8(sd + SD_CTL) & !1);
    for _ in 0..1_000_000 {
        if h.r8(sd + SD_CTL) & 1 == 0 {
            break;
        }
    }
    for _ in 0..PAGES {
        h.pages.push(crate::memory::alloc_dma_frame().ok_or("out of memory")?);
    }
    let bdl = h.bdl.1;
    for (i, (phys, _)) in h.pages.iter().enumerate() {
        unsafe {
            write_volatile(bdl.add(i * 16) as *mut u64, *phys);
            write_volatile(bdl.add(i * 16 + 8) as *mut u32, FRAME_BYTES as u32);
            write_volatile(bdl.add(i * 16 + 12) as *mut u32, 0);
        }
    }
    h.w32(sd + SD_BDPL, h.bdl.0 as u32);
    h.w32(sd + SD_BDPU, (h.bdl.0 >> 32) as u32);
    h.w32(sd + SD_CBL, (PAGES * FRAME_BYTES) as u32);
    h.w16(sd + SD_LVI, PAGES as u16 - 1);
    h.w16(sd + SD_FMT, FORMAT);
    // stream tag in CTL bits 23:20 (byte 2)
    h.w8(sd + SD_CTL + 2, (STREAM_TAG << 4) as u8);
    Ok(h)
}

/// Find and start an HD Audio controller. Returns whether one plays.
pub fn init(devices: &[PciDevice]) -> bool {
    for dev in devices.iter().filter(|d| d.class == 0x04 && d.subclass == 0x03) {
        match bring_up(dev) {
            Ok(h) => {
                crate::println!(
                    "[hda] {:04x}:{:04x}: {} output(s), 48 kHz 16-bit stereo",
                    dev.vendor_id,
                    dev.device_id,
                    h.outputs
                );
                *DEVICE.lock() = Some(h);
                return true;
            }
            Err(e) => crate::println!("[hda] {:04x}:{:04x}: {}", dev.vendor_id, dev.device_id, e),
        }
    }
    false
}

pub fn present() -> bool {
    DEVICE.lock().is_some()
}

pub fn volume() -> u32 {
    DEVICE.lock().as_ref().map_or(0, |h| h.volume)
}

pub fn is_muted() -> bool {
    DEVICE.lock().as_ref().is_some_and(|h| h.muted)
}

pub fn set_volume(pct: u32) -> bool {
    let mut g = DEVICE.lock();
    let Some(h) = g.as_mut() else { return false };
    h.volume = pct.min(100);
    if h.volume > 0 {
        h.muted = false;
    }
    true
}

pub fn toggle_mute() -> bool {
    let mut g = DEVICE.lock();
    let Some(h) = g.as_mut() else { return false };
    h.muted = !h.muted;
    h.muted
}

pub fn is_streaming() -> bool {
    DEVICE.try_lock().is_some_and(|g| g.as_ref().is_some_and(|h| h.streaming))
}

/// Feed the cyclic pages from the mixer (see the AC'97 pump).
pub fn stream_pump() {
    let Some(mut g) = DEVICE.try_lock() else { return };
    let Some(h) = g.as_mut() else { return };
    let fill = |h: &Hda, p: usize| -> bool {
        let mut tmp = [0i16; FRAME_BYTES / 2];
        let any = crate::mixer::mix(&mut tmp);
        let out = h.pages[p].1 as *mut i16;
        for (k, v) in tmp.iter().enumerate() {
            unsafe { write_volatile(out.add(k), h.scale(*v)) };
        }
        any
    };
    if !h.streaming {
        if !crate::mixer::has_data() {
            return;
        }
        h.stop();
        for p in 0..PAGES {
            fill(h, p);
        }
        h.w8(h.sd + SD_STS, 0x1C);
        h.w8(h.sd + SD_CTL, h.r8(h.sd + SD_CTL) | 0x02); // RUN
        h.running = true;
        h.streaming = true;
        h.idle = 0;
        h.wpage = (h.r32(h.sd + SD_LPIB) as usize / FRAME_BYTES) % PAGES;
        super::audio::STREAM_FILLS.fetch_add(PAGES as u64, core::sync::atomic::Ordering::Relaxed);
        return;
    }
    let cur = (h.r32(h.sd + SD_LPIB) as usize / FRAME_BYTES) % PAGES;
    while h.wpage != cur {
        if fill(h, h.wpage) {
            h.idle = 0;
        } else {
            h.idle += 1;
        }
        h.wpage = (h.wpage + 1) % PAGES;
        super::audio::STREAM_FILLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }
    if h.idle >= PAGES {
        h.stop();
        h.streaming = false;
    }
}

pub fn is_playing() -> bool {
    DEVICE.lock().as_ref().is_some_and(|h| h.running)
}

/// Stop the stream once its sound is over (called by the desktop loop).
pub fn poll() {
    if let Some(mut g) = DEVICE.try_lock() {
        if let Some(h) = g.as_mut() {
            if h.running && !h.streaming && crate::arch::cpu::timestamp() > h.stop_at {
                h.stop();
            }
        }
    }
}

/// Play a tone (`gen` writes `frames` stereo frames into a page and returns
/// the phase to continue from).
pub fn play_tone(freq: u32, ms: u32, gen: fn(u64, usize, u32, u32) -> u32) -> bool {
    let mut g = DEVICE.lock();
    let Some(h) = g.as_mut() else { return false };
    h.stop();
    let per_page = FRAME_BYTES / 4;
    let max_ms = (SOUND_PAGES * per_page) as u64 * 1000 / 48_000;
    let ms = (ms as u64).min(max_ms);
    let mut left = (48_000 * ms / 1000) as usize;
    let mut phase = 0;
    let mut used = 0;
    while left > 0 && used < SOUND_PAGES {
        let n = left.min(per_page);
        let va = h.pages[used].1;
        unsafe { core::ptr::write_bytes(va, 0, FRAME_BYTES) };
        phase = gen(va as u64, n, freq, phase);
        let s = va as *mut i16;
        for k in 0..n * 2 {
            unsafe { write_volatile(s.add(k), h.scale(read_volatile(s.add(k)))) };
        }
        left -= n;
        used += 1;
    }
    h.start(used, ms);
    true
}

/// Play interleaved 16-bit stereo PCM from a caller buffer (read per sample).
pub fn play_pcm(src: u64, len: u64) -> bool {
    let mut g = DEVICE.lock();
    let Some(h) = g.as_mut() else { return false };
    h.stop();
    let bytes = (len as usize).min(SOUND_PAGES * FRAME_BYTES);
    let words = bytes / 2;
    let src = src as *const i16;
    let used = bytes.div_ceil(FRAME_BYTES);
    for p in 0..used {
        let out = h.pages[p].1 as *mut i16;
        for w in 0..FRAME_BYTES / 2 {
            let i = p * FRAME_BYTES / 2 + w;
            let v = if i < words { unsafe { read_volatile(src.add(i)) } } else { 0 };
            unsafe { write_volatile(out.add(w), h.scale(v)) };
        }
    }
    let ms = (bytes / 4) as u64 * 1000 / 48_000;
    h.start(used, ms);
    true
}
