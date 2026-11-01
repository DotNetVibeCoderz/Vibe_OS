//! PCI bus enumeration via legacy config space ports (v0.3 "Batang":
//! foundation of the driver framework — drivers probe this list).

use alloc::vec::Vec;
use spin::Mutex;
use x86_64::instructions::port::Port;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

static CONFIG_PORTS: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
}

impl PciDevice {
    pub fn class_name(&self) -> &'static str {
        match (self.class, self.subclass) {
            (0x01, 0x01) => "IDE controller",
            (0x01, 0x06) => "SATA/AHCI controller",
            (0x01, 0x08) => "NVMe controller",
            (0x01, _) => "storage controller",
            (0x02, _) => "network controller",
            (0x03, _) => "display controller",
            (0x04, _) => "multimedia controller",
            (0x06, 0x00) => "host bridge",
            (0x06, 0x01) => "ISA bridge",
            (0x06, _) => "bridge",
            (0x0C, 0x03) => "USB controller",
            _ => "device",
        }
    }
}

fn read_config_u32(bus: u8, slot: u8, function: u8, offset: u8) -> u32 {
    let address: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((function as u32) << 8)
        | (offset as u32 & 0xFC);
    let _guard = CONFIG_PORTS.lock();
    unsafe {
        Port::<u32>::new(CONFIG_ADDRESS).write(address);
        Port::<u32>::new(CONFIG_DATA).read()
    }
}

fn write_config_u32(bus: u8, slot: u8, function: u8, offset: u8, value: u32) {
    let address: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((function as u32) << 8)
        | (offset as u32 & 0xFC);
    let _guard = CONFIG_PORTS.lock();
    unsafe {
        Port::<u32>::new(CONFIG_ADDRESS).write(address);
        Port::<u32>::new(CONFIG_DATA).write(value);
    }
}

impl PciDevice {
    /// Read a 32-bit config-space register (offset 4-byte aligned).
    pub fn read32(&self, offset: u8) -> u32 {
        read_config_u32(self.bus, self.slot, self.function, offset)
    }

    /// Read one byte of config space.
    pub fn read8(&self, offset: u8) -> u8 {
        (self.read32(offset & !3) >> ((offset & 3) * 8)) as u8
    }

    /// Read 16 bits of config space (offset 2-byte aligned).
    pub fn read16(&self, offset: u8) -> u16 {
        (self.read32(offset & !3) >> ((offset & 2) * 8)) as u16
    }

    /// Physical address of memory BAR `index` (64-bit BARs span index and
    /// index+1). None for an I/O BAR or an unassigned one.
    pub fn mem_bar(&self, index: u8) -> Option<u64> {
        let lo = self.bar(index);
        if lo & 1 != 0 {
            return None;
        }
        let mut addr = (lo & !0xF) as u64;
        if (lo >> 1) & 3 == 2 && index < 5 {
            addr |= (self.bar(index + 1) as u64) << 32;
        }
        (addr != 0).then_some(addr)
    }

    /// Enable memory-space decoding and bus-master DMA.
    pub fn enable_mem_and_bus_master(&self) {
        let cmd = read_config_u32(self.bus, self.slot, self.function, 0x04);
        write_config_u32(self.bus, self.slot, self.function, 0x04, cmd | 0x0006);
    }

    /// Walk the capability list: (capability id, config offset) pairs.
    pub fn capabilities(&self) -> Vec<(u8, u8)> {
        let mut out = Vec::new();
        if self.read16(0x06) & 0x10 == 0 {
            return out; // no capability list
        }
        let mut ptr = self.read8(0x34) & !3;
        while ptr != 0 && out.len() < 48 {
            out.push((self.read8(ptr), ptr));
            ptr = self.read8(ptr + 1) & !3;
        }
        out
    }

    /// Read a base address register (0..=5) as its raw 32-bit value.
    pub fn bar(&self, index: u8) -> u32 {
        read_config_u32(self.bus, self.slot, self.function, 0x10 + index * 4)
    }

    /// Base of an I/O-space BAR (bit 0 set): the port range start.
    pub fn io_bar(&self, index: u8) -> u16 {
        (self.bar(index) & 0xFFFC) as u16
    }

    /// Enable I/O-space decoding and bus-master DMA in the command register.
    pub fn enable_io_and_bus_master(&self) {
        let cmd = read_config_u32(self.bus, self.slot, self.function, 0x04);
        write_config_u32(self.bus, self.slot, self.function, 0x04, cmd | 0x0005);
    }
}

fn probe(bus: u8, slot: u8, function: u8) -> Option<PciDevice> {
    let id = read_config_u32(bus, slot, function, 0x00);
    let vendor_id = (id & 0xFFFF) as u16;
    if vendor_id == 0xFFFF {
        return None;
    }
    let class_reg = read_config_u32(bus, slot, function, 0x08);
    Some(PciDevice {
        bus,
        slot,
        function,
        vendor_id,
        device_id: (id >> 16) as u16,
        class: (class_reg >> 24) as u8,
        subclass: (class_reg >> 16) as u8,
        prog_if: (class_reg >> 8) as u8,
    })
}

/// Brute-force scan of bus 0 plus any buses reachable through it. QEMU's
/// default machines put everything on bus 0; bridges are followed one level.
pub fn scan() -> Vec<PciDevice> {
    let mut devices = Vec::new();
    for bus in 0..=1u8 {
        for slot in 0..32 {
            let Some(dev) = probe(bus, slot, 0) else { continue };
            let header = (read_config_u32(bus, slot, 0, 0x0C) >> 16) as u8;
            devices.push(dev);
            if header & 0x80 != 0 {
                for function in 1..8 {
                    if let Some(dev) = probe(bus, slot, function) {
                        devices.push(dev);
                    }
                }
            }
        }
    }
    devices
}
