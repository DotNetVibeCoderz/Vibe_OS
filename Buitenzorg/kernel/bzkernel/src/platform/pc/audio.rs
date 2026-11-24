//! Intel AC'97 audio driver (v0.16 "Panen": OS audio subsystem).
//!
//! Drives QEMU's `AC97` sound card: enumerates it on the PCI bus, cold-resets
//! the codec, programs the mixer (master + PCM-out volume), and plays 16-bit
//! signed stereo PCM at 48 kHz through the Native Audio Bus-Master DMA engine
//! using a buffer-descriptor list (BDL). Recording (2026-11-19): the PCM-in
//! box (NABM 0x00) fills its own ring of buffers from the codec's record
//! source (the microphone), drained into a queue by `capture_pump`.
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
// PCM-in box (recording), NABM offset 0x00
const PI_BDBAR: u16 = 0x00;
const PI_CIV: u16 = 0x04;
const PI_LVI: u16 = 0x05;
const PI_SR: u16 = 0x06;
const PI_CR: u16 = 0x0B;
// mixer registers for recording
const NAM_MIC_VOL: u16 = 0x0E;
const NAM_REC_SELECT: u16 = 0x1A;
const NAM_REC_GAIN: u16 = 0x1C;
/// Buffers in the recording ring (4 KiB each = 21 ms of 48 kHz stereo).
const CAP_BUFFERS: usize = 16;
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
const CR_IOCE: u8 = 0x10; // interrupt on completion enable
/// BDL control bit: raise an interrupt when this buffer is done.
const BDL_IOC: u32 = 1 << 31;
/// Status bits an interrupt reports (write 1 to clear): last valid buffer,
/// buffer completion, FIFO error.
const SR_IRQ_BITS: u16 = 0x1C;
/// IDT vector of the AC'97 interrupt (an I/O APIC route of its PCI line).
pub const IRQ_VECTOR: u8 = 0x50;

/// DMA buffers of 4 KiB: a one-shot sound uses the first ones; streaming
/// (the mixer) cycles through all of them -- 32 x 1024 frames = 680 ms.
const NUM_BUFFERS: usize = 32;
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
    /// The mixer streams into the ring (LVI moves ahead of the current index).
    streaming: bool,
    lvi: usize,
    /// Buffers in a row filled with silence (a full ring of them stops it).
    idle: usize,
    /// Recording: its own BDL and ring, whether it runs, the next buffer to drain.
    cap_bdl: (u64, u64),
    cap_bufs: [(u64, u64); CAP_BUFFERS],
    capturing: bool,
    cap_next: usize,
}

// Only accessed behind the global Mutex; the addresses are plain integers.
unsafe impl Send for Ac97 {}

static DEVICE: Mutex<Option<Ac97>> = Mutex::new(None);

// ---- interrupt-driven streaming (2026-11-24) ---------------------------------
//
// With an I/O APIC, the card's PCI interrupt is routed to `IRQ_VECTOR`, and the
// buffers of the streaming and recording rings carry the IOC bit. The
// interrupt handler only acknowledges the card and wakes the `audio` kernel
// task, which refills the play ring from the mixer and drains the recording
// ring -- the mixer's locks are never taken in interrupt context. Without a
// route (no I/O APIC, or the line cannot be routed) the desktop loop keeps
// pumping as before.

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering as AOrd};

static NABM: AtomicU16 = AtomicU16::new(0);
static IRQ_GSI: AtomicU64 = AtomicU64::new(u64::MAX);
/// Interrupts taken (tests, `audio` shell output).
pub static IRQS: AtomicU64 = AtomicU64::new(0);
/// Wake-ups of the audio task that refilled the play ring.
pub static IRQ_REFILLS: AtomicU64 = AtomicU64::new(0);
static STREAMING: AtomicBool = AtomicBool::new(false);
static PENDING: AtomicBool = AtomicBool::new(false);
static SPURIOUS: AtomicU64 = AtomicU64::new(0);
const TASK_KEY: u64 = 0xA0D1 << 40;

/// Ring state for a failing test.
pub fn debug_state() -> alloc::string::String {
    match DEVICE.try_lock() {
        Some(g) => match g.as_ref() {
            Some(d) => alloc::format!("picb {} civ {} lvi {} idle {} sr {:#x} cr {:#x} streaming {} irqs {} spurious {}", inw(d.nabm + PO_PICB), inb(d.nabm + PO_CIV), d.lvi, d.idle, inw(d.nabm + PO_SR), inb(d.nabm + PO_CR), d.streaming, IRQS.load(AOrd::Relaxed), SPURIOUS.load(AOrd::Relaxed)),
            None => alloc::string::String::from("no AC97"),
        },
        None => alloc::string::String::from("locked"),
    }
}

/// Whether the card's interrupt is routed (refills happen without polling).
pub fn irq_routed() -> bool {
    IRQ_GSI.load(AOrd::Relaxed) != u64::MAX
}

/// The interrupt: acknowledge both DMA engines, wake the audio task. Runs
/// with interrupts off and takes no lock but the scheduler's (as every IRQ
/// waker does).
pub fn on_irq() {
    let nabm = NABM.load(AOrd::Relaxed);
    if nabm == 0 {
        return;
    }
    let po = inw(nabm + PO_SR);
    let pi = inw(nabm + PI_SR);
    if (po | pi) & SR_IRQ_BITS == 0 {
        // Not ours. A wrongly configured level-triggered line would fire
        // for ever: after a run of these, mask it and go back to polling.
        if SPURIOUS.fetch_add(1, AOrd::Relaxed) > 256 {
            let gsi = IRQ_GSI.swap(u64::MAX, AOrd::Relaxed);
            if gsi != u64::MAX {
                crate::ioapic::mask(gsi as u32);
            }
        }
        return;
    }
    SPURIOUS.store(0, AOrd::Relaxed);
    if po & SR_IRQ_BITS != 0 {
        outw(nabm + PO_SR, po & SR_IRQ_BITS);
    }
    if pi & SR_IRQ_BITS != 0 {
        outw(nabm + PI_SR, pi & SR_IRQ_BITS);
    }
    IRQS.fetch_add(1, AOrd::Relaxed);
    PENDING.store(true, AOrd::SeqCst);
    crate::task::futex_wake(TASK_KEY, 1);
}

/// The audio task: sleeps until an interrupt, then pumps both rings.
fn audio_task() {
    loop {
        crate::task::block_while(TASK_KEY, || !PENDING.load(AOrd::SeqCst));
        PENDING.store(false, AOrd::SeqCst);
        let before = STREAM_FILLS.load(AOrd::Relaxed);
        stream_pump();
        capture_pump();
        if STREAM_FILLS.load(AOrd::Relaxed) != before {
            IRQ_REFILLS.fetch_add(1, AOrd::Relaxed);
        }
    }
}

/// Route the card's PCI interrupt line through the I/O APIC and start the
/// audio task. Returns the GSI.
fn enable_irq(dev: &pci::PciDevice, nabm: u16) -> Option<u32> {
    if !crate::ioapic::active() {
        return None;
    }
    let line = dev.read8(0x3C);
    let pin = dev.read8(0x3D);
    if pin == 0 || line == 0 || line == 0xFF {
        return None;
    }
    NABM.store(nabm, AOrd::Relaxed);
    let gsi = crate::ioapic::route_pci(line, IRQ_VECTOR)?;
    crate::task::spawn_unlocked("audio", audio_task);
    IRQ_GSI.store(gsi as u64, AOrd::Relaxed);
    Some(gsi)
}

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
    let up = init_card();
    if up {
        // a card of our own plays the mixer; a USB output only plays its test tone
        crate::usb::AUDIO_SINK.store(false, AOrd::Relaxed);
    } else if crate::usb::audio_present() {
        crate::usb::AUDIO_SINK.store(true, AOrd::Relaxed);
        crate::println!("[audio] no AC'97 / HD Audio card: USB audio plays the mixer");
    }
    up
}

fn init_card() -> bool {
    let devices = pci::scan();
    // AC'97: class 0x04 (multimedia), subclass 0x01 (audio device). QEMU's model
    // is Intel 82801AA (vendor 0x8086, device 0x2415).
    let dev = devices.into_iter().find(|d| {
        // Not a virtio sound card: it shares class 04/01 but is driven by
        // virtio::snd (found when both were attached).
        (d.class == 0x04 && d.subclass == 0x01 && d.vendor_id != 0x1AF4)
            || (d.vendor_id == 0x8086 && d.device_id == 0x2415)
    });
    let Some(dev) = dev else {
        // most PCs since 2004: Intel High Definition Audio
        return super::hda::init(&pci::scan());
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

    let Some((cap_bdl_phys, cap_bdl_virt)) = memory::alloc_dma_frame() else {
        crate::println!("[audio] failed to allocate the recording BDL");
        return false;
    };
    let mut cap_bufs = [(0u64, 0u64); CAP_BUFFERS];
    for slot in cap_bufs.iter_mut() {
        match memory::alloc_dma_frame() {
            Some((p, v)) => *slot = (p, v as u64),
            None => return false,
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
        streaming: false,
        lvi: 0,
        idle: 0,
        cap_bdl: (cap_bdl_phys, cap_bdl_virt as u64),
        cap_bufs,
        capturing: false,
        cap_next: 0,
    };
    // Apply the default volume.
    outw(nam + NAM_MASTER_VOL, vol_encode(ac.volume, ac.muted));
    *DEVICE.lock() = Some(ac);

    crate::println!(
        "[audio] AC'97 up: NAM={nam:#06x} NABM={nabm:#06x} caps={reset_caps:#06x} mixer_ok={mixer_ok} rate={SAMPLE_RATE}"
    );
    if !irq_routed() {
        match enable_irq(&dev, nabm) {
            Some(gsi) => crate::println!(
                "[audio] interrupt: PCI line {} -> GSI {} -> vector {:#x} (rings refilled by the audio task)",
                dev.read8(0x3C),
                gsi,
                IRQ_VECTOR
            ),
            None => crate::println!("[audio] interrupt: not routed -- the desktop loop pumps the rings"),
        }
    }
    true
}

/// An AC'97 or HD Audio card (not counting USB audio).
pub fn has_card() -> bool {
    DEVICE.lock().is_some() || super::hda::present()
}

/// No card of our own, but a USB audio output: it plays the mixer.
fn usb_out() -> bool {
    crate::usb::audio_is_sink() && !has_card()
}

/// Is a sound card present and initialized?
pub fn is_present() -> bool {
    has_card() || crate::usb::audio_is_sink()
}

/// Let the HD Audio stream stop once its sound is over, and keep the
/// mixer's stream fed (desktop loop, AUDIO_WRITE).
pub fn poll() {
    super::hda::poll();
    if irq_routed() && IRQS.load(AOrd::Relaxed) > 0 && DEVICE.try_lock().is_some_and(|g| g.is_some()) {
        // AC'97 with a working interrupt: the audio task refills the rings.
        // Starting a stream still happens here / in AUDIO_WRITE (an idle
        // card raises no interrupt).
        if !is_streaming() && crate::mixer::has_data() {
            stream_pump();
        }
        return;
    }
    stream_pump();
    capture_pump();
}

/// Streams refilled (tests).
pub static STREAM_FILLS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Whether the mixer stream is playing.
pub fn is_streaming() -> bool {
    if usb_out() {
        return true; // the isochronous stream always runs
    }
    if super::hda::present() {
        return super::hda::is_streaming();
    }
    // An atomic mirror of `Ac97.streaming`: a `try_lock` here answered
    // "not streaming" whenever the audio task was refilling the ring, so a
    // tone could take the hardware from under the stream.
    STREAMING.load(AOrd::SeqCst)
}

/// Whether this card can stream (the mixer's AUDIO_WRITE).
pub fn can_stream() -> bool {
    is_present()
}

fn fill_from_mixer(buf_virt: u64) -> bool {
    let mut tmp = [0i16; WORDS_PER_BUF];
    let any = crate::mixer::mix(&mut tmp);
    let out = buf_virt as *mut i16;
    for (k, v) in tmp.iter().enumerate() {
        unsafe { out.add(k).write_volatile(*v) };
    }
    any
}

/// Feed the ring from the mixer: start it when a stream has data, refill
/// the buffers the DMA engine has played (between LVI and the current
/// index), stop after a whole ring of silence.
pub fn stream_pump() {
    if usb_out() {
        return crate::usb::audio_pump();
    }
    if super::hda::present() {
        return super::hda::stream_pump();
    }
    let Some(mut g) = DEVICE.try_lock() else { return };
    let Some(d) = g.as_mut() else { return };
    use core::sync::atomic::Ordering::Relaxed;
    if !d.streaming {
        if !crate::mixer::has_data() {
            return;
        }
        outb(d.nabm + PO_CR, 0);
        outb(d.nabm + PO_CR, CR_RR);
        for _ in 0..100_000 {
            if inb(d.nabm + PO_CR) & CR_RR == 0 {
                break;
            }
        }
        for i in 0..NUM_BUFFERS {
            fill_from_mixer(d.bufs[i].1);
        }
        let bdl = d.bdl_virt as *mut u32;
        for i in 0..NUM_BUFFERS {
            unsafe {
                bdl.add(i * 2).write_volatile(d.bufs[i].0 as u32);
                bdl.add(i * 2 + 1).write_volatile(WORDS_PER_BUF as u32 | if irq_routed() { BDL_IOC } else { 0 });
            }
        }
        outl(d.nabm + PO_BDBAR, d.bdl_phys as u32);
        d.lvi = NUM_BUFFERS - 1;
        outb(d.nabm + PO_LVI, d.lvi as u8);
        outw(d.nabm + PO_SR, inw(d.nabm + PO_SR));
        outb(d.nabm + PO_CR, CR_RPBM | if irq_routed() { CR_IOCE } else { 0 });
        d.streaming = true;
        STREAMING.store(true, AOrd::SeqCst);
        d.idle = 0;
        STREAM_FILLS.fetch_add(NUM_BUFFERS as u64, Relaxed);
        return;
    }
    if inw(d.nabm + PO_SR) & SR_DCH != 0 {
        // ran past the last valid buffer (nobody pumped in time): start over
        outb(d.nabm + PO_CR, 0);
        d.streaming = false;
        STREAMING.store(false, AOrd::SeqCst);
        return;
    }
    let civ = inb(d.nabm + PO_CIV) as usize % NUM_BUFFERS;
    loop {
        let next = (d.lvi + 1) % NUM_BUFFERS;
        if next == civ {
            break;
        }
        if fill_from_mixer(d.bufs[next].1) {
            d.idle = 0;
        } else {
            d.idle += 1;
        }
        d.lvi = next;
        outb(d.nabm + PO_LVI, next as u8);
        STREAM_FILLS.fetch_add(1, Relaxed);
    }
    if d.idle >= NUM_BUFFERS {
        outb(d.nabm + PO_CR, 0);
        d.streaming = false;
        STREAMING.store(false, AOrd::SeqCst);
    }
}

// ---- recording ------------------------------------------------------------------

/// Recorded samples (48 kHz stereo i16) not read yet; at most `CAP_QUEUE`.
static CAPTURED: Mutex<alloc::collections::VecDeque<i16>> = Mutex::new(alloc::collections::VecDeque::new());
/// Four seconds: a reader that falls further behind loses the oldest.
const CAP_QUEUE: usize = 48_000 * 2 * 4;
/// Buffers drained (tests).
pub static CAPTURE_FILLS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Whether this card records (AC'97's PCM-in, or an HD Audio input path).
pub fn can_capture() -> bool {
    DEVICE.lock().is_some() || super::hda::can_capture()
}

/// Start recording from the codec's record source (the microphone, 0 dB).
pub fn capture_start() -> bool {
    if DEVICE.lock().is_none() {
        CAPTURED.lock().clear();
        return super::hda::capture_start();
    }
    let mut g = DEVICE.lock();
    let Some(d) = g.as_mut() else { return false };
    CAPTURED.lock().clear();
    outw(d.nam + NAM_REC_SELECT, 0x0000); // microphone, both channels
    outw(d.nam + NAM_REC_GAIN, 0x0000); // 0 dB, unmuted
    outw(d.nam + NAM_MIC_VOL, 0x0008); // 0 dB, unmuted
    outb(d.nabm + PI_CR, 0);
    outb(d.nabm + PI_CR, CR_RR);
    for _ in 0..100_000 {
        if inb(d.nabm + PI_CR) & CR_RR == 0 {
            break;
        }
    }
    let bdl = d.cap_bdl.1 as *mut u32;
    for i in 0..CAP_BUFFERS {
        unsafe {
            bdl.add(i * 2).write_volatile(d.cap_bufs[i].0 as u32);
            bdl.add(i * 2 + 1).write_volatile(WORDS_PER_BUF as u32 | if irq_routed() { BDL_IOC } else { 0 });
        }
    }
    outl(d.nabm + PI_BDBAR, d.cap_bdl.0 as u32);
    outb(d.nabm + PI_LVI, (CAP_BUFFERS - 1) as u8);
    outw(d.nabm + PI_SR, inw(d.nabm + PI_SR));
    outb(d.nabm + PI_CR, CR_RPBM | if irq_routed() { CR_IOCE } else { 0 });
    d.capturing = true;
    d.cap_next = 0;
    true
}

pub fn capture_stop() {
    if DEVICE.lock().is_none() {
        return super::hda::capture_stop();
    }
    if let Some(d) = DEVICE.lock().as_mut() {
        outb(d.nabm + PI_CR, 0);
        d.capturing = false;
    }
}

/// Move the buffers the engine has filled (behind its current index) into
/// the queue and hand them back to it (LVI follows).
pub fn capture_pump() {
    let Some(mut g) = DEVICE.try_lock() else { return };
    let Some(d) = g.as_mut() else {
        drop(g);
        let mut q = CAPTURED.lock();
        let pages = super::hda::capture_pump(&mut q);
        CAPTURE_FILLS.fetch_add(pages, core::sync::atomic::Ordering::Relaxed);
        while q.len() > CAP_QUEUE {
            q.pop_front();
        }
        return;
    };
    if !d.capturing {
        return;
    }
    if inw(d.nabm + PI_SR) & SR_DCH != 0 {
        // nobody drained in time and the engine stopped: go on from here
        outw(d.nabm + PI_SR, inw(d.nabm + PI_SR));
        outb(d.nabm + PI_LVI, ((d.cap_next + CAP_BUFFERS - 1) % CAP_BUFFERS) as u8);
        outb(d.nabm + PI_CR, CR_RPBM | if irq_routed() { CR_IOCE } else { 0 });
    }
    let civ = inb(d.nabm + PI_CIV) as usize % CAP_BUFFERS;
    let mut q = CAPTURED.lock();
    while d.cap_next != civ {
        let src = d.cap_bufs[d.cap_next].1 as *const i16;
        for k in 0..WORDS_PER_BUF {
            q.push_back(unsafe { src.add(k).read_volatile() });
        }
        outb(d.nabm + PI_LVI, d.cap_next as u8);
        d.cap_next = (d.cap_next + 1) % CAP_BUFFERS;
        CAPTURE_FILLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }
    while q.len() > CAP_QUEUE {
        q.pop_front();
    }
}

/// Up to `out.len()` recorded samples (interleaved stereo); how many.
pub fn capture_read(out: &mut [i16]) -> usize {
    capture_pump();
    let mut q = CAPTURED.lock();
    let n = out.len().min(q.len()) & !1;
    for v in out.iter_mut().take(n) {
        *v = q.pop_front().unwrap_or(0);
    }
    n
}

/// Recorded milliseconds waiting to be read.
pub fn capture_queued_ms() -> u64 {
    (CAPTURED.lock().len() / 2) as u64 * 1000 / SAMPLE_RATE as u64
}

/// Was the mixer write/readback check successful (proves the NAM path works)?
pub fn mixer_ok() -> bool {
    if usb_out() || super::hda::present() {
        return true;
    }
    DEVICE.lock().as_ref().map(|d| d.mixer_ok).unwrap_or(false)
}

/// Current master volume percentage.
pub fn volume() -> u32 {
    if usb_out() {
        return crate::usb::AUDIO_VOLUME.load(AOrd::Relaxed);
    }
    if super::hda::present() {
        return super::hda::volume();
    }
    DEVICE.lock().as_ref().map(|d| d.volume).unwrap_or(0)
}

/// Is output currently muted?
pub fn is_muted() -> bool {
    if usb_out() {
        return crate::usb::AUDIO_MUTED.load(AOrd::Relaxed);
    }
    if super::hda::present() {
        return super::hda::is_muted();
    }
    DEVICE.lock().as_ref().map(|d| d.muted).unwrap_or(false)
}

/// NAM reset-register capabilities readback (nonzero on a real/QEMU codec).
pub fn caps() -> u16 {
    DEVICE.lock().as_ref().map(|d| d.reset_caps).unwrap_or(0)
}

/// Set the master output volume (0..=100). Non-zero un-mutes.
pub fn set_volume(pct: u32) -> bool {
    if usb_out() {
        crate::usb::AUDIO_VOLUME.store(pct.min(100), AOrd::Relaxed);
        if pct > 0 {
            crate::usb::AUDIO_MUTED.store(false, AOrd::Relaxed);
        }
        return true;
    }
    if super::hda::present() {
        return super::hda::set_volume(pct);
    }
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
    if usb_out() {
        let m = !crate::usb::AUDIO_MUTED.load(AOrd::Relaxed);
        crate::usb::AUDIO_MUTED.store(m, AOrd::Relaxed);
        return m;
    }
    if super::hda::present() {
        return super::hda::toggle_mute();
    }
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
    if is_streaming() {
        // the ring belongs to the mixer: the tone becomes the system stream
        let frames = (SAMPLE_RATE as u64 * ms.min(10_000) as u64 / 1000) as usize;
        let mut pcm = alloc::vec![0i16; frames * 2];
        fill_tone(pcm.as_mut_ptr() as u64, frames, freq, 0);
        let bytes: alloc::vec::Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        crate::mixer::write(0, &bytes, 0);
        return true;
    }
    if super::hda::present() {
        return super::hda::play_tone(freq, ms, fill_tone);
    }
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
    if is_streaming() {
        let n = (len_bytes as usize).min(4 << 20) & !3;
        let p = src_ptr as *const u8;
        let bytes: alloc::vec::Vec<u8> = (0..n).map(|i| unsafe { p.add(i).read_volatile() }).collect();
        crate::mixer::write(0, &bytes, 0);
        return true;
    }
    if super::hda::present() {
        return super::hda::play_pcm(src_ptr, len_bytes);
    }
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
    if usb_out() {
        return crate::mixer::has_data();
    }
    if super::hda::present() {
        return super::hda::is_playing();
    }
    let guard = DEVICE.lock();
    let Some(d) = guard.as_ref() else { return false };
    inw(d.nabm + PO_SR) & SR_DCH == 0
}
