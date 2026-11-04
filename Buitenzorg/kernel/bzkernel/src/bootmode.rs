//! Boot mode: a normal boot goes straight to the desktop; the **self-test**
//! boot runs every demo, self-test and milestone (what the smoke tests and CI
//! check). The self-test mode is switched on only from outside:
//!
//! * QEMU's fw_cfg file `opt/buitenzorg/selftest` (`-fw_cfg
//!   name=opt/buitenzorg/selftest,string=1`): the I/O ports on PCs, the MMIO
//!   device the Device Tree names (`qemu,fw-cfg-mmio`) on ARM64 / RISC-V
//!   `virt`;
//! * `bz.selftest` in the Device Tree's `/chosen/bootargs` (QEMU `-append` on
//!   the Raspberry Pi's direct boot, or a board's `cmdline.txt`).
//!
//! Real hardware therefore always boots normally.

use core::sync::atomic::{AtomicBool, Ordering};

/// The fw_cfg file that turns the self-test boot on (content "1"; "0" = off).
pub const FW_CFG_FILE: &str = "opt/buitenzorg/selftest";
/// The boot argument that does the same.
#[cfg_attr(target_arch = "x86_64", allow(dead_code))]
pub const BOOTARG: &str = "bz.selftest";

static SELFTEST: AtomicBool = AtomicBool::new(false);

/// Whether this is the self-test boot.
pub fn selftest() -> bool {
    SELFTEST.load(Ordering::Relaxed)
}

/// Decide the mode (once, early: after the paging context, before the demos).
pub fn detect(fdt: Option<&crate::dtb::Fdt>) -> bool {
    #[cfg(target_arch = "x86_64")]
    let on = {
        let _ = fdt;
        crate::arch::x86::fwcfg::file_first_byte(FW_CFG_FILE).is_some_and(|b| b != b'0')
    };
    #[cfg(not(target_arch = "x86_64"))]
    let mut on = false;
    #[cfg(not(target_arch = "x86_64"))]
    if let Some(f) = fdt {
        if let Some(i) = f.nodes.iter().position(|n| n.name == "chosen") {
            on |= f.nodes[i].strings("bootargs").any(|s| s.split_whitespace().any(|a| a == BOOTARG));
        }
        if let Some(i) = f.compatible("qemu,fw-cfg-mmio").next() {
            if let Some(&(pa, len)) = f.reg(i).first() {
                if let Some(va) = crate::memory::map_mmio(pa, len.max(0x18)) {
                    on |= mmio_file_first_byte(va, FW_CFG_FILE).is_some_and(|b| b != b'0');
                }
            }
        }
    }
    SELFTEST.store(on, Ordering::Relaxed);
    crate::println!("[boot] mode: {}", if on { "self-test (every demo + milestone)" } else { "normal (straight to the desktop)" });
    on
}

/// fw_cfg over MMIO: data register at +0 (byte reads), selector at +8
/// (16-bit, big-endian).
#[cfg(not(target_arch = "x86_64"))]
fn mmio_file_first_byte(base: u64, name: &str) -> Option<u8> {
    let select = |key: u16| unsafe { ((base + 8) as *mut u16).write_volatile(key.to_be()) };
    let byte = || unsafe { (base as *const u8).read_volatile() };
    let be32 = || u32::from_be_bytes([byte(), byte(), byte(), byte()]);
    select(0);
    if [byte(), byte(), byte(), byte()] != *b"QEMU" {
        return None;
    }
    select(0x19);
    let count = be32().min(1024);
    let mut found = None;
    for _ in 0..count {
        let _size = be32();
        let key = u16::from_be_bytes([byte(), byte()]);
        let _ = [byte(), byte()];
        let mut n = [0u8; 56];
        for b in n.iter_mut() {
            *b = byte();
        }
        let len = n.iter().position(|&b| b == 0).unwrap_or(56);
        if found.is_none() && &n[..len] == name.as_bytes() {
            found = Some(key);
        }
    }
    select(found?);
    Some(byte())
}
