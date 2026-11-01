//! Glue for the `bootloader` crate (0.11, x86-64 BIOS + UEFI): the kernel's
//! entry point. It turns the crate's `BootInfo` into the protocol-neutral
//! [`crate::boot::BootInfo`] and calls `kernel_main` — the only place the
//! kernel touches `bootloader_api`.

use bootloader_api::config::{BootloaderConfig, Mapping};
use bootloader_api::info::{MemoryRegionKind, Optional, PixelFormat};
use bootloader_api::{entry_point, BootInfo};

use crate::boot;

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    // The boot task runs the milestone demos (compositor renders, terminal +
    // shell + VFS call chains) on this stack; the default is too small.
    config.kernel_stack_size = 512 * 1024;
    config
};

entry_point!(bootloader_entry, config = &BOOTLOADER_CONFIG);

fn bootloader_entry(bi: &'static mut BootInfo) -> ! {
    let phys_offset = bi
        .physical_memory_offset
        .into_option()
        .expect("bootloader must map physical memory");
    let mut info = boot::BootInfo::new("bootloader-0.11", phys_offset);
    for r in bi.memory_regions.iter() {
        info.add_region(r.start, r.end, r.kind == MemoryRegionKind::Usable);
    }
    if let Optional::Some(fb) = core::mem::replace(&mut bi.framebuffer, Optional::None) {
        let fi = fb.info();
        let buf = fb.into_buffer();
        info.framebuffer = Some(boot::Framebuffer {
            addr: buf.as_mut_ptr() as u64,
            len: buf.len(),
            width: fi.width,
            height: fi.height,
            stride: fi.stride,
            bytes_per_pixel: fi.bytes_per_pixel,
            format: match fi.pixel_format {
                PixelFormat::Rgb => boot::PixelFormat::Rgb,
                PixelFormat::Bgr => boot::PixelFormat::Bgr,
                PixelFormat::U8 => boot::PixelFormat::Gray,
                _ => boot::PixelFormat::Unknown,
            },
        });
    }
    info.firmware.rsdp = bi.rsdp_addr.into_option();
    crate::kernel_main(boot::install(info))
}
