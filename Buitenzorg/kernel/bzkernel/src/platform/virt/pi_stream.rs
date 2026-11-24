//! Streaming sound on the Raspberry Pi 4: the kernel mixer (`mixer.rs`,
//! AUDIO_WRITE) played on the headphone jack (PWM1) and on HDMI0 (MAI) by the
//! SoC's DMA engine, so a program such as DOOM can keep a stream going while
//! the CPU does other work. Without it the Pi could only play a whole sound
//! with the CPU feeding the FIFOs (`rpi_io::play_frames`), which is why
//! AUDIO_WRITE answered NOSYS there.
//!
//! Each output has a ring of `NBUF` buffers of `FRAMES` stereo frames
//! (~21 ms each at 48 kHz) and one DMA control block per buffer, chained in a
//! circle; the channel is paced by the peripheral's DREQ (PWM1 = 1,
//! HDMI0 = 10). The kernel task `pi-audio` refills every buffer the engine
//! has left behind (`CONBLK_AD` names the block it is on) with the next mixed
//! frames -- converted to PWM levels for the jack and to IEC 958 sub-frames
//! for HDMI -- and stops the channels after ~0.2 s with nothing to play.
//!
//! Legacy DMA channels (0-6) address the first GiB of RAM at bus
//! 0xC000_0000 and the peripherals at 0x7Exx_xxxx. Channels 4 and 5 are in
//! the DTB's `brcm,dma-channel-mask` (0x7f5) on the Pi 4. QEMU `raspi4b`
//! models neither PWM nor HDMI audio, so this has only run on a board; every
//! output checks at start-up that its engine moves (else it is not used and
//! the CPU path stays).

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

const NBUF: usize = 8;
const FRAMES: usize = 1024;
const WORDS: usize = FRAMES * 2; // one u32 per sample (left, right)
const BUF_BYTES: usize = WORDS * 4;
/// Buffers of silence in a row before the channels stop.
const IDLE_STOP: u32 = NBUF as u32 + 2;

const DMA_OFF: u64 = 0x7000; // from the peripheral base (0xfe00_0000)
const CS: u64 = 0x00;
const CONBLK_AD: u64 = 0x04;
const DEBUG: u64 = 0x20;
const ENABLE: u64 = 0xff0;
const CS_ACTIVE: u32 = 1 << 0;
const CS_END: u32 = 1 << 1;
const CS_INT: u32 = 1 << 2;
const CS_ERROR: u32 = 1 << 8;
const CS_WAIT_WRITES: u32 = 1 << 28;
const CS_ABORT: u32 = 1 << 30;
const CS_RESET: u32 = 1 << 31;
// TI: wait for write responses, destination paced by DREQ, source increments
const TI_WAIT_RESP: u32 = 1 << 3;
const TI_DEST_DREQ: u32 = 1 << 6;
const TI_SRC_INC: u32 = 1 << 8;
const BUS_RAM: u32 = 0xC000_0000;
const BUS_LIMIT: u64 = 0x4000_0000; // legacy DMA reaches the first GiB only

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Jack,
    Hdmi,
}

struct Out {
    kind: Kind,
    chan: u64,     // channel registers (VA)
    cbs: *mut u32, // NBUF control blocks of 8 words
    cbs_bus: u32,
    bufs: *mut u32, // NBUF buffers of WORDS
    fill: usize,   // next buffer to write
    frame: u32,    // IEC 958 frame counter (HDMI)
}

unsafe impl Send for Out {}

struct Stream {
    outs: Vec<Out>,
    running: bool,
    idle: u32,
    last_pump: u64,
}

static STREAM: Mutex<Option<Stream>> = Mutex::new(None);
static AVAILABLE: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);
static WAKE: AtomicU32 = AtomicU32::new(0);
static FILLS: AtomicU64 = AtomicU64::new(0);
static UNDERRUNS: AtomicU64 = AtomicU64::new(0);
const TASK_KEY: u64 = 0xA5 << 48 | 0x5157;

fn r(a: u64) -> u32 {
    unsafe { (a as *const u32).read_volatile() }
}
fn w(a: u64, v: u32) {
    unsafe { (a as *mut u32).write_volatile(v) }
}

/// Whether the Pi streams the mixer (an output passed its start-up check).
pub fn available() -> bool {
    AVAILABLE.load(Ordering::Relaxed)
}
pub fn is_streaming() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

/// Contiguous zeroed memory below 1 GiB (what a legacy DMA channel reaches).
fn alloc_low(pages: usize) -> Option<(u64, *mut u8)> {
    let (phys, va) = crate::memory::alloc_dma_contiguous(pages)?;
    if phys + (pages as u64) * 4096 > BUS_LIMIT {
        crate::println!("[pi-audio] DMA memory at {:#x} is above 1 GiB: streaming unavailable", phys);
        return None;
    }
    Some((phys, va))
}

fn new_out(dma: u64, n: u64, kind: Kind, dreq: u32, dest_bus: u32) -> Option<Out> {
    let (cphys, cva) = alloc_low(1)?;
    let pages = (NBUF * BUF_BYTES).div_ceil(4096);
    let (bphys, bva) = alloc_low(pages)?;
    let cbs = cva as *mut u32;
    let ti = TI_WAIT_RESP | TI_DEST_DREQ | TI_SRC_INC | (dreq << 16);
    for i in 0..NBUF {
        let cb = unsafe { cbs.add(i * 8) };
        let vals = [
            ti,
            BUS_RAM | (bphys as u32 + (i * BUF_BYTES) as u32),
            dest_bus,
            BUF_BYTES as u32,
            0,
            BUS_RAM | (cphys as u32 + (((i + 1) % NBUF) * 32) as u32),
            0,
            0,
        ];
        for (k, v) in vals.iter().enumerate() {
            unsafe { cb.add(k).write_volatile(*v) };
        }
    }
    crate::arch::cpu::dma_sync(cva as u64, 4096);
    let mut o = Out { kind, chan: dma + n * 0x100, cbs, cbs_bus: BUS_RAM | cphys as u32, bufs: bva as *mut u32, fill: 0, frame: 0 };
    for i in 0..NBUF {
        o.write_silence(i);
    }
    Some(o)
}

impl Out {
    fn buf(&self, i: usize) -> *mut u32 {
        unsafe { self.bufs.add(i * WORDS) }
    }

    fn write(&mut self, i: usize, pcm: &[i16], vol: i32) {
        let b = self.buf(i);
        match self.kind {
            Kind::Jack => {
                let range = super::rpi_io::PWM_RANGE;
                for (k, &s) in pcm.iter().enumerate() {
                    let v = s as i32 * vol / 100;
                    let lvl = ((v + 32768) as u32 * range / 65536).min(range);
                    unsafe { b.add(k).write_volatile(lvl) };
                }
            }
            Kind::Hdmi => {
                for f in 0..FRAMES {
                    let n = self.frame % 192;
                    self.frame = self.frame.wrapping_add(1);
                    let cbit = n == 25; // channel status: 48 kHz
                    let l = (pcm[2 * f] as i32 * vol / 100) as i16;
                    let rr = (pcm[2 * f + 1] as i32 * vol / 100) as i16;
                    let pre = if n == 0 { 0x8 } else { 0x2 };
                    unsafe {
                        b.add(2 * f).write_volatile(super::hdmi_audio::subframe(l, pre, cbit));
                        b.add(2 * f + 1).write_volatile(super::hdmi_audio::subframe(rr, 0x4, cbit));
                    }
                }
            }
        }
        crate::arch::cpu::dma_sync(b as u64, BUF_BYTES);
    }

    fn write_silence(&mut self, i: usize) {
        let zero = [0i16; WORDS];
        self.write(i, &zero, 100);
    }

    /// The buffer the engine is on.
    fn playing(&self) -> usize {
        let at = r(self.chan + CONBLK_AD);
        let i = (at.wrapping_sub(self.cbs_bus) / 32) as usize;
        if i < NBUF { i } else { 0 }
    }

    fn start(&mut self) {
        w(self.chan + CS, CS_RESET);
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
        w(self.chan + CS, CS_END | CS_INT);
        w(self.chan + CONBLK_AD, self.cbs_bus);
        w(self.chan + CS, CS_ACTIVE | CS_WAIT_WRITES | (8 << 16) | (15 << 20));
        self.fill = 0;
    }

    fn stop(&mut self) {
        w(self.chan + CS, r(self.chan + CS) & !CS_ACTIVE);
        w(self.chan + CS, CS_ABORT);
        w(self.chan + CS, CS_RESET);
    }
}

/// Set up the DMA rings for the outputs there are and check that each engine
/// moves (one buffer of silence must play within 100 ms). Called once after
/// `rpi_io::init`; starts the `pi-audio` task when an output works.
pub fn init(periph: u64) {
    let probe = crate::arch::aarch64::exceptions::probe_read32;
    let Some(dma) = crate::memory::map_mmio(periph + DMA_OFF, 0x1000) else { return };
    if probe(dma + ENABLE).is_none() {
        crate::println!("[pi-audio] no DMA controller answers: no streaming");
        return;
    }
    let mut outs = Vec::new();
    if let Some(pwm) = super::rpi_io::jack_for_dma() {
        // PWM1 FIFO 0x7e20c818; DMA requests when the FIFO is below 7 words
        w(pwm + 0x08, (1 << 31) | (7 << 8) | 7);
        if let Some(o) = new_out(dma, 4, Kind::Jack, 1, 0x7e20_c818) {
            outs.push(o);
        }
    }
    if super::hdmi_audio::enable_dma() {
        // MAI_DATA in the HD block: 0x7ef2001c
        if let Some(o) = new_out(dma, 5, Kind::Hdmi, 10, 0x7ef2_001c) {
            outs.push(o);
        }
    }
    if outs.is_empty() {
        crate::println!("[pi-audio] no output for streaming (no jack, no HDMI audio)");
        return;
    }
    w(dma + ENABLE, r(dma + ENABLE) | (1 << 4) | (1 << 5));
    // start-up check: each engine must leave its first block
    let hz = crate::arch::cpu::counter_hz().max(1);
    let mut good = Vec::new();
    for mut o in outs {
        o.start();
        let t0 = crate::arch::cpu::timestamp();
        let mut moved = false;
        while crate::arch::cpu::timestamp() - t0 < hz / 10 {
            if o.playing() != 0 {
                moved = true;
                break;
            }
        }
        let cs = r(o.chan + CS);
        let dbg = r(o.chan + DEBUG);
        o.stop();
        let name = if o.kind == Kind::Jack { "headphone jack (PWM1, DREQ 1, channel 4)" } else { "HDMI0 (MAI, DREQ 10, channel 5)" };
        if moved && cs & CS_ERROR == 0 {
            crate::println!("[pi-audio] {}: DMA stream ok", name);
            good.push(o);
        } else {
            crate::println!("[pi-audio] {}: DMA did not move (CS {:#x}, DEBUG {:#x}): not streamed", name, cs, dbg);
            if o.kind == Kind::Hdmi {
                super::hdmi_audio::disable_dma();
            }
        }
    }
    if good.is_empty() {
        return;
    }
    *STREAM.lock() = Some(Stream { outs: good, running: false, idle: 0, last_pump: 0 });
    AVAILABLE.store(true, Ordering::Release);
    crate::task::spawn_unlocked("pi-audio", task);
}

/// Mixed frames were queued (AUDIO_WRITE / a tone): start the stream if it is
/// stopped and wake the task.
pub fn kick() {
    if !available() {
        return;
    }
    if !RUNNING.load(Ordering::Acquire) {
        if let Some(mut g) = STREAM.try_lock() {
            if let Some(s) = g.as_mut() {
                start(s);
            }
        }
    }
    WAKE.fetch_add(1, Ordering::Release);
    crate::task::futex_wake(TASK_KEY, 1);
}

fn start(s: &mut Stream) {
    if s.running || !crate::mixer::has_data() {
        return;
    }
    // prefill every buffer with what the mixer has (silence after it)
    let mut pcm = alloc::vec![0i16; WORDS];
    let vol = super::rpi_io::audio_volume() as i32;
    for i in 0..NBUF {
        pcm.iter_mut().for_each(|x| *x = 0);
        crate::mixer::mix(&mut pcm);
        for o in s.outs.iter_mut() {
            o.write(i, &pcm, vol);
        }
    }
    for o in s.outs.iter_mut() {
        o.start();
    }
    s.running = true;
    s.idle = 0;
    s.last_pump = crate::arch::cpu::nanos();
    RUNNING.store(true, Ordering::Release);
}

/// Refill the buffers the engines have played; stop after a stretch of silence.
fn pump(s: &mut Stream) {
    if !s.running {
        return;
    }
    let vol = super::rpi_io::audio_volume() as i32;
    let mut pcm = alloc::vec![0i16; WORDS];
    // a pause longer than the ring: the engines lapped and replayed old buffers
    let now = crate::arch::cpu::nanos();
    let ring_ns = (NBUF * FRAMES) as u64 * 1_000_000_000 / super::rpi_io::SAMPLE_RATE as u64;
    if now - s.last_pump > ring_ns {
        UNDERRUNS.fetch_add(1, Ordering::Relaxed);
    }
    s.last_pump = now;
    // the first output paces the mixer; the others take the same frames
    while s.outs[0].fill != s.outs[0].playing() {
        pcm.iter_mut().for_each(|x| *x = 0);
        let any = crate::mixer::mix(&mut pcm);
        s.idle = if any { 0 } else { s.idle + 1 };
        for (k, o) in s.outs.iter_mut().enumerate() {
            let slot = o.fill;
            // an output on a slightly different clock (PWM runs at 48.04 kHz):
            // skip a slot its engine is still on rather than write under it
            if k == 0 || o.playing() != slot {
                o.write(slot, &pcm, vol);
                o.fill = (slot + 1) % NBUF;
            }
        }
        FILLS.fetch_add(1, Ordering::Relaxed);
    }
    if s.idle >= IDLE_STOP {
        for o in s.outs.iter_mut() {
            o.stop();
        }
        s.running = false;
        RUNNING.store(false, Ordering::Release);
    }
}

fn task() {
    loop {
        let running = {
            let mut g = STREAM.lock();
            let s = g.as_mut().unwrap();
            if !s.running {
                start(s);
            }
            pump(s);
            s.running
        };
        if running {
            // a buffer lasts ~21 ms; wake every ~8 ms (timed waits expire on
            // the 100 Hz tick, so this is 10 ms in practice)
            let seen = WAKE.load(Ordering::Acquire);
            let deadline = crate::arch::cpu::nanos() + 8_000_000;
            crate::task::futex_wait_timeout_if(&WAKE as *const AtomicU32 as u64, seen, deadline);
        } else {
            crate::task::block_while(TASK_KEY, || !crate::mixer::has_data());
        }
    }
}

/// For `audio` in the shell: buffers refilled and how often an engine had stopped.
pub fn stats() -> (u64, u64) {
    (FILLS.load(Ordering::Relaxed), UNDERRUNS.load(Ordering::Relaxed))
}
