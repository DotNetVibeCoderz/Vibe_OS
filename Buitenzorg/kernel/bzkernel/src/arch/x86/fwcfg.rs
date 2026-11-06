//! QEMU's firmware configuration device on PCs (I/O ports 0x510 selector,
//! 0x511 data): just enough to look a named file up -- the boot mode switch
//! (`bootmode.rs`). On real hardware the ports float (0xFF) and the "QEMU"
//! signature check fails, so nothing is found.

use x86_64::instructions::port::Port;

const SELECTOR: u16 = 0x510;
const DATA: u16 = 0x511;
const SIGNATURE: u16 = 0x0000;
const FILE_DIR: u16 = 0x0019;

fn select(key: u16) {
    unsafe { Port::<u16>::new(SELECTOR).write(key) };
}

fn byte() -> u8 {
    unsafe { Port::<u8>::new(DATA).read() }
}

fn be32() -> u32 {
    u32::from_be_bytes([byte(), byte(), byte(), byte()])
}

/// Whether this is QEMU (its fw_cfg device answers with the signature).
pub fn present() -> bool {
    select(SIGNATURE);
    [byte(), byte(), byte(), byte()] == *b"QEMU"
}

/// The first byte of fw_cfg file `name`, if QEMU provides one.
pub fn file_first_byte(name: &str) -> Option<u8> {
    let mut b = [0u8; 1];
    (file_read(name, &mut b)? > 0).then_some(b[0])
}

/// Read fw_cfg file `name` into `out` (as much as fits); the bytes read.
pub fn file_read(name: &str, out: &mut [u8]) -> Option<usize> {
    select(SIGNATURE);
    if [byte(), byte(), byte(), byte()] != *b"QEMU" {
        return None;
    }
    select(FILE_DIR);
    let count = be32().min(1024);
    let want = name.as_bytes();
    let mut found = None;
    for _ in 0..count {
        let size = be32();
        let key = u16::from_be_bytes([byte(), byte()]);
        let _ = [byte(), byte()]; // reserved
        let mut n = [0u8; 56];
        for b in n.iter_mut() {
            *b = byte();
        }
        let len = n.iter().position(|&b| b == 0).unwrap_or(56);
        if found.is_none() && &n[..len] == want {
            found = Some((key, size as usize));
        }
    }
    let (key, size) = found?;
    select(key);
    let n = size.min(out.len());
    for b in out[..n].iter_mut() {
        *b = byte();
    }
    Some(n)
}
