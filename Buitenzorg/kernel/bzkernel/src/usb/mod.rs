//! USB (roadmap R4): xHCI host controllers, USB 2.0 hubs and HID boot-protocol
//! keyboards and mice and mass storage, in generic code. A PC finds its controllers on PCI
//! (`init_pci`); a Raspberry Pi 4 brings up its PCIe root complex and the
//! VL805 behind it first (`platform::virt::brcm_pcie`), then calls
//! `add_controller`. The desktop loop calls `poll`.
//!
//! Hot-plug: `poll` also attaches devices plugged in after boot and detaches
//! the ones pulled out -- on root ports (xHCI Port Status Change events) and
//! behind hubs (each hub's status-change endpoint). A stick plugged in later
//! is mounted at `/usb0`, `/usb1`, ... and unmounted when it is pulled out.

pub mod hid;
pub mod xhci;

use alloc::{boxed::Box, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

static CONTROLLERS: Mutex<Vec<xhci::Xhci>> = Mutex::new(Vec::new());
static INPUT_REPORTED: AtomicBool = AtomicBool::new(false);
static LAST_REPORT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static REPORTS_LEFT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(12);

// ---- USB audio (Audio Class 1 output, isochronous) -----------------------

/// USB audio outputs attached now.
pub static AUDIO_DEVICES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
/// The USB output plays the mixer (set when the machine has no other sound card).
pub static AUDIO_SINK: AtomicBool = AtomicBool::new(false);
/// Output volume 0..=100 and mute.
pub static AUDIO_VOLUME: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(80);
pub static AUDIO_MUTED: AtomicBool = AtomicBool::new(false);

pub fn audio_present() -> bool {
    AUDIO_DEVICES.load(Ordering::Relaxed) > 0
}

/// Whether USB audio is the machine's sound output (mixer sink).
pub fn audio_is_sink() -> bool {
    audio_present() && AUDIO_SINK.load(Ordering::Relaxed)
}

/// Sample scale in 1/256 (volume, mute).
pub(crate) fn audio_gain() -> i32 {
    if AUDIO_MUTED.load(Ordering::Relaxed) {
        0
    } else {
        (AUDIO_VOLUME.load(Ordering::Relaxed).min(100) * 256 / 100) as i32
    }
}

/// Keep the USB audio stream fed (refills happen in the event handler).
pub fn audio_pump() {
    if let Some(mut list) = CONTROLLERS.try_lock() {
        for c in list.iter_mut() {
            c.poll();
        }
    }
}

/// Boot check (`USB AUDIO OK`): a 440 Hz tone for 300 ms on every USB
/// audio output, polled until the controller completed 250 packets.
pub fn audio_self_test() -> Option<Result<alloc::string::String, alloc::string::String>> {
    if !audio_present() {
        return None;
    }
    let before = {
        let mut list = CONTROLLERS.lock();
        let b = list.iter().fold((0, 0, 0, 0), |t, c| {
            let s = c.audio_stats();
            (t.0 + s.0, t.1 + s.1, t.2 + s.2, t.3 + s.3)
        });
        for c in list.iter_mut() {
            c.audio_tone(440, 300);
        }
        b
    };
    let t0 = crate::arch::cpu::nanos();
    let mut now = before;
    while crate::arch::cpu::nanos() - t0 < 3_000_000_000 {
        let mut list = CONTROLLERS.lock();
        now = (0, 0, 0, 0);
        for c in list.iter_mut() {
            c.poll();
            let s = c.audio_stats();
            now = (now.0 + s.0, now.1 + s.1, now.2 + s.2, now.3 + s.3);
        }
        drop(list);
        if now.0 - before.0 >= 250 {
            break;
        }
        core::hint::spin_loop();
    }
    let ms = (crate::arch::cpu::nanos() - t0) / 1_000_000;
    let done = now.0 - before.0;
    let msg = alloc::format!(
        "{} output(s), 48000 Hz 2 ch 16-bit isochronous: {} packets of 1 ms in {} ms, {} error(s), {} underrun(s){}",
        AUDIO_DEVICES.load(Ordering::Relaxed),
        done,
        ms,
        now.1 - before.1,
        now.2 - before.2,
        if audio_is_sink() { ", the mixer's output for now" } else { "" }
    );
    Some(if done >= 250 { Ok(msg) } else { Err(msg) })
}

/// Start the xHCI controller mapped at `mmio`, enumerate its devices and keep
/// it for `poll`. `dma_limit` = the first bus address the controller cannot
/// reach. Returns (keyboards, mice).
pub fn add_controller(mmio: u64, dma_limit: u64, name: &'static str) -> Result<(usize, usize), &'static str> {
    let mut c = xhci::Xhci::new(mmio, dma_limit, name)?;
    crate::println!("[usb] {}: xHCI running, {} ports", name, c.ports());
    c.scan();
    let (k, m, h, n, d) = (c.keyboards, c.mice, c.hubs, c.device_count(), c.disks);
    if n > 0 {
        crate::println!(
            "[kernel] MILESTONE: USB OK ({}: xHCI, {} device(s): {} hub(s), {} keyboard(s), {} mouse/mice, {} disk(s); HID boot protocol)",
            name,
            n,
            h,
            k,
            m,
            d
        );
    } else {
        crate::println!("[usb] {}: no devices connected", name);
    }
    let disks = c.disks_found();
    // provisional: the platform's audio init (which may run later) decides
    if c.speakers > 0 && !crate::audio::has_card() {
        AUDIO_SINK.store(true, Ordering::Relaxed);
    }
    let controller = {
        let mut list = CONTROLLERS.lock();
        list.push(c);
        list.len() - 1
    };
    for (device, sectors, name) in disks {
        crate::driver::register_block_device(Box::new(UsbDisk { controller, device, sectors, name }));
    }
    Ok((k, m))
}

/// A USB stick or disk. `/disk` can live here (a PC booted from a stick), so
/// file operations reach the controller while the desktop polls it for input:
/// `poll` only try_locks, and this lock is held for one command.
struct UsbDisk {
    controller: usize,
    device: usize,
    sectors: u64,
    name: String,
}

impl crate::driver::BlockDevice for UsbDisk {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read_sector(&mut self, lba: u64, buf: &mut [u8; crate::driver::SECTOR_SIZE]) -> Result<(), &'static str> {
        self.read_sectors(lba, buf)
    }

    fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        if lba + (buf.len() / crate::driver::SECTOR_SIZE) as u64 > self.sectors {
            return Err("USB disk: read past the end");
        }
        CONTROLLERS.lock()[self.controller].msd_read(self.device, lba, buf)
    }

    fn write_sector(&mut self, lba: u64, buf: &[u8; crate::driver::SECTOR_SIZE]) -> Result<(), &'static str> {
        if lba >= self.sectors {
            return Err("USB disk: write past the end");
        }
        CONTROLLERS.lock()[self.controller].msd_write(self.device, lba, buf)
    }
}

/// Every xHCI controller on PCI (class 0C/03, interface 30).
#[cfg(target_arch = "x86_64")]
pub fn init_pci() -> usize {
    let mut found = 0;
    for dev in crate::pci::scan() {
        if (dev.class, dev.subclass, dev.prog_if) != (0x0C, 0x03, 0x30) {
            continue;
        }
        let Some(bar) = dev.mem_bar(0) else { continue };
        dev.enable_mem_and_bus_master();
        let Some(va) = crate::memory::map_mmio(bar, 0x10000) else { continue };
        match add_controller(va, u64::MAX, "xhci-pci") {
            Ok(_) => found += 1,
            Err(e) => crate::println!("[usb] xHCI {:04x}:{:04x} at {:#x}: {}", dev.vendor_id, dev.device_id, bar, e),
        }
    }
    found
}

/// Hot-plugged disks that are mounted: (controller, device, mount name).
static HOT_MOUNTS: Mutex<Vec<(usize, usize, String)>> = Mutex::new(Vec::new());

/// Process reports from keyboards and mice and hot-plug changes (the desktop
/// loop calls this).
pub fn poll() {
    let mut new_disks = Vec::new();
    let mut gone_disks = Vec::new();
    if let Some(mut list) = CONTROLLERS.try_lock() {
        for (ci, c) in list.iter_mut().enumerate() {
            c.poll();
            c.service();
            let (n, g) = c.take_disk_changes();
            new_disks.extend(n.into_iter().map(|d| (ci, d)));
            gone_disks.extend(g.into_iter().map(|d| (ci, d)));
        }
        // endpoint states every 5 s, a dozen times, for hardware bring-up
        let now = crate::arch::cpu::timestamp();
        let hz = crate::arch::cpu::counter_hz().max(1);
        if !list.is_empty() && REPORTS_LEFT.load(Ordering::Relaxed) > 0 && now - LAST_REPORT.load(Ordering::Relaxed) > 5 * hz {
            LAST_REPORT.store(now, Ordering::Relaxed);
            REPORTS_LEFT.fetch_sub(1, Ordering::Relaxed);
            for c in list.iter_mut() {
                c.report_endpoints();
            }
        }
    }
    // mounts happen with the controller list unlocked: every sector the
    // FAT driver reads takes it
    for (controller, device) in gone_disks {
        let mut mounts = HOT_MOUNTS.lock();
        if let Some(k) = mounts.iter().position(|m| m.0 == controller && m.1 == device) {
            let (_, _, name) = mounts.remove(k);
            drop(mounts);
            let ok = crate::vfs::unmount(&name);
            crate::println!("[usb] /{} unmounted (the disk was unplugged){}", name, if ok { "" } else { " -- it was not mounted" });
        }
    }
    for (controller, (device, sectors, name)) in new_disks {
        let mount = {
            let mounts = crate::vfs::mounts();
            (0..16).map(|n| alloc::format!("usb{}", n)).find(|m| !mounts.iter().any(|x| x.trim_start_matches('/') == m)).unwrap_or_else(|| String::from("usb"))
        };
        let disk = Box::new(UsbDisk { controller, device, sectors, name });
        match crate::vfs::mount(&mount, disk, false) {
            Ok(()) => {
                crate::println!("[kernel] MILESTONE: USB HOTPLUG DISK OK (a disk plugged in after boot is mounted at /{})", mount);
                crate::notify::post("USB", "Disk USB terpasang", &alloc::format!("dibuka di /{}", mount));
                HOT_MOUNTS.lock().push((controller, device, mount));
            }
            Err(e) => crate::println!("[usb] hot-plugged disk not mounted: {}", e),
        }
    }
    let (k, p) = (hid::KEYS.load(Ordering::Relaxed), hid::POINTER.load(Ordering::Relaxed));
    if k > 0 && p > 0 && !INPUT_REPORTED.swap(true, Ordering::Relaxed) {
        crate::println!("[kernel] MILESTONE: USB INPUT OK (a key and a pointer movement arrived from USB HID devices)");
    }
}

/// Shell `lsusb`: every USB device the controllers know, and hot-plug counts.
pub fn describe() -> Vec<String> {
    let list = CONTROLLERS.lock();
    let mut out = Vec::new();
    for c in list.iter() {
        out.extend(c.describe());
        out.push(alloc::format!(
            "{}: {} device(s); after boot {} plugged in, {} unplugged",
            c.name,
            c.device_count(),
            c.attached,
            c.detached
        ));
    }
    if out.is_empty() {
        out.push(String::from("no USB controller"));
    }
    out
}
