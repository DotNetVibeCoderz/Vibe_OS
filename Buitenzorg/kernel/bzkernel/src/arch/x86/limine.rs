//! Glue for the **Limine boot protocol** (multi-arch M1, increment 2) — the
//! loader the ARM64 and RISC-V ports will use, booting the very same kernel
//! ELF on x86-64 today.
//!
//! The kernel image carries both hand-offs: `_start` for the `bootloader`
//! crate (`boot.rs`), and the Limine *requests* below, which Limine finds by
//! scanning the loaded image for their magic numbers (8-byte aligned; no
//! request delimiters are used, so the whole image is scanned). The entry-
//! point request makes Limine jump to [`limine_entry`] instead of `_start`.
//! Request layouts and IDs follow `limine.h` / PROTOCOL.md of the
//! limine-protocol repository (base revision 6).
//!
//! What Limine changes for the kernel, compared to the `bootloader` crate:
//! the executable runs in the higher half (>= 0xffff_ffff_8000_0000, slid —
//! the kernel is PIE); there is no identity map and PML4[0] is empty (see
//! `aspace::init`); the direct map (HHDM) covers only RAM-type regions + ACPI +
//! the framebuffer, so MMIO is mapped explicitly (`smp::map_mmio` already
//! does that); the RSDP comes back as an HHDM *virtual* address.

use core::ptr::{addr_of, read_volatile};

use crate::boot;

const MAGIC0: u64 = 0xc7b1dd30df4c8b88;
const MAGIC1: u64 = 0x0a82e883a194f07b;
const BASE_REVISION: u64 = 6;

#[repr(C)]
struct Request<R, X = ()> {
    id: [u64; 4],
    revision: u64,
    response: *const R,
    extra: X,
}

const fn req<R, X>(a: u64, b: u64, extra: X) -> Request<R, X> {
    Request { id: [MAGIC0, MAGIC1, a, b], revision: 0, response: core::ptr::null(), extra }
}

#[repr(C)]
struct HhdmResponse {
    revision: u64,
    offset: u64,
}

#[repr(C)]
struct MemmapEntry {
    base: u64,
    length: u64,
    kind: u64,
}

#[repr(C)]
struct MemmapResponse {
    revision: u64,
    count: u64,
    entries: *const *const MemmapEntry,
}

#[repr(C)]
struct LimineFramebuffer {
    address: u64,
    width: u64,
    height: u64,
    pitch: u64,
    bpp: u16,
    memory_model: u8,
    red_size: u8,
    red_shift: u8,
    green_size: u8,
    green_shift: u8,
    blue_size: u8,
    blue_shift: u8,
}

#[repr(C)]
struct FramebufferResponse {
    revision: u64,
    count: u64,
    framebuffers: *const *const LimineFramebuffer,
}

#[repr(C)]
struct AddrResponse {
    revision: u64,
    address: u64,
}

const MEMMAP_USABLE: u64 = 0;

// The requests. `static mut` + volatile reads: Limine writes the response
// pointers before the kernel runs, behind the compiler's back.
#[used]
static mut BASE_REVISION_TAG: [u64; 3] = [0xf9562b2d5c95a6c8, 0x6a7b384944536bdc, BASE_REVISION];
#[used]
static mut ENTRY: Request<u64, extern "C" fn() -> !> = req(0x13d86c035a1cd3e1, 0x2b0caa89d8f3026a, limine_entry);
#[used]
static mut STACK: Request<u64, u64> = req(0x224ef0460a8e8926, 0xe1cb0fc25f46ea3d, 512 * 1024);
#[used]
static mut HHDM: Request<HhdmResponse> = req(0x48dcf1cb8ad2b852, 0x63984e959a98244b, ());
#[used]
static mut MEMMAP: Request<MemmapResponse> = req(0x67cf3d9d378a806f, 0xe304acdfc50c3c62, ());
#[used]
static mut FRAMEBUFFER: Request<FramebufferResponse> = req(0x9d5827dcd881dd75, 0xa3148604f6fab11b, ());
#[used]
static mut RSDP: Request<AddrResponse> = req(0xc5e77b6b397e7b43, 0x27637845accdcf3c, ());
#[used]
static mut DTB: Request<AddrResponse> = req(0xb40ddb48fb54bac7, 0x545081493f81ffb7, ());

fn response<R, X>(r: *const Request<R, X>) -> Option<&'static R> {
    let p = unsafe { read_volatile(addr_of!((*r).response)) };
    unsafe { p.as_ref() }
}

/// Limine jumps here (entry-point request), on its own 512 KiB stack, with
/// interrupts off and paging set up as described above.
extern "C" fn limine_entry() -> ! {
    // Serial first: any problem below should at least be reported.
    crate::serial::init();
    let tag = unsafe { read_volatile(addr_of!(BASE_REVISION_TAG)) };
    if tag[2] != 0 {
        panic!("limine: base revision {} not supported by this Limine", BASE_REVISION);
    }
    let hhdm = response(addr_of!(HHDM)).expect("limine: no HHDM response").offset;
    let mut info = boot::BootInfo::new("limine", hhdm);
    if let Some(mm) = response(addr_of!(MEMMAP)) {
        for i in 0..mm.count as usize {
            let e = unsafe { &**mm.entries.add(i) };
            info.add_region(e.base, e.base + e.length, e.kind == MEMMAP_USABLE);
        }
    }
    if let Some(fbr) = response(addr_of!(FRAMEBUFFER)) {
        if fbr.count > 0 {
            let f = unsafe { &**fbr.framebuffers };
            let bpp = (f.bpp as usize).div_ceil(8);
            let format = match (bpp, f.red_shift, f.green_shift, f.blue_shift) {
                (3 | 4, 16, 8, 0) => boot::PixelFormat::Bgr,
                (3 | 4, 0, 8, 16) => boot::PixelFormat::Rgb,
                _ => boot::PixelFormat::Unknown,
            };
            info.framebuffer = Some(boot::Framebuffer {
                addr: f.address,
                len: (f.pitch * f.height) as usize,
                width: f.width as usize,
                height: f.height as usize,
                stride: f.pitch as usize / bpp.max(1),
                bytes_per_pixel: bpp,
                format,
            });
        }
    }
    // Base revision >= 4: the RSDP is an HHDM virtual address.
    info.firmware.rsdp = response(addr_of!(RSDP)).map(|r| r.address - hhdm);
    info.firmware.dtb = response(addr_of!(DTB)).filter(|r| r.address != 0).map(|r| r.address - hhdm);
    crate::kernel_main(boot::install(info))
}
