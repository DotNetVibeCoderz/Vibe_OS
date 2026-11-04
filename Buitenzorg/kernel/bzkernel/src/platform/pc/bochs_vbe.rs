//! Bochs / QEMU "std VGA" display (the BGA "DISPI" interface on I/O ports
//! 0x1CE / 0x1CF): switch the boot framebuffer from 24 to 32 bits per pixel.
//!
//! The BIOS loader (bootloader 0.11) takes the first 1280x720 VESA mode
//! SeaBIOS lists, which is the 24-bit one. The compositor works in 32-bit
//! 0x00RRGGBB words, so every present had to repack each pixel into 3 bytes
//! (~1/3 of the first desktop frame under QEMU). In 32-bit BGR a present is a
//! row copy. The device keeps its linear framebuffer at PCI BAR 0, so only the
//! mapping has to grow (4 bytes per pixel instead of 3).

use crate::boot::{Framebuffer, PixelFormat};
use crate::pci::PciDevice;
use x86_64::instructions::port::Port;

const INDEX: u16 = 0x1CE;
const DATA: u16 = 0x1CF;
const REG_ID: u16 = 0;
const REG_XRES: u16 = 1;
const REG_YRES: u16 = 2;
const REG_BPP: u16 = 3;
const REG_ENABLE: u16 = 4;
const REG_VIRT_WIDTH: u16 = 6;
const REG_X_OFFSET: u16 = 8;
const REG_Y_OFFSET: u16 = 9;
const ENABLED: u16 = 0x01;
const LFB: u16 = 0x40;

fn read(reg: u16) -> u16 {
    unsafe {
        Port::<u16>::new(INDEX).write(reg);
        Port::<u16>::new(DATA).read()
    }
}

fn write(reg: u16, value: u16) {
    unsafe {
        Port::<u16>::new(INDEX).write(reg);
        Port::<u16>::new(DATA).write(value);
    }
}

/// If the screen is a Bochs/QEMU std-VGA LFB in a 24-bit mode, set the same
/// resolution at 32 bpp and return the new framebuffer description (mapped
/// afresh). None = not that device, or nothing to do.
pub fn upgrade_to_32bpp(fb: &Framebuffer, devices: &[PciDevice]) -> Option<Framebuffer> {
    if fb.bytes_per_pixel != 3 {
        return None;
    }
    // QEMU std VGA (1234:1111) or Bochs/VirtualBox BGA (80ee:beef)
    let dev = devices.iter().find(|d| {
        d.class == 0x03 && ((d.vendor_id == 0x1234 && d.device_id == 0x1111) || (d.vendor_id == 0x80EE && d.device_id == 0xBEEF))
    })?;
    let id = read(REG_ID);
    // 0xB0C2 and later take 32 bpp
    if !(0xB0C2..=0xB0CF).contains(&id) {
        return None;
    }
    let (w, h) = (fb.width, fb.height);
    if read(REG_XRES) as usize != w || read(REG_YRES) as usize != h || read(REG_BPP) != 24 {
        return None; // not the mode the loader described
    }
    let phys = dev.mem_bar(0)?;
    let len = w * h * 4;
    let va = crate::memory::map_phys_normal(phys, len as u64)?;
    write(REG_ENABLE, 0);
    write(REG_BPP, 32);
    write(REG_XRES, w as u16);
    write(REG_YRES, h as u16);
    write(REG_VIRT_WIDTH, w as u16);
    write(REG_X_OFFSET, 0);
    write(REG_Y_OFFSET, 0);
    write(REG_ENABLE, ENABLED | LFB);
    if read(REG_BPP) != 32 || read(REG_XRES) as usize != w {
        // refused: put the loader's mode back
        write(REG_ENABLE, 0);
        write(REG_BPP, 24);
        write(REG_ENABLE, ENABLED | LFB);
        return None;
    }
    Some(Framebuffer {
        addr: va,
        len,
        width: w,
        height: h,
        stride: w,
        bytes_per_pixel: 4,
        format: PixelFormat::Bgr,
    })
}
