//! VideoCore VI V3D (Raspberry Pi 4) bring-up for G3, after Linux's `v3d`
//! driver (`v3d_drv.c`, `v3d_regs.h`) and `raspberrypi-power`.
//!
//! What this does: find the `brcm,2711-v3d` node in the Device Tree (hub +
//! core0 register blocks, through the `/v3dbus` ranges), power the V3D
//! domain on through the firmware (mailbox SET_DOMAIN_STATE, domain 10), set
//! the V3D clock (id 5) to its maximum, map the registers and read the
//! identification registers: the hub's IDENT1 gives the technology version,
//! revision, core count and the TFU / L3 cache / MMU options, core 0's IDENT0
//! carries the "V3D" magic and IDENT1 the slice and QPU counts.
//!
//! What it does not do yet: run jobs. Rendering on V3D needs binning and
//! render control lists plus QPU shaders (Mesa's `v3d` compiler output); the
//! 3D API keeps using the software rasterizer on the Pi until that backend
//! exists. QEMU `raspi4b` does not model V3D, so every register access goes
//! through `probe_read32` and an absent block is reported, not faulted on;
//! the identification has to be confirmed on a board.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::arch::aarch64::exceptions::probe_read32;
use crate::arch::aarch64::rpi::firmware_property;

const SET_DOMAIN_STATE: u32 = 0x0003_8030;
const GET_MAX_CLOCK_RATE: u32 = 0x0003_0004;
const SET_CLOCK_RATE: u32 = 0x0003_8002;
const DOMAIN_V3D: u32 = 10;
const CLOCK_V3D: u32 = 5;

// hub registers
const HUB_IDENT0: u64 = 0x08;
const HUB_IDENT1: u64 = 0x0c;
const HUB_IDENT2: u64 = 0x10;
const HUB_IDENT3: u64 = 0x14;
// core registers
const CTL_IDENT0: u64 = 0x00;
const CTL_IDENT1: u64 = 0x04;
const CTL_IDENT2: u64 = 0x08;

/// What was found: version * 10 + revision (42 = V3D 4.2), 0 = none.
static VERSION: AtomicU32 = AtomicU32::new(0);

/// Identification of the V3D on this board.
pub struct Info {
    pub version: u32,
    pub cores: u32,
    pub slices: u32,
    pub qpus_per_slice: u32,
    pub tfu: bool,
    pub l3c: bool,
    pub mmu: bool,
    pub clock_hz: u32,
    pub ident: [u32; 7],
}

/// V3D version * 10 + revision when one was identified (shell `gfx3d`).
pub fn version() -> u32 {
    VERSION.load(Ordering::Relaxed)
}

/// Bring V3D up as far as identification. `None` when the board has none or
/// it does not answer (QEMU).
pub fn init(f: &crate::dtb::Fdt) -> Option<Info> {
    let i = f.compatible("brcm,2711-v3d").next()?;
    let regs = f.reg(i);
    // reg-names says which block is which ("hub", "core0"); else hub is the lower address
    let names: alloc::vec::Vec<&str> = f.nodes[i].strings("reg-names").collect();
    let by = |n: &str| names.iter().position(|s| *s == n).and_then(|k| regs.get(k).copied());
    let (hub, core) = match (by("hub"), by("core0")) {
        (Some(h), Some(c)) => (h, c),
        _ if regs.len() >= 2 => {
            let (a, b) = (regs[0], regs[1]);
            if a.0 < b.0 { (a, b) } else { (b, a) }
        }
        _ => return None,
    };
    // power and clock (the firmware answers on a board; QEMU leaves them alone)
    let mut dom = [DOMAIN_V3D, 1];
    let powered = firmware_property(SET_DOMAIN_STATE, &mut dom);
    let mut max = [CLOCK_V3D, 0];
    let mut clock_hz = 0;
    if firmware_property(GET_MAX_CLOCK_RATE, &mut max) && max[1] != 0 {
        let mut set = [CLOCK_V3D, max[1], 0];
        if firmware_property(SET_CLOCK_RATE, &mut set) {
            clock_hz = set[1];
        }
    }
    let hub_va = crate::memory::map_mmio(hub.0, hub.1.max(0x1000))?;
    let core_va = crate::memory::map_mmio(core.0, core.1.max(0x1000))?;
    let Some(c0) = probe_read32(core_va + CTL_IDENT0) else {
        crate::println!(
            "[v3d] V3D registers at {:#x} / {:#x} do not answer (not emulated by QEMU raspi4b) -- 3D stays in software",
            hub.0, core.0
        );
        return None;
    };
    let rd = |va: u64, off: u64| probe_read32(va + off).unwrap_or(0);
    let ident = [
        rd(hub_va, HUB_IDENT0),
        rd(hub_va, HUB_IDENT1),
        rd(hub_va, HUB_IDENT2),
        rd(hub_va, HUB_IDENT3),
        c0,
        rd(core_va, CTL_IDENT1),
        rd(core_va, CTL_IDENT2),
    ];
    let h1 = ident[1];
    let tver = h1 & 0xF;
    let rev = (h1 >> 4) & 0xF;
    let info = Info {
        version: tver * 10 + rev,
        cores: (h1 >> 8) & 0xF,
        slices: (ident[5] >> 4) & 0xF,
        qpus_per_slice: (ident[5] >> 8) & 0xF,
        tfu: h1 & (1 << 17) != 0,
        l3c: h1 & (1 << 16) != 0,
        mmu: ident[2] & (1 << 4) != 0,
        clock_hz,
        ident,
    };
    let magic = c0 & 0x00FF_FFFF == 0x0044_3356; // "V3D"
    crate::println!(
        "[v3d] hub {:#x} core0 {:#x}: power {} clock {} MHz, ident {:08x?}, magic {}",
        hub.0,
        core.0,
        if powered { "on" } else { "?" },
        clock_hz / 1_000_000,
        ident,
        if magic { "V3D" } else { "missing" }
    );
    if !magic || info.version == 0 {
        return None;
    }
    VERSION.store(info.version, Ordering::Relaxed);
    Some(info)
}
