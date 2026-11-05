//! Intel graphics, step 2 of GPU support: the blitter engine (BCS) of a
//! Gen9 GPU (Skylake .. Comet Lake), driven through a legacy ring buffer
//! (no execlists; Gen9 hardware still accepts that mode).
//!
//! Step 2a is a self-test only (shell `gpu blit`): nothing here runs at boot,
//! the engine is woken the first time the test runs. It reports every step,
//! because none of this can run under QEMU:
//! 1. forcewake (render, media, blitter domains) and the ring registers;
//! 2. an empty submission -- does the command streamer run at all?
//! 3. a timed fill, GPU against CPU;
//! 4. XY_COLOR_BLT squares into the firmware's framebuffer (GGTT offset of
//!    display plane 1A, linear XRGB8888), read back by the CPU;
//! 5. XY_SRC_COPY_BLT of a CPU-drawn pattern living in system memory pages
//!    mapped into the GGTT -- what presenting the desktop would need.
//! The squares stay on screen for 4 s, then the desktop is redrawn.
//!
//! Memory: our pages are mapped into the GGTT at `BASE` (256 MiB, beyond the
//! framebuffer and the CPU aperture); the CPU flushes its caches (`clflush`)
//! over everything the GPU reads and before reading what the GPU wrote.

use alloc::{format, string::String, vec::Vec};
use core::ptr::{read_volatile, write_volatile};
use spin::Mutex;

const BCS: u64 = 0x22000;
const RING_TAIL: u64 = 0x30;
const RING_HEAD: u64 = 0x34;
const RING_START: u64 = 0x38;
const RING_CTL: u64 = 0x3C;
const RING_IPEHR: u64 = 0x68;
const RING_INSTDONE: u64 = 0x6C;
const RING_ACTHD: u64 = 0x74;
const RING_MI_MODE: u64 = 0x9C;
const RING_GFX_MODE: u64 = 0x29C;
const GFX_FLSH_CNTL: u64 = 0x101008;
/// (forcewake request, ack, domain) on Gen9.
const FORCEWAKE: [(u64, u64, &str); 3] = [(0xA278, 0x0D84, "render"), (0xA270, 0x0D88, "media"), (0xA188, 0x130044, "blitter")];

/// Our part of the GGTT.
const BASE: u32 = 0x1000_0000;
const RING_GGTT: u32 = BASE;
const STATUS_GGTT: u32 = BASE + 0x1000;
const PATTERN_GGTT: u32 = BASE + 0x10000;
const RING_SIZE: u32 = 4096;
/// Pattern: 64 x 64 pixels, pitch 256 bytes = 16 rows per page.
const PAT: u32 = 64;

const MI_NOOP: u32 = 0;
const XY_COLOR_BLT: u32 = 2 << 29 | 0x50 << 22 | 3 << 20 | 5;
const XY_SRC_COPY_BLT: u32 = 2 << 29 | 0x53 << 22 | 3 << 20 | 8;
const BPP32: u32 = 3 << 24;

struct Blitter {
    regs: u64,
    gtt: u64,
    fb_ggtt: u32,
    pitch: u32,
    w: u32,
    h: u32,
    up: bool,
    ring: (u64, *mut u8),
    status: (u64, *mut u8),
    pattern: Vec<(u64, *mut u8)>,
    tail: u32,
    seqno: u32,
}

// MMIO and DMA pages owned by this driver; used under BLT's lock only.
unsafe impl Send for Blitter {}

static BLT: Mutex<Option<Blitter>> = Mutex::new(None);

/// Called by `intel_gpu` when a Gen9 GPU scans out a linear XRGB8888 plane.
pub fn configure(regs: u64, gtt: u64, fb_ggtt: u32, pitch: u32, w: u32, h: u32) {
    *BLT.lock() = Some(Blitter {
        regs,
        gtt,
        fb_ggtt,
        pitch,
        w,
        h,
        up: false,
        ring: (0, core::ptr::null_mut()),
        status: (0, core::ptr::null_mut()),
        pattern: Vec::new(),
        tail: 0,
        seqno: 0,
    });
}

fn clflush(va: *const u8, len: usize) {
    let mut p = va as usize & !63;
    let end = va as usize + len;
    while p < end {
        unsafe { core::arch::asm!("clflush [{}]", in(reg) p, options(nostack, preserves_flags)) };
        p += 64;
    }
    unsafe { core::arch::asm!("mfence", options(nostack, preserves_flags)) };
}

fn now() -> u64 {
    crate::arch::cpu::timestamp()
}

fn us_since(t0: u64) -> u64 {
    now().wrapping_sub(t0) * 1_000_000 / crate::arch::cpu::counter_hz().max(1)
}

impl Blitter {
    fn rd(&self, o: u64) -> u32 {
        unsafe { read_volatile((self.regs + o) as *const u32) }
    }
    fn wr(&self, o: u64, v: u32) {
        unsafe { write_volatile((self.regs + o) as *mut u32, v) }
    }

    fn map(&self, ggtt: u32, phys: u64) {
        unsafe { write_volatile((self.gtt + (ggtt as u64 >> 12) * 8) as *mut u64, phys | 1) };
    }
    fn flush_ggtt(&self) {
        self.wr(GFX_FLSH_CNTL, 1);
        let _ = self.rd(GFX_FLSH_CNTL);
    }

    /// Wake the GT, map our pages, start the ring. Lines describe each step.
    fn bring_up(&mut self, out: &mut Vec<String>) -> bool {
        let mut acks = String::new();
        for (req, ack, name) in FORCEWAKE {
            self.wr(req, 0x0001_0001);
            let t0 = now();
            while self.rd(ack) & 1 == 0 && us_since(t0) < 50_000 {}
            acks += &format!(" {} {}", name, if self.rd(ack) & 1 != 0 { "ok" } else { "NO ACK" });
        }
        out.push(format!("  forcewake:{}", acks));

        let page = || crate::memory::alloc_dma_frame();
        let (Some(ring), Some(status)) = (page(), page()) else {
            out.push(String::from("  out of memory"));
            return false;
        };
        unsafe {
            core::ptr::write_bytes(ring.1, 0, 4096);
            core::ptr::write_bytes(status.1, 0, 4096);
        }
        clflush(ring.1, 4096);
        clflush(status.1, 4096);
        let old = unsafe { read_volatile((self.gtt + (BASE as u64 >> 12) * 8) as *const u64) };
        self.map(RING_GGTT, ring.0);
        self.map(STATUS_GGTT, status.0);
        for i in 0..4 {
            let Some(p) = page() else { return false };
            self.map(PATTERN_GGTT + i * 4096, p.0);
            self.pattern.push(p);
        }
        self.flush_ggtt();
        self.ring = ring;
        self.status = status;
        out.push(format!("  GGTT {:#x}: entry was {:#x}, now our pages (ring {:#x})", BASE, old, ring.0));

        let e = |r: u64| BCS + r;
        self.wr(e(RING_CTL), 0);
        self.wr(e(RING_HEAD), 0);
        self.wr(e(RING_TAIL), 0);
        self.wr(e(RING_GFX_MODE), (1 << 15) << 16); // execlists off (masked write)
        self.wr(e(RING_MI_MODE), (1 << 8) << 16); // STOP_RING off (masked write)
        self.wr(e(RING_START), RING_GGTT);
        self.wr(e(RING_CTL), (RING_SIZE - 4096) | 1);
        let t0 = now();
        while self.rd(e(RING_CTL)) & 1 == 0 && us_since(t0) < 10_000 {}
        out.push(format!(
            "  BCS ring: CTL {:#x} START {:#x} HEAD {:#x} TAIL {:#x} GFX_MODE {:#x} MI_MODE {:#x}",
            self.rd(e(RING_CTL)),
            self.rd(e(RING_START)),
            self.rd(e(RING_HEAD)),
            self.rd(e(RING_TAIL)),
            self.rd(e(RING_GFX_MODE)),
            self.rd(e(RING_MI_MODE))
        ));
        self.tail = 0;
        self.up = self.rd(e(RING_START)) == RING_GGTT && self.rd(e(RING_CTL)) & 1 != 0;
        self.up
    }

    fn emit(&mut self, dw: u32) {
        unsafe { write_volatile(self.ring.1.add(self.tail as usize) as *mut u32, dw) };
        self.tail = (self.tail + 4) % RING_SIZE;
    }

    /// Queue `cmds` + a flush that writes the next seqno, and wait for it.
    /// Ok(microseconds) or Err(register dump).
    fn submit(&mut self, cmds: &[u32]) -> Result<u64, String> {
        let need = (cmds.len() as u32 + 6) * 4;
        if self.tail + need > RING_SIZE {
            let start = self.tail;
            while self.tail != 0 {
                self.emit(MI_NOOP);
            }
            clflush(unsafe { self.ring.1.add(start as usize) }, (RING_SIZE - start) as usize);
        }
        let start = self.tail;
        for &c in cmds {
            self.emit(c);
        }
        self.seqno = self.seqno.wrapping_add(1);
        // MI_FLUSH_DW with a post-sync dword write into the status page
        self.emit(0x26 << 23 | 1 << 14 | 2);
        self.emit(STATUS_GGTT | 1 << 2);
        self.emit(0);
        self.emit(self.seqno);
        if self.tail % 8 != 0 {
            self.emit(MI_NOOP);
        }
        let end = if self.tail == 0 { RING_SIZE } else { self.tail };
        clflush(unsafe { self.ring.1.add(start as usize) }, (end - start) as usize);
        let t0 = now();
        self.wr(BCS + RING_TAIL, self.tail);
        loop {
            clflush(self.status.1, 64);
            if unsafe { read_volatile(self.status.1 as *const u32) } == self.seqno {
                return Ok(us_since(t0));
            }
            if us_since(t0) > 200_000 {
                return Err(format!(
                    "GPU did not finish: HEAD {:#x} TAIL {:#x} ACTHD {:#x} CTL {:#x} IPEHR {:#x} INSTDONE {:#x}",
                    self.rd(BCS + RING_HEAD),
                    self.rd(BCS + RING_TAIL),
                    self.rd(BCS + RING_ACTHD),
                    self.rd(BCS + RING_CTL),
                    self.rd(BCS + RING_IPEHR),
                    self.rd(BCS + RING_INSTDONE)
                ));
            }
        }
    }

    fn color_blt(&self, x: u32, y: u32, w: u32, h: u32, color: u32) -> [u32; 7] {
        [XY_COLOR_BLT, BPP32 | 0xF0 << 16 | self.pitch, y << 16 | x, (y + h) << 16 | (x + w), self.fb_ggtt, 0, color]
    }

    fn copy_blt(&self, x: u32, y: u32) -> [u32; 10] {
        [
            XY_SRC_COPY_BLT,
            BPP32 | 0xCC << 16 | self.pitch,
            y << 16 | x,
            (y + PAT) << 16 | (x + PAT),
            self.fb_ggtt,
            0,
            0,
            PAT * 4,
            PATTERN_GGTT,
            0,
        ]
    }
}

/// Shell `gpu blit`.
pub fn self_test() -> Vec<String> {
    let mut guard = BLT.lock();
    let Some(b) = guard.as_mut() else {
        return alloc::vec![String::from("gpu blit: needs a Gen9 Intel GPU showing a linear 32-bit plane (see 'gpu')")];
    };
    let mut out = alloc::vec![format!("blitter self-test: framebuffer at GGTT {:#x}, {}x{}, pitch {}", b.fb_ggtt, b.w, b.h, b.pitch)];
    if !b.up && !b.bring_up(&mut out) {
        out.push(String::from("  blitter did not come up; nothing drawn"));
        return out;
    }
    let Some(fb) = crate::framebuffer::virtual_base() else { return out };
    let pitch = b.pitch;
    let px = |x: u32, y: u32| unsafe { read_volatile((fb + (y * pitch + x * 4) as u64) as *const u32) } & 0xFF_FFFF;

    // 1. does the command streamer run?
    match b.submit(&[MI_NOOP, MI_NOOP]) {
        Ok(us) => out.push(format!("  1 empty submission: done in {} us (command streamer runs)", us)),
        Err(e) => {
            out.push(format!("  1 empty submission: {}", e));
            return out;
        }
    }

    // 2. timed fill, GPU vs CPU, same area (the squares come after it)
    let (tw, th) = (b.w.min(1024), b.h.min(512));
    let fill = b.color_blt(0, 0, tw, th, 0xFF20_4030);
    match b.submit(&fill) {
        Ok(us) => {
            let gpu_ok = px(tw / 2, th / 2) == 0x20_4030;
            let t0 = now();
            for y in 0..th {
                for x in 0..tw {
                    unsafe { write_volatile((fb + (y * pitch + x * 4) as u64) as *mut u32, 0x0030_2040) };
                }
            }
            let cpu = us_since(t0).max(1);
            out.push(format!(
                "  2 fill {}x{}: GPU {} us (read back {}), CPU {} us",
                tw,
                th,
                us,
                if gpu_ok { "ok" } else { "WRONG" },
                cpu
            ));
        }
        Err(e) => {
            out.push(format!("  2 fill: {}", e));
            return out;
        }
    }

    // 3. four colored squares, top right, read back
    let colors = [0x00E0_4040u32, 0x0040_C060, 0x0040_70E0, 0x00F0_C030];
    let x0 = b.w.saturating_sub(4 * 90 + 20);
    let mut cmds = Vec::new();
    for (i, c) in colors.iter().enumerate() {
        cmds.extend_from_slice(&b.color_blt(x0 + i as u32 * 90, 40, 80, 80, 0xFF00_0000 | c));
    }
    match b.submit(&cmds) {
        Ok(us) => {
            let ok = colors.iter().enumerate().filter(|(i, c)| px(x0 + *i as u32 * 90 + 40, 80) == **c).count();
            out.push(format!("  3 color blit, 4 squares: {} us, read back {}/4 correct", us, ok));
        }
        Err(e) => {
            out.push(format!("  3 color blit: {}", e));
            return out;
        }
    }

    // 4. a CPU-drawn gradient in system memory, copied four times
    for y in 0..PAT {
        let (_, va) = b.pattern[(y / 16) as usize];
        for x in 0..PAT {
            let v = (x * 4) << 16 | (y * 4) << 8 | 0x80;
            unsafe { write_volatile(va.add(((y % 16) * PAT * 4 + x * 4) as usize) as *mut u32, v) };
        }
    }
    for &(_, va) in &b.pattern {
        clflush(va, 4096);
    }
    let mut cmds = Vec::new();
    for i in 0..4 {
        cmds.extend_from_slice(&b.copy_blt(x0 + i * 90 + 8, 140));
    }
    match b.submit(&cmds) {
        Ok(us) => {
            let want = |x: u32, y: u32| (x * 4) << 16 | (y * 4) << 8 | 0x80;
            let probes = [(0u32, 0u32), (63, 0), (0, 63), (40, 20)];
            let ok = probes.iter().filter(|(x, y)| px(x0 + 8 + x, 140 + y) == want(*x, *y)).count();
            out.push(format!("  4 copy from system memory, 4 x 64x64: {} us, read back {}/4 correct", us, ok));
        }
        Err(e) => out.push(format!("  4 copy: {}", e)),
    }
    out.push(String::from("  (squares + gradients were on screen for 4 s, top right)"));
    drop(guard);
    let t0 = now();
    while us_since(t0) < 4_000_000 {
        core::hint::spin_loop();
    }
    crate::wm::screen_overwritten();
    crate::wm::present_now();
    out
}
