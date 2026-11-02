//! An xHCI (USB 3) host controller, polled: no interrupts, events are read
//! from the event ring by `poll()` (the desktop loop) and while waiting for a
//! command or a control transfer.
//!
//! What it does: reset + start the controller (BIOS hand-off, scratchpad
//! buffers, one command ring, one event ring), enumerate every device on the
//! root ports and behind USB 2.0 hubs (route strings; LS/FS devices behind a
//! high-speed hub get the hub's Transaction Translator in their slot), and
//! drive HID boot-protocol keyboards and mice through one interrupt-IN
//! endpoint each (`super::hid`).
//!
//! DMA: every structure the controller reads or writes is a 4 KiB frame from
//! `memory::alloc_dma_frame`, addressed by its physical address (the bus
//! address on a PC, and on a Raspberry Pi 4 below 3 GiB, the PCIe inbound
//! window -- `dma_limit`). Writes the controller must see and reads of what
//! it wrote go through `arch::cpu::dma_sync`: the Pi's PCIe does not snoop
//! the CPU caches.

use alloc::vec::Vec;

use crate::arch::cpu;

const TRB_NORMAL: u32 = 1;
const TRB_SETUP: u32 = 2;
const TRB_DATA: u32 = 3;
const TRB_STATUS: u32 = 4;
const TRB_LINK: u32 = 6;
const TRB_ENABLE_SLOT: u32 = 9;
const TRB_ADDRESS_DEVICE: u32 = 11;
const TRB_CONFIGURE_EP: u32 = 12;
const TRB_EVALUATE_CTX: u32 = 13;
const EV_TRANSFER: u32 = 32;
const EV_COMMAND: u32 = 33;

const IOC: u32 = 1 << 5;
const IDT: u32 = 1 << 6;
const ISP: u32 = 1 << 2;

const CC_SUCCESS: u32 = 1;
const CC_SHORT: u32 = 13;

// PORTSC
const PORT_CCS: u32 = 1 << 0;
const PORT_PED: u32 = 1 << 1;
const PORT_PR: u32 = 1 << 4;
const PORT_PP: u32 = 1 << 9;
const PORT_PRC: u32 = 1 << 21;
const PORT_CHANGES: u32 = 0x7F << 17;
/// Bits written back unchanged (power, indicators, wake enables); the RW1C
/// change bits and PED (writing 1 disables the port) are left out.
const PORT_PRESERVE: u32 = PORT_PP | (3 << 14) | (7 << 25);

const RING_TRBS: usize = 256;

fn rd(a: u64) -> u32 {
    unsafe { (a as *const u32).read_volatile() }
}
fn wr(a: u64, v: u32) {
    unsafe { (a as *mut u32).write_volatile(v) }
}
fn wr64(a: u64, v: u64) {
    wr(a, v as u32);
    wr(a + 4, (v >> 32) as u32);
}

fn now() -> u64 {
    cpu::timestamp()
}
fn ms_ticks(ms: u64) -> u64 {
    ms * cpu::counter_hz().max(1) / 1000
}
fn delay_ms(ms: u64) {
    let t0 = now();
    while now() - t0 < ms_ticks(ms) {
        core::hint::spin_loop();
    }
}
fn wait(ms: u64, mut cond: impl FnMut() -> bool) -> bool {
    let t0 = now();
    while !cond() {
        if now() - t0 > ms_ticks(ms) {
            return false;
        }
        core::hint::spin_loop();
    }
    true
}

/// One zeroed, DMA-reachable 4 KiB frame.
struct Page {
    phys: u64,
    va: *mut u8,
}

unsafe impl Send for Page {}

impl Page {
    fn new(limit: u64) -> Result<Page, &'static str> {
        // A frame the controller can reach: set aside any above `limit`
        // (Raspberry Pi 4: 3 GiB) and give them back afterwards.
        let mut rejected = Vec::new();
        let mut found = None;
        for _ in 0..64 {
            let (phys, va) = crate::memory::alloc_dma_frame().ok_or("xhci: out of memory")?;
            if phys + 4096 <= limit {
                found = Some((phys, va));
                break;
            }
            rejected.push(phys);
        }
        for phys in rejected {
            crate::memory::free_frame(phys);
        }
        let (phys, va) = found.ok_or("xhci: no DMA frame inside the bus window")?;
        cpu::dma_sync(va as u64, 4096);
        Ok(Page { phys, va })
    }
    fn w32(&self, off: usize, v: u32) {
        unsafe { (self.va.add(off) as *mut u32).write_volatile(v) }
    }
    fn r32(&self, off: usize) -> u32 {
        unsafe { (self.va.add(off) as *const u32).read_volatile() }
    }
    fn w64(&self, off: usize, v: u64) {
        self.w32(off, v as u32);
        self.w32(off + 4, (v >> 32) as u32);
    }
    fn byte(&self, off: usize) -> u8 {
        unsafe { self.va.add(off).read_volatile() }
    }
    fn zero(&self, off: usize, len: usize) {
        unsafe { core::ptr::write_bytes(self.va.add(off), 0, len) }
    }
    fn sync(&self, off: usize, len: usize) {
        cpu::dma_sync(self.va as u64 + off as u64, len);
    }
}

/// A producer ring (command or transfer): 255 TRBs + a link TRB back to the
/// start that toggles the cycle bit.
struct Ring {
    page: Page,
    idx: usize,
    cycle: u32,
}

impl Ring {
    fn new(limit: u64) -> Result<Ring, &'static str> {
        let page = Page::new(limit)?;
        let l = (RING_TRBS - 1) * 16;
        page.w64(l, page.phys);
        page.w32(l + 12, (TRB_LINK << 10) | (1 << 1));
        page.sync(0, 4096);
        Ok(Ring { page, idx: 0, cycle: 1 })
    }

    /// Enqueue one TRB; returns its bus address (events point at it).
    fn push(&mut self, param: u64, status: u32, control: u32) -> u64 {
        let off = self.idx * 16;
        self.page.w64(off, param);
        self.page.w32(off + 8, status);
        // the cycle bit last: it hands the TRB to the controller
        self.page.w32(off + 12, (control & !1) | self.cycle);
        self.page.sync(off, 16);
        let at = self.page.phys + off as u64;
        self.idx += 1;
        if self.idx == RING_TRBS - 1 {
            let l = (RING_TRBS - 1) * 16;
            self.page.w32(l + 12, (self.page.r32(l + 12) & !1) | self.cycle);
            self.page.sync(l, 16);
            self.idx = 0;
            self.cycle ^= 1;
        }
        at
    }
}

struct EventRing {
    page: Page,
    _erst: Page,
    idx: usize,
    cycle: u32,
}

impl EventRing {
    fn next(&mut self) -> Option<[u32; 4]> {
        let off = self.idx * 16;
        self.page.sync(off, 16);
        let c = self.page.r32(off + 12);
        if c & 1 != self.cycle {
            return None;
        }
        let ev = [self.page.r32(off), self.page.r32(off + 4), self.page.r32(off + 8), c];
        self.idx += 1;
        if self.idx == RING_TRBS {
            self.idx = 0;
            self.cycle ^= 1;
        }
        Some(ev)
    }
    fn dequeue(&self) -> u64 {
        self.page.phys + (self.idx * 16) as u64
    }
}

fn trb_type(ev: &[u32; 4]) -> u32 {
    (ev[3] >> 10) & 0x3F
}
fn trb_ptr(ev: &[u32; 4]) -> u64 {
    ev[0] as u64 | (ev[1] as u64) << 32
}
fn completion(ev: &[u32; 4]) -> u32 {
    ev[2] >> 24
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HidKind {
    Keyboard,
    Mouse,
}

struct Interrupt {
    dci: u32,
    ring: Ring,
    buf: Page,
    len: u32,
    kind: HidKind,
    last: [u8; 8],
    broken: bool,
    reports: u32,
    state: u32,
}

struct Device {
    slot: u32,
    speed: u32,
    route: u32,
    root_port: u32,
    depth: u32,
    tt_slot: u32,
    tt_port: u32,
    mtt: bool,
    out: Page,
    inp: Page,
    ep0: Ring,
    intr: Option<Interrupt>,
}

pub struct Xhci {
    pub name: &'static str,
    op: u64,
    rt: u64,
    db: u64,
    csz: usize,
    max_ports: u32,
    limit: u64,
    dcbaa: Page,
    _scratch: Vec<Page>,
    cmd: Ring,
    ev: EventRing,
    buf: Page,
    devices: Vec<Device>,
    pub keyboards: usize,
    pub mice: usize,
    pub hubs: usize,
}

pub fn speed_name(s: u32) -> &'static str {
    match s {
        1 => "full-speed",
        2 => "low-speed",
        3 => "high-speed",
        4 => "SuperSpeed",
        5 => "SuperSpeed+",
        _ => "unknown-speed",
    }
}

impl Xhci {
    /// Reset and start the controller whose registers are mapped at `base`.
    pub fn new(base: u64, dma_limit: u64, name: &'static str) -> Result<Xhci, &'static str> {
        let caplen = (rd(base) & 0xFF) as u64;
        if caplen == 0 || rd(base) == 0xFFFF_FFFF {
            return Err("xhci: no controller at that address");
        }
        let hcs1 = rd(base + 4);
        let hcs2 = rd(base + 8);
        let hcc1 = rd(base + 0x10);
        let op = base + caplen;
        let rt = base + (rd(base + 0x18) & !0x1F) as u64;
        let db = base + (rd(base + 0x14) & !3) as u64;
        let csz = if hcc1 & (1 << 2) != 0 { 64 } else { 32 };
        let max_ports = hcs1 >> 24;
        let max_slots = (hcs1 & 0xFF).min(64);

        // Take the controller from the firmware (USB Legacy Support capability).
        let mut x = ((hcc1 >> 16) & 0xFFFF) as u64 * 4;
        while x != 0 {
            let v = rd(base + x);
            if v & 0xFF == 1 {
                wr(base + x, v | (1 << 24));
                wait(1000, || rd(base + x) & (1 << 16) == 0);
            }
            let next = ((v >> 8) & 0xFF) as u64;
            if next == 0 {
                break;
            }
            x += next * 4;
        }

        // Stop, then reset.
        wr(op, rd(op) & !1);
        wait(100, || rd(op + 4) & 1 != 0);
        wr(op, 1 << 1);
        if !wait(1000, || rd(op) & (1 << 1) == 0 && rd(op + 4) & (1 << 11) == 0) {
            return Err("xhci: controller reset timed out");
        }
        wr(op + 0x38, max_slots);

        let dcbaa = Page::new(dma_limit)?;
        let mut scratch = Vec::new();
        let n_scratch = (((hcs2 >> 21) & 0x1F) << 5 | ((hcs2 >> 27) & 0x1F)) as usize;
        if n_scratch > 0 {
            let array = Page::new(dma_limit)?;
            for i in 0..n_scratch.min(512) {
                let p = Page::new(dma_limit)?;
                array.w64(i * 8, p.phys);
                scratch.push(p);
            }
            array.sync(0, 4096);
            dcbaa.w64(0, array.phys);
            scratch.push(array);
        }
        dcbaa.sync(0, 4096);
        wr64(op + 0x30, dcbaa.phys);

        let cmd = Ring::new(dma_limit)?;
        wr64(op + 0x18, cmd.page.phys | 1);

        let evp = Page::new(dma_limit)?;
        let erst = Page::new(dma_limit)?;
        erst.w64(0, evp.phys);
        erst.w32(8, RING_TRBS as u32);
        erst.sync(0, 64);
        let ir = rt + 0x20;
        wr(ir + 0x08, 1); // ERSTSZ
        wr64(ir + 0x18, evp.phys); // ERDP
        wr64(ir + 0x10, erst.phys); // ERSTBA (last: it starts the event ring)
        wr(ir, rd(ir) & !(1 << 1)); // IMAN.IE off: polled

        wr(op, 1); // Run
        if !wait(100, || rd(op + 4) & 1 == 0) {
            return Err("xhci: controller did not start");
        }
        let buf = Page::new(dma_limit)?;
        Ok(Xhci {
            name,
            op,
            rt,
            db,
            csz,
            max_ports,
            limit: dma_limit,
            dcbaa,
            _scratch: scratch,
            cmd,
            ev: EventRing { page: evp, _erst: erst, idx: 0, cycle: 1 },
            buf,
            devices: Vec::new(),
            keyboards: 0,
            mice: 0,
            hubs: 0,
        })
    }

    pub fn ports(&self) -> u32 {
        self.max_ports
    }

    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    fn doorbell(&self, slot: u32, target: u32) {
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        wr(self.db + 4 * slot as u64, target);
    }

    fn ack_events(&self) {
        wr64(self.rt + 0x20 + 0x18, self.ev.dequeue() | (1 << 3));
    }

    /// Process events until `want` matches one (returned) or `ms` pass.
    fn wait_event(&mut self, ms: u64, want: impl Fn(&[u32; 4]) -> bool) -> Option<[u32; 4]> {
        let t0 = now();
        loop {
            while let Some(ev) = self.ev.next() {
                self.ack_events();
                if want(&ev) {
                    return Some(ev);
                }
                self.dispatch(ev);
            }
            if now() - t0 > ms_ticks(ms) {
                return None;
            }
            core::hint::spin_loop();
        }
    }

    fn command(&mut self, param: u64, control: u32) -> Result<[u32; 4], &'static str> {
        let at = self.cmd.push(param, 0, control);
        self.doorbell(0, 0);
        let ev = self
            .wait_event(2000, |e| trb_type(e) == EV_COMMAND && trb_ptr(e) == at)
            .ok_or("xhci: command timed out")?;
        if completion(&ev) != CC_SUCCESS {
            return Err("xhci: command failed");
        }
        Ok(ev)
    }

    /// A control transfer on device `di`'s endpoint 0, data through `self.buf`.
    fn control(&mut self, di: usize, req_type: u8, req: u8, value: u16, index: u16, len: u16) -> Result<usize, &'static str> {
        let setup = req_type as u64 | (req as u64) << 8 | (value as u64) << 16 | (index as u64) << 32 | (len as u64) << 48;
        let dir_in = req_type & 0x80 != 0;
        let trt = if len == 0 { 0 } else if dir_in { 3 } else { 2 };
        let bufp = self.buf.phys;
        if len > 0 {
            self.buf.sync(0, len as usize);
        }
        let dev = &mut self.devices[di];
        let slot = dev.slot;
        dev.ep0.push(setup, 8, (TRB_SETUP << 10) | IDT | (trt << 16));
        let mut data_trb = 0;
        if len > 0 {
            data_trb = dev.ep0.push(bufp, len as u32, (TRB_DATA << 10) | ISP | if dir_in { 1 << 16 } else { 0 });
        }
        let status_dir = if dir_in && len > 0 { 0 } else { 1 << 16 };
        let st = dev.ep0.push(0, 0, (TRB_STATUS << 10) | IOC | status_dir);
        self.doorbell(slot, 1);
        let mut got = len as usize;
        loop {
            let ev = self
                .wait_event(1000, |e| trb_type(e) == EV_TRANSFER && e[3] >> 24 == slot && (e[3] >> 16) & 0x1F == 1)
                .ok_or("xhci: control transfer timed out")?;
            let cc = completion(&ev);
            if cc == CC_SHORT && trb_ptr(&ev) == data_trb {
                got = len as usize - (ev[2] & 0xFF_FFFF) as usize;
                continue; // the status stage still follows
            }
            if cc != CC_SUCCESS {
                return Err("xhci: control transfer failed (stall or error)");
            }
            if trb_ptr(&ev) == st {
                break;
            }
        }
        if len > 0 {
            self.buf.sync(0, len as usize);
        }
        Ok(got)
    }

    fn bytes(&self, n: usize) -> Vec<u8> {
        (0..n).map(|i| self.buf.byte(i)).collect()
    }

    /// Enumerate every device on the root ports (and behind hubs).
    pub fn scan(&mut self) {
        for port in 1..=self.max_ports {
            let a = self.op + 0x400 + 0x10 * (port as u64 - 1);
            let mut sc = rd(a);
            if sc & PORT_PP == 0 {
                wr(a, (sc & PORT_PRESERVE) | PORT_PP);
                delay_ms(20);
                sc = rd(a);
            }
            if sc & PORT_CCS == 0 {
                continue;
            }
            if sc & PORT_PED == 0 {
                // USB 2 ports need a reset to be enabled (USB 3 ones train themselves)
                wr(a, (sc & PORT_PRESERVE) | PORT_PR);
                wait(500, || rd(a) & PORT_PRC != 0);
                delay_ms(10);
                sc = rd(a);
            }
            wr(a, (sc & PORT_PRESERVE) | PORT_CHANGES);
            if sc & PORT_PED == 0 {
                crate::println!("[usb] {} port {}: device did not enable", self.name, port);
                continue;
            }
            let speed = (sc >> 10) & 0xF;
            let label = alloc::format!("{}", port);
            if let Err(e) = self.enumerate(&label, speed, 0, port, 0, 0, 0, false) {
                crate::println!("[usb] {} port {}: {}", self.name, port, e);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn enumerate(&mut self, label: &str, speed: u32, route: u32, root_port: u32, depth: u32, tt_slot: u32, tt_port: u32, mtt: bool) -> Result<(), &'static str> {
        let ev = self.command(0, TRB_ENABLE_SLOT << 10)?;
        let slot = ev[3] >> 24;
        let out = Page::new(self.limit)?;
        self.dcbaa.w64(slot as usize * 8, out.phys);
        self.dcbaa.sync(slot as usize * 8, 8);
        let inp = Page::new(self.limit)?;
        let ep0 = Ring::new(self.limit)?;
        let mps0: u32 = match speed {
            3 => 64,
            4 | 5 => 512,
            _ => 8,
        };
        let s = self.csz;
        let e = 2 * self.csz;
        inp.w32(4, 0b11); // add slot + EP0
        inp.w32(s, (1 << 27) | (speed << 20) | if mtt { 1 << 25 } else { 0 } | route);
        inp.w32(s + 4, root_port << 16);
        inp.w32(s + 8, (tt_port << 8) | tt_slot);
        inp.w32(e + 4, (mps0 << 16) | (4 << 3) | (3 << 1)); // control, CErr 3
        inp.w64(e + 8, ep0.page.phys | 1);
        inp.w32(e + 16, 8);
        inp.sync(0, 4096);
        self.command(inp.phys, (TRB_ADDRESS_DEVICE << 10) | (slot << 24))?;
        delay_ms(5);
        let di = self.devices.len();
        self.devices.push(Device { slot, speed, route, root_port, depth, tt_slot, tt_port, mtt, out, inp, ep0, intr: None });

        // Device descriptor: the first 8 bytes give EP0's real packet size.
        self.control(di, 0x80, 6, 0x0100, 0, 8)?;
        let d8 = self.bytes(8);
        let mps = if speed >= 4 { 1u32 << d8[7].min(9) } else { d8[7] as u32 };
        if mps != mps0 && mps >= 8 {
            let dev = &self.devices[di];
            dev.inp.w32(0, 0);
            dev.inp.w32(4, 1 << 1);
            dev.inp.w32(e + 4, (mps << 16) | (4 << 3) | (3 << 1));
            dev.inp.sync(0, 4096);
            let p = dev.inp.phys;
            self.command(p, (TRB_EVALUATE_CTX << 10) | (slot << 24))?;
        }
        self.control(di, 0x80, 6, 0x0100, 0, 18)?;
        let dd = self.bytes(18);
        let (class, protocol) = (dd[4], dd[6]);
        let vid = u16::from_le_bytes([dd[8], dd[9]]);
        let pid = u16::from_le_bytes([dd[10], dd[11]]);

        // Configuration descriptor (+ interfaces + endpoints).
        self.control(di, 0x80, 6, 0x0200, 0, 9)?;
        let total = (u16::from_le_bytes([self.buf.byte(2), self.buf.byte(3)]) as usize).clamp(9, 1024);
        self.control(di, 0x80, 6, 0x0200, 0, total as u16)?;
        let cfg = self.bytes(total);
        self.control(di, 0x00, 9, cfg[5] as u16, 0, 0)?; // SET_CONFIGURATION

        if class == 9 {
            crate::println!("[usb] {} port {}: {:04x}:{:04x} {} hub", self.name, label, vid, pid, speed_name(speed));
            self.hubs += 1;
            return self.setup_hub(di, label, protocol);
        }
        // the first HID boot-protocol interface with an interrupt-IN endpoint
        let mut i = 0;
        let mut iface: Option<(u8, u8)> = None;
        while i + 2 <= cfg.len() {
            let len = cfg[i] as usize;
            if len < 2 || i + len > cfg.len() {
                break;
            }
            match cfg[i + 1] {
                4 if len >= 9 => {
                    iface = if cfg[i + 5] == 3 && cfg[i + 6] == 1 && (cfg[i + 7] == 1 || cfg[i + 7] == 2) {
                        Some((cfg[i + 2], cfg[i + 7]))
                    } else {
                        None
                    };
                }
                5 if len >= 7 => {
                    if let Some((num, proto)) = iface {
                        let addr = cfg[i + 2];
                        if addr & 0x80 != 0 && cfg[i + 3] & 3 == 3 {
                            let mps = u16::from_le_bytes([cfg[i + 4], cfg[i + 5]]) as u32 & 0x7FF;
                            let kind = if proto == 1 { HidKind::Keyboard } else { HidKind::Mouse };
                            crate::println!(
                                "[usb] {} port {}: {:04x}:{:04x} {} {}",
                                self.name,
                                label,
                                vid,
                                pid,
                                speed_name(speed),
                                if kind == HidKind::Keyboard { "keyboard (HID boot protocol)" } else { "mouse (HID boot protocol)" }
                            );
                            return self.setup_hid(di, num, kind, addr, mps, cfg[i + 6]);
                        }
                    }
                }
                _ => {}
            }
            i += len;
        }
        crate::println!("[usb] {} port {}: {:04x}:{:04x} {} class {:#04x} (no driver)", self.name, label, vid, pid, speed_name(speed), class);
        Ok(())
    }

    fn setup_hid(&mut self, di: usize, iface: u8, kind: HidKind, ep_addr: u8, mps: u32, b_interval: u8) -> Result<(), &'static str> {
        self.control(di, 0x21, 0x0B, 0, iface as u16, 0)?; // SET_PROTOCOL boot
        if kind == HidKind::Keyboard {
            let idle = self.control(di, 0x21, 0x0A, 0, iface as u16, 0); // SET_IDLE: reports only on change
            // SET_REPORT(output): Num Lock LED on, as a PC host does at start-up
            // (some keyboards stay silent until the host has sent one).
            self.buf.w32(0, 0x01);
            let led = self.control(di, 0x21, 0x09, 0x0200, iface as u16, 1);
            crate::println!(
                "[usb] {}: keyboard interface {}: SET_IDLE {}, LED report {}",
                self.name,
                iface,
                if idle.is_ok() { "ok" } else { "refused" },
                if led.is_ok() { "ok (Num Lock on)" } else { "refused" }
            );
        }
        let dci = (ep_addr as u32 & 0xF) * 2 + 1;
        let ring = Ring::new(self.limit)?;
        let buf = Page::new(self.limit)?;
        let dev = &self.devices[di];
        let interval = if dev.speed >= 3 {
            (b_interval.max(1) as u32 - 1).min(15)
        } else {
            // frames (1 ms) -> 125 us exponent, at most 8 ms: like Linux, poll a
            // LS/FS interrupt endpoint behind a TT no less often than that
            let us8 = (b_interval.max(1) as u32) * 8;
            (31 - us8.leading_zeros()).clamp(3, 6)
        };
        crate::println!(
            "[usb] {}: slot {} endpoint {:#04x} ({} bytes, bInterval {}) -> DCI {}, xHCI interval {}{}",
            self.name,
            dev.slot,
            ep_addr,
            mps,
            b_interval,
            dci,
            interval,
            if dev.tt_slot != 0 { alloc::format!(", via the TT of slot {} port {}", dev.tt_slot, dev.tt_port) } else { alloc::string::String::new() }
        );
        let s = self.csz;
        let e = (dci as usize + 1) * self.csz;
        dev.inp.zero(0, 4096);
        dev.inp.w32(4, 1 | (1 << dci));
        dev.inp.w32(s, (dci << 27) | (dev.speed << 20) | if dev.mtt { 1 << 25 } else { 0 } | dev.route);
        dev.inp.w32(s + 4, dev.root_port << 16);
        dev.inp.w32(s + 8, (dev.tt_port << 8) | dev.tt_slot);
        dev.inp.w32(e, interval << 16);
        dev.inp.w32(e + 4, (mps << 16) | (7 << 3) | (3 << 1)); // interrupt IN, CErr 3
        dev.inp.w64(e + 8, ring.page.phys | 1);
        dev.inp.w32(e + 16, (mps << 16) | mps);
        dev.inp.sync(0, 4096);
        let (p, slot) = (dev.inp.phys, dev.slot);
        self.command(p, (TRB_CONFIGURE_EP << 10) | (slot << 24))?;
        self.devices[di].intr = Some(Interrupt { dci, ring, buf, len: mps.min(64), kind, last: [0; 8], broken: false, reports: 0, state: 0 });
        match kind {
            HidKind::Keyboard => self.keyboards += 1,
            HidKind::Mouse => self.mice += 1,
        }
        self.queue_report(di);
        Ok(())
    }

    fn queue_report(&mut self, di: usize) {
        let dev = &mut self.devices[di];
        let slot = dev.slot;
        if let Some(i) = dev.intr.as_mut() {
            i.buf.sync(0, i.len as usize);
            i.ring.push(i.buf.phys, i.len, (TRB_NORMAL << 10) | IOC | ISP);
            let dci = i.dci;
            self.doorbell(slot, dci);
        }
    }

    fn setup_hub(&mut self, di: usize, label: &str, protocol: u8) -> Result<(), &'static str> {
        if self.devices[di].depth >= 4 {
            return Err("hubs nested too deep");
        }
        self.control(di, 0xA0, 6, 0x2900, 0, 9)?;
        let hd = self.bytes(9);
        let ports = hd[2] as u32;
        let chars = u16::from_le_bytes([hd[3], hd[4]]) as u32;
        let power_ms = (hd[5] as u64 * 2).max(100);
        // Tell the controller this slot is a hub (and its TT), so it can
        // schedule split transactions for the LS/FS devices behind it.
        {
            let dev = &self.devices[di];
            dev.out.sync(0, 64); // the output slot context, written by the controller
            let entries = (dev.out.r32(0) >> 27).max(1);
            let s = self.csz;
            dev.inp.zero(0, 4096);
            dev.inp.w32(4, 1);
            let mtt = dev.speed == 3 && protocol == 2;
            dev.inp.w32(s, (entries << 27) | (1 << 26) | if mtt { 1 << 25 } else { 0 } | (dev.speed << 20) | dev.route);
            dev.inp.w32(s + 4, (ports << 24) | (dev.root_port << 16));
            dev.inp.w32(s + 8, (((chars >> 5) & 3) << 16) | (dev.tt_port << 8) | dev.tt_slot);
            dev.inp.sync(0, 4096);
            let (p, slot) = (dev.inp.phys, dev.slot);
            let how = if self.command(p, (TRB_EVALUATE_CTX << 10) | (slot << 24)).is_ok() {
                "Evaluate Context"
            } else if self.command(p, (TRB_CONFIGURE_EP << 10) | (slot << 24)).is_ok() {
                "Configure Endpoint"
            } else {
                "refused by the controller"
            };
            crate::println!(
                "[usb] {}: hub slot {}: {} ports, {}, TT think time {}, hub context via {}",
                self.name,
                slot,
                ports,
                if mtt { "multi-TT" } else { "single TT" },
                (chars >> 5) & 3,
                how
            );
        }
        for port in 1..=ports {
            self.control(di, 0x23, 3, 8, port as u16, 0)?; // SET_FEATURE(PORT_POWER)
        }
        delay_ms(power_ms);
        for port in 1..=ports {
            self.control(di, 0xA3, 0, 0, port as u16, 4)?;
            if self.buf.byte(0) & 1 == 0 {
                continue; // nothing connected
            }
            self.control(di, 0x23, 3, 4, port as u16, 0)?; // SET_FEATURE(PORT_RESET)
            let mut status = 0u16;
            let t0 = now();
            while now() - t0 < ms_ticks(500) {
                delay_ms(10);
                self.control(di, 0xA3, 0, 0, port as u16, 4)?;
                status = u16::from_le_bytes([self.buf.byte(0), self.buf.byte(1)]);
                if self.buf.byte(2) & (1 << 4) != 0 {
                    break; // C_PORT_RESET
                }
            }
            let _ = self.control(di, 0x23, 1, 20, port as u16, 0); // clear C_PORT_RESET
            let _ = self.control(di, 0x23, 1, 16, port as u16, 0); // clear C_PORT_CONNECTION
            if status & 2 == 0 {
                crate::println!("[usb] {} port {}.{}: device did not enable", self.name, label, port);
                continue;
            }
            let speed = if status & (1 << 9) != 0 {
                2
            } else if status & (1 << 10) != 0 {
                3
            } else {
                1
            };
            delay_ms(10);
            let hub = &self.devices[di];
            let (tt_slot, tt_port, mtt) = if hub.speed == 3 && speed <= 2 {
                (hub.slot, port, protocol == 2)
            } else {
                (hub.tt_slot, hub.tt_port, hub.mtt)
            };
            let route = hub.route | (port.min(15) << (4 * hub.depth));
            let (root_port, depth) = (hub.root_port, hub.depth + 1);
            let child = alloc::format!("{}.{}", label, port);
            if let Err(e) = self.enumerate(&child, speed, route, root_port, depth, tt_slot, tt_port, mtt) {
                crate::println!("[usb] {} port {}: {}", self.name, child, e);
            }
        }
        Ok(())
    }

    fn dispatch(&mut self, ev: [u32; 4]) {
        if trb_type(&ev) != EV_TRANSFER {
            return;
        }
        let slot = ev[3] >> 24;
        let dci = (ev[3] >> 16) & 0x1F;
        let Some(di) = self.devices.iter().position(|d| d.slot == slot && d.intr.as_ref().map_or(false, |i| i.dci == dci)) else { return };
        let cc = completion(&ev);
        let i = self.devices[di].intr.as_mut().unwrap();
        if cc != CC_SUCCESS && cc != CC_SHORT {
            if !i.broken {
                crate::println!("[usb] {}: slot {} endpoint {} stopped (completion code {})", self.name, slot, dci, cc);
                i.broken = true;
            }
            return;
        }
        let got = (i.len as usize).saturating_sub((ev[2] & 0xFF_FFFF) as usize).min(8);
        i.buf.sync(0, i.len as usize);
        let mut report = [0u8; 8];
        for (k, r) in report.iter_mut().enumerate().take(got) {
            *r = i.buf.byte(k);
        }
        match i.kind {
            HidKind::Keyboard => super::hid::keyboard(&i.last, &report),
            HidKind::Mouse => super::hid::mouse(&report[..got]),
        }
        i.last = report;
        i.reports += 1;
        self.queue_report(di);
    }

    /// Handle whatever events arrived (reports from keyboards and mice).
    pub fn poll(&mut self) {
        while let Some(ev) = self.ev.next() {
            self.ack_events();
            self.dispatch(ev);
        }
    }

    /// Log each HID endpoint's state (from the output device context) and
    /// report count when the state changed -- for bring-up on new hardware.
    pub fn report_endpoints(&mut self) {
        let csz = self.csz;
        let name = self.name;
        for d in self.devices.iter_mut() {
            let slot = d.slot;
            let Some(i) = d.intr.as_mut() else { continue };
            let off = i.dci as usize * csz;
            d.out.sync(off, 32);
            let state = d.out.r32(off) & 7;
            let st = match state {
                0 => "disabled",
                1 => "running",
                2 => "halted",
                3 => "stopped",
                4 => "error",
                _ => "?",
            };
            if state != i.state || i.reports == 0 {
                crate::println!(
                    "[usb] {}: slot {} {} endpoint {}: {}, {} report(s)",
                    name,
                    slot,
                    if i.kind == HidKind::Keyboard { "keyboard" } else { "mouse" },
                    i.dci,
                    st,
                    i.reports
                );
                i.state = state;
            }
        }
    }
}
