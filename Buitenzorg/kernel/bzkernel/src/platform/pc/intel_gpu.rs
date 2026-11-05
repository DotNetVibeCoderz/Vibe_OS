//! Intel integrated graphics, step 1 of GPU support: identify the GPU and
//! read how the firmware set it up -- stolen memory, the global GTT, the
//! display plane that scans out the boot framebuffer -- without writing a
//! single register (the firmware's display is what is on the screen). The
//! check that matters for the next steps (the blitter, then 3D): the plane's
//! surface address, looked up in the GGTT, must land on the framebuffer the
//! loader handed over. Shell `gpu` prints the result.

use alloc::{format, string::String, vec::Vec};
use core::ptr::read_volatile;
use spin::Mutex;

use crate::pci::PciDevice;

static INFO: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// (generation, family, name) from the PCI device id.
fn identify(id: u16) -> (&'static str, &'static str, &'static str) {
    match id {
        0x5917 => ("9.5", "Kaby Lake-R GT2", "UHD Graphics 620"),
        0x5916 => ("9.5", "Kaby Lake GT2", "HD Graphics 620"),
        0x5912 => ("9.5", "Kaby Lake GT2", "HD Graphics 630"),
        0x591B => ("9.5", "Kaby Lake GT2", "HD Graphics 630"),
        0x3EA0 => ("9.5", "Whiskey Lake GT2", "UHD Graphics 620"),
        0x3E92 | 0x3E91 | 0x3E98 => ("9.5", "Coffee Lake GT2", "UHD Graphics 630"),
        0x9B41 | 0x9BC4 | 0x9BC5 | 0x9BC8 => ("9.5", "Comet Lake GT2", "UHD Graphics"),
        0x1916 => ("9", "Skylake GT2", "HD Graphics 520"),
        0x1912 => ("9", "Skylake GT2", "HD Graphics 530"),
        0x8A52 | 0x8A56 | 0x8A5A => ("11", "Ice Lake", "Iris Plus / UHD"),
        0x9A49 | 0x9A40 | 0x9A78 => ("12", "Tiger Lake", "Iris Xe / UHD"),
        0x4680 | 0x4692 | 0x46A6 | 0x46A8 => ("12", "Alder Lake", "UHD / Iris Xe"),
        _ => match id >> 8 {
            0x19 => ("9", "Skylake", "HD Graphics"),
            0x59 | 0x3E | 0x9B | 0x87 => ("9.5", "Kaby/Coffee Lake", "UHD Graphics"),
            0x8A => ("11", "Ice Lake", "Graphics"),
            0x9A | 0x46 | 0x4C | 0xA7 => ("12", "Xe", "Graphics"),
            _ => ("?", "unknown generation", "Intel graphics"),
        },
    }
}

fn probe(dev: &PciDevice) -> Vec<String> {
    let (gen, family, name) = identify(dev.device_id);
    let mut out = Vec::new();
    out.push(format!("{} -- 8086:{:04x}, {} (Gen{})", name, dev.device_id, family, gen));
    let gen9 = gen.starts_with('9');
    // GMCH control (PCI 0x50): stolen memory and GTT size; base of stolen memory (0x5C)
    let gmch = dev.read16(0x50) as u32;
    let gms = (gmch >> 8) & 0xFF;
    let stolen_mib = if gms <= 0x10 { gms * 32 } else if gms >= 0xF0 { (gms - 0xF0 + 1) * 4 } else { gms * 32 };
    let gtt_mib = match (gmch >> 6) & 3 {
        0 => 0,
        g => 1 << g,
    };
    let bsm = dev.read32(0x5C) & !0xFFFFF;
    out.push(format!("  stolen memory {} MiB at {:#x}, global GTT {} MiB", stolen_mib, bsm, gtt_mib));
    let (Some(mmio), Some(aperture)) = (dev.mem_bar(0), dev.mem_bar(2)) else {
        out.push(String::from("  no memory BARs"));
        return out;
    };
    out.push(format!("  registers + GTT at {:#x}, aperture at {:#x}", mmio, aperture));
    if !gen9 {
        out.push(String::from("  display registers not read: only Gen9 (Skylake to Comet Lake) is mapped out so far"));
        return out;
    }
    // registers: the first 2 MiB; the GGTT: the upper half of the 16 MiB BAR
    let Some(regs) = crate::memory::map_mmio(mmio, 2 << 20) else { return out };
    let Some(gtt) = crate::memory::map_mmio(mmio + (8 << 20), 8 << 20) else { return out };
    let rd = |o: u64| unsafe { read_volatile((regs + o) as *const u32) };
    // pipe A source size, plane 1A control/stride/surface (display engine, powered by the firmware)
    let pipesrc = rd(0x6001C);
    let (w, h) = ((pipesrc >> 16) + 1, (pipesrc & 0xFFFF) + 1);
    let plane_ctl = rd(0x70180);
    let stride = rd(0x70188) & 0x3FF;
    let surf = rd(0x7019C) & !0xFFF;
    out.push(format!(
        "  pipe A {}x{}, plane 1A {} (control {:#010x}), stride {} x 64 bytes, surface at GGTT {:#x}",
        w,
        h,
        if plane_ctl & (1 << 31) != 0 { "on" } else { "off" },
        plane_ctl,
        stride,
        surf
    ));
    // the plane's first page in the GGTT (64-bit entries, bit 0 = present)
    let pte_at = gtt + (surf as u64 >> 12) * 8;
    let pte = unsafe { read_volatile(pte_at as *const u32) as u64 | (read_volatile((pte_at + 4) as *const u32) as u64) << 32 };
    let page = pte & 0x7F_FFFF_F000;
    let fb_phys = crate::framebuffer::physical_hint();
    // the firmware's framebuffer is either the backing page itself or, as GOP
    // usually does, the aperture window at the plane's GGTT offset (the
    // aperture is the GGTT seen through BAR2)
    let verdict = match fb_phys {
        Some(p) if p & !0xFFF == page => "GGTT OK: = the boot framebuffer",
        Some(p) if p >= aperture && p - aperture == surf as u64 => "GGTT OK: = boot framebuffer via aperture",
        Some(_) => "GGTT ?: not the boot framebuffer",
        None => "",
    };
    out.push(format!(
        "  GGTT entry {:#018x}: page {:#x} {} {}",
        pte,
        page,
        if pte & 1 != 0 { "present" } else { "NOT present" },
        verdict
    ));
    if let Some(p) = fb_phys {
        out.push(format!("  boot framebuffer at {:#x} (aperture {:#x} + {:#x})", p, aperture, p.wrapping_sub(aperture)));
    }
    // linear (tiling bits 12:10 = 0) XRGB8888 (format 4 in bits 27:24): the blitter can draw on it
    if plane_ctl & (1 << 31) != 0 && plane_ctl >> 24 & 0xF == 4 && plane_ctl >> 10 & 7 == 0 && pte & 1 != 0 {
        super::intel_blt::configure(regs, gtt, surf, stride * 64, w, h);
        out.push(String::from("  blitter: ready for 'gpu blit' (step 2 self-test)"));
    }
    out
}

/// Look for an Intel display controller and record what it is.
pub fn init(devices: &[PciDevice]) {
    let Some(dev) = devices.iter().find(|d| d.vendor_id == 0x8086 && d.class == 0x03) else { return };
    let lines = probe(dev);
    for l in &lines {
        crate::println!("[gpu] {}", l.trim_start());
    }
    crate::println!("[gpu] drawing still goes through the CPU (GPU step 2 of 3: blitter self-test with 'gpu blit')");
    *INFO.lock() = lines;
}

/// The lines `init` found, for the shell's `gpu`.
pub fn info() -> Vec<String> {
    let v = INFO.lock().clone();
    if v.is_empty() {
        alloc::vec![String::from("gpu: no Intel graphics found")]
    } else {
        v
    }
}
