//! virtio-snd (§5.14): PCM output. Control requests (query stream info, set
//! parameters, prepare / start / stop / release) go over queue 0; audio
//! periods go over the TX queue (2) as `{ stream id } + samples + { status,
//! latency }` chains, and the device completes each one once it has consumed
//! the samples — so a completed period is audio the host actually took.

use alloc::boxed::Box;
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
const FMT_S16: u8 = 5;
const RATE_48000: u8 = 7;
const PERIOD: usize = 4096; // bytes: 1024 stereo S16 frames, ~21 ms at 48 kHz

pub struct VirtioSnd {
    t: Box<dyn Transport>,
    ctrl: Virtqueue,
    tx: Virtqueue,
    _event: Virtqueue,
    _rx: Virtqueue,
    stream: u32,
    /// Page 0: request (0) / response (2048). Page 1: one audio period.
    cmd_phys: u64,
    cmd: *mut u8,
    pcm_phys: u64,
    pcm: *mut u8,
    pub periods: u64,
}

unsafe impl Send for VirtioSnd {}

static SND: Mutex<Option<VirtioSnd>> = Mutex::new(None);

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
        let mut s = VirtioSnd { t, ctrl, tx, _event: event, _rx: rx, stream: u32::MAX, cmd_phys, cmd, pcm_phys, pcm, periods: 0 };

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
            if dir == DIR_OUTPUT && formats & (1 << FMT_S16) != 0 && rates & (1 << RATE_48000) != 0 && cmin <= 2 && cmax >= 2 {
                s.stream = i as u32;
                break;
            }
        }
        if s.stream == u32::MAX {
            return Err("virtio-snd: no 48 kHz S16 stereo output stream");
        }
        crate::println!(
            "[virtio] snd at {}: {} stream(s), output stream {} (48 kHz S16 stereo)",
            s.t.location(),
            streams,
            s.stream
        );
        Ok(s)
    }

    /// Play `periods` periods of a `freq` Hz tone; returns how many the device
    /// completed with status OK.
    fn play_tone(&mut self, freq: u32, periods: usize) -> Result<usize, &'static str> {
        let id = self.stream;
        // set_params: hdr{code, stream}, buffer_bytes, period_bytes, features,
        // then channels u8, format u8, rate u8, padding u8.
        let packed = 2u32 | (FMT_S16 as u32) << 8 | (RATE_48000 as u32) << 16;
        self.ok(&[R_PCM_SET_PARAMS, id, (PERIOD * 4) as u32, PERIOD as u32, 0, packed])?;
        self.ok(&[R_PCM_PREPARE, id])?;
        self.ok(&[R_PCM_START, id])?;
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
        self.ok(&[R_PCM_STOP, id])?;
        self.ok(&[R_PCM_RELEASE, id])?;
        Ok(good)
    }
}

/// Find the first virtio sound device and play a short test tone through it.
pub fn init() {
    for t in super::pci::find(super::device::SOUND) {
        let loc = t.location();
        match VirtioSnd::new(t) {
            Ok(mut s) => {
                const PERIODS: usize = 12; // ~0.25 s
                match s.play_tone(440, PERIODS) {
                    Ok(n) if n == PERIODS => crate::println!(
                        "[kernel] MILESTONE: VIRTIO SND OK (PCM_INFO + SET_PARAMS/PREPARE/START/STOP/RELEASE; {} periods of a 440 Hz tone consumed by the device, status OK)",
                        n
                    ),
                    Ok(n) => crate::println!("[virtio] snd: only {}/{} periods completed OK", n, PERIODS),
                    Err(e) => crate::println!("[virtio] snd: {}", e),
                }
                *SND.lock() = Some(s);
                return;
            }
            Err(e) => crate::println!("[virtio] snd at {}: {}", loc, e),
        }
    }
}
