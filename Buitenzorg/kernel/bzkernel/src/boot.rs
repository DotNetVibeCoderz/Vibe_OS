//! Boot information, independent of the boot protocol (multi-arch M1).
//!
//! Whatever loaded the kernel — the `bootloader` crate (x86-64 BIOS/UEFI)
//! today, Limine or a UEFI stub on ARM64 / RISC-V later — its glue in
//! `arch/<arch>/` converts the loader's hand-off into this one [`BootInfo`]
//! and calls `kernel_main`. Generic code (memory, framebuffer, platform
//! discovery) only ever sees this module, never a loader crate.
//!
//! Everything here is fixed-size: it is filled before the heap exists.

use spin::Once;

/// Most memory-map entries kept. UEFI maps are long (one entry per firmware
/// allocation); adjacent entries of the same kind are merged on insert, so
/// this is never reached on QEMU/OVMF in practice.
pub const MAX_REGIONS: usize = 512;

/// One physical memory range `[start, end)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemRegion {
    pub start: u64,
    pub end: u64,
    /// Free RAM the kernel may allocate (everything else is reserved:
    /// firmware, ACPI, MMIO, the kernel image and the loader's own data).
    pub usable: bool,
}

/// Pixel layout of the boot framebuffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// Byte 0 red, 1 green, 2 blue.
    Rgb,
    /// Byte 0 blue, 1 green, 2 red.
    Bgr,
    /// One gray byte.
    Gray,
    Unknown,
}

/// A linear framebuffer, already mapped by the loader.
#[derive(Clone, Copy, Debug)]
pub struct Framebuffer {
    /// Virtual address of the first pixel.
    pub addr: u64,
    /// Size of the mapping in bytes.
    pub len: usize,
    pub width: usize,
    pub height: usize,
    /// Pixels per row (>= width).
    pub stride: usize,
    pub bytes_per_pixel: usize,
    pub format: PixelFormat,
}

/// Where the firmware describes the platform (interrupt controllers, timers,
/// CPUs, devices): ACPI on PCs (and ARM servers), a Device Tree on most ARM64 /
/// RISC-V boards and QEMU `virt`. A loader may hand over both.
#[derive(Clone, Copy, Debug, Default)]
pub struct Firmware {
    /// Physical address of the ACPI RSDP.
    pub rsdp: Option<u64>,
    /// Physical address of a flattened Device Tree blob.
    pub dtb: Option<u64>,
}

/// Most boot modules kept.
pub const MAX_MODULES: usize = 16;

/// A file the loader placed in memory for the kernel (Limine modules): the
/// ARM64 port loads its first programs this way, before it has a disk driver.
#[derive(Clone, Copy)]
#[cfg_attr(target_arch = "x86_64", allow(dead_code))]
pub struct Module {
    name: [u8; 32],
    name_len: usize,
    /// Virtual address (direct map) and size of the contents.
    pub addr: u64,
    pub size: u64,
}

#[cfg_attr(target_arch = "x86_64", allow(dead_code))]
impl Module {
    pub fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }
    /// The module's bytes.
    pub fn bytes(&self) -> &'static [u8] {
        unsafe { core::slice::from_raw_parts(self.addr as *const u8, self.size as usize) }
    }
}

pub struct BootInfo {
    /// Which loader / protocol handed over (for logs and tests).
    pub loader: &'static str,
    /// Virtual address at which all physical memory is mapped (the kernel's
    /// physical-memory window: `virt = phys + phys_offset`).
    pub phys_offset: u64,
    regions: [MemRegion; MAX_REGIONS],
    region_count: usize,
    /// Entries that did not fit (should stay 0).
    pub regions_dropped: usize,
    pub framebuffer: Option<Framebuffer>,
    pub firmware: Firmware,
    modules: [Module; MAX_MODULES],
    module_count: usize,
}

impl BootInfo {
    pub const fn new(loader: &'static str, phys_offset: u64) -> Self {
        BootInfo {
            loader,
            phys_offset,
            regions: [MemRegion { start: 0, end: 0, usable: false }; MAX_REGIONS],
            region_count: 0,
            regions_dropped: 0,
            framebuffer: None,
            firmware: Firmware { rsdp: None, dtb: None },
            modules: [Module { name: [0; 32], name_len: 0, addr: 0, size: 0 }; MAX_MODULES],
            module_count: 0,
        }
    }

    /// Record a loader module (`name` truncated to 32 bytes).
    pub fn add_module(&mut self, name: &str, addr: u64, size: u64) {
        if self.module_count == MAX_MODULES {
            return;
        }
        let mut m = Module { name: [0; 32], name_len: name.len().min(32), addr, size };
        m.name[..m.name_len].copy_from_slice(&name.as_bytes()[..m.name_len]);
        self.modules[self.module_count] = m;
        self.module_count += 1;
    }

    /// Files the loader handed over.
    #[cfg_attr(target_arch = "x86_64", allow(dead_code))]
    pub fn modules(&self) -> &[Module] {
        &self.modules[..self.module_count]
    }

    /// Append a memory range (loaders report them in ascending order; an entry
    /// directly continuing the previous one of the same kind is merged).
    pub fn add_region(&mut self, start: u64, end: u64, usable: bool) {
        if end <= start {
            return;
        }
        if let Some(last) = self.regions[..self.region_count].last_mut() {
            if last.usable == usable && last.end == start {
                last.end = end;
                return;
            }
        }
        if self.region_count == MAX_REGIONS {
            self.regions_dropped += 1;
            return;
        }
        self.regions[self.region_count] = MemRegion { start, end, usable };
        self.region_count += 1;
    }

    /// The physical memory map.
    pub fn regions(&self) -> &[MemRegion] {
        &self.regions[..self.region_count]
    }

    /// Bytes of usable RAM.
    pub fn usable_bytes(&self) -> u64 {
        self.regions().iter().filter(|r| r.usable).map(|r| r.end - r.start).sum()
    }
}

static INFO: Once<BootInfo> = Once::new();

/// Record the hand-off (once, by the loader glue) and return it.
pub fn install(info: BootInfo) -> &'static BootInfo {
    INFO.call_once(|| info)
}

/// The boot information (panics before the loader glue installed it).
pub fn info() -> &'static BootInfo {
    INFO.get().expect("boot info not installed")
}

/// One line describing the hand-off, printed early in `kernel_main`.
pub fn describe(info: &BootInfo) -> alloc::string::String {
    let fb = match info.framebuffer {
        Some(f) => alloc::format!("{}x{} {:?}", f.width, f.height, f.format),
        None => alloc::string::String::from("none"),
    };
    let fw = match (info.firmware.rsdp, info.firmware.dtb) {
        (Some(r), Some(d)) => alloc::format!("ACPI (RSDP {:#x}) + Device Tree ({:#x})", r, d),
        (Some(r), None) => alloc::format!("ACPI (RSDP {:#x})", r),
        (None, Some(d)) => alloc::format!("Device Tree ({:#x})", d),
        (None, None) => alloc::string::String::from("none"),
    };
    alloc::format!(
        "loader={} memory map: {} regions ({} MiB usable{}), framebuffer {}, firmware tables: {}",
        info.loader,
        info.regions().len(),
        info.usable_bytes() / (1024 * 1024),
        if info.regions_dropped > 0 { alloc::format!(", {} dropped", info.regions_dropped) } else { alloc::string::String::new() },
        fb,
        fw
    )
}
