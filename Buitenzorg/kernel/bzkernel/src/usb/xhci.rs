//! An xHCI (USB 3) host controller, polled: no interrupts, events are read
//! from the event ring by `poll()` (the desktop loop) and while waiting for a
//! command or a control transfer.
//!
//! What it does: reset + start the controller (BIOS hand-off, scratchpad
//! buffers, one command ring, one event ring), enumerate every device on the
//! root ports and behind USB 2.0 hubs (route strings; LS/FS devices behind a
//! high-speed hub get the hub's Transaction Translator in their slot), and
//! drive HID boot-protocol keyboards and mice through one interrupt-IN
//! endpoint each (`super::hid`), and USB mass storage (Bulk-Only Transport +
//! SCSI: `msd_read` / `msd_write`, a `BlockDevice` in `super`).
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
const TRB_ISOCH: u32 = 5;
/// Isoch TRB: start the transfer as soon as possible (no frame id).
const SIA: u32 = 1 << 31;
const CC_RING_UNDERRUN: u32 = 14;
const CC_RING_OVERRUN: u32 = 15;
const CC_MISSED_SERVICE: u32 = 23;
/// USB audio: packets kept queued (1 ms each) and bytes per packet
/// (48 frames of 16-bit stereo at 48 kHz).
const ISO_SLOTS: usize = 32;
const ISO_PACKET: usize = 192;
const TRB_ENABLE_SLOT: u32 = 9;
const TRB_DISABLE_SLOT: u32 = 10;
const TRB_ADDRESS_DEVICE: u32 = 11;
const TRB_CONFIGURE_EP: u32 = 12;
const TRB_EVALUATE_CTX: u32 = 13;
const TRB_RESET_EP: u32 = 14;
const TRB_SET_DEQUEUE: u32 = 16;
const EV_TRANSFER: u32 = 32;
const EV_COMMAND: u32 = 33;
const EV_PORT_STATUS: u32 = 34;

const IOC: u32 = 1 << 5;
const IDT: u32 = 1 << 6;
const ISP: u32 = 1 << 2;
const CHAIN: u32 = 1 << 4;

const CC_SUCCESS: u32 = 1;
const CC_STALL: u32 = 6;
const CC_SHORT: u32 = 13;

/// A mass-storage command moves at most this many pages (64 KiB).
const MSD_PAGES: usize = 16;
const MSD_MAX_SECTORS: usize = MSD_PAGES * 4096 / 512;
/// A stick may take seconds for a write (wear levelling) or its first read.
const MSD_TIMEOUT_MS: u64 = 15_000;

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

/// A device that is unplugged gives its rings, contexts and buffers back.
impl Drop for Page {
    fn drop(&mut self) {
        crate::memory::free_frame(self.phys);
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
            // the link TRB belongs to the TD when the TRB before it chains on
            self.page.w32(l + 12, (self.page.r32(l + 12) & !(1 | CHAIN)) | (control & CHAIN) | self.cycle);
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
    /// A hub's status-change endpoint: a bitmap of ports that changed.
    Hub,
    /// A gamepad / joystick (HID report descriptor, or XInput).
    Gamepad,
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
    /// A gamepad's report layout and held buttons.
    pad: Option<super::hid::Pad>,
}

/// A mass-storage interface: two bulk endpoints and the driver's DMA pages.
struct Msd {
    in_dci: u32,
    out_dci: u32,
    in_addr: u8,
    out_addr: u8,
    in_mps: u32,
    out_mps: u32,
    in_ring: Ring,
    out_ring: Ring,
    /// Command block at 0, status block at 64.
    cmd: Page,
    pages: Vec<Page>,
    tag: u32,
    sectors: u64,
    label: alloc::string::String,
}

/// A USB Audio Class 1 output stream: an isochronous OUT endpoint fed one
/// 1 ms packet per TRB, `ISO_SLOTS` queued, each completion refilled.
struct AudioOut {
    dci: u32,
    ring: Ring,
    /// 16 packets per page.
    pages: [Page; 2],
    next: usize,
    /// Packets completed / failed / underruns (the ring ran dry).
    done: u64,
    errors: u64,
    underruns: u64,
    /// A test tone (Hz, packets left) played instead of the mixer.
    tone: (u32, u32),
    phase: u32,
    /// Packets that carried mixer audio.
    mixed: u64,
}

struct BulkEndpoints {
    iface: u8,
    in_ep: (u8, u32, u32),
    out_ep: (u8, u32, u32),
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
    msd: Option<Msd>,
    audio: Option<AudioOut>,
    /// Port path ("3", "1.2") and what it is ("046d:c52b keyboard"), for logs.
    label: alloc::string::String,
    what: alloc::string::String,
    /// Hubs: port count and protocol (2 = multi-TT).
    hub_ports: u32,
    hub_protocol: u8,
}

/// The controller's devices by index. An unplugged device leaves an empty
/// entry, so the indices `UsbDisk`s hold never point at another device.
struct Devices(Vec<Option<Device>>);

impl core::ops::Index<usize> for Devices {
    type Output = Device;
    fn index(&self, i: usize) -> &Device {
        self.0[i].as_ref().expect("xhci: device was unplugged")
    }
}

impl core::ops::IndexMut<usize> for Devices {
    fn index_mut(&mut self, i: usize) -> &mut Device {
        self.0[i].as_mut().expect("xhci: device was unplugged")
    }
}

impl Devices {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn push(&mut self, d: Device) {
        self.0.push(Some(d));
    }
    fn alive(&self, i: usize) -> bool {
        self.0.get(i).is_some_and(|d| d.is_some())
    }
    fn live(&self) -> impl Iterator<Item = (usize, &Device)> {
        self.0.iter().enumerate().filter_map(|(i, d)| d.as_ref().map(|d| (i, d)))
    }
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
    devices: Devices,
    pub keyboards: usize,
    pub pads: usize,
    pub mice: usize,
    pub hubs: usize,
    pub disks: usize,
    /// USB audio outputs.
    pub speakers: usize,
    /// Hot-plug work found while handling events, done by `service`: root
    /// ports with a Port Status Change event, (hub device, changed-port bitmap).
    root_changes: Vec<u32>,
    hub_changes: Vec<(usize, u32)>,
    /// Disks that appeared / disappeared since `take_disk_changes`.
    new_disks: Vec<(usize, u64, alloc::string::String)>,
    gone_disks: Vec<usize>,
    /// Devices attached / detached after boot (`lsusb`, smoke test).
    pub attached: usize,
    pub detached: usize,
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
            devices: Devices(Vec::new()),
            keyboards: 0,
            pads: 0,
            mice: 0,
            hubs: 0,
            disks: 0,
            speakers: 0,
            root_changes: Vec::new(),
            hub_changes: Vec::new(),
            new_disks: Vec::new(),
            gone_disks: Vec::new(),
            attached: 0,
            detached: 0,
        })
    }

    pub fn ports(&self) -> u32 {
        self.max_ports
    }

    pub fn device_count(&self) -> usize {
        self.devices.live().count()
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

    /// Connected root ports, once the count has been stable for 200 ms (at
    /// least 300 ms, at most 1.5 s after the controller started). Right after
    /// a reset a real controller still trains its links: QEMU reports every
    /// device at once, a PC's xHCI does not, and scanning straight away missed
    /// the USB stick on a Dell Latitude 7290 (no /disk, no apps).
    fn settle(&self) -> u32 {
        let count = || (1..=self.max_ports).filter(|p| rd(self.op + 0x400 + 0x10 * (*p as u64 - 1)) & PORT_CCS != 0).count() as u32;
        let t0 = now();
        let (mut last, mut since) = (count(), now());
        loop {
            delay_ms(50);
            let n = count();
            if n != last {
                last = n;
                since = now();
            }
            let elapsed = now() - t0;
            if (elapsed > ms_ticks(300) && now() - since > ms_ticks(200)) || elapsed > ms_ticks(1500) {
                return n;
            }
        }
    }

    /// Enumerate every device on the root ports (and behind hubs).
    pub fn scan(&mut self) {
        let connected = self.settle();
        crate::println!("[usb] {}: {} of {} ports connected", self.name, connected, self.max_ports);
        for port in 1..=self.max_ports {
            let a = self.portsc(port);
            let sc = rd(a);
            if sc & PORT_PP == 0 {
                wr(a, (sc & PORT_PRESERVE) | PORT_PP);
                delay_ms(20);
            }
            self.attach_root(port);
        }
        // events the scan itself caused (connect changes) are handled already,
        // and the boot's disks are reported by `disks_found`
        self.root_changes.clear();
        self.hub_changes.clear();
        self.new_disks.clear();
    }

    fn portsc(&self, port: u32) -> u64 {
        self.op + 0x400 + 0x10 * (port as u64 - 1)
    }

    /// Reset (if needed) and enumerate the device on root port `port`.
    fn attach_root(&mut self, port: u32) {
        let a = self.portsc(port);
        let mut sc = rd(a);
        if sc & PORT_CCS == 0 {
            wr(a, (sc & PORT_PRESERVE) | PORT_CHANGES);
            return;
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
            return;
        }
        let speed = (sc >> 10) & 0xF;
        let label = alloc::format!("{}", port);
        if let Err(e) = self.enumerate(&label, speed, 0, port, 0, 0, 0, false) {
            crate::println!("[usb] {} port {}: {}", self.name, port, e);
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
        self.devices.push(Device {
            slot,
            speed,
            route,
            root_port,
            depth,
            tt_slot,
            tt_port,
            mtt,
            out,
            inp,
            ep0,
            intr: None,
            msd: None,
            audio: None,
            label: alloc::string::String::from(label),
            what: alloc::string::String::from("device"),
            hub_ports: 0,
            hub_protocol: 0,
        });

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
        self.devices[di].what = alloc::format!("{:04x}:{:04x} {}", vid, pid, speed_name(speed));

        if class == 9 {
            crate::println!("[usb] {} port {}: {:04x}:{:04x} {} hub", self.name, label, vid, pid, speed_name(speed));
            self.hubs += 1;
            self.devices[di].what = alloc::format!("{:04x}:{:04x} hub", vid, pid);
            // the status-change endpoint: the first interrupt-IN endpoint
            let mut status_ep = None;
            let mut i = 0;
            while i + 2 <= cfg.len() && cfg[i] >= 2 && i + cfg[i] as usize <= cfg.len() {
                if cfg[i + 1] == 5 && cfg[i] >= 7 && cfg[i + 2] & 0x80 != 0 && cfg[i + 3] & 3 == 3 {
                    status_ep = Some((cfg[i + 2], u16::from_le_bytes([cfg[i + 4], cfg[i + 5]]) as u32 & 0x7FF, cfg[i + 6]));
                    break;
                }
                i += cfg[i] as usize;
            }
            return self.setup_hub(di, label, protocol, status_ep);
        }
        // the first HID boot-protocol interface with an interrupt-IN endpoint,
        // or a mass-storage one (SCSI over Bulk-Only Transport) with two bulk ones
        let mut i = 0;
        let mut iface: Option<(u8, u8)> = None;
        // a HID interface without the boot protocol (gamepads): (number, report
        // descriptor length, XInput)
        let mut pad_iface: Option<(u8, u16, bool)> = None;
        let mut storage: Option<u8> = None;
        let (mut bulk_in, mut bulk_out) = (None, None);
        // USB Audio Class 1 streaming: (interface, alternate setting) of the
        // current interface descriptor, whether its format is 2 ch / 16 bit /
        // 48 kHz, and the first such setting with an isochronous OUT endpoint
        let mut as_iface: Option<(u8, u8)> = None;
        let mut as_ok = false;
        let mut audio_out: Option<(u8, u8, u8, u32, u8)> = None;
        while i + 2 <= cfg.len() {
            let len = cfg[i] as usize;
            if len < 2 || i + len > cfg.len() {
                break;
            }
            match cfg[i + 1] {
                4 if len >= 9 => {
                    as_iface = (cfg[i + 5] == 1 && cfg[i + 6] == 2 && cfg[i + 3] > 0).then_some((cfg[i + 2], cfg[i + 3]));
                    as_ok = false;
                    iface = if cfg[i + 5] == 3 && cfg[i + 6] == 1 && (cfg[i + 7] == 1 || cfg[i + 7] == 2) {
                        Some((cfg[i + 2], cfg[i + 7]))
                    } else {
                        None
                    };
                    storage = (cfg[i + 5] == 8 && cfg[i + 6] == 6 && cfg[i + 7] == 0x50).then_some(cfg[i + 2]);
                    pad_iface = if iface.is_none() && cfg[i + 5] == 3 {
                        Some((cfg[i + 2], 0, false))
                    } else if cfg[i + 5] == 0xFF && cfg[i + 6] == 0x5D && cfg[i + 7] == 1 {
                        Some((cfg[i + 2], 0, true)) // Xbox 360 controller
                    } else {
                        None
                    };
                }
                0x24 if len >= 8 && as_iface.is_some() && cfg[i + 2] == 2 && cfg[i + 3] == 1 => {
                    // FORMAT_TYPE I: channels, subframe size, bits, rates
                    let (ch, sub, bits, nrates) = (cfg[i + 4], cfg[i + 5], cfg[i + 6], cfg[i + 7] as usize);
                    let mut has48 = false;
                    if nrates == 0 && len >= 14 {
                        let lo = cfg[i + 8] as u32 | (cfg[i + 9] as u32) << 8 | (cfg[i + 10] as u32) << 16;
                        let hi = cfg[i + 11] as u32 | (cfg[i + 12] as u32) << 8 | (cfg[i + 13] as u32) << 16;
                        has48 = lo <= 48000 && 48000 <= hi;
                    }
                    for r in 0..nrates {
                        let o = i + 8 + r * 3;
                        if o + 3 <= i + len && (cfg[o] as u32 | (cfg[o + 1] as u32) << 8 | (cfg[o + 2] as u32) << 16) == 48000 {
                            has48 = true;
                        }
                    }
                    as_ok = ch == 2 && sub == 2 && bits == 16 && has48;
                }
                0x21 if len >= 9 => {
                    // HID descriptor: the report descriptor's length
                    if let Some((n, _, false)) = pad_iface {
                        if cfg[i + 6] == 0x22 {
                            pad_iface = Some((n, u16::from_le_bytes([cfg[i + 7], cfg[i + 8]]), false));
                        }
                    }
                }
                5 if len >= 7 => {
                    if let (Some((num, alt)), true, None) = (as_iface, as_ok, audio_out) {
                        if cfg[i + 3] & 3 == 1 && cfg[i + 2] & 0x80 == 0 {
                            let mps = u16::from_le_bytes([cfg[i + 4], cfg[i + 5]]) as u32 & 0x7FF;
                            audio_out = Some((num, alt, cfg[i + 2], mps, cfg[i + 6]));
                        }
                    }
                    if storage.is_some() && cfg[i + 3] & 3 == 2 {
                        let mps = u16::from_le_bytes([cfg[i + 4], cfg[i + 5]]) as u32 & 0x7FF;
                        // SuperSpeed: bMaxBurst in the companion descriptor that follows
                        let next = i + len;
                        let burst = if next + 3 <= cfg.len() && cfg[next + 1] == 0x30 { cfg[next + 2] as u32 } else { 0 };
                        let ep = Some((cfg[i + 2], mps, burst));
                        if cfg[i + 2] & 0x80 != 0 {
                            bulk_in = bulk_in.or(ep);
                        } else {
                            bulk_out = bulk_out.or(ep);
                        }
                    }
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
                            self.devices[di].what =
                                alloc::format!("{:04x}:{:04x} {}", vid, pid, if kind == HidKind::Keyboard { "keyboard" } else { "mouse" });
                            return self.setup_hid(di, num, kind, addr, mps, cfg[i + 6]);
                        }
                    }
                    if let Some((num, dlen, xinput)) = pad_iface {
                        let addr = cfg[i + 2];
                        if addr & 0x80 != 0 && cfg[i + 3] & 3 == 3 {
                            pad_iface = None;
                            let pad = if xinput {
                                Some(super::hid::Pad::xinput())
                            } else if dlen > 0 {
                                let n = dlen.min(1024);
                                match self.control(di, 0x81, 6, 0x2200, num as u16, n) {
                                    Ok(got) => super::hid::Pad::parse(&self.bytes(got.min(n as usize))),
                                    Err(_) => None,
                                }
                            } else {
                                None
                            };
                            if let Some(pad) = pad {
                                let mps = u16::from_le_bytes([cfg[i + 4], cfg[i + 5]]) as u32 & 0x7FF;
                                crate::println!("[usb] {} port {}: {:04x}:{:04x} {} gamepad ({})", self.name, label, vid, pid, speed_name(speed), pad.summary());
                                self.devices[di].what = alloc::format!("{:04x}:{:04x} gamepad", vid, pid);
                                self.setup_hid(di, num, HidKind::Gamepad, addr, mps, cfg[i + 6])?;
                                if let Some(it) = self.devices[di].intr.as_mut() {
                                    it.pad = Some(pad);
                                }
                                return Ok(());
                            }
                        }
                    }
                }
                _ => {}
            }
            i += len;
        }
        if let Some((num, alt, ep, mps, b_interval)) = audio_out {
            crate::println!("[usb] {} port {}: {:04x}:{:04x} {} audio output (USB Audio Class 1, 48000 Hz 2 ch 16-bit)", self.name, label, vid, pid, speed_name(speed));
            self.devices[di].what = alloc::format!("{:04x}:{:04x} audio", vid, pid);
            return self.setup_audio(di, num, alt, ep, mps, b_interval);
        }
        if let (Some(iface), Some(in_ep), Some(out_ep)) = (storage, bulk_in, bulk_out) {
            crate::println!("[usb] {} port {}: {:04x}:{:04x} {} mass storage", self.name, label, vid, pid, speed_name(speed));
            self.devices[di].what = alloc::format!("{:04x}:{:04x} disk", vid, pid);
            return self.setup_msd(di, label, BulkEndpoints { iface, in_ep, out_ep });
        }
        crate::println!("[usb] {} port {}: {:04x}:{:04x} {} class {:#04x} (no driver)", self.name, label, vid, pid, speed_name(speed), class);
        Ok(())
    }

    fn setup_msd(&mut self, di: usize, label: &str, eps: BulkEndpoints) -> Result<(), &'static str> {
        let (in_addr, in_mps, in_burst) = eps.in_ep;
        let (out_addr, out_mps, out_burst) = eps.out_ep;
        if in_mps == 0 || out_mps == 0 {
            return Err("mass storage: endpoint without a packet size");
        }
        let in_dci = (in_addr as u32 & 0xF) * 2 + 1;
        let out_dci = (out_addr as u32 & 0xF) * 2;
        let in_ring = Ring::new(self.limit)?;
        let out_ring = Ring::new(self.limit)?;
        let cmd = Page::new(self.limit)?;
        let mut pages = Vec::with_capacity(MSD_PAGES);
        for _ in 0..MSD_PAGES {
            pages.push(Page::new(self.limit)?);
        }
        let s = self.csz;
        let dev = &self.devices[di];
        dev.inp.zero(0, 4096);
        dev.inp.w32(4, 1 | (1 << in_dci) | (1 << out_dci));
        dev.inp.w32(s, (in_dci.max(out_dci) << 27) | (dev.speed << 20) | if dev.mtt { 1 << 25 } else { 0 } | dev.route);
        dev.inp.w32(s + 4, dev.root_port << 16);
        dev.inp.w32(s + 8, (dev.tt_port << 8) | dev.tt_slot);
        for (dci, mps, burst, ep_type, ring) in [(in_dci, in_mps, in_burst, 6u32, &in_ring), (out_dci, out_mps, out_burst, 2u32, &out_ring)] {
            let e = (dci as usize + 1) * s;
            dev.inp.w32(e + 4, (mps << 16) | (burst << 8) | (ep_type << 3) | (3 << 1));
            dev.inp.w64(e + 8, ring.page.phys | 1);
            dev.inp.w32(e + 16, 3072); // average TRB length of a bulk endpoint
        }
        dev.inp.sync(0, 4096);
        let (p, slot) = (dev.inp.phys, dev.slot);
        self.command(p, (TRB_CONFIGURE_EP << 10) | (slot << 24))?;
        self.devices[di].msd = Some(Msd {
            in_dci,
            out_dci,
            in_addr,
            out_addr,
            in_mps,
            out_mps,
            in_ring,
            out_ring,
            cmd,
            pages,
            tag: 1,
            sectors: 0,
            label: alloc::format!("usb{}", label),
        });
        let _ = eps.iface;
        match self.msd_start(di) {
            Ok(sectors) => {
                crate::println!("[usb] {} port {}: disk of {} MiB", self.name, label, sectors / 2048);
                self.disks += 1;
                self.devices[di].what = alloc::format!("{} of {} MiB", self.devices[di].what, sectors / 2048);
                self.new_disks.push((di, sectors, alloc::format!("usb{}", label)));
                Ok(())
            }
            Err(e) => {
                self.devices[di].msd = None;
                Err(e)
            }
        }
    }

    /// INQUIRY, wait until the medium is ready, READ CAPACITY. Returns sectors.
    fn msd_start(&mut self, di: usize) -> Result<u64, &'static str> {
        self.msd_scsi(di, &[0x12, 0, 0, 0, 36, 0], 36, false)?; // INQUIRY
        // a stick reports "not ready" for a moment after a reset
        let t0 = now();
        loop {
            match self.msd_scsi(di, &[0, 0, 0, 0, 0, 0], 0, false) {
                Ok(()) => break,
                Err(e) if now() - t0 > ms_ticks(5000) => return Err(e),
                Err(_) => {
                    let _ = self.msd_scsi(di, &[0x03, 0, 0, 0, 18, 0], 18, false); // REQUEST SENSE
                    delay_ms(50);
                }
            }
        }
        self.msd_scsi(di, &[0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0], 8, false)?; // READ CAPACITY (10)
        let m = self.devices[di].msd.as_mut().ok_or("mass storage: gone")?;
        m.pages[0].sync(0, 8);
        let be = |o: usize| (0..4).fold(0u32, |a, k| a << 8 | m.pages[0].byte(o + k) as u32);
        let (last, block) = (be(0), be(4));
        if block != 512 {
            return Err("mass storage: block size is not 512 bytes");
        }
        m.sectors = last as u64 + 1;
        Ok(m.sectors)
    }

    /// Queue `bytes` of the driver's pages (IN or OUT) and wait for the end.
    /// Returns the completion code.
    fn msd_bulk(&mut self, di: usize, dir_in: bool, segs: &[(u64, u32)]) -> Result<u32, &'static str> {
        let dev = &mut self.devices[di];
        let slot = dev.slot;
        let m = dev.msd.as_mut().ok_or("mass storage: gone")?;
        let (dci, mps) = if dir_in { (m.in_dci, m.in_mps) } else { (m.out_dci, m.out_mps) };
        let ring = if dir_in { &mut m.in_ring } else { &mut m.out_ring };
        let mut left: u32 = segs.iter().map(|s| s.1).sum();
        for (k, &(phys, len)) in segs.iter().enumerate() {
            left -= len;
            let last = k + 1 == segs.len();
            // TD Size: packets still to come after this TRB
            let td = if last { 0 } else { left.div_ceil(mps).min(31) };
            let flags = if last { IOC | ISP } else { CHAIN };
            ring.push(phys, len | (td << 17), (TRB_NORMAL << 10) | flags);
        }
        self.doorbell(slot, dci);
        let ev = self
            .wait_event(MSD_TIMEOUT_MS, |e| trb_type(e) == EV_TRANSFER && e[3] >> 24 == slot && (e[3] >> 16) & 0x1F == dci)
            .ok_or("mass storage: transfer timed out")?;
        Ok(completion(&ev))
    }

    /// A stalled bulk endpoint: reset it in the controller, skip the failed
    /// transfer, clear the halt in the device.
    fn msd_clear_stall(&mut self, di: usize, dir_in: bool) -> Result<(), &'static str> {
        let dev = &self.devices[di];
        let slot = dev.slot;
        let m = dev.msd.as_ref().ok_or("mass storage: gone")?;
        let (dci, addr) = if dir_in { (m.in_dci, m.in_addr) } else { (m.out_dci, m.out_addr) };
        let ring = if dir_in { &m.in_ring } else { &m.out_ring };
        let deq = (ring.page.phys + (ring.idx * 16) as u64) | ring.cycle as u64;
        self.command(0, (TRB_RESET_EP << 10) | (dci << 16) | (slot << 24))?;
        self.command(deq, (TRB_SET_DEQUEUE << 10) | (dci << 16) | (slot << 24))?;
        self.control(di, 0x02, 1, 0, addr as u16, 0)?; // CLEAR_FEATURE(ENDPOINT_HALT)
        Ok(())
    }

    /// One SCSI command: command block, `bytes` of data in the driver's pages
    /// (host to device if `write`), status block.
    fn msd_scsi(&mut self, di: usize, cdb: &[u8], bytes: usize, write: bool) -> Result<(), &'static str> {
        let m = self.devices[di].msd.as_mut().ok_or("mass storage: gone")?;
        m.tag = m.tag.wrapping_add(1);
        let tag = m.tag;
        let c = &m.cmd;
        c.zero(0, 128);
        c.w32(0, 0x4342_5355); // "USBC"
        c.w32(4, tag);
        c.w32(8, bytes as u32);
        c.w32(12, if write || bytes == 0 { 0 } else { 0x80 } | (cdb.len() as u32) << 16);
        for (k, b) in cdb.iter().enumerate() {
            unsafe { c.va.add(15 + k).write_volatile(*b) };
        }
        c.sync(0, 128);
        let cmd_phys = c.phys;
        let mut segs = [(0u64, 0u32); MSD_PAGES];
        let mut left = bytes;
        let mut n = 0;
        while left > 0 {
            let len = left.min(4096);
            segs[n] = (m.pages[n].phys, len as u32);
            m.pages[n].sync(0, len);
            left -= len;
            n += 1;
        }
        if self.msd_bulk(di, false, &[(cmd_phys, 31)])? != CC_SUCCESS {
            return Err("mass storage: command block refused");
        }
        if n > 0 {
            match self.msd_bulk(di, !write, &segs[..n])? {
                CC_SUCCESS | CC_SHORT => {}
                CC_STALL => self.msd_clear_stall(di, !write)?, // the status block says why
                _ => return Err("mass storage: data transfer failed"),
            }
        }
        let status_seg = [(cmd_phys + 64, 13u32)];
        match self.msd_bulk(di, true, &status_seg)? {
            CC_SUCCESS | CC_SHORT => {}
            CC_STALL => {
                self.msd_clear_stall(di, true)?;
                if self.msd_bulk(di, true, &status_seg)? != CC_SUCCESS {
                    return Err("mass storage: no status block");
                }
            }
            _ => return Err("mass storage: status transfer failed"),
        }
        let m = self.devices[di].msd.as_ref().ok_or("mass storage: gone")?;
        m.cmd.sync(64, 13);
        if m.cmd.r32(64) != 0x5342_5355 || m.cmd.r32(68) != tag {
            return Err("mass storage: bad status block");
        }
        if n > 0 && !write {
            for (k, s) in segs[..n].iter().enumerate() {
                m.pages[k].sync(0, s.1 as usize);
            }
        }
        match m.cmd.byte(76) {
            0 => Ok(()),
            1 => Err("mass storage: command failed"),
            _ => Err("mass storage: phase error"),
        }
    }

    /// (device index, sectors, name) of every mass-storage device.
    pub fn disks_found(&self) -> Vec<(usize, u64, alloc::string::String)> {
        self.devices.live().filter_map(|(i, d)| d.msd.as_ref().map(|m| (i, m.sectors, m.label.clone()))).collect()
    }

    /// Read whole sectors from device `di`.
    pub fn msd_read(&mut self, di: usize, lba: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        if !self.devices.alive(di) {
            return Err("USB disk was unplugged");
        }
        let total = buf.len() / 512;
        let mut done = 0;
        while done < total {
            let n = (total - done).min(MSD_MAX_SECTORS);
            let at = (lba + done as u64) as u32;
            let (a, c) = (at.to_be_bytes(), (n as u16).to_be_bytes());
            self.msd_scsi(di, &[0x28, 0, a[0], a[1], a[2], a[3], 0, c[0], c[1], 0], n * 512, false)?;
            let m = self.devices[di].msd.as_ref().ok_or("mass storage: gone")?;
            for (k, chunk) in buf[done * 512..(done + n) * 512].chunks_mut(4096).enumerate() {
                unsafe { core::ptr::copy_nonoverlapping(m.pages[k].va, chunk.as_mut_ptr(), chunk.len()) };
            }
            done += n;
        }
        Ok(())
    }

    /// Write one sector to device `di` and flush it to the medium.
    pub fn msd_write(&mut self, di: usize, lba: u64, buf: &[u8]) -> Result<(), &'static str> {
        if !self.devices.alive(di) {
            return Err("USB disk was unplugged");
        }
        let m = self.devices[di].msd.as_ref().ok_or("mass storage: gone")?;
        unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), m.pages[0].va, 512) };
        let a = (lba as u32).to_be_bytes();
        self.msd_scsi(di, &[0x2A, 0, a[0], a[1], a[2], a[3], 0, 0, 1, 0], 512, true)?;
        // SYNCHRONIZE CACHE: not every stick implements it
        let _ = self.msd_scsi(di, &[0x35, 0, 0, 0, 0, 0, 0, 0, 0, 0], 0, false);
        Ok(())
    }

    fn setup_hid(&mut self, di: usize, iface: u8, kind: HidKind, ep_addr: u8, mps: u32, b_interval: u8) -> Result<(), &'static str> {
        if kind != HidKind::Gamepad {
            self.control(di, 0x21, 0x0B, 0, iface as u16, 0)?; // SET_PROTOCOL boot
        }
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
        self.devices[di].intr = Some(Interrupt { dci, ring, buf, len: mps.min(64), kind, last: [0; 8], broken: false, reports: 0, state: 0, pad: None });
        match kind {
            HidKind::Keyboard => self.keyboards += 1,
            HidKind::Mouse => self.mice += 1,
            HidKind::Gamepad => {
                self.pads += 1;
                super::hid::PADS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            }
            HidKind::Hub => {}
        }
        self.queue_report(di);
        Ok(())
    }

    /// Configure the isochronous OUT endpoint, select the streaming
    /// setting, set 48 kHz, and queue the first packets (silence).
    fn setup_audio(&mut self, di: usize, iface: u8, alt: u8, ep_addr: u8, mps: u32, b_interval: u8) -> Result<(), &'static str> {
        if (mps as usize) < ISO_PACKET {
            return Err("usb audio: packets too small for 48 kHz stereo");
        }
        let dci = (ep_addr as u32 & 0xF) * 2; // OUT
        let ring = Ring::new(self.limit)?;
        let pages = [Page::new(self.limit)?, Page::new(self.limit)?];
        let dev = &self.devices[di];
        // full speed: frames -> 125 us exponent (bInterval 1 = every frame = 3);
        // high speed: 2^(bInterval-1) microframes
        let interval = if dev.speed >= 3 { (b_interval.max(1) as u32 - 1).min(15) } else { (b_interval.max(1) as u32 - 1 + 3).min(15) };
        let s = self.csz;
        let e = (dci as usize + 1) * self.csz;
        dev.inp.zero(0, 4096);
        dev.inp.w32(4, 1 | (1 << dci));
        dev.inp.w32(s, (dci << 27) | (dev.speed << 20) | if dev.mtt { 1 << 25 } else { 0 } | dev.route);
        dev.inp.w32(s + 4, dev.root_port << 16);
        dev.inp.w32(s + 8, (dev.tt_port << 8) | dev.tt_slot);
        dev.inp.w32(e, interval << 16);
        dev.inp.w32(e + 4, (mps << 16) | (1 << 3)); // isoch OUT, CErr 0
        dev.inp.w64(e + 8, ring.page.phys | 1);
        dev.inp.w32(e + 16, (mps << 16) | ISO_PACKET as u32);
        dev.inp.sync(0, 4096);
        let (p, slot) = (dev.inp.phys, dev.slot);
        self.command(p, (TRB_CONFIGURE_EP << 10) | (slot << 24))?;
        self.control(di, 0x01, 11, alt as u16, iface as u16, 0)?; // SET_INTERFACE
        // SET_CUR sampling frequency on the endpoint (some devices have one rate and refuse)
        self.buf.w32(0, 48000);
        let rate = self.control(di, 0x22, 0x01, 0x0100, ep_addr as u16, 3).is_ok();
        crate::println!(
            "[usb] {}: slot {} audio interface {} alt {} endpoint {:#04x} ({} bytes, bInterval {}) -> DCI {}, xHCI interval {}, rate {}",
            self.name, slot, iface, alt, ep_addr, mps, b_interval, dci, interval, if rate { "48000 Hz set" } else { "fixed" }
        );
        self.devices[di].audio = Some(AudioOut { dci, ring, pages, next: 0, done: 0, errors: 0, underruns: 0, tone: (0, 0), phase: 0, mixed: 0 });
        self.speakers += 1;
        super::AUDIO_DEVICES.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        for _ in 0..ISO_SLOTS {
            self.queue_audio(di);
        }
        Ok(())
    }

    /// Fill the next packet (tone, mixer or silence) and queue it.
    fn queue_audio(&mut self, di: usize) {
        let dev = &mut self.devices[di];
        let slot = dev.slot;
        let Some(a) = dev.audio.as_mut() else { return };
        let k = a.next % ISO_SLOTS;
        a.next += 1;
        let (page, off) = (&a.pages[k / 16], (k % 16) * ISO_PACKET);
        let mut pcm = [0i16; ISO_PACKET / 2];
        if a.tone.1 > 0 {
            // a triangle wave, 1/4 of full scale
            let step = (a.tone.0 as u64 * (1u64 << 32) / 48000) as u32;
            for f in 0..ISO_PACKET / 4 {
                let ph = a.phase;
                let tri = if ph < 0x8000_0000 { (ph >> 15) as i32 - 0x8000 } else { 0x8000 - ((ph - 0x8000_0000) >> 15) as i32 };
                let v = (tri / 4) as i16;
                pcm[f * 2] = v;
                pcm[f * 2 + 1] = v;
                a.phase = a.phase.wrapping_add(step);
            }
            a.tone.1 -= 1;
        } else if super::AUDIO_SINK.load(core::sync::atomic::Ordering::Relaxed) && crate::mixer::mix(&mut pcm) {
            a.mixed += 1;
        }
        let vol = super::audio_gain();
        for f in 0..ISO_PACKET / 4 {
            let l = ((pcm[f * 2] as i32 * vol) >> 8) as i16 as u16 as u32;
            let r = ((pcm[f * 2 + 1] as i32 * vol) >> 8) as i16 as u16 as u32;
            page.w32(off + f * 4, l | r << 16);
        }
        page.sync(off, ISO_PACKET);
        let phys = page.phys + off as u64;
        a.ring.push(phys, ISO_PACKET as u32, (TRB_ISOCH << 10) | IOC | SIA);
        let dci = a.dci;
        self.doorbell(slot, dci);
    }

    /// Play a test tone on every USB audio output for `ms`.
    pub fn audio_tone(&mut self, hz: u32, ms: u32) -> bool {
        let mut any = false;
        for d in self.devices.0.iter_mut().flatten() {
            if let Some(a) = d.audio.as_mut() {
                a.tone = (hz, ms);
                any = true;
            }
        }
        any
    }

    /// (packets completed, errors, underruns, packets with mixer audio) over
    /// every USB audio output.
    pub fn audio_stats(&self) -> (u64, u64, u64, u64) {
        let mut t = (0, 0, 0, 0);
        for (_, d) in self.devices.live() {
            if let Some(a) = d.audio.as_ref() {
                t = (t.0 + a.done, t.1 + a.errors, t.2 + a.underruns, t.3 + a.mixed);
            }
        }
        t
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

    fn setup_hub(&mut self, di: usize, label: &str, protocol: u8, status_ep: Option<(u8, u32, u8)>) -> Result<(), &'static str> {
        if self.devices[di].depth >= 4 {
            return Err("hubs nested too deep");
        }
        self.control(di, 0xA0, 6, 0x2900, 0, 9)?;
        let hd = self.bytes(9);
        let ports = hd[2] as u32;
        let chars = u16::from_le_bytes([hd[3], hd[4]]) as u32;
        let power_ms = (hd[5] as u64 * 2).max(100);
        let mtt = self.devices[di].speed == 3 && protocol == 2;
        self.devices[di].hub_ports = ports;
        self.devices[di].hub_protocol = protocol;
        // Tell the controller this slot is a hub (and its TT), so it can
        // schedule split transactions for the LS/FS devices behind it.
        {
            let dev = &self.devices[di];
            dev.out.sync(0, 64); // the output slot context, written by the controller
            let entries = (dev.out.r32(0) >> 27).max(1);
            let s = self.csz;
            dev.inp.zero(0, 4096);
            dev.inp.w32(4, 1);
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
            self.hub_attach(di, label, port);
        }
        // Hot-plug: the hub reports port changes on its status-change
        // endpoint (a bitmap: bit n = port n), like a keyboard reports keys.
        if let Some((addr, mps, b_interval)) = status_ep {
            match self.setup_hub_status(di, addr, mps, b_interval, ports, ((chars >> 5) & 3, mtt)) {
                Ok(()) => crate::println!("[usb] {}: hub {}: watching its ports (status endpoint {:#04x})", self.name, label, addr),
                Err(e) => crate::println!("[usb] {}: hub {}: no hot-plug ({})", self.name, label, e),
            }
        }
        Ok(())
    }

    /// Reset and enumerate whatever is on port `port` of hub `di`.
    fn hub_attach(&mut self, di: usize, label: &str, port: u32) {
        if self.control(di, 0x23, 3, 4, port as u16, 0).is_err() {
            return; // SET_FEATURE(PORT_RESET) refused
        }
        let mut status = 0u16;
        let t0 = now();
        while now() - t0 < ms_ticks(500) {
            delay_ms(10);
            if self.control(di, 0xA3, 0, 0, port as u16, 4).is_err() {
                return;
            }
            status = u16::from_le_bytes([self.buf.byte(0), self.buf.byte(1)]);
            if self.buf.byte(2) & (1 << 4) != 0 {
                break; // C_PORT_RESET
            }
        }
        let _ = self.control(di, 0x23, 1, 20, port as u16, 0); // clear C_PORT_RESET
        let _ = self.control(di, 0x23, 1, 16, port as u16, 0); // clear C_PORT_CONNECTION
        if status & 2 == 0 {
            crate::println!("[usb] {} port {}.{}: device did not enable", self.name, label, port);
            return;
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
        let protocol = hub.hub_protocol;
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

    /// Configure a hub's status-change endpoint (interrupt IN) with the hub
    /// fields kept in the slot context.
    fn setup_hub_status(&mut self, di: usize, addr: u8, mps: u32, b_interval: u8, ports: u32, (ttt, mtt): (u32, bool)) -> Result<(), &'static str> {
        let dci = (addr as u32 & 0xF) * 2 + 1;
        let ring = Ring::new(self.limit)?;
        let buf = Page::new(self.limit)?;
        let dev = &self.devices[di];
        let interval = if dev.speed >= 3 { (b_interval.max(1) as u32 - 1).min(15) } else { (31 - ((b_interval.max(1) as u32) * 8).leading_zeros()).clamp(3, 10) };
        let s = self.csz;
        let e = (dci as usize + 1) * s;
        dev.inp.zero(0, 4096);
        dev.inp.w32(4, 1 | (1 << dci));
        dev.inp.w32(s, (dci << 27) | (1 << 26) | if mtt { 1 << 25 } else { 0 } | (dev.speed << 20) | dev.route);
        dev.inp.w32(s + 4, (ports << 24) | (dev.root_port << 16));
        dev.inp.w32(s + 8, (ttt << 16) | (dev.tt_port << 8) | dev.tt_slot);
        dev.inp.w32(e, interval << 16);
        dev.inp.w32(e + 4, (mps.max(1) << 16) | (7 << 3) | (3 << 1));
        dev.inp.w64(e + 8, ring.page.phys | 1);
        dev.inp.w32(e + 16, (mps << 16) | mps);
        dev.inp.sync(0, 4096);
        let (p, slot) = (dev.inp.phys, dev.slot);
        self.command(p, (TRB_CONFIGURE_EP << 10) | (slot << 24))?;
        let len = mps.clamp(1, 8);
        self.devices[di].intr = Some(Interrupt { dci, ring, buf, len, kind: HidKind::Hub, last: [0; 8], broken: false, reports: 0, state: 0, pad: None });
        self.queue_report(di);
        Ok(())
    }

    fn dispatch(&mut self, ev: [u32; 4]) {
        if trb_type(&ev) == EV_PORT_STATUS {
            // a root port changed (connect, disconnect, reset done): handled
            // by `service`, never inside a command or transfer wait
            let port = ev[0] >> 24;
            if port >= 1 && port <= self.max_ports && !self.root_changes.contains(&port) {
                self.root_changes.push(port);
            }
            return;
        }
        if trb_type(&ev) != EV_TRANSFER {
            return;
        }
        let slot = ev[3] >> 24;
        let dci = (ev[3] >> 16) & 0x1F;
        let audio_dev = self.devices.live().find(|(_, d)| d.slot == slot && d.audio.as_ref().is_some_and(|a| a.dci == dci)).map(|(i, _)| i);
        if let Some(di) = audio_dev {
            let cc = completion(&ev);
            let a = self.devices[di].audio.as_mut().unwrap();
            match cc {
                CC_SUCCESS | CC_SHORT => a.done += 1,
                CC_RING_UNDERRUN | CC_RING_OVERRUN => {
                    // the ring ran dry: nothing completed, refill it (SIA restarts it)
                    a.underruns += 1;
                    for _ in 0..ISO_SLOTS {
                        self.queue_audio(di);
                    }
                    return;
                }
                CC_MISSED_SERVICE => a.errors += 1,
                _ => a.errors += 1,
            }
            self.queue_audio(di);
            return;
        }
        let Some(di) = self.devices.live().find(|(_, d)| d.slot == slot && d.intr.as_ref().is_some_and(|i| i.dci == dci)).map(|(i, _)| i) else { return };
        let cc = completion(&ev);
        let i = self.devices[di].intr.as_mut().unwrap();
        if cc != CC_SUCCESS && cc != CC_SHORT {
            if !i.broken {
                crate::println!("[usb] {}: slot {} endpoint {} stopped (completion code {})", self.name, slot, dci, cc);
                i.broken = true;
            }
            return;
        }
        let got = (i.len as usize).saturating_sub((ev[2] & 0xFF_FFFF) as usize).min(64);
        i.buf.sync(0, i.len as usize);
        let mut long = [0u8; 64];
        for (k, r) in long.iter_mut().enumerate().take(got) {
            *r = i.buf.byte(k);
        }
        let mut report = [0u8; 8];
        report.copy_from_slice(&long[..8]);
        let got8 = got.min(8);
        match i.kind {
            HidKind::Gamepad => {
                if let Some(p) = i.pad.as_mut() {
                    p.feed(&long[..got]);
                }
            }
            HidKind::Keyboard => super::hid::keyboard(&i.last, &report),
            HidKind::Mouse => super::hid::mouse(&report[..got8]),
            HidKind::Hub => {
                let bits = report[..got8.min(4)].iter().enumerate().fold(0u32, |a, (k, b)| a | (*b as u32) << (8 * k));
                if bits & !1 != 0 {
                    self.hub_changes.push((di, bits & !1));
                }
            }
        }
        let i = self.devices[di].intr.as_mut().unwrap();
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

    /// Hot-plug: attach what was plugged in and detach what was pulled out,
    /// on root ports (Port Status Change events) and behind hubs (their
    /// status-change endpoints). Called by `usb::poll` after `poll`.
    pub fn service(&mut self) {
        while let Some(port) = self.root_changes.pop() {
            let a = self.portsc(port);
            let sc = rd(a);
            wr(a, (sc & PORT_PRESERVE) | PORT_CHANGES);
            let child = self.devices.live().find(|(_, d)| d.root_port == port && d.depth == 0).map(|(i, _)| i);
            // a connect change while something is attached = unplugged (and
            // maybe plugged again); without a connection there is nothing
            if let Some(di) = child {
                if sc & PORT_CCS == 0 || sc & (1 << 17) != 0 {
                    self.detach_tree(di);
                } else {
                    continue;
                }
            }
            if sc & PORT_CCS != 0 {
                delay_ms(100); // debounce: contacts settle (USB 2.0 7.1.7.3)
                if rd(a) & PORT_CCS != 0 {
                    let before = self.devices.live().count();
                    self.attach_root(port);
                    self.attached += self.devices.live().count().saturating_sub(before);
                }
            }
        }
        while let Some((hub, bits)) = self.hub_changes.pop() {
            if !self.devices.alive(hub) {
                continue;
            }
            let ports = self.devices[hub].hub_ports;
            for port in 1..=ports.min(31) {
                if bits & (1 << port) != 0 {
                    self.hub_port_changed(hub, port);
                }
            }
        }
    }

    fn hub_port_changed(&mut self, hub: usize, port: u32) {
        if self.control(hub, 0xA3, 0, 0, port as u16, 4).is_err() {
            return;
        }
        let status = u16::from_le_bytes([self.buf.byte(0), self.buf.byte(1)]);
        let change = u16::from_le_bytes([self.buf.byte(2), self.buf.byte(3)]);
        // clear every change bit that is set (C_PORT_CONNECTION .. C_PORT_RESET)
        for bit in 0..5u16 {
            if change & (1 << bit) != 0 {
                let _ = self.control(hub, 0x23, 1, 16 + bit, port as u16, 0);
            }
        }
        let h = &self.devices[hub];
        let route = h.route | (port.min(15) << (4 * h.depth));
        let (root_port, depth, label) = (h.root_port, h.depth + 1, h.label.clone());
        let child = self.devices.live().find(|(_, d)| d.root_port == root_port && d.depth == depth && d.route == route).map(|(i, _)| i);
        if let Some(di) = child {
            if status & 1 == 0 || change & 1 != 0 {
                self.detach_tree(di);
            } else {
                return;
            }
        }
        if status & 1 != 0 {
            delay_ms(100);
            if self.control(hub, 0xA3, 0, 0, port as u16, 4).is_ok() && self.buf.byte(0) & 1 != 0 {
                let before = self.devices.live().count();
                self.hub_attach(hub, &label, port);
                self.attached += self.devices.live().count().saturating_sub(before);
            }
        }
    }

    /// Detach device `di` and everything behind it (a hub's children first).
    fn detach_tree(&mut self, di: usize) {
        let (root_port, depth, route) = {
            let d = &self.devices[di];
            (d.root_port, d.depth, d.route)
        };
        let mask = if depth == 0 { 0 } else { (1u32 << (4 * depth)) - 1 };
        let mut below: Vec<(usize, u32)> = self
            .devices
            .live()
            .filter(|(i, d)| *i != di && d.root_port == root_port && d.depth > depth && d.route & mask == route)
            .map(|(i, d)| (i, d.depth))
            .collect();
        below.sort_by(|a, b| b.1.cmp(&a.1)); // deepest first
        for (i, _) in below {
            self.detach(i);
        }
        self.detach(di);
    }

    /// Forget one device: release held keys / buttons, give its slot back to
    /// the controller, free its memory, report a disk as gone.
    fn detach(&mut self, di: usize) {
        let Some(mut dev) = self.devices.0[di].take() else { return };
        if let Some(p) = dev.intr.as_mut().and_then(|i| i.pad.as_mut()) {
            p.release(); // buttons held at the moment it was unplugged
        }
        if let Some(i) = dev.intr.as_ref() {
            match i.kind {
                HidKind::Keyboard => {
                    super::hid::keyboard(&i.last, &[0; 8]);
                    self.keyboards = self.keyboards.saturating_sub(1);
                }
                HidKind::Mouse => {
                    crate::mouse::inject_relative(0, 0, 0);
                    self.mice = self.mice.saturating_sub(1);
                }
                HidKind::Gamepad => {
                    self.pads = self.pads.saturating_sub(1);
                    super::hid::PADS.fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                }
                HidKind::Hub => {}
            }
        }
        if dev.hub_ports > 0 {
            self.hubs = self.hubs.saturating_sub(1);
        }
        if dev.audio.is_some() {
            self.speakers = self.speakers.saturating_sub(1);
            super::AUDIO_DEVICES.fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
        }
        if dev.msd.is_some() {
            self.disks = self.disks.saturating_sub(1);
            self.gone_disks.push(di);
        }
        let ok = self.command(0, (TRB_DISABLE_SLOT << 10) | (dev.slot << 24)).is_ok();
        self.dcbaa.w64(dev.slot as usize * 8, 0);
        self.dcbaa.sync(dev.slot as usize * 8, 8);
        self.detached += 1;
        crate::println!(
            "[usb] {} port {}: {} unplugged (slot {} {})",
            self.name,
            dev.label,
            dev.what,
            dev.slot,
            if ok { "released" } else { "release refused" }
        );
        // `dev` drops here: rings, contexts and buffers go back to the allocator
    }

    /// Disks plugged in / unplugged since the last call:
    /// (new: (device, sectors, name), gone: device indices).
    pub fn take_disk_changes(&mut self) -> (Vec<(usize, u64, alloc::string::String)>, Vec<usize>) {
        (core::mem::take(&mut self.new_disks), core::mem::take(&mut self.gone_disks))
    }

    /// One line per device (shell `lsusb`).
    pub fn describe(&self) -> Vec<alloc::string::String> {
        self.devices
            .live()
            .map(|(_, d)| alloc::format!("{} port {:<7} slot {:>2}  {}", self.name, d.label, d.slot, d.what))
            .collect()
    }

    /// Log each HID endpoint's state (from the output device context) and
    /// report count when the state changed -- for bring-up on new hardware.
    pub fn report_endpoints(&mut self) {
        let csz = self.csz;
        let name = self.name;
        for d in self.devices.0.iter_mut().flatten() {
            let slot = d.slot;
            let Some(i) = d.intr.as_mut() else { continue };
            if i.kind == HidKind::Hub {
                continue;
            }
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
                    match i.kind {
                        HidKind::Keyboard => "keyboard",
                        HidKind::Gamepad => "gamepad",
                        _ => "mouse",
                    },
                    i.dci,
                    st,
                    i.reports
                );
                i.state = state;
            }
        }
    }
}
