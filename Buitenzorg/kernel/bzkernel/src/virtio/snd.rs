//! virtio-snd (§5.14): PCM output and input. Control requests (query stream
//! info, set parameters, prepare / start / stop / release) go over queue 0;
//! audio periods go over the TX queue (2) as `{ stream id } + samples +
//! { status, latency }` chains, and the device completes each one once it
//! has consumed the samples — so a completed period is audio the host
//! actually took. Recording uses the RX queue (3): `{ stream id }` + an
//! empty buffer the device fills + status.
//!
//! On Device-Tree boards (QEMU `virt`, ARM64 / RISC-V) this is the sound
//! card behind the AUDIO_* syscalls: the mixer is streamed with [`pump`]
//! (`SLOTS` periods kept in flight, ~170 ms), a microphone is read with
//! [`capture_start`] / [`capture_read`]. PCs keep AC'97 / HD Audio and only
//! run the boot test tone here.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use spin::Mutex;

use super::{Buf, Transport, Virtqueue};

const R_PCM_INFO: u32 = 0x0100;
const R_PCM_SET_PARAMS: u32 = 0x0101;
const R_PCM_PREPARE: u32 = 0x0102;
const R_PCM_RELEASE: u32 = 0x0103;
const R_PCM_START: u32 = 0x0104;
const R_PCM_STOP: u32 = 0x0105;
const S_OK: u32 = 0x8000;
const DIR_OUTPUT: u8 = 0;
const DIR_INPUT: u8 = 1;
const FMT_S16: u8 = 5;
const RATE_48000: u8 = 7;
const PERIOD: usize = 4096; // bytes: 1024 stereo S16 frames, ~21 ms at 48 kHz
/// Periods in flight while streaming / recording.
const SLOTS: usize = 8;
/// Recorded frames kept for the reader (4 s).
const CAPTURE_MAX: usize = 4 * 48_000;

/// One direction's ring: a data page and a header/status area per slot.
struct Ring {
    pages: [(u64, *mut u8); SLOTS],
    /// per slot: header (4 bytes) at slot * 32, status (8 bytes) at slot * 32 + 16
    meta_phys: u64,
    meta: *mut u8,
    heads: [Option<u16>; SLOTS],
    running: bool,
}

impl Ring {
    fn new() -> Option<Ring> {
        let (meta_phys, meta) = crate::memory::alloc_dma_frame()?;
        let mut pages = [(0u64, core::ptr::null_mut()); SLOTS];
        for p in pages.iter_mut() {
            *p = crate::memory::alloc_dma_frame()?;
        }
        Some(Ring { pages, meta_phys, meta, heads: [None; SLOTS], running: false })
    }
    fn busy(&self) -> usize {
        self.heads.iter().filter(|h| h.is_some()).count()
    }
}

pub struct VirtioSnd {
    t: Box<dyn Transport>,
    ctrl: Virtqueue,
    tx: Virtqueue,
    _event: Virtqueue,
    rx: Virtqueue,
    stream: u32,
    /// the first input stream (48 kHz S16, 1 or 2 channels), u32::MAX if none
    in_stream: u32,
    in_channels: u8,
    /// Page 0: request (0) / response (2048). Page 1: one audio period.
    cmd_phys: u64,
    cmd: *mut u8,
    pcm_phys: u64,
    pcm: *mut u8,
    pub periods: u64,
    out: Option<Ring>,
    inp: Option<Ring>,
    silent: u32,
    captured: VecDeque<(i16, i16)>,
    /// recording: when it started (ns) and the frames received since -- a
    /// device may return input buffers faster than real time (QEMU's `none`
    /// backend does), so buffers are queued only as fast as the clock allows
    cap_start_ns: u64,
    cap_frames: u64,
}

unsafe impl Send for VirtioSnd {}

static SND: Mutex<Option<VirtioSnd>> = Mutex::new(None);
/// Periods the mixer filled / the microphone delivered (tests, `audio` status).
pub static STREAM_FILLS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static CAPTURE_FILLS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

impl VirtioSnd {
    fn control(&mut self, words: &[u32], resp_len: usize) -> Result<u32, &'static str> {
        unsafe {
            core::ptr::write_bytes(self.cmd, 0, 4096);
            for (i, w) in words.iter().enumerate() {
                (self.cmd.add(i * 4) as *mut u32).write_volatile(*w);
            }
        }
        let req = Buf { phys: self.cmd_phys, len: (words.len() * 4) as u32, device_writes: false };
        let resp = Buf { phys: self.cmd_phys + 2048, len: resp_len as u32, device_writes: true };
        self.ctrl.submit_wait(self.t.as_ref(), &[req, resp])?;
        Ok(unsafe { (self.cmd.add(2048) as *const u32).read_volatile() })
    }

    fn ok(&mut self, words: &[u32]) -> Result<(), &'static str> {
        match self.control(words, 4)? {
            S_OK => Ok(()),
            _ => Err("virtio-snd: request refused"),
        }
    }

    fn new(mut t: Box<dyn Transport>) -> Result<VirtioSnd, &'static str> {
        super::begin_init(t.as_mut(), 0)?;
        let ctrl = Virtqueue::new(t.as_mut(), 0)?;
        let event = Virtqueue::new(t.as_mut(), 1)?;
        let tx = Virtqueue::new(t.as_mut(), 2)?;
        let rx = Virtqueue::new(t.as_mut(), 3)?;
        super::finish_init(t.as_mut());
        let streams = t.config_read32(4);
        let (cmd_phys, cmd) = crate::memory::alloc_dma_frame().ok_or("virtio-snd: out of memory")?;
        let (pcm_phys, pcm) = crate::memory::alloc_dma_frame().ok_or("virtio-snd: out of memory")?;
        let mut s = VirtioSnd {
            t,
            ctrl,
            tx,
            _event: event,
            rx,
            stream: u32::MAX,
            in_stream: u32::MAX,
            in_channels: 0,
            cmd_phys,
            cmd,
            pcm_phys,
            pcm,
            periods: 0,
            out: None,
            inp: None,
            silent: 0,
            captured: VecDeque::new(),
            cap_start_ns: 0,
            cap_frames: 0,
        };

        // Stream info: 32 bytes each after the 4-byte status.
        let n = streams.min(16);
        if s.control(&[R_PCM_INFO, 0, n, 32], 4 + 32 * n as usize)? != S_OK {
            return Err("virtio-snd: PCM_INFO failed");
        }
        for i in 0..n as usize {
            let info = unsafe { s.cmd.add(2048 + 4 + i * 32) };
            let formats = unsafe { (info.add(8) as *const u64).read_volatile() };
            let rates = unsafe { (info.add(16) as *const u64).read_volatile() };
            let (dir, cmin, cmax) = unsafe { (info.add(24).read_volatile(), info.add(25).read_volatile(), info.add(26).read_volatile()) };
            let fmt_ok = formats & (1 << FMT_S16) != 0 && rates & (1 << RATE_48000) != 0;
            if dir == DIR_OUTPUT && fmt_ok && cmin <= 2 && cmax >= 2 && s.stream == u32::MAX {
                s.stream = i as u32;
            }
            if dir == DIR_INPUT && fmt_ok && s.in_stream == u32::MAX {
                s.in_stream = i as u32;
                s.in_channels = if cmin <= 2 && cmax >= 2 { 2 } else { cmin.max(1) };
            }
        }
        if s.stream == u32::MAX {
            return Err("virtio-snd: no 48 kHz S16 stereo output stream");
        }
        crate::println!(
            "[virtio] snd at {}: {} stream(s), output stream {} (48 kHz S16 stereo){}",
            s.t.location(),
            streams,
            s.stream,
            if s.in_stream != u32::MAX { alloc::format!(", input stream {} ({} channel(s))", s.in_stream, s.in_channels) } else { alloc::string::String::new() }
        );
        Ok(s)
    }

    fn open(&mut self, id: u32, channels: u8) -> Result<(), &'static str> {
        // set_params: hdr{code, stream}, buffer_bytes, period_bytes, features,
        // then channels u8, format u8, rate u8, padding u8.
        let packed = channels as u32 | (FMT_S16 as u32) << 8 | (RATE_48000 as u32) << 16;
        self.ok(&[R_PCM_SET_PARAMS, id, (PERIOD * SLOTS) as u32, PERIOD as u32, 0, packed])?;
        self.ok(&[R_PCM_PREPARE, id])?;
        self.ok(&[R_PCM_START, id])
    }

    fn close(&mut self, id: u32) {
        let _ = self.ok(&[R_PCM_STOP, id]);
        let _ = self.ok(&[R_PCM_RELEASE, id]);
    }

    /// Play `periods` periods of a `freq` Hz tone; returns how many the device
    /// completed with status OK.
    fn play_tone(&mut self, freq: u32, periods: usize) -> Result<usize, &'static str> {
        let id = self.stream;
        self.open(id, 2)?;
        let frames = PERIOD / 4;
        let step = ((freq as u64) << 16) / 48_000;
        let mut phase: u32 = 0;
        let mut good = 0;
        for _ in 0..periods {
            let out = self.pcm as *mut i16;
            for f in 0..frames {
                // Triangle wave, about -6 dB.
                let p = (phase >> 6) & 0x3FF;
                let tri = if p < 512 { p as i32 - 256 } else { 767 - p as i32 } * 32;
                unsafe {
                    out.add(f * 2).write_volatile(tri as i16);
                    out.add(f * 2 + 1).write_volatile(tri as i16);
                }
                phase = phase.wrapping_add(step as u32);
            }
            // xfer header at cmd+3072, status at cmd+3584 (cmd page is ours).
            unsafe {
                (self.cmd.add(3072) as *mut u32).write_volatile(id);
                (self.cmd.add(3584) as *mut u32).write_volatile(0xFFFF_FFFF);
            }
            let hdr = Buf { phys: self.cmd_phys + 3072, len: 4, device_writes: false };
            let data = Buf { phys: self.pcm_phys, len: PERIOD as u32, device_writes: false };
            let status = Buf { phys: self.cmd_phys + 3584, len: 8, device_writes: true };
            self.tx.submit_wait(self.t.as_ref(), &[hdr, data, status])?;
            if unsafe { (self.cmd.add(3584) as *const u32).read_volatile() } == S_OK {
                good += 1;
                self.periods += 1;
            }
        }
        self.close(id);
        Ok(good)
    }

    // ---- streaming the mixer ------------------------------------------------

    /// `vol` (percent) is read by the caller before it locks `SND`: the
    /// platform's volume query asks `available()`, which takes that lock.
    fn pump_out(&mut self, vol: i32) {
        if self.out.is_none() {
            self.out = Ring::new();
        }
        let running = self.out.as_ref().is_some_and(|r| r.running);
        if !running {
            if !crate::mixer::has_data() {
                return;
            }
            if self.open(self.stream, 2).is_err() {
                return;
            }
            self.silent = 0;
            if let Some(r) = self.out.as_mut() {
                r.running = true;
            }
        }
        // finished periods free their slot
        while let Some((id, _)) = self.tx.pop_used() {
            if let Some(r) = self.out.as_mut() {
                if let Some(k) = r.heads.iter().position(|h| *h == Some(id)) {
                    r.heads[k] = None;
                }
            }
        }
        let mut pushed = false;
        let mut buf = [0i16; PERIOD / 2];
        for k in 0..SLOTS {
            let Some(r) = self.out.as_ref() else { return };
            if r.heads[k].is_some() {
                continue;
            }
            let any = crate::mixer::mix(&mut buf);
            if any {
                self.silent = 0;
                STREAM_FILLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            } else {
                self.silent += 1;
                if self.silent > SLOTS as u32 * 2 {
                    break; // nothing to play: let the ring drain
                }
            }
            let r = self.out.as_mut().unwrap();
            let (phys, va) = r.pages[k];
            unsafe {
                let out = va as *mut i16;
                for (i, s) in buf.iter().enumerate() {
                    out.add(i).write_volatile((*s as i32 * vol / 100) as i16);
                }
                (r.meta.add(k * 32) as *mut u32).write_volatile(self.stream);
                (r.meta.add(k * 32 + 16) as *mut u32).write_volatile(0xFFFF_FFFF);
            }
            let hdr = Buf { phys: r.meta_phys + (k * 32) as u64, len: 4, device_writes: false };
            let data = Buf { phys, len: PERIOD as u32, device_writes: false };
            let status = Buf { phys: r.meta_phys + (k * 32 + 16) as u64, len: 8, device_writes: true };
            match self.tx.push(&[hdr, data, status]) {
                Ok(head) => {
                    self.out.as_mut().unwrap().heads[k] = Some(head);
                    pushed = true;
                }
                Err(_) => break,
            }
        }
        if pushed {
            self.t.notify(self.tx.index);
        }
        let r = self.out.as_ref().unwrap();
        if self.silent > SLOTS as u32 * 2 && r.busy() == 0 {
            let id = self.stream;
            self.close(id);
            self.out.as_mut().unwrap().running = false;
        }
    }

    // ---- recording -------------------------------------------------------------

    fn capture_start(&mut self) -> bool {
        if self.in_stream == u32::MAX {
            return false;
        }
        if self.inp.is_none() {
            self.inp = Ring::new();
        }
        if self.inp.as_ref().is_some_and(|r| r.running) {
            return true;
        }
        let (id, ch) = (self.in_stream, self.in_channels);
        if self.open(id, ch).is_err() {
            return false;
        }
        self.captured.clear();
        self.cap_start_ns = crate::arch::cpu::nanos();
        self.cap_frames = 0;
        self.inp.as_mut().unwrap().running = true;
        self.capture_pump();
        true
    }

    fn capture_stop(&mut self) {
        if !self.inp.as_ref().is_some_and(|r| r.running) {
            return;
        }
        self.capture_pump();
        let id = self.in_stream;
        self.close(id);
        // the device returns the buffers it held
        for _ in 0..1000 {
            while let Some((head, _)) = self.rx.pop_used() {
                if let Some(r) = self.inp.as_mut() {
                    if let Some(k) = r.heads.iter().position(|h| *h == Some(head)) {
                        r.heads[k] = None;
                    }
                }
            }
            if self.inp.as_ref().map(|r| r.busy()).unwrap_or(0) == 0 {
                break;
            }
            core::hint::spin_loop();
        }
        if let Some(r) = self.inp.as_mut() {
            r.running = false;
            r.heads = [None; SLOTS];
        }
    }

    fn capture_pump(&mut self) {
        if !self.inp.as_ref().is_some_and(|r| r.running) {
            return;
        }
        let ch = self.in_channels.max(1) as usize;
        while let Some((head, len)) = self.rx.pop_used() {
            let Some(r) = self.inp.as_mut() else { break };
            let Some(k) = r.heads.iter().position(|h| *h == Some(head)) else { continue };
            r.heads[k] = None;
            let ok = unsafe { (r.meta.add(k * 32 + 16) as *const u32).read_volatile() } == S_OK;
            // `len` counts the data and the 8-byte status the device wrote
            let bytes = (len as usize).saturating_sub(8).min(PERIOD);
            if ok {
                let p = r.pages[k].1 as *const i16;
                let frames = bytes / 2 / ch;
                for f in 0..frames {
                    let l = unsafe { p.add(f * ch).read_volatile() };
                    let rr = if ch >= 2 { unsafe { p.add(f * ch + 1).read_volatile() } } else { l };
                    if self.captured.len() >= CAPTURE_MAX {
                        self.captured.pop_front();
                    }
                    self.captured.push_back((l, rr));
                }
                self.cap_frames += frames as u64;
                CAPTURE_FILLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            }
        }
        let mut pushed = false;
        let frames_per_period = (PERIOD / 2 / ch) as u64;
        for k in 0..SLOTS {
            let Some(r) = self.inp.as_ref() else { return };
            if r.heads[k].is_some() {
                continue;
            }
            // pace to real time: what is in flight plus what arrived may not
            // run more than SLOTS periods ahead of the clock
            let due = (crate::arch::cpu::nanos() - self.cap_start_ns) * 48 / 1_000_000;
            let ahead = self.cap_frames + r.busy() as u64 * frames_per_period;
            if ahead > due + SLOTS as u64 * frames_per_period {
                break;
            }
            unsafe {
                (r.meta.add(k * 32) as *mut u32).write_volatile(self.in_stream);
                (r.meta.add(k * 32 + 16) as *mut u32).write_volatile(0xFFFF_FFFF);
            }
            let hdr = Buf { phys: r.meta_phys + (k * 32) as u64, len: 4, device_writes: false };
            let data = Buf { phys: r.pages[k].0, len: PERIOD as u32, device_writes: true };
            let status = Buf { phys: r.meta_phys + (k * 32 + 16) as u64, len: 8, device_writes: true };
            match self.rx.push(&[hdr, data, status]) {
                Ok(head) => {
                    self.inp.as_mut().unwrap().heads[k] = Some(head);
                    pushed = true;
                }
                Err(_) => break,
            }
        }
        if pushed {
            self.t.notify(self.rx.index);
        }
    }
}

/// Find the first virtio sound device; `test` plays a short tone through it.
pub fn init_with(test: bool) {
    for t in super::find(super::device::SOUND) {
        let loc = t.location();
        match VirtioSnd::new(t) {
            Ok(mut s) => {
                const PERIODS: usize = 12; // ~0.25 s
                if test {
                    match s.play_tone(440, PERIODS) {
                        Ok(n) if n == PERIODS => crate::println!(
                            "[kernel] MILESTONE: VIRTIO SND OK (PCM_INFO + SET_PARAMS/PREPARE/START/STOP/RELEASE; {} periods of a 440 Hz tone consumed by the device, status OK)",
                            n
                        ),
                        Ok(n) => crate::println!("[virtio] snd: only {}/{} periods completed OK", n, PERIODS),
                        Err(e) => crate::println!("[virtio] snd: {}", e),
                    }
                }
                *SND.lock() = Some(s);
                return;
            }
            Err(e) => crate::println!("[virtio] snd at {}: {}", loc, e),
        }
    }
}

/// The boot test (self-test boots of a PC with a virtio sound card).
pub fn init() {
    init_with(true);
}

/// A virtio sound card was found.
pub fn available() -> bool {
    SND.lock().is_some()
}

pub fn can_capture() -> bool {
    SND.lock().as_ref().is_some_and(|s| s.in_stream != u32::MAX)
}

pub fn is_streaming() -> bool {
    SND.lock().as_ref().is_some_and(|s| s.out.as_ref().is_some_and(|r| r.running))
}

/// Move mixed audio to the device and recorded audio from it (desktop loop,
/// AUDIO_WRITE / AUDIO_READ). Never blocks: `try_lock`.
pub fn pump() {
    // never call into crate::audio while holding SND (it asks available())
    let vol = crate::audio::volume().min(100) as i32;
    if let Some(mut g) = SND.try_lock() {
        if let Some(s) = g.as_mut() {
            s.pump_out(vol);
            s.capture_pump();
        }
    }
}

pub fn capture_start() -> bool {
    SND.lock().as_mut().is_some_and(|s| s.capture_start())
}

pub fn capture_stop() {
    if let Some(s) = SND.lock().as_mut() {
        s.capture_stop();
    }
}

/// Take recorded frames into `out` (interleaved stereo); the samples copied.
pub fn capture_read(out: &mut [i16]) -> usize {
    let mut g = SND.lock();
    let Some(s) = g.as_mut() else { return 0 };
    s.capture_pump();
    let frames = (out.len() / 2).min(s.captured.len());
    for k in 0..frames {
        let (l, r) = s.captured.pop_front().unwrap();
        out[2 * k] = l;
        out[2 * k + 1] = r;
    }
    frames * 2
}

pub fn capture_queued_ms() -> u64 {
    SND.lock().as_ref().map(|s| s.captured.len() as u64 * 1000 / 48_000).unwrap_or(0)
}

/// Self-test on Device-Tree boards: one second of mixer audio streams through
/// the TX ring (periods refilled while it plays, the stream stops once the
/// mixer is empty), then the microphone records for half a second.
pub fn stream_self_test() -> bool {
    use core::sync::atomic::Ordering;
    if !available() {
        crate::println!("[virtio] snd: no sound device (stream test skipped)");
        return false;
    }
    const PID: u64 = u64::MAX - 3;
    let frames = 48_000usize;
    let mut bytes = alloc::vec::Vec::with_capacity(frames * 4);
    for n in 0..frames {
        let s = if (n / 55) % 2 == 0 { 4000i16 } else { -4000 };
        bytes.extend_from_slice(&s.to_le_bytes());
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    let took = crate::mixer::write(PID, &bytes, 0);
    let f0 = STREAM_FILLS.load(Ordering::Relaxed);
    pump();
    let started = is_streaming();
    let t0 = crate::arch::cpu::nanos();
    while crate::arch::cpu::nanos() - t0 < 600_000_000 {
        pump();
        crate::task::yield_now();
    }
    let fills = STREAM_FILLS.load(Ordering::Relaxed) - f0;
    let left = crate::mixer::queued_ms(PID);
    crate::mixer::on_exit(PID);
    let t1 = crate::arch::cpu::nanos();
    while is_streaming() && crate::arch::cpu::nanos() - t1 < 3_000_000_000 {
        pump();
        crate::task::yield_now();
    }
    let stopped = !is_streaming();
    let ok = took == bytes.len() && started && fills > 12 && left < 1000 && stopped;
    if ok {
        crate::println!(
            "[kernel] MILESTONE: VIRTIO SND STREAM OK (mixer -> TX ring: {} periods in 0.6 s, {} ms left of 1000, stream stopped after it)",
            fills,
            left
        );
    } else {
        crate::println!("[virtio] snd stream test: took {} started {} fills {} left {} ms stopped {}", took, started, fills, left, stopped);
    }
    // the microphone
    if can_capture() {
        let c0 = CAPTURE_FILLS.load(Ordering::Relaxed);
        let on = capture_start();
        let t0 = crate::arch::cpu::nanos();
        let mut got = 0;
        let mut buf = alloc::vec![0i16; 8192];
        while crate::arch::cpu::nanos() - t0 < 500_000_000 {
            got += capture_read(&mut buf);
            crate::task::yield_now();
        }
        capture_stop();
        got += capture_read(&mut buf);
        let ms = (crate::arch::cpu::nanos() - t0) / 1_000_000;
        let fills = CAPTURE_FILLS.load(Ordering::Relaxed) - c0;
        let frames = got / 2;
        if on && fills >= 4 && frames as u64 * 1000 >= 48_000 * ms / 3 {
            crate::println!("[kernel] MILESTONE: VIRTIO SND CAPTURE OK (RX ring: {} periods, {} frames in {} ms)", fills, frames, ms);
        } else {
            crate::println!("[virtio] snd capture test: started {} periods {} frames {} in {} ms", on, fills, frames, ms);
        }
    }
    ok
}
