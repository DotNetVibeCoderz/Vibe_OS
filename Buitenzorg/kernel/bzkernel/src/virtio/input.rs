//! virtio-input (§5.8): keyboards, mice and tablets. The device reports Linux
//! evdev events (type, code, value) into buffers the driver keeps posted on
//! queue 0. Keys are translated to PC set-1 scancodes and fed to the shared
//! decoder (`keyboard::feed_scancode`, the same one the PS/2 IRQ uses);
//! relative / absolute motion and buttons go to `mouse`. Polled from the
//! desktop loop.

use alloc::{boxed::Box, string::String, vec::Vec};
use spin::Mutex;

use super::{Buf, Transport, Virtqueue};

// Config space (§5.8.4).
const CFG_SELECT: usize = 0;
const CFG_SUBSEL: usize = 1;
const CFG_SIZE: usize = 2;
const CFG_DATA: usize = 8;
const ID_NAME: u8 = 1;
const EV_BITS: u8 = 0x11;
const ABS_INFO: u8 = 0x12;

// evdev event types / codes.
const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const EV_ABS: u16 = 3;
const REL_X: u16 = 0;
const REL_Y: u16 = 1;
const REL_WHEEL: u16 = 8;
const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;

const EVENTS: usize = 64;

struct InputDev {
    t: Box<dyn Transport>,
    q: Virtqueue,
    name: String,
    dma_phys: u64,
    dma: *mut u8,
    /// Descriptor head -> event buffer index.
    slot_of: [u16; 256],
    abs_max: (i64, i64),
    dx: i64,
    dy: i64,
    abs: (i64, i64),
    abs_dirty: bool,
    rel_dirty: bool,
    buttons: u8,
}

unsafe impl Send for InputDev {}

static DEVICES: Mutex<Vec<InputDev>> = Mutex::new(Vec::new());

/// Events seen so far: (keys, pointer events).
static KEYS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static POINTER: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static REPORTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn cfg_select(t: &dyn Transport, select: u8, subsel: u8) -> usize {
    t.config_write8(CFG_SELECT, select);
    t.config_write8(CFG_SUBSEL, subsel);
    t.config_read8(CFG_SIZE) as usize
}

impl InputDev {
    fn new(mut t: Box<dyn Transport>) -> Result<InputDev, &'static str> {
        super::begin_init(t.as_mut(), 0)?;
        let q = Virtqueue::new(t.as_mut(), 0)?;
        super::finish_init(t.as_mut());
        let n = cfg_select(t.as_ref(), ID_NAME, 0);
        let raw: String = (0..n.min(64)).map(|i| t.config_read8(CFG_DATA + i) as char).collect();
        let name = String::from(raw.trim_end_matches(|c: char| c == ' ' || c == '\0'));
        let abs_axis = |t: &dyn Transport, axis: u8| -> i64 {
            if cfg_select(t, ABS_INFO, axis) >= 8 {
                t.config_read32(CFG_DATA + 4) as i64 // max (min at +0)
            } else {
                0
            }
        };
        let has = |t: &dyn Transport, ev: u16| cfg_select(t, EV_BITS, ev as u8) > 0;
        let (keys, rel, abs) = (has(t.as_ref(), EV_KEY), has(t.as_ref(), EV_REL), has(t.as_ref(), EV_ABS));
        let abs_max = (abs_axis(t.as_ref(), ABS_X as u8), abs_axis(t.as_ref(), ABS_Y as u8));
        let (dma_phys, dma) = crate::memory::alloc_dma_frame().ok_or("virtio-input: out of memory")?;
        crate::println!(
            "[virtio] input at {}: \"{}\" ({}{}{}{})",
            t.location(),
            name,
            if keys { "keys " } else { "" },
            if rel { "relative " } else { "" },
            if abs { "absolute " } else { "" },
            if abs { alloc::format!("{}x{}", abs_max.0, abs_max.1) } else { String::new() },
        );
        let mut d = InputDev {
            t,
            q,
            name,
            dma_phys,
            dma,
            slot_of: [0; 256],
            abs_max,
            dx: 0,
            dy: 0,
            abs: (0, 0),
            abs_dirty: false,
            rel_dirty: false,
            buttons: 0,
        };
        for i in 0..EVENTS.min(d.q.size() as usize) {
            d.post(i)?;
        }
        d.t.notify(0);
        Ok(d)
    }

    /// Hand event buffer `i` (8 bytes) to the device.
    fn post(&mut self, i: usize) -> Result<(), &'static str> {
        let head = self.q.push(&[Buf { phys: self.dma_phys + (i * 8) as u64, len: 8, device_writes: true }])?;
        self.slot_of[head as usize] = i as u16;
        Ok(())
    }

    fn handle(&mut self, kind: u16, code: u16, value: u32) {
        match kind {
            EV_KEY => match code {
                BTN_LEFT | BTN_RIGHT | BTN_MIDDLE => {
                    let bit = 1 << (code - BTN_LEFT);
                    if value != 0 {
                        self.buttons |= bit;
                    } else {
                        self.buttons &= !bit;
                    }
                    self.rel_dirty = true;
                }
                _ => {
                    if let Some((e0, sc)) = scancode(code) {
                        // value: 1 press, 2 autorepeat, 0 release.
                        let byte = if value == 0 { sc | 0x80 } else { sc };
                        if e0 {
                            crate::keyboard::feed_scancode(0xE0);
                        }
                        crate::keyboard::feed_scancode(byte);
                        KEYS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    }
                }
            },
            EV_REL => {
                match code {
                    REL_X => self.dx += value as i32 as i64,
                    REL_Y => self.dy += value as i32 as i64,
                    REL_WHEEL => {
                        crate::mouse::inject_wheel(value as i32 as i64);
                        POINTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                        return;
                    }
                    _ => {}
                }
                self.rel_dirty = true;
            }
            EV_ABS => {
                match code {
                    ABS_X => self.abs.0 = value as i32 as i64,
                    ABS_Y => self.abs.1 = value as i32 as i64,
                    _ => {}
                }
                self.abs_dirty = true;
            }
            EV_SYN => {
                if self.abs_dirty {
                    crate::mouse::inject_absolute(self.abs.0, self.abs.1, self.abs_max.0, self.abs_max.1, self.buttons);
                    POINTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                } else if self.rel_dirty {
                    crate::mouse::inject_relative(self.dx, self.dy, self.buttons);
                    POINTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                }
                self.dx = 0;
                self.dy = 0;
                self.abs_dirty = false;
                self.rel_dirty = false;
            }
            _ => {}
        }
    }

    fn poll(&mut self) {
        let mut reposted = false;
        while let Some((head, _len)) = self.q.pop_used() {
            let i = self.slot_of[head as usize] as usize;
            let e = unsafe { self.dma.add(i * 8) };
            let (kind, code, value) = unsafe {
                (
                    (e as *const u16).read_volatile(),
                    (e.add(2) as *const u16).read_volatile(),
                    (e.add(4) as *const u32).read_volatile(),
                )
            };
            self.handle(kind, code, value);
            if self.post(i).is_ok() {
                reposted = true;
            }
        }
        if reposted {
            self.t.notify(0);
        }
    }
}

/// Linux key code -> PC set-1 scancode (`true` = needs the 0xE0 prefix).
/// Codes 1..=88 are the set-1 make codes themselves (evdev numbered keys that
/// way); the navigation block and right-hand modifiers are extended keys.
fn scancode(code: u16) -> Option<(bool, u8)> {
    match code {
        1..=88 => Some((false, code as u8)),
        96 => Some((true, 0x1C)),  // keypad enter
        97 => Some((true, 0x1D)),  // right ctrl
        98 => Some((true, 0x35)),  // keypad /
        100 => Some((true, 0x38)), // right alt
        102 => Some((true, 0x47)), // home
        103 => Some((true, 0x48)), // up
        104 => Some((true, 0x49)), // page up
        105 => Some((true, 0x4B)), // left
        106 => Some((true, 0x4D)), // right
        107 => Some((true, 0x4F)), // end
        108 => Some((true, 0x50)), // down
        109 => Some((true, 0x51)), // page down
        110 => Some((true, 0x52)), // insert
        111 => Some((true, 0x53)), // delete
        _ => None,
    }
}

/// Find and start every virtio input device.
pub fn init() {
    let mut devs = DEVICES.lock();
    for t in super::find(super::device::INPUT) {
        let loc = t.location();
        match InputDev::new(t) {
            Ok(d) => devs.push(d),
            Err(e) => crate::println!("[virtio] input at {}: {}", loc, e),
        }
    }
}

/// Drain pending events of every device (desktop loop). Reports once that
/// both a key and a pointer event arrived through virtio.
pub fn poll() {
    let Some(mut devs) = DEVICES.try_lock() else { return };
    if devs.is_empty() {
        return;
    }
    for d in devs.iter_mut() {
        d.poll();
    }
    drop(devs);
    let k = KEYS.load(core::sync::atomic::Ordering::Relaxed);
    let p = POINTER.load(core::sync::atomic::Ordering::Relaxed);
    if k > 0 && p > 0 && !REPORTED.swap(true, core::sync::atomic::Ordering::Relaxed) {
        let names: Vec<String> = DEVICES.lock().iter().map(|d| d.name.clone()).collect();
        crate::println!(
            "[kernel] MILESTONE: VIRTIO INPUT OK ({} key event(s) + {} pointer event(s) delivered by {})",
            k,
            p,
            names.join(", ")
        );
    }
}
