//! Direct boot on a Raspberry Pi 4 (roadmap R1): the Pi firmware's own
//! protocol, `kernel8.img`, without UEFI or Limine. The firmware (and QEMU
//! `-M raspi4b -kernel`) loads the flat image at physical 0x80000 and enters
//! its first instruction at EL2 with the MMU off and x0 = the Device Tree.
//!
//! This is the path that can be tested without a board: QEMU cannot get the
//! Pi's UEFI firmware as far as Limine (it stops at an unmodelled RNG
//! register, and `raspi4b` has no SD or PCIe for UEFI to read a disk from).
//!
//! The kernel is linked at 0xffffffff80000000 and expects what Limine leaves:
//! EL1, the MMU on, itself in the higher half, all RAM at an offset (HHDM),
//! Normal write-back in MAIR index 0. The head below recreates exactly that:
//! 1. secondary cores park; a missing DTB stops here, with a letter on the UART;
//! 2. the image moves to 2 MiB, so 2 MiB blocks can map it;
//! 3. .bss is cleared, EL2 drops to EL1 (the timers and FP handed down);
//! 4. page tables go up, the MMU comes on, and Rust runs in the higher half.
//!
//! Progress goes straight to the PL011 before the MMU is on, because a board
//! has no debugger attached: `B` entered, `R` relocated, `E` at EL1, `M` MMU
//! about to go on. `D` means no Device Tree, `3` entered at EL3.
//!
//! The screen (roadmap R2): before `kernel_main`, the VideoCore firmware is
//! asked for a framebuffer through its mailbox (property channel 8). It is
//! mapped Normal non-cacheable in its own window, so the GPU sees every write
//! without cache maintenance, and handed over in `BootInfo` exactly as Limine
//! hands over a UEFI framebuffer: console, window system and desktop need no
//! Pi-specific code.
//!
//! Limits, for now: the kernel image is mapped RWX (blocks, one attribute
//! set), and RAM above 0xFC00_0000 is ignored (the peripherals start there).
//! RAM holes inside the first 4 GiB (a 1 GiB board) are mapped Normal, which
//! is harmless: only device memory has read side effects.

use core::arch::global_asm;

use crate::boot;

/// Physical address the image runs at (the start of a 2 MiB block).
pub const KERNEL_PHYS: u64 = 0x20_0000;
const KERNEL_VIRT: u64 = 0xffff_ffff_8000_0000;
/// The direct map of physical memory (what Limine calls the HHDM).
const PHYS_OFFSET: u64 = 0xffff_8000_0000_0000;
/// Peripherals (and the top of what the tables map as RAM).
const RAM_TOP: u64 = 0xFC00_0000;

const TABLES: usize = 8;
/// Where the framebuffer is mapped Normal non-cacheable (TTBR1 root entry 257).
const FB_WINDOW: u64 = 0xffff_8080_0000_0000;
/// The VideoCore mailbox (BCM2711), seen through the device mapping the head
/// puts over the peripherals.
const MAILBOX: u64 = PHYS_OFFSET + 0xfe00_b880;

#[repr(C, align(4096))]
pub struct Tables([[u64; 512]; TABLES]);
#[repr(C, align(16))]
pub struct Stack([u8; 512 * 1024]);

// Both live in .bss, cleared by the head before use. `#[no_mangle]` + `#[used]`:
// only the assembly below refers to them.
#[no_mangle]
#[used]
static mut bz_rpi_tables: Tables = Tables([[0; 512]; TABLES]);
#[no_mangle]
#[used]
static mut bz_rpi_stack: Stack = Stack([0; 512 * 1024]);

extern "C" {
    static __bz_kernel_end: u8;
}

global_asm!(
    r#"
    .section .text.rpi_head, "ax"
    .global bz_rpi_head
bz_rpi_head:
    mrs     x9, mpidr_el1
    tst     x9, #0xff
    b.ne    9f                          // only core 0 boots here
    mov     x19, x0                     // the Device Tree, in a register that survives
    mov     w0, #'B'
    bl      2f
    ldr     w9, [x19]
    ldr     w10, =0xedfe0dd0            // FDT magic, read little-endian
    cmp     x19, #0
    ccmp    w9, w10, #0, ne
    b.eq    1f
    mov     w0, #'D'
    bl      2f
    b       9f

    // Move the image to KERNEL_PHYS, from the top down (the copy may overlap).
1:  adr     x1, bz_rpi_head
    ldr     x20, ={phys}
    ldr     x3, =(__bz_data_end - {virt})
    add     x3, x3, #7
    bic     x3, x3, #7
    cmp     x1, x20
    b.eq    4f
3:  subs    x3, x3, #8
    ldr     x4, [x1, x3]
    str     x4, [x20, x3]
    b.ne    3b
    dsb     sy
    ic      iallu
    dsb     sy
    isb
    adr     x4, 4f
    sub     x4, x4, x1
    add     x4, x4, x20
    br      x4

4:  mov     w0, #'R'
    bl      2f
    // Clear .bss (the tables and the stack are in it).
    ldr     x1, =(__bz_data_end - {virt})
    ldr     x2, =(__bz_kernel_end - {virt})
    add     x1, x1, x20
    add     x2, x2, x20
    add     x1, x1, #7
    bic     x1, x1, #7
5:  cmp     x1, x2
    b.hs    6f
    str     xzr, [x1], #8
    b       5b

    // Down to EL1.
6:  mrs     x0, currentel
    lsr     x0, x0, #2
    cmp     x0, #2
    b.lo    7f
    b.eq    61f
    mov     w0, #'3'
    bl      2f
    b       9f
61: mov     x0, #(1 << 31)              // HCR_EL2.RW: EL1 is AArch64
    msr     hcr_el2, x0
    mov     x0, #3                      // EL1 reaches the physical counter + timer
    msr     cnthctl_el2, x0
    msr     cntvoff_el2, xzr
    mov     x0, #0x33ff                 // no FP / SIMD traps to EL2
    msr     cptr_el2, x0
    msr     hstr_el2, xzr
    ldr     x0, =0x30d00800             // SCTLR_EL1 RES1 bits, MMU off
    msr     sctlr_el1, x0
    mov     x0, #0x3c5                  // EL1h, DAIF masked
    msr     spsr_el2, x0
    adr     x0, 7f
    msr     elr_el2, x0
    eret

7:  msr     spsel, #1
    mov     w0, #'E'
    bl      2f

    // Page tables (each 4 KiB, cleared above):
    //  0 root TTBR1   [256] -> 2 (all RAM at PHYS_OFFSET)   [511] -> 4 (the kernel)
    //  1 root TTBR0   [0]   -> 2 (identity, for the jump to the higher half)
    //  2 RAM L1       [0..2] 1 GiB blocks, [3] -> 3
    //  3 RAM L2       2 MiB blocks from 3 GiB up to the peripherals, then the
    //                 peripherals themselves as Device-nGnRnE (the mailbox)
    //  4 kernel L1    [510] -> 5
    //  5 kernel L2    2 MiB blocks over the image
    ldr     x21, =(bz_rpi_tables - {virt})
    add     x21, x21, x20
    mov     x1, #0x1000
    add     x22, x21, x1                // 1
    add     x23, x22, x1                // 2
    add     x24, x23, x1                // 3
    add     x25, x24, x1                // 4
    add     x26, x25, x1                // 5
    orr     x0, x23, #3                 // table descriptor
    str     x0, [x21, #256 * 8]
    str     x0, [x22]
    orr     x0, x25, #3
    str     x0, [x21, #511 * 8]
    orr     x0, x26, #3
    str     x0, [x25, #510 * 8]
    orr     x0, x24, #3
    str     x0, [x23, #3 * 8]
    mov     x2, #0x701                  // block: AF, inner shareable, Normal (attr 0)
    mov     x0, x2
    mov     x5, #0x40000000
    str     x0, [x23]
    add     x0, x0, x5
    str     x0, [x23, #8]
    add     x0, x0, x5
    str     x0, [x23, #16]
    add     x0, x0, x5                  // now 3 GiB + attributes
    mov     x3, #480                    // 0xC000_0000 .. 0xFC00_0000
    mov     x4, x24
8:  str     x0, [x4], #8
    add     x0, x0, #0x200000
    subs    x3, x3, #1
    b.ne    8b
    // 0xFC00_0000 .. 4 GiB: device blocks (attr 2), never executable.
    ldr     x0, =(0xfc000000 | 0x409 | (3 << 53))
    mov     x3, #32
82: str     x0, [x4], #8
    add     x0, x0, #0x200000
    subs    x3, x3, #1
    b.ne    82b
    ldr     x3, =(__bz_kernel_end - {virt} + 0x1fffff)
    lsr     x3, x3, #21
    orr     x0, x20, x2
    mov     x4, x26
81: str     x0, [x4], #8
    add     x0, x0, #0x200000
    subs    x3, x3, #1
    b.ne    81b

    mov     w0, #'M'
    bl      2f
    dsb     sy
    ldr     x0, =0x44ff                 // attr 0 Normal WB, attr 1 Normal NC
    msr     mair_el1, x0
    // T0SZ = T1SZ = 16, 4 KiB granules, WB inner-shareable walks, 40-bit PA.
    ldr     x0, =((16) | (1 << 8) | (1 << 10) | (3 << 12) | (16 << 16) | (1 << 24) | (1 << 26) | (3 << 28) | (2 << 30) | (2 << 32))
    msr     tcr_el1, x0
    msr     ttbr0_el1, x22
    msr     ttbr1_el1, x21
    isb
    tlbi    vmalle1
    dsb     nsh
    isb
    ldr     x0, =(0x30d00800 | 1 | (1 << 2) | (1 << 12)) // M, C, I
    msr     sctlr_el1, x0
    isb
    ldr     x1, =bz_rpi_main
    ldr     x2, =bz_rpi_stack
    mov     x3, #(512 * 1024)
    add     x2, x2, x3
    mov     sp, x2
    mov     x0, x19
    br      x1

    // One character on the PL011 the firmware left running; never hangs
    // on a dead UART. x0 = the character; clobbers x9-x11.
2:  ldr     x9, =0xfe201000
    mov     x10, #0x10000
21: ldr     w11, [x9, #0x18]
    tbz     w11, #5, 22f
    subs    x10, x10, #1
    b.ne    21b
22: str     w0, [x9]
    ret

9:  wfe
    b       9b
    .ltorg
    .text
    "#,
    phys = const KERNEL_PHYS,
    virt = const KERNEL_VIRT,
);

/// First Rust code of the direct boot, in the higher half with the MMU on:
/// describe the machine the way a loader would, then the normal `kernel_main`.
#[no_mangle]
extern "C" fn bz_rpi_main(dtb: u64) -> ! {
    crate::serial::init();
    let mut info = boot::BootInfo::new("rpi-direct", PHYS_OFFSET);
    let image_end = core::ptr::addr_of!(__bz_kernel_end) as u64 - KERNEL_VIRT + KERNEL_PHYS;
    let image_end = (image_end + 0xFFF) & !0xFFF;
    let blob = (PHYS_OFFSET + dtb) as *const u8;
    let total = unsafe { crate::dtb::total_size(blob) } as u64;
    let (dtb_lo, dtb_hi) = (dtb & !0xFFF, (dtb + total + 0xFFF) & !0xFFF);

    // RAM from the Device Tree, minus the firmware's low memory + the image,
    // minus the tree itself. Regions go in ascending order, as add_region merges.
    let mut ram = [(0u64, 0u64); 8];
    let mut n = 0;
    unsafe {
        crate::dtb::scan_memory(blob, &mut |base, size| {
            if n < ram.len() {
                ram[n] = (base, base + size);
                n += 1;
            }
        })
    };
    info.add_region(0, image_end, false);
    for &(start, end) in &ram[..n] {
        let (start, end) = (start.max(image_end), end.min(RAM_TOP));
        if end <= start {
            continue;
        }
        if dtb_hi <= start || dtb_lo >= end {
            info.add_region(start, end, true);
        } else {
            info.add_region(start, dtb_lo.max(start), true);
            info.add_region(dtb_lo.max(start), dtb_hi.min(end), false);
            info.add_region(dtb_hi.min(end), end, true);
        }
    }
    info.firmware.dtb = Some(dtb);
    info.kernel_base = Some((KERNEL_PHYS, KERNEL_VIRT));
    info.framebuffer = videocore_framebuffer(1280, 720);
    EMMC2_CLOCK_HZ.store(videocore_clock_rate(12) as u64, core::sync::atomic::Ordering::SeqCst);
    crate::kernel_main(boot::install(info))
}

/// The EMMC2 (SD card) base clock the firmware set up, in Hz (0 = unknown).
pub static EMMC2_CLOCK_HZ: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// A firmware clock's rate (mailbox tag 0x00030002); 0 if it would not say.
fn videocore_clock_rate(id: u32) -> u32 {
    let m = unsafe { core::ptr::addr_of_mut!(MAILBOX_MSG.0) as *mut u32 };
    let words: [u32; 8] = [8 * 4, 0, 0x0003_0002, 8, 8, id, 0, 0];
    for (i, w) in words.iter().enumerate() {
        unsafe { m.add(i).write_volatile(*w) };
    }
    if !mailbox_call(m, 8 * 4) {
        return 0;
    }
    unsafe { m.add(6).read_volatile() }
}

/// The board's Ethernet MAC address from the firmware (mailbox tag 0x00010003).
pub fn board_mac() -> Option<[u8; 6]> {
    let m = unsafe { core::ptr::addr_of_mut!(MAILBOX_MSG.0) as *mut u32 };
    let words: [u32; 8] = [8 * 4, 0, 0x0001_0003, 8, 0, 0, 0, 0];
    for (i, w) in words.iter().enumerate() {
        unsafe { m.add(i).write_volatile(*w) };
    }
    if !mailbox_call(m, 8 * 4) {
        return None;
    }
    let b = unsafe { core::slice::from_raw_parts(m.add(5) as *const u8, 6) };
    let mac = [b[0], b[1], b[2], b[3], b[4], b[5]];
    if mac == [0; 6] { None } else { Some(mac) }
}

/// Tell the VideoCore the VL805 USB controller at PCI address `dev_addr`
/// (bus << 20 | device << 15 | function << 12) was reset, so it loads the
/// VL805's firmware (mailbox tag 0x00030058, NOTIFY_XHCI_RESET).
pub fn notify_xhci_reset(dev_addr: u32) -> bool {
    let m = unsafe { core::ptr::addr_of_mut!(MAILBOX_MSG.0) as *mut u32 };
    let words: [u32; 7] = [7 * 4, 0, 0x0003_0058, 4, 4, dev_addr, 0];
    for (i, w) in words.iter().enumerate() {
        unsafe { m.add(i).write_volatile(*w) };
    }
    mailbox_call(m, 7 * 4)
}

#[repr(C, align(16))]
struct Message([u32; 36]);
static mut MAILBOX_MSG: Message = Message([0; 36]);

fn kernel_phys(va: u64) -> u64 {
    va - KERNEL_VIRT + KERNEL_PHYS
}

/// Clean + invalidate `[va, va+len)` to the point of coherency (the VideoCore
/// reads and writes the message in memory, not in our cache).
fn flush(va: u64, len: u64) {
    let mut a = va & !63;
    while a < va + len {
        unsafe { core::arch::asm!("dc civac, {}", in(reg) a, options(nostack)) };
        a += 64;
    }
    unsafe { core::arch::asm!("dsb sy", options(nostack)) };
}

/// One mailbox property call on channel 8; false if the firmware did not answer.
fn mailbox_call(msg: *mut u32, len: u64) -> bool {
    let reg = |off: u64| (MAILBOX + off) as *mut u32;
    let addr = kernel_phys(msg as u64) as u32;
    flush(msg as u64, len);
    unsafe {
        let mut n = 0u32;
        while reg(0x18).read_volatile() & (1 << 31) != 0 {
            n += 1;
            if n > 10_000_000 {
                return false;
            }
        }
        reg(0x20).write_volatile(addr | 8);
        loop {
            let mut n = 0u32;
            while reg(0x18).read_volatile() & (1 << 30) != 0 {
                n += 1;
                if n > 50_000_000 {
                    return false;
                }
            }
            if reg(0x00).read_volatile() == addr | 8 {
                break;
            }
        }
    }
    flush(msg as u64, len);
    unsafe { msg.add(1).read_volatile() == 0x8000_0000 }
}

/// Ask the VideoCore for a `width` x `height`, 32-bit framebuffer and map it.
fn videocore_framebuffer(width: u32, height: u32) -> Option<boot::Framebuffer> {
    let m = unsafe { core::ptr::addr_of_mut!(MAILBOX_MSG.0) as *mut u32 };
    let words: [u32; 35] = [
        35 * 4, 0,
        0x0004_8003, 8, 8, width, height,      // physical size
        0x0004_8004, 8, 8, width, height,      // virtual size
        0x0004_8009, 8, 8, 0, 0,               // virtual offset
        0x0004_8005, 4, 4, 32,                 // depth
        0x0004_8006, 4, 4, 0,                  // pixel order: BGR in memory (0x00RRGGBB words)
        0x0004_0001, 8, 8, 4096, 0,            // allocate buffer -> base, size
        0x0004_0008, 4, 4, 0,                  // pitch
        0,
    ];
    for (i, w) in words.iter().enumerate() {
        unsafe { m.add(i).write_volatile(*w) };
    }
    if !mailbox_call(m, 35 * 4) {
        return None;
    }
    let at = |i: usize| unsafe { m.add(i).read_volatile() };
    let (w, h, order, bus, size, pitch) = (at(5), at(6), at(24), at(28), at(29), at(33));
    if bus == 0 || size == 0 || pitch < w * 4 || w == 0 || h == 0 {
        return None;
    }
    let phys = (bus & 0x3FFF_FFFF) as u64;
    let len = (pitch as u64) * h as u64;

    // Map it in its own window: Normal non-cacheable (MAIR index 1), 2 MiB blocks.
    let tables = unsafe { core::ptr::addr_of_mut!(bz_rpi_tables.0) };
    let (l0, l1, l2) = unsafe { (&mut (*tables)[0], &mut (*tables)[6], &mut (*tables)[7]) };
    let base = phys & !0x1F_FFFF;
    let blocks = ((phys + len - base) + 0x1F_FFFF) >> 21;
    if blocks > 512 {
        return None;
    }
    for i in 0..blocks as usize {
        l2[i] = (base + ((i as u64) << 21)) | 0x400 | 0x300 | (1 << 2) | 1 | (3 << 53);
    }
    l1[0] = kernel_phys(l2.as_ptr() as u64) | 3;
    l0[((FB_WINDOW >> 39) & 511) as usize] = kernel_phys(l1.as_ptr() as u64) | 3;
    unsafe { core::arch::asm!("dsb ishst", "tlbi vmalle1", "dsb ish", "isb", options(nostack)) };

    Some(boot::Framebuffer {
        addr: FB_WINDOW + (phys - base),
        len: len as usize,
        width: w as usize,
        height: h as usize,
        stride: (pitch / 4) as usize,
        bytes_per_pixel: 4,
        format: if order == 1 { boot::PixelFormat::Rgb } else { boot::PixelFormat::Bgr },
    })
}
