//! Power management (v0.12 "Nalar"): Shutdown, Restart, Sleep. Uses ACPI where
//! possible (RSDP handed over by the bootloader → FADT → PM1a_CNT, and the
//! `\_S5` sleep type scanned from the DSDT), with QEMU/VirtualBox port
//! fallbacks so it works in the dev VMs.

use core::sync::atomic::{AtomicU64, AtomicU16, AtomicBool, Ordering};

use x86_64::instructions::port::Port;

use crate::memory::phys_to_virt;

// Parsed ACPI state.
static PM1A_CNT: AtomicU16 = AtomicU16::new(0);
static PM1B_CNT: AtomicU16 = AtomicU16::new(0);
static SLP_TYPA: AtomicU16 = AtomicU16::new(0);
static SLP_TYPB: AtomicU16 = AtomicU16::new(0);
static RESET_PORT: AtomicU16 = AtomicU16::new(0);
static RESET_VALUE: AtomicU16 = AtomicU16::new(0);
static ACPI_OK: AtomicBool = AtomicBool::new(false);
static RSDP: AtomicU64 = AtomicU64::new(0);

const SLP_EN: u16 = 1 << 13;

fn read_u32(phys: u64, off: u64) -> u32 {
    unsafe { core::ptr::read_unaligned(phys_to_virt(phys + off) as *const u32) }
}
fn read_u64(phys: u64, off: u64) -> u64 {
    unsafe { core::ptr::read_unaligned(phys_to_virt(phys + off) as *const u64) }
}
fn signature(phys: u64) -> [u8; 4] {
    unsafe {
        let p = phys_to_virt(phys) as *const u8;
        [*p, *p.add(1), *p.add(2), *p.add(3)]
    }
}

/// Initialize from the RSDP the bootloader provides. Returns true if a usable
/// FADT was parsed.
pub fn init(rsdp_addr: u64) -> bool {
    RSDP.store(rsdp_addr, Ordering::SeqCst);
    if rsdp_addr == 0 {
        return false;
    }
    // RSDP: revision at +15, RsdtAddress u32 at +16, XsdtAddress u64 at +24.
    let revision = unsafe { *(phys_to_virt(rsdp_addr + 15) as *const u8) };
    let (sdt, entry_size) = if revision >= 2 {
        (read_u64(rsdp_addr, 24), 8usize)
    } else {
        (read_u32(rsdp_addr, 16) as u64, 4usize)
    };
    if sdt == 0 {
        return false;
    }
    // RSDT/XSDT header: length at +4; entries follow the 36-byte header.
    let len = read_u32(sdt, 4) as u64;
    let count = (len.saturating_sub(36)) / entry_size as u64;
    let mut fadt = 0u64;
    for i in 0..count {
        let ent_off = 36 + i * entry_size as u64;
        let ent = if entry_size == 8 {
            read_u64(sdt, ent_off)
        } else {
            read_u32(sdt, ent_off) as u64
        };
        if &signature(ent) == b"FACP" {
            fadt = ent;
            break;
        }
    }
    if fadt == 0 {
        return false;
    }
    parse_fadt(fadt)
}

fn parse_fadt(fadt: u64) -> bool {
    // FADT fields (ACPI): DSDT u32 @ +40, PM1a_CNT_BLK u32 @ +64,
    // PM1b_CNT_BLK u32 @ +68, RESET_REG (GAS, 12 bytes) @ +116,
    // RESET_VALUE u8 @ +128, X_DSDT u64 @ +140.
    let pm1a = read_u32(fadt, 64) as u16;
    let pm1b = read_u32(fadt, 68) as u16;
    PM1A_CNT.store(pm1a, Ordering::SeqCst);
    PM1B_CNT.store(pm1b, Ordering::SeqCst);

    // RESET_REG: GAS { addr_space(1), bit_width(1), bit_offset(1), size(1),
    // address(8) }. Use it only for a system-I/O reset (addr_space == 1).
    let len = read_u32(fadt, 4);
    if len > 128 {
        let rr_space = unsafe { *(phys_to_virt(fadt + 116) as *const u8) };
        let rr_addr = read_u64(fadt, 116 + 4);
        let rr_val = unsafe { *(phys_to_virt(fadt + 128) as *const u8) };
        if rr_space == 1 && rr_addr != 0 && rr_addr < 0xFFFF {
            RESET_PORT.store(rr_addr as u16, Ordering::SeqCst);
            RESET_VALUE.store(rr_val as u16, Ordering::SeqCst);
        }
    }

    // DSDT (prefer X_DSDT if present).
    let dsdt = {
        let x = if len > 148 { read_u64(fadt, 140) } else { 0 };
        if x != 0 { x } else { read_u32(fadt, 40) as u64 }
    };
    if dsdt != 0 {
        scan_s5(dsdt);
    }
    ACPI_OK.store(pm1a != 0, Ordering::SeqCst);
    pm1a != 0
}

/// Scan the DSDT AML for the `\_S5_` package to recover SLP_TYPa/b. This is the
/// classic minimal technique (no full AML interpreter).
fn scan_s5(dsdt: u64) {
    let len = read_u32(dsdt, 4) as usize;
    let base = phys_to_virt(dsdt) as *const u8;
    let bytes = unsafe { core::slice::from_raw_parts(base, len.min(0x20000)) };
    // Find "_S5_" then the package: NameOp(08) '_S5_' PackageOp(12) len elements.
    let mut i = 0;
    while i + 6 < bytes.len() {
        if &bytes[i..i + 4] == b"_S5_" {
            // Look a few bytes ahead for the PackageOp 0x12.
            let mut j = i + 4;
            while j < i + 8 && j < bytes.len() && bytes[j] != 0x12 {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == 0x12 {
                // Skip PackageOp, pkglength byte(s), numElements byte.
                let mut k = j + 2; // 0x12 + pkglen (assume 1-byte pkglen)
                // numElements
                k += 1;
                // First element: SLP_TYPa. Could be a byte prefix 0x0A <val> or
                // a direct small integer (0x00..0x0F encodes value directly).
                let a = read_aml_byte(bytes, &mut k);
                let b = read_aml_byte(bytes, &mut k);
                SLP_TYPA.store((a as u16) << 10, Ordering::SeqCst);
                SLP_TYPB.store((b as u16) << 10, Ordering::SeqCst);
                return;
            }
        }
        i += 1;
    }
}

fn read_aml_byte(bytes: &[u8], k: &mut usize) -> u8 {
    if *k >= bytes.len() {
        return 0;
    }
    let op = bytes[*k];
    match op {
        0x0A => {
            // BytePrefix: value in next byte.
            *k += 1;
            let v = if *k < bytes.len() { bytes[*k] } else { 0 };
            *k += 1;
            v
        }
        0x00 => { *k += 1; 0 } // ZeroOp
        0x01 => { *k += 1; 1 } // OneOp
        v if v <= 0x0F => { *k += 1; v } // small integer encoded directly
        _ => { *k += 1; 0 }
    }
}

/// Whether ACPI power-off is available (else port fallback is used).
pub fn acpi_available() -> bool {
    ACPI_OK.load(Ordering::SeqCst)
}

/// Diagnostic summary for the boot demo.
pub fn summary() -> (bool, u16, u16, u16) {
    (
        acpi_available(),
        PM1A_CNT.load(Ordering::SeqCst),
        SLP_TYPA.load(Ordering::SeqCst) >> 10,
        RESET_PORT.load(Ordering::SeqCst),
    )
}

/// Power off the machine. Tries ACPI, then QEMU/VirtualBox port fallbacks.
pub fn shutdown() -> ! {
    crate::println!("[power] shutting down...");
    x86_64::instructions::interrupts::disable();

    let pm1a = PM1A_CNT.load(Ordering::SeqCst);
    if ACPI_OK.load(Ordering::SeqCst) && pm1a != 0 {
        let slp = SLP_TYPA.load(Ordering::SeqCst) | SLP_EN;
        unsafe { Port::<u16>::new(pm1a).write(slp) };
        let pm1b = PM1B_CNT.load(Ordering::SeqCst);
        if pm1b != 0 {
            let slpb = SLP_TYPB.load(Ordering::SeqCst) | SLP_EN;
            unsafe { Port::<u16>::new(pm1b).write(slpb) };
        }
    }
    // VM fallbacks: QEMU (new & old) and VirtualBox.
    unsafe {
        Port::<u16>::new(0x604).write(0x2000); // QEMU
        Port::<u16>::new(0xB004).write(0x2000); // QEMU (older/Bochs)
        Port::<u16>::new(0x4004).write(0x3400); // VirtualBox
    }
    // If we are still here, halt.
    loop {
        x86_64::instructions::hlt();
    }
}

/// Reboot the machine.
pub fn restart() -> ! {
    crate::println!("[power] restarting...");
    x86_64::instructions::interrupts::disable();

    // ACPI reset register (system-I/O), if the FADT provided one.
    let rp = RESET_PORT.load(Ordering::SeqCst);
    if rp != 0 {
        unsafe { Port::<u8>::new(rp).write(RESET_VALUE.load(Ordering::SeqCst) as u8) };
    }
    // 8042 keyboard-controller reset: pulse the CPU reset line.
    unsafe {
        let mut cmd = Port::<u8>::new(0x64);
        for _ in 0..10 {
            // Drain the input buffer, then send the reset pulse (0xFE).
            cmd.write(0xFE);
        }
    }
    // Last resort: triple fault via a null IDT.
    unsafe {
        let idt = x86_64::structures::DescriptorTablePointer {
            limit: 0,
            base: x86_64::VirtAddr::new(0),
        };
        x86_64::instructions::tables::lidt(&idt);
        core::arch::asm!("int3", options(noreturn));
    }
}

/// Light sleep (standby): blank the screen and halt until input arrives.
/// (ACPI S3 suspend-to-RAM is future work.) Returns when woken.
pub fn sleep() {
    crate::println!("[power] sleeping (tekan tombol / gerakkan mouse untuk bangun)...");
    // Blank the framebuffer.
    if let Some((w, h)) = crate::framebuffer::dimensions() {
        let black = alloc::vec![0u32; w * h];
        crate::framebuffer::present(&black);
    }
    // Snapshot input state; wake when it changes.
    let (_, _, _, mouse0) = crate::mouse::state();
    loop {
        x86_64::instructions::hlt();
        let woke_key = crate::keyboard::pop().is_some();
        let (_, _, _, mouse1) = crate::mouse::state();
        if woke_key || mouse1 != mouse0 {
            break;
        }
    }
    crate::println!("[power] awake.");
}
