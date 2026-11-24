//! Intel Wireless 8265 / 8260 (the "8000 family", `iwlmvm` firmware), step 1:
//! wake the NIC, set up a receive queue, DMA the INIT firmware image into the
//! chip through the service channel and wait for its ALIVE notification.
//! Follows Linux `drivers/net/wireless/intel/iwlwifi` (`pcie/trans.c`
//! `iwl_pcie_apm_init`, `iwl_pcie_load_given_ucode_8000`,
//! `iwl_pcie_load_firmware_chunk_fh`; `pcie/rx.c` legacy RX; `iwl-csr.h`,
//! `iwl-fh.h`, `iwl-prph.h` for the numbers). There is no QEMU model of this
//! chip, so it only runs when asked (`wifi up` on a PC) and logs every step
//! (`dmesg iwl`). Scanning and joining need the rest of the MVM command set
//! (NVM, PHY/MAC contexts, UMAC scan, host-side WPA2 handshake): later steps.
//!
//! Firmware: `iwlwifi-8265-36.ucode` from linux-firmware (Intel's
//! redistributable licence; not committed), fetched by
//! `scripts/fetch-wifi-firmware.*` and put on the PC images as
//! `/disk/WIFI/IWL8265.UCO`.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};

use crate::pci::PciDevice;

const FIRMWARE: &str = "/disk/WIFI/IWL8265.UCO";

// CSR (BAR 0)
const CSR_HW_IF_CONFIG: usize = 0x000;
const HW_IF_HAP_WAKE_L1A: u32 = 0x0008_0000;
const HW_IF_NIC_READY: u32 = 0x0040_0000;
const HW_IF_PREPARE: u32 = 0x0800_0000;
const CSR_INT: usize = 0x008;
const CSR_INT_MASK: usize = 0x00C;
const CSR_FH_INT_STATUS: usize = 0x010;
const CSR_RESET: usize = 0x020;
const RESET_SW: u32 = 1 << 7;
const CSR_GP_CNTRL: usize = 0x024;
const GP_MAC_CLOCK_READY: u32 = 1 << 0;
const GP_INIT_DONE: u32 = 1 << 2;
const GP_MAC_ACCESS_REQ: u32 = 1 << 3;
const GP_GOING_TO_SLEEP: u32 = 1 << 4;
const GP_RF_KILL_SW: u32 = 1 << 27;
const CSR_HW_REV: usize = 0x028;
const CSR_GIO: usize = 0x03C;
const GIO_L0S_DISABLED: u32 = 0x02;
const CSR_UCODE_DRV_GP1: usize = 0x054;
const CSR_UCODE_DRV_GP1_CLR: usize = 0x05C;
const GP1_SW_RFKILL: u32 = 1 << 1;
const GP1_CMD_BLOCKED: u32 = 1 << 2;
const CSR_HW_RF_ID: usize = 0x09C;
const CSR_INT_COALESCING: usize = 0x004;
const CSR_GIO_CHICKEN: usize = 0x100;
const CHICKEN_L1A_NO_L0S_RX: u32 = 0x0080_0000;
const CSR_DBG_HPET_MEM: usize = 0x240;
const CSR_DBG_LINK_PWR_MGMT: usize = 0x250;

const INT_ALIVE: u32 = 1 << 0;
const INT_SW_ERR: u32 = 1 << 25;
const INT_FH_TX: u32 = 1 << 27;
const INT_HW_ERR: u32 = 1 << 29;
const INT_FH_RX: u32 = 1 << 31;

// periphery registers through the HBUS window
const PRPH_WADDR: usize = 0x444;
const PRPH_RADDR: usize = 0x448;
const PRPH_WDAT: usize = 0x44C;
const PRPH_RDAT: usize = 0x450;
const RELEASE_CPU_RESET: u32 = 0x300C;
const RELEASE_CPU_RESET_BIT: u32 = 1 << 24;
const WFPM_GP2: u32 = 0xA030B4;
const LMPM_CHICK: u32 = 0xA01FF8;
const LMPM_CHICK_EXTENDED: u32 = 0x01;
const SCD_TXFACT: u32 = 0xA02C00 + 0x10;

// flow handler
const FH_SRVC_CHNL: usize = 9;
const fn fh_tcsr_config(ch: usize) -> usize {
    0x1D00 + 0x20 * ch
}
const fn fh_tcsr_buf_sts(ch: usize) -> usize {
    0x1D00 + 0x20 * ch + 0x8
}
const FH_SRVC_SRAM_ADDR: usize = 0x19C8;
const FH_TFDIB_CTRL0: usize = 0x1900 + 8 * FH_SRVC_CHNL;
const FH_TFDIB_CTRL1: usize = FH_TFDIB_CTRL0 + 4;
const TX_CONFIG_PAUSE: u32 = 0;
const TX_CONFIG_ENABLE: u32 = 0x8000_0000;
const TX_CONFIG_HOST_ENDTFD: u32 = 0x0010_0000;
const BUF_STS_VALID: u32 = (1 << 20) | (1 << 12) | 3;
const FH_UCODE_LOAD_STATUS: usize = 0x1AF0;
const FH_KW_MEM_ADDR: usize = 0x197C;
const FH_RX_CONFIG: usize = 0x1C00;
const FH_RX_STTS_WPTR: usize = 0x1BC0;
const FH_RX_RBDCB_BASE: usize = 0x1BC4;
const FH_RX_WPTR: usize = 0x1BC8;
const FH_RX_STATUS: usize = 0x1C44;

const CPU1_CPU2_SEPARATOR: u32 = 0xFFFF_CCCC;
const PAGING_SEPARATOR: u32 = 0xAAAA_BBBB;
const EXTENDED_START: u32 = 0x40000;
const EXTENDED_END: u32 = 0x57FFF;

const RX_QUEUE: usize = 256;
const RX_POSTED: u32 = 248;

/// The 8000-family PCI ids iwlwifi lists (8260, 4165, 8265, 8275).
pub fn supported(d: &PciDevice) -> bool {
    d.vendor_id == 0x8086 && matches!(d.device_id, 0x24F3 | 0x24F4 | 0x24F5 | 0x24F6 | 0x24FD | 0x24FB)
}

fn delay_us(us: u64) {
    let hz = crate::arch::cpu::counter_hz().max(1_000_000_000);
    let t0 = crate::arch::cpu::timestamp();
    while crate::arch::cpu::timestamp().wrapping_sub(t0) < us * hz / 1_000_000 {
        core::hint::spin_loop();
    }
}

struct Nic {
    regs: *mut u8,
    log: Vec<String>,
}

impl Nic {
    fn r(&self, off: usize) -> u32 {
        unsafe { read_volatile(self.regs.add(off) as *const u32) }
    }
    fn w(&self, off: usize, v: u32) {
        unsafe { write_volatile(self.regs.add(off) as *mut u32, v) }
    }
    fn set(&self, off: usize, bits: u32) {
        self.w(off, self.r(off) | bits)
    }
    fn clear(&self, off: usize, bits: u32) {
        self.w(off, self.r(off) & !bits)
    }
    /// Poll until `reg & mask == want`, at most `us` microseconds.
    fn poll(&self, off: usize, mask: u32, want: u32, us: u64) -> bool {
        let mut t = 0;
        loop {
            if self.r(off) & mask == want {
                return true;
            }
            if t >= us {
                return false;
            }
            delay_us(10);
            t += 10;
        }
    }
    fn note(&mut self, s: String) {
        crate::println!("[iwl] {}", s);
        self.log.push(s);
    }

    fn grab(&self) -> bool {
        self.set(CSR_GP_CNTRL, GP_MAC_ACCESS_REQ);
        delay_us(2);
        self.poll(CSR_GP_CNTRL, GP_MAC_CLOCK_READY | GP_GOING_TO_SLEEP, GP_MAC_CLOCK_READY, 15_000)
    }
    fn release(&self) {
        self.clear(CSR_GP_CNTRL, GP_MAC_ACCESS_REQ);
    }
    fn write_prph(&self, addr: u32, v: u32) -> bool {
        if !self.grab() {
            return false;
        }
        self.w(PRPH_WADDR, (addr & 0x000F_FFFF) | (3 << 24));
        self.w(PRPH_WDAT, v);
        self.release();
        true
    }
    fn read_prph(&self, addr: u32) -> Option<u32> {
        if !self.grab() {
            return None;
        }
        self.w(PRPH_RADDR, (addr & 0x000F_FFFF) | (3 << 24));
        let v = self.r(PRPH_RDAT);
        self.release();
        Some(v)
    }
    /// `iwl_write_direct32`: a register that needs the MAC awake.
    fn write_direct(&self, off: usize, v: u32) -> bool {
        if !self.grab() {
            return false;
        }
        self.w(off, v);
        self.release();
        true
    }
    fn read_direct(&self, off: usize) -> Option<u32> {
        if !self.grab() {
            return None;
        }
        let v = self.r(off);
        self.release();
        Some(v)
    }

    fn dump(&mut self, why: &str) {
        let s = format!(
            "{}: INT {:#010x} FH_INT {:#010x} RESET {:#010x} GP_CNTRL {:#010x} GP1 {:#010x} RX_STATUS {:#010x}",
            why,
            self.r(CSR_INT),
            self.r(CSR_FH_INT_STATUS),
            self.r(CSR_RESET),
            self.r(CSR_GP_CNTRL),
            self.r(CSR_UCODE_DRV_GP1),
            self.r(FH_RX_STATUS)
        );
        self.note(s);
    }

    /// `iwl_pcie_prepare_card_hw`.
    fn prepare(&mut self) -> Result<(), String> {
        let ready = |n: &Nic| {
            n.set(CSR_HW_IF_CONFIG, HW_IF_NIC_READY);
            n.poll(CSR_HW_IF_CONFIG, HW_IF_NIC_READY, HW_IF_NIC_READY, 50)
        };
        if ready(self) {
            return Ok(());
        }
        self.set(CSR_DBG_LINK_PWR_MGMT, 0x8000_0000);
        delay_us(1000);
        for _ in 0..10 {
            self.set(CSR_HW_IF_CONFIG, HW_IF_PREPARE);
            let mut t = 0;
            while t < 150_000 {
                if ready(self) {
                    return Ok(());
                }
                delay_us(200);
                t += 200;
            }
            delay_us(25_000);
        }
        Err(format!("NIC never became ready (HW_IF_CONFIG {:#010x})", self.r(CSR_HW_IF_CONFIG)))
    }

    /// `iwl_pcie_apm_init` for the 8000 family (no APMG).
    fn apm_init(&mut self) -> Result<(), String> {
        self.set(CSR_GIO_CHICKEN, CHICKEN_L1A_NO_L0S_RX);
        self.set(CSR_DBG_HPET_MEM, 0xFFFF_0000);
        self.set(CSR_HW_IF_CONFIG, HW_IF_HAP_WAKE_L1A);
        self.set(CSR_GIO, GIO_L0S_DISABLED);
        self.set(CSR_GP_CNTRL, GP_INIT_DONE);
        delay_us(2);
        if !self.poll(CSR_GP_CNTRL, GP_MAC_CLOCK_READY, GP_MAC_CLOCK_READY, 25_000) {
            return Err(format!("MAC clock not ready (GP_CNTRL {:#010x})", self.r(CSR_GP_CNTRL)));
        }
        Ok(())
    }

    /// DMA one chunk (at most a page) into the device's SRAM at `dst` through
    /// the service channel, then wait for the "uCode load" interrupt.
    fn load_chunk(&mut self, dst: u32, phys: u64, len: u32) -> Result<(), String> {
        self.w(CSR_INT, INT_FH_TX);
        self.w(CSR_FH_INT_STATUS, 0x3);
        if !self.grab() {
            return Err("no NIC access for a firmware chunk".into());
        }
        self.w(fh_tcsr_config(FH_SRVC_CHNL), TX_CONFIG_PAUSE);
        self.w(FH_SRVC_SRAM_ADDR, dst);
        self.w(FH_TFDIB_CTRL0, phys as u32);
        self.w(FH_TFDIB_CTRL1, (((phys >> 32) as u32 & 0xF) << 28) | len);
        self.w(fh_tcsr_buf_sts(FH_SRVC_CHNL), BUF_STS_VALID);
        self.w(fh_tcsr_config(FH_SRVC_CHNL), TX_CONFIG_ENABLE | TX_CONFIG_HOST_ENDTFD);
        self.release();
        let mut t = 0u64;
        loop {
            if self.r(CSR_INT) & INT_FH_TX != 0 || self.r(CSR_FH_INT_STATUS) & 0x2 != 0 {
                self.w(CSR_INT, INT_FH_TX);
                self.w(CSR_FH_INT_STATUS, 0x3);
                return Ok(());
            }
            if t > 5_000_000 {
                self.dump("chunk timeout");
                return Err(format!("firmware chunk to {:#x} ({} bytes) never completed", dst, len));
            }
            delay_us(20);
            t += 20;
        }
    }

    fn load_section(&mut self, dst: u32, data: &[u8], page: (u64, *mut u8)) -> Result<(), String> {
        let mut off = 0;
        while off < data.len() {
            let n = (data.len() - off).min(4096);
            unsafe { core::ptr::copy_nonoverlapping(data[off..].as_ptr(), page.1, n) };
            crate::arch::cpu::dma_sync(page.1 as u64, n);
            let at = dst + off as u32;
            let ext = (EXTENDED_START..=EXTENDED_END).contains(&at);
            if ext {
                let v = self.read_prph(LMPM_CHICK).unwrap_or(0);
                self.write_prph(LMPM_CHICK, v | LMPM_CHICK_EXTENDED);
            }
            let r = self.load_chunk(at, page.0, n as u32);
            if ext {
                let v = self.read_prph(LMPM_CHICK).unwrap_or(0);
                self.write_prph(LMPM_CHICK, v & !LMPM_CHICK_EXTENDED);
            }
            r?;
            off += n;
        }
        Ok(())
    }

    /// `iwl_pcie_load_cpu_sections_8000`: the sections of one CPU, up to the
    /// separator; returns the index after it.
    fn load_cpu(&mut self, secs: &[(u32, &[u8])], from: usize, cpu: u32, page: (u64, *mut u8)) -> Result<usize, String> {
        let shift = if cpu == 1 { 0 } else { 16 };
        let mut sec_num: u32 = 1;
        let mut i = from;
        let mut loaded = 0;
        while i < secs.len() {
            let (dst, data) = secs[i];
            if dst == CPU1_CPU2_SEPARATOR || dst == PAGING_SEPARATOR {
                break;
            }
            self.load_section(dst, data, page)?;
            let v = self.read_direct(FH_UCODE_LOAD_STATUS).unwrap_or(0);
            self.write_direct(FH_UCODE_LOAD_STATUS, v | (sec_num << shift));
            sec_num = (sec_num << 1) | 1;
            loaded += data.len();
            i += 1;
        }
        self.write_direct(FH_UCODE_LOAD_STATUS, if cpu == 1 { 0xFFFF } else { 0xFFFF_FFFF });
        let s = format!("CPU{}: {} section(s), {} KiB", cpu, sec_num.count_ones() - 1, loaded / 1024);
        self.note(s);
        Ok(i)
    }
}

/// The firmware file's TLV sections of one image type (19 regular, 20 INIT).
fn sections(fw: &[u8], kind: u32) -> Result<(String, Vec<(u32, &[u8])>), String> {
    if fw.len() < 88 || fw[0..4] != [0; 4] || u32::from_le_bytes([fw[4], fw[5], fw[6], fw[7]]) != 0x0A4C_5749 {
        return Err("not an iwlwifi TLV firmware file".into());
    }
    let name: String = fw[8..72].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
    let mut out = Vec::new();
    let mut p = 88;
    while p + 8 <= fw.len() {
        let t = u32::from_le_bytes([fw[p], fw[p + 1], fw[p + 2], fw[p + 3]]);
        let l = u32::from_le_bytes([fw[p + 4], fw[p + 5], fw[p + 6], fw[p + 7]]) as usize;
        let body = fw.get(p + 8..p + 8 + l).ok_or("firmware file truncated")?;
        if t == kind && l >= 4 {
            out.push((u32::from_le_bytes([body[0], body[1], body[2], body[3]]), &body[4..]));
        }
        p += 8 + ((l + 3) & !3);
    }
    Ok((name, out))
}

/// `wifi up` on a PC: bring the adapter up to the firmware's ALIVE.
pub fn bring_up(dev: &PciDevice) -> Vec<String> {
    let mut nic = Nic { regs: core::ptr::null_mut(), log: Vec::new() };
    if let Err(e) = run(dev, &mut nic) {
        nic.note(format!("failed: {}", e));
    }
    nic.log
}

fn run(dev: &PciDevice, nic: &mut Nic) -> Result<(), String> {
    let fw = crate::vfs::read(FIRMWARE)
        .map_err(|_| format!("firmware {} missing (scripts/fetch-wifi-firmware, then rebuild the image)", FIRMWARE))?;
    let (name, init) = sections(&fw, 20)?;
    nic.note(format!("firmware '{}': INIT image {} section(s)", name, init.len()));
    let bar = dev.mem_bar(0).ok_or("no memory BAR")?;
    dev.enable_mem_and_bus_master();
    let va = crate::memory::map_mmio(bar, 0x2000).ok_or("cannot map the registers")?;
    nic.regs = va as *mut u8;
    let rev = nic.r(CSR_HW_REV);
    let rf = nic.r(CSR_HW_RF_ID);
    nic.note(format!("BAR0 {:#x}, HW_REV {:#010x}, RF_ID {:#010x}", bar, rev, rf));
    if rev == 0xFFFF_FFFF {
        return Err("registers read all ones (device asleep or not mapped)".into());
    }

    // iwl_trans_pcie_start_fw
    nic.prepare()?;
    nic.w(CSR_RESET, RESET_SW);
    delay_us(6000);
    nic.w(CSR_INT_MASK, 0);
    nic.w(CSR_INT, 0xFFFF_FFFF);
    nic.w(CSR_FH_INT_STATUS, 0xFFFF_FFFF);
    nic.w(CSR_UCODE_DRV_GP1_CLR, GP1_SW_RFKILL);
    nic.w(CSR_UCODE_DRV_GP1_CLR, GP1_CMD_BLOCKED);
    nic.prepare()?;
    nic.apm_init()?;
    let gp = nic.r(CSR_GP_CNTRL);
    nic.note(format!(
        "MAC clock ready; GP_CNTRL {:#010x} -- radio switch {}",
        gp,
        if gp & GP_RF_KILL_SW != 0 { "on" } else { "OFF (hardware rfkill: Fn key / BIOS)" }
    ));

    // RX (legacy single queue): 256 descriptors, one 4 KiB buffer each.
    let (rbd_phys, rbd) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
    let (stts_phys, stts) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
    let mut bufs = Vec::with_capacity(RX_QUEUE);
    for i in 0..RX_QUEUE {
        let (p, v) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
        unsafe { write_volatile((rbd as *mut u32).add(i), (p >> 8) as u32) };
        bufs.push((p, v));
    }
    crate::arch::cpu::dma_sync(rbd as u64, 4096);
    crate::arch::cpu::dma_sync(stts as u64, 4096);
    let (kw_phys, _) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
    if !nic.grab() {
        return Err("no NIC access for the RX setup".into());
    }
    nic.w(FH_RX_CONFIG, 0);
    nic.w(FH_RX_WPTR, 0);
    nic.w(FH_RX_RBDCB_BASE, (rbd_phys >> 8) as u32);
    nic.w(FH_RX_STTS_WPTR, (stts_phys >> 4) as u32);
    nic.w(FH_RX_CONFIG, 0x8000_0000 | 0x4 | 0x1000 | (0x11 << 4) | (8 << 20));
    nic.w(FH_KW_MEM_ADDR, (kw_phys >> 4) as u32);
    nic.release();
    unsafe { write_volatile(nic.regs.add(CSR_INT_COALESCING), 0x40u8) };
    nic.write_direct(FH_RX_WPTR, RX_POSTED);
    nic.write_prph(SCD_TXFACT, 0);

    nic.w(CSR_UCODE_DRV_GP1_CLR, GP1_SW_RFKILL);
    nic.w(CSR_UCODE_DRV_GP1_CLR, GP1_CMD_BLOCKED);
    nic.w(CSR_INT, 0xFFFF_FFFF);

    // iwl_pcie_load_given_ucode_8000
    let wfpm = nic.read_prph(WFPM_GP2);
    nic.write_prph(WFPM_GP2, 0x0101_0101);
    if !nic.write_prph(RELEASE_CPU_RESET, RELEASE_CPU_RESET_BIT) {
        return Err("no NIC access to release the CPU reset".into());
    }
    nic.note(format!("CPU reset released (WFPM_GP2 was {:#x?})", wfpm));
    let page = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
    let t0 = crate::arch::cpu::nanos();
    let next = nic.load_cpu(&init, 0, 1, page)?;
    nic.load_cpu(&init, next + 1, 2, page)?;
    let ms = (crate::arch::cpu::nanos() - t0) / 1_000_000;
    nic.note(format!("INIT image loaded in {} ms; waiting for ALIVE", ms));

    // ALIVE arrives as the first packet on the RX queue.
    let mut t = 0u64;
    loop {
        crate::arch::cpu::dma_sync(stts as u64, 16);
        let closed = unsafe { read_volatile(stts as *const u16) } & 0xFFF;
        let int = nic.r(CSR_INT);
        if closed != 0 {
            nic.note(format!("RX: {} buffer(s) closed (CSR_INT {:#010x})", closed, int));
            break;
        }
        if int & (INT_SW_ERR | INT_HW_ERR) != 0 {
            nic.dump("firmware error");
            return Err(String::from("the firmware reported an error before ALIVE"));
        }
        if t > 2_000_000 {
            nic.dump("no ALIVE");
            return Err(format!("no ALIVE within 2 s (CSR_INT alive bit {})", int & INT_ALIVE != 0));
        }
        delay_us(100);
        t += 100;
    }
    nic.w(CSR_INT, INT_FH_RX | INT_ALIVE);
    let (_, buf) = bufs[0];
    crate::arch::cpu::dma_sync(buf as u64, 4096);
    let rd = |o: usize| unsafe { read_volatile(buf.add(o) as *const u32) };
    let len_flags = rd(0);
    let cmd = rd(4) & 0xFF;
    let group = (rd(4) >> 8) & 0xFF;
    let words: Vec<String> = (0..12).map(|i| format!("{:08x}", rd(i * 4))).collect();
    nic.note(format!("packet: len {} cmd {:#x} group {:#x}: {}", len_flags & 0x3FFF, cmd, group, words.join(" ")));
    let status = rd(8) & 0xFFFF;
    if cmd != 0x1 {
        return Err(format!("first packet is cmd {:#x}, not ALIVE (0x1)", cmd));
    }
    if status != 0xCAFE {
        return Err(format!("ALIVE status {:#x} (expected 0xcafe)", status));
    }
    nic.note(format!(
        "ALIVE ok (status 0xcafe, ucode {}.{}); next step: NVM read, MAC address, scan",
        rd(12),
        rd(16)
    ));
    crate::println!("[kernel] MILESTONE: IWLWIFI ALIVE OK (Intel {:04x}, INIT firmware in {} ms)", dev.device_id, ms);
    Ok(())
}
