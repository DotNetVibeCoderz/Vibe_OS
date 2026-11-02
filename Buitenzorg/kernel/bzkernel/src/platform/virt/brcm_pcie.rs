//! The Raspberry Pi 4's PCIe root complex (`brcm,bcm2711-pcie`, roadmap R4)
//! and the one device behind it, the VL805 USB 3 controller, which carries
//! every USB port of the board (USB 2 ports through a hub inside the VL805).
//!
//! The bring-up follows Linux's `pcie-brcmstb`: reset the bridge, power the
//! SerDes, open the inbound window (PCI address = physical address for the
//! first 4 GiB; the SoC bus reaches only the first 3 GiB, hence `DMA_LIMIT`),
//! release PERST#, wait for the link, then map the outbound window (PCI
//! 0xc000_0000 is CPU 0x6_0000_0000 on the Pi 4) and turn on spread-spectrum
//! clocking. Config space: bus 0 is the root port itself (its registers sit at
//! the start of the block), bus 1 is reached through an index/data pair.
//! There is no interrupt or MSI setup: the xHCI driver polls.

use crate::arch::cpu;

/// The first address the PCIe bus master cannot reach (Pi 4: 3 GiB).
pub const DMA_LIMIT: u64 = 0xC000_0000;

const RC_CFG_VENDOR_SPECIFIC_REG1: u64 = 0x0188;
const RC_CFG_PRIV1_ID_VAL3: u64 = 0x043c;
const RC_LINK_STATUS: u64 = 0x00be; // PCIe capability at 0xac + LNKSTA (0x12)
const RC_DL_MDIO_ADDR: u64 = 0x1100;
const RC_DL_MDIO_WR_DATA: u64 = 0x1104;
const RC_DL_MDIO_RD_DATA: u64 = 0x1108;
const MISC_CTRL: u64 = 0x4008;
const MISC_MEM_WIN0_LO: u64 = 0x400c;
const MISC_MEM_WIN0_HI: u64 = 0x4010;
const MISC_RC_BAR1_CONFIG_LO: u64 = 0x402c;
const MISC_RC_BAR2_CONFIG_LO: u64 = 0x4034;
const MISC_RC_BAR2_CONFIG_HI: u64 = 0x4038;
const MISC_RC_BAR3_CONFIG_LO: u64 = 0x403c;
const MISC_PCIE_STATUS: u64 = 0x4068;
const MISC_REVISION: u64 = 0x406c;
const MISC_MEM_WIN0_BASE_LIMIT: u64 = 0x4070;
const MISC_MEM_WIN0_BASE_HI: u64 = 0x4080;
const MISC_MEM_WIN0_LIMIT_HI: u64 = 0x4084;
const MISC_HARD_DEBUG: u64 = 0x4204;
const INTR2_CPU: u64 = 0x4300;
const EXT_CFG_DATA: u64 = 0x8000;
const EXT_CFG_INDEX: u64 = 0x9000;
const RGR1_SW_INIT_1: u64 = 0x9210;

const STATUS_PHYLINKUP: u32 = 0x10;
const STATUS_DL_ACTIVE: u32 = 0x20;
const STATUS_RC_MODE: u32 = 0x80;
const HARD_DEBUG_CLKREQ: u32 = 0x2;
const HARD_DEBUG_SERDES_IDDQ: u32 = 0x0800_0000;

fn delay_us(us: u64) {
    let hz = cpu::counter_hz().max(1);
    let t0 = cpu::timestamp();
    while cpu::timestamp() - t0 < us * hz / 1_000_000 {
        core::hint::spin_loop();
    }
}

pub struct Pcie {
    base: u64,
}

impl Pcie {
    fn r(&self, off: u64) -> u32 {
        unsafe { ((self.base + off) as *const u32).read_volatile() }
    }
    fn w(&self, off: u64, v: u32) {
        unsafe { ((self.base + off) as *mut u32).write_volatile(v) }
    }
    fn modify(&self, off: u64, clear: u32, set: u32) {
        self.w(off, (self.r(off) & !clear) | set);
    }

    fn mdio_write(&self, reg: u32, data: u32) -> bool {
        self.w(RC_DL_MDIO_ADDR, reg & 0xFFFF); // port 0, command write (0)
        let _ = self.r(RC_DL_MDIO_ADDR);
        self.w(RC_DL_MDIO_WR_DATA, 0x8000_0000 | data);
        (0..100).any(|_| {
            delay_us(10);
            self.r(RC_DL_MDIO_WR_DATA) & 0x8000_0000 == 0
        })
    }
    fn mdio_read(&self, reg: u32) -> Option<u32> {
        self.w(RC_DL_MDIO_ADDR, (1 << 20) | (reg & 0xFFFF)); // command read
        let _ = self.r(RC_DL_MDIO_ADDR);
        for _ in 0..100 {
            let d = self.r(RC_DL_MDIO_RD_DATA);
            if d & 0x8000_0000 != 0 {
                return Some(d & 0x7FFF_FFFF);
            }
            delay_us(10);
        }
        None
    }

    /// Spread-spectrum clocking on the reference clock (the DT asks for it).
    fn enable_ssc(&self) -> bool {
        if !self.mdio_write(0x1f, 0x1100) {
            return false;
        }
        let Some(c) = self.mdio_read(0x2) else { return false };
        if !self.mdio_write(0x2, c | 0x8000 | 0x4000) {
            return false;
        }
        delay_us(2000);
        self.mdio_read(0x1).map_or(false, |s| s & 0xC00 == 0xC00) // SSC on + PLL locked
    }

    /// Config space of bus/device/function (bus 0 = only the root port).
    fn cfg_addr(&self, bus: u8, dev: u8, func: u8, reg: u16) -> Option<u64> {
        if bus == 0 {
            return if dev == 0 && func == 0 { Some(self.base + reg as u64) } else { None };
        }
        self.w(EXT_CFG_INDEX, (bus as u32) << 20 | (dev as u32) << 15 | (func as u32) << 12);
        Some(self.base + EXT_CFG_DATA + reg as u64)
    }
    pub fn cfg_read(&self, bus: u8, dev: u8, func: u8, reg: u16) -> u32 {
        self.cfg_addr(bus, dev, func, reg).map_or(0xFFFF_FFFF, |a| unsafe { (a as *const u32).read_volatile() })
    }
    pub fn cfg_write(&self, bus: u8, dev: u8, func: u8, reg: u16, v: u32) {
        if let Some(a) = self.cfg_addr(bus, dev, func, reg) {
            unsafe { (a as *mut u32).write_volatile(v) }
        }
    }

    /// Bring the link up. `regs` = the controller's mapped registers;
    /// outbound: PCI `pci_addr` appears at CPU `cpu_addr` for `size` bytes.
    pub fn init(regs: u64, cpu_addr: u64, pci_addr: u64, size: u64) -> Result<Pcie, &'static str> {
        let p = Pcie { base: regs };
        // Reset the bridge and assert PERST#.
        p.modify(RGR1_SW_INIT_1, 0, 0x2 | 0x1);
        delay_us(200);
        p.modify(RGR1_SW_INIT_1, 0x2, 0);
        // Power the SerDes.
        p.modify(MISC_HARD_DEBUG, HARD_DEBUG_SERDES_IDDQ, 0);
        delay_us(200);
        // SCB access, unsupported config reads return 0xffffffff, 128-byte bursts.
        p.modify(MISC_CTRL, 0x30_0000, 0x1000 | 0x2000);
        // Inbound: 4 GiB at PCI address 0 = physical 0 (size code log2 - 15).
        p.w(MISC_RC_BAR2_CONFIG_LO, 32 - 15);
        p.w(MISC_RC_BAR2_CONFIG_HI, 0);
        p.modify(MISC_CTRL, 0xF800_0000, (32 - 15) << 27); // SCB0 size
        p.modify(MISC_RC_BAR1_CONFIG_LO, 0x1F, 0);
        p.modify(MISC_RC_BAR3_CONFIG_LO, 0x1F, 0);
        // No interrupts from the bridge: mask and clear them all.
        p.w(INTR2_CPU + 0x10, 0xFFFF_FFFF);
        p.w(INTR2_CPU + 0x08, 0xFFFF_FFFF);
        // Release PERST# and wait (up to 100 ms more) for the link.
        p.modify(RGR1_SW_INIT_1, 0x1, 0);
        delay_us(100_000);
        let up = |p: &Pcie| p.r(MISC_PCIE_STATUS) & (STATUS_PHYLINKUP | STATUS_DL_ACTIVE) == (STATUS_PHYLINKUP | STATUS_DL_ACTIVE);
        let mut waited = 0;
        while !up(&p) && waited < 100 {
            delay_us(5000);
            waited += 5;
        }
        if !up(&p) {
            return Err("PCIe link did not come up");
        }
        if p.r(MISC_PCIE_STATUS) & STATUS_RC_MODE == 0 {
            return Err("PCIe controller is in endpoint mode");
        }
        // Outbound memory window 0.
        p.w(MISC_MEM_WIN0_LO, pci_addr as u32);
        p.w(MISC_MEM_WIN0_HI, (pci_addr >> 32) as u32);
        let base_mb = cpu_addr >> 20;
        let limit_mb = (cpu_addr + size - 1) >> 20;
        p.modify(MISC_MEM_WIN0_BASE_LIMIT, 0xFFF0 | 0xFFF0_0000, ((base_mb as u32 & 0xFFF) << 4) | ((limit_mb as u32 & 0xFFF) << 20));
        p.modify(MISC_MEM_WIN0_BASE_HI, 0xFF, (base_mb >> 12) as u32 & 0xFF);
        p.modify(MISC_MEM_WIN0_LIMIT_HI, 0xFF, (limit_mb >> 12) as u32 & 0xFF);
        // The root port presents itself as a PCI-PCI bridge.
        p.modify(RC_CFG_PRIV1_ID_VAL3, 0xFF_FFFF, 0x06_0400);
        let ssc = p.enable_ssc();
        // Little-endian BAR2 accesses; CLKREQ# gating of the reference clock.
        p.modify(RC_CFG_VENDOR_SPECIFIC_REG1, 0xC, 0);
        p.modify(MISC_HARD_DEBUG, 0, HARD_DEBUG_CLKREQ);
        let lnk = unsafe { ((regs + RC_LINK_STATUS) as *const u16).read_volatile() };
        crate::println!(
            "[pcie] BCM2711 root complex rev {:#x}: link up, gen {} x{}, spread spectrum {}",
            p.r(MISC_REVISION),
            lnk & 0xF,
            (lnk >> 4) & 0x3F,
            if ssc { "on" } else { "off" }
        );
        Ok(p)
    }
}

/// Bring up the root complex and the VL805 behind it; returns the CPU address
/// and size of the xHCI registers. `load_firmware(device address)` asks the
/// VideoCore to load the VL805's firmware (mailbox NOTIFY_XHCI_RESET); like
/// Linux it is only called when the VL805 reports no firmware version.
pub fn init_vl805(regs: u64, cpu_addr: u64, pci_addr: u64, size: u64, load_firmware: &dyn Fn(u32) -> bool) -> Result<(u64, u64), &'static str> {
    let p = Pcie::init(regs, cpu_addr, pci_addr, size)?;
    // Root port: bus 0 -> 1, memory window over the first 64 MiB of the outbound range.
    p.cfg_write(0, 0, 0, 0x18, 0x0001_0100);
    let lo = (pci_addr >> 20) as u32 & 0xFFF;
    let hi = ((pci_addr + 0x3F_FFFF) >> 20) as u32 & 0xFFF;
    p.cfg_write(0, 0, 0, 0x20, (hi << 20) | (lo << 4));
    p.cfg_write(0, 0, 0, 0x24, 0x0000_FFF0); // no prefetchable window
    p.cfg_write(0, 0, 0, 0x04, p.cfg_read(0, 0, 0, 0x04) | 0x6);
    delay_us(1000);
    let id = p.cfg_read(1, 0, 0, 0);
    if id == 0xFFFF_FFFF || id == 0 {
        return Err("no device behind the PCIe root port");
    }
    let class = p.cfg_read(1, 0, 0, 0x08) >> 8;
    if class != 0x0C_0330 {
        crate::println!("[pcie] 01:00.0 {:04x}:{:04x} class {:06x} is not an xHCI controller", id & 0xFFFF, id >> 16, class);
        return Err("the PCIe device is not an xHCI controller");
    }
    // BAR0: size it, then place it at the start of the window.
    let bar = p.cfg_read(1, 0, 0, 0x10);
    let is64 = (bar >> 1) & 3 == 2;
    p.cfg_write(1, 0, 0, 0x10, 0xFFFF_FFFF);
    let bar_size = (!(p.cfg_read(1, 0, 0, 0x10) & !0xF)).wrapping_add(1) as u64;
    p.cfg_write(1, 0, 0, 0x10, pci_addr as u32);
    if is64 {
        p.cfg_write(1, 0, 0, 0x14, (pci_addr >> 32) as u32);
    }
    p.cfg_write(1, 0, 0, 0x04, p.cfg_read(1, 0, 0, 0x04) | 0x6); // memory + bus master
    let mut fw = p.cfg_read(1, 0, 0, 0x50);
    if fw == 0 {
        let asked = load_firmware(1 << 20); // bus 1, device 0, function 0
        delay_us(1000);
        fw = p.cfg_read(1, 0, 0, 0x50);
        crate::println!("[pcie] VL805 firmware load requested from the VideoCore: {}", if asked { "acknowledged" } else { "no answer" });
        // keep the BAR and command register in case the reload reset them
        p.cfg_write(1, 0, 0, 0x10, pci_addr as u32);
        if is64 {
            p.cfg_write(1, 0, 0, 0x14, (pci_addr >> 32) as u32);
        }
        p.cfg_write(1, 0, 0, 0x04, p.cfg_read(1, 0, 0, 0x04) | 0x6);
    }
    crate::println!(
        "[pcie] 01:00.0 {:04x}:{:04x} xHCI, BAR0 {} KiB at CPU {:#x}, VL805 firmware {:#x}",
        id & 0xFFFF,
        id >> 16,
        bar_size / 1024,
        cpu_addr,
        fw
    );
    let _ = size;
    Ok((cpu_addr, bar_size.max(0x1000)))
}
