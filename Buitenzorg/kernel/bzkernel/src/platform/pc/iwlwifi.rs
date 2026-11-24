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
/// SHA-256 pinned in scripts/fetch-wifi-firmware.* (linux-firmware 20241110).
const FIRMWARE_SHA256: &str = "d7d84c9b6a3b1976acb97a2bec7102e24a6b5d794110c0dcbd107ab5d9877e5f";

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
const CSR_MAC_SHADOW_REG_CTRL: usize = 0x0A8;
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
/// CSR_INI_SET_MASK: FH_RX, HW_ERR, FH_TX, SW_ERR, RF_KILL, SW_RX, WAKEUP,
/// RX_PERIODIC, ALIVE.
const INI_SET_MASK: u32 = (1 << 31) | (1 << 29) | (1 << 27) | (1 << 25) | (1 << 7) | (1 << 3) | (1 << 1) | (1 << 28) | 1;

// periphery registers through the HBUS window
const HBUS_MEM_RADDR: usize = 0x40C;
const HBUS_MEM_WADDR: usize = 0x410;
const HBUS_MEM_WDAT: usize = 0x418;
const HBUS_TARG_WRPTR: usize = 0x460;
const FH_TX_CHICKEN_BITS: usize = 0x1E98;
const SCD_SRAM_BASE_ADDR: u32 = 0xA02C00;
const SCD_DRAM_BASE_ADDR: u32 = 0xA02C00 + 0x8;
const SCD_CHAINEXT_EN: u32 = 0xA02C00 + 0x244;
const SCD_AGGR_SEL: u32 = 0xA02C00 + 0x248;
const SCD_CONTEXT_MEM_LOWER: u32 = 0x600;
const SCD_TRANS_TBL_MEM_UPPER: u32 = 0x838;
const fn scd_queue_rdptr(q: u32) -> u32 {
    if q < 20 { 0xA02C00 + 0x68 + 4 * q } else { 0xA02C00 + 0x2B4 + 4 * (q - 20) }
}
const fn scd_queue_status(q: u32) -> u32 {
    if q < 20 { 0xA02C00 + 0x10C + 4 * q } else { 0xA02C00 + 0x334 + 4 * (q - 20) }
}
/// iwlmvm's DQA command queue and the command FIFO.
const CMD_QUEUE: u32 = 0;
const CMD_FIFO: u32 = 7;
/// WFMP_MAC_ADDR_0/1: the OTP MAC address (8000 family).
const WFMP_MAC_ADDR_0: u32 = 0xA03080;
const WFMP_MAC_ADDR_1: u32 = 0xA03084;
// host command ids (group 1, wide header, like iwlmvm)
const NVM_ACCESS_CMD: u8 = 0x88;
const TX_ANT_CONFIGURATION_CMD: u8 = 0x98;
const PHY_CONFIGURATION_CMD: u8 = 0x6A;
const INIT_COMPLETE_NOTIF: u8 = 0x4;
const CALIB_RES_NOTIF_PHY_DB: u8 = 0x6B;
const NVM_CHUNK: usize = 2048;
const HBUS_MEM_RDAT: usize = 0x41C;
const PRPH_WADDR: usize = 0x444;
const PRPH_RADDR: usize = 0x448;
const PRPH_WDAT: usize = 0x44C;
const PRPH_RDAT: usize = 0x450;
const RELEASE_CPU_RESET: u32 = 0x300C;
const RELEASE_CPU_RESET_BIT: u32 = 1 << 24;
const WFPM_GP2: u32 = 0xA030B4;
const LMPM_CHICK: u32 = 0xA01FF8;
// secure-boot status of each CPU and the firmware's program counters, read
// when ALIVE does not come (what iwlmvm prints on an ALIVE timeout)
const SB_CPU_1_STATUS: u32 = 0xA01E30;
const SB_CPU_2_STATUS: u32 = 0xA01E34;
const UMAC_CURRENT_PC: u32 = 0xA05C18;
const LMAC1_CURRENT_PC: u32 = 0xA05C1C;
const LMPM_CHICK_EXTENDED: u32 = 0x01;
const SCD_TXFACT: u32 = 0xA02C00 + 0x10;
const SCD_GP_CTRL: u32 = 0xA02C00 + 0x1A8;
const SCD_GP_AUTO_ACTIVE: u32 = 1 << 18;
const SCD_GP_31_QUEUES: u32 = 1 << 0;
/// TX queues of the 8000 family (`num_of_queues`).
const TX_QUEUES: usize = 31;
/// FH_MEM_CBBC_QUEUE: where the device finds queue `q`'s TFD ring.
const fn fh_cbbc(q: usize) -> usize {
    if q < 16 {
        0x19D0 + 4 * q
    } else if q < 20 {
        0x1BF0 + 4 * (q - 16)
    } else {
        0x1B20 + 4 * (q - 20)
    }
}

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
const FH_RX_RDPTR: usize = 0x1BCC;
const FH_RX_CHNL0_RBDCB_WPTR: usize = 0x1C08;
const FH_RX_FLUSH_RB_REQ: usize = 0x1C10;
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
    pci: Option<PciDevice>,
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
            "{}: INT {:#010x} FH_INT {:#010x} RESET {:#010x}",
            why,
            self.r(CSR_INT),
            self.r(CSR_FH_INT_STATUS),
            self.r(CSR_RESET),
        );
        self.note(s);
        let s = format!(
            "  GP_CNTRL {:#010x} GP1 {:#010x} RX_STATUS {:#010x} LOAD_STATUS {:#010x}",
            self.r(CSR_GP_CNTRL),
            self.r(CSR_UCODE_DRV_GP1),
            self.r(FH_RX_STATUS),
            self.r(FH_UCODE_LOAD_STATUS)
        );
        self.note(s);
        if let Some(d) = self.pci {
            let e = crate::pci::dma_error_bits(&d);
            self.note(format!("  {}", e));
        }
    }

    /// A word of device SRAM (`iwl_trans_read_mem32`).
    fn read_mem(&self, addr: u32) -> Option<u32> {
        if !self.grab() {
            return None;
        }
        self.w(HBUS_MEM_RADDR, addr);
        let v = self.r(HBUS_MEM_RDAT);
        self.release();
        Some(v)
    }

    /// Read back every loaded section, one word per 4 KiB plus its last
    /// word (addresses in the extended range read with LMPM_CHICK set, as
    /// they were written): tells a DMA or address-mapping problem apart
    /// from a firmware that refuses to start.
    fn verify_sram(&mut self, secs: &[(u32, &[u8])]) {
        let mut bad_total = 0;
        let mut ok_secs = String::new();
        for &(dst, data) in secs {
            if dst == PAGING_SEPARATOR {
                break;
            }
            if dst == CPU1_CPU2_SEPARATOR || data.len() < 4 {
                continue;
            }
            let mut offs: Vec<usize> = (0..data.len() / 4096 + 1).map(|i| i * 4096).filter(|&o| o + 4 <= data.len()).collect();
            offs.push((data.len() - 4) & !3);
            let (mut good, mut first_bad) = (0, None);
            for &o in &offs {
                let at = dst + o as u32;
                let want = u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
                let ext = (EXTENDED_START..=EXTENDED_END).contains(&at);
                if ext {
                    let v = self.read_prph(LMPM_CHICK).unwrap_or(0);
                    self.write_prph(LMPM_CHICK, v | LMPM_CHICK_EXTENDED);
                }
                let got = self.read_mem(at);
                if ext {
                    let v = self.read_prph(LMPM_CHICK).unwrap_or(0);
                    self.write_prph(LMPM_CHICK, v & !LMPM_CHICK_EXTENDED);
                }
                if got == Some(want) {
                    good += 1;
                } else if first_bad.is_none() {
                    first_bad = Some((o, got.unwrap_or(0xFFFF_FFFF), want));
                }
            }
            match first_bad {
                None => ok_secs.push_str(&format!(" {:x}", dst)),
                Some((o, g, w)) => {
                    bad_total += 1;
                    self.note(format!("  sram {:x}: {}/{} words ok; first bad +{:#x}: {:08x}, want {:08x}", dst, good, offs.len(), o, g, w));
                }
            }
        }
        self.note(format!("  sram all words ok:{}{}", ok_secs, if bad_total == 0 { "" } else { " (others above)" }));
    }

    /// What iwlmvm logs when ALIVE times out: secure-boot status per CPU
    /// and where each firmware CPU is executing (read twice: a moving PC
    /// means the firmware runs but did not report), plus the RX status page
    /// and the first words of RB 0.
    fn firmware_state(&mut self, stts: *mut u8, rb0: *mut u8) {
        let rd = |n: &Nic, a| n.read_prph(a).map(|v| format!("{:#010x}", v)).unwrap_or_else(|| String::from("?"));
        let s = format!(
            "  SB_CPU1 {} SB_CPU2 {} UMAC_PC {} LMAC_PC {}",
            rd(self, SB_CPU_1_STATUS), rd(self, SB_CPU_2_STATUS), rd(self, UMAC_CURRENT_PC), rd(self, LMAC1_CURRENT_PC)
        );
        self.note(s);
        delay_us(1000);
        let s = format!("  again: UMAC_PC {} LMAC_PC {} WFPM_GP2 {}", rd(self, UMAC_CURRENT_PC), rd(self, LMAC1_CURRENT_PC), rd(self, WFPM_GP2));
        self.note(s);
        crate::arch::cpu::dma_sync(stts as u64, 16);
        crate::arch::cpu::dma_sync(rb0 as u64, 64);
        let w = |p: *mut u8, i: usize| unsafe { read_volatile((p as *const u32).add(i)) };
        let s = format!(
            "  rb_status {:08x} {:08x}; rb0 {:08x} {:08x} {:08x} {:08x}",
            w(stts, 0), w(stts, 1), w(rb0, 0), w(rb0, 1), w(rb0, 2), w(rb0, 3)
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
                self.dump(&format!("chunk timeout (page {:#x})", phys));
                return Err(format!("firmware chunk to {:#x} ({} bytes) never completed", dst, len));
            }
            delay_us(20);
            t += 20;
        }
    }

    fn load_section(&mut self, dst: u32, data: &[u8], page: (u64, *mut u8, usize)) -> Result<(), String> {
        let mut off = 0;
        while off < data.len() {
            let n = (data.len() - off).min(page.2);
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
    fn load_cpu(&mut self, secs: &[(u32, &[u8])], from: usize, cpu: u32, page: (u64, *mut u8, usize)) -> Result<usize, String> {
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
        // iwl_enable_interrupts (the PCI INTx line is disabled: we poll).
        self.w(CSR_INT_MASK, INI_SET_MASK);
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

/// The first 32-bit value of a TLV of type `kind` (PHY_SKU and the like).
fn tlv_u32(fw: &[u8], kind: u32) -> Option<u32> {
    let mut p = 88;
    while p + 8 <= fw.len() {
        let t = u32::from_le_bytes([fw[p], fw[p + 1], fw[p + 2], fw[p + 3]]);
        let l = u32::from_le_bytes([fw[p + 4], fw[p + 5], fw[p + 6], fw[p + 7]]) as usize;
        if t == kind && l >= 4 && p + 12 <= fw.len() {
            return Some(u32::from_le_bytes([fw[p + 8], fw[p + 9], fw[p + 10], fw[p + 11]]));
        }
        p += 8 + ((l + 3) & !3);
    }
    None
}

/// `wifi up` on a PC: bring the adapter up to the firmware's ALIVE.
pub fn bring_up(dev: &PciDevice) -> Vec<String> {
    let mut nic = Nic { regs: core::ptr::null_mut(), log: Vec::new(), pci: None };
    if let Err(e) = run(dev, &mut nic) {
        nic.note(format!("failed: {}", e));
        // Firmware loaded but never reported: whatever state the boot
        // firmware (or an earlier attempt) left the device in, a PCIe
        // function-level reset clears it. One more try after that.
        if e.starts_with("no ALIVE") {
            match crate::pci::function_level_reset(dev) {
                Ok(()) => {
                    nic.note(String::from("PCIe function-level reset done; second attempt"));
                    if let Err(e) = run(dev, &mut nic) {
                        nic.note(format!("failed again: {}", e));
                    }
                }
                Err(e) => nic.note(format!("no second attempt: {}", e)),
            }
        }
    }
    nic.log
}

fn run(dev: &PciDevice, nic: &mut Nic) -> Result<(), String> {
    let fw = crate::vfs::read(FIRMWARE)
        .map_err(|_| format!("firmware {} missing (scripts/fetch-wifi-firmware, then rebuild the image)", FIRMWARE))?;
    let (name, init) = sections(&fw, 20)?;
    {
        use sha2::{Digest, Sha256};
        let h = Sha256::digest(&fw);
        let hex: String = h.iter().map(|b| format!("{:02x}", b)).collect();
        if hex == FIRMWARE_SHA256 {
            nic.note(format!("firmware file {} bytes, SHA-256 ok", fw.len()));
        } else {
            nic.note(format!("firmware file {} bytes, SHA-256 MISMATCH {}", fw.len(), &hex[..16]));
        }
    }
    nic.note(format!("firmware '{}': INIT image {} section(s)", name, init.len()));
    let bar = dev.mem_bar(0).ok_or("no memory BAR")?;
    for line in crate::pci::enable_upstream_bridges(dev) {
        nic.note(line);
    }
    dev.enable_mem_and_bus_master();
    // We poll CSR_INT: keep the legacy interrupt line quiet (command bit 10).
    let cmd = dev.read32(0x04);
    crate::pci::write_config_u32(dev.bus, dev.slot, dev.function, 0x04, (cmd & 0xFFFF) | 0x400);
    // Linux iwl_pci_probe: PCI retry timeout off (config 0x41).
    let r40 = dev.read32(0x40);
    crate::pci::write_config_u32(dev.bus, dev.slot, dev.function, 0x40, r40 & !0xFF00);
    nic.pci = Some(*dev);
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

    // iwl_mvm_nic_config: MAC step/dash from HW_REV, radio type/step/dash
    // from the firmware's PHY_SKU TLV (23). No RADIO_SI on the 8000 family.
    let phy = tlv_u32(&fw, 23).unwrap_or(0);
    let mut cfg = ((rev >> 2) & 0x3) << 2 | (rev & 0x3);
    cfg |= (phy & 0x3) << 10 | ((phy >> 2) & 0x3) << 14 | ((phy >> 4) & 0x3) << 12;
    let mask = 0x3 | 0xC | 0xC00 | 0x3000 | 0xC000 | 0x200 | 0x100;
    let v = (nic.r(CSR_HW_IF_CONFIG) & !mask) | cfg;
    nic.w(CSR_HW_IF_CONFIG, v);
    // iwl8000_base_params.shadow_reg_enable
    nic.set(CSR_MAC_SHADOW_REG_CTRL, 0x800F_FFFF);
    nic.note(format!("nic config: PHY_SKU {:#x}, HW_IF_CONFIG {:#010x}", phy, nic.r(CSR_HW_IF_CONFIG)));

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
    nic.w(FH_RX_CHNL0_RBDCB_WPTR, 0);
    nic.w(FH_RX_FLUSH_RB_REQ, 0);
    nic.w(FH_RX_RDPTR, 0);
    nic.w(FH_RX_WPTR, 0);
    nic.w(FH_RX_RBDCB_BASE, (rbd_phys >> 8) as u32);
    nic.w(FH_RX_STTS_WPTR, (stts_phys >> 4) as u32);
    nic.w(FH_RX_CONFIG, 0x8000_0000 | 0x4 | 0x1000 | (0x11 << 4) | (8 << 20));
    nic.w(FH_KW_MEM_ADDR, (kw_phys >> 4) as u32);
    nic.release();
    unsafe { write_volatile(nic.regs.add(CSR_INT_COALESCING), 0x40u8) };
    nic.write_direct(FH_RX_WPTR, RX_POSTED);
    nic.write_prph(SCD_TXFACT, 0);
    // iwl_pcie_tx_init: every queue's TFD ring base (one zeroed ring for
    // now -- nothing is queued before ALIVE), then the scheduler modes.
    let (tfd_phys, _) = crate::memory::alloc_dma_contiguous(8).ok_or("out of memory")?;
    for q in 0..TX_QUEUES {
        nic.write_direct(fh_cbbc(q), (tfd_phys >> 8) as u32);
    }
    let gp = nic.read_prph(SCD_GP_CTRL).unwrap_or(0);
    nic.write_prph(SCD_GP_CTRL, gp | SCD_GP_AUTO_ACTIVE | SCD_GP_31_QUEUES);

    nic.w(CSR_UCODE_DRV_GP1_CLR, GP1_SW_RFKILL);
    nic.w(CSR_UCODE_DRV_GP1_CLR, GP1_CMD_BLOCKED);
    nic.w(CSR_INT, 0xFFFF_FFFF);

    // iwl_pcie_load_given_ucode_8000
    let wfpm = nic.read_prph(WFPM_GP2);
    nic.write_prph(WFPM_GP2, 0x0101_0101);
    if !nic.write_prph(RELEASE_CPU_RESET, RELEASE_CPU_RESET_BIT) {
        return Err("no NIC access to release the CPU reset".into());
    }
    nic.note(format!("CPU reset released (WFPM_GP2 was {:#x})", wfpm.unwrap_or(0xFFFF_FFFF)));
    // Chunks of FH_MEM_TB_MAX_LENGTH (128 KiB) like Linux; one page if no
    // contiguous run is free.
    let page = match crate::memory::alloc_dma_contiguous(32) {
        Some((p, v)) => (p, v, 0x20000),
        None => {
            let (p, v) = crate::memory::alloc_dma_frame().ok_or("out of memory")?;
            (p, v, 4096)
        }
    };
    nic.note(format!("load chunks of {} KiB", page.2 / 1024));
    let t0 = crate::arch::cpu::nanos();
    let next = nic.load_cpu(&init, 0, 1, page)?;
    nic.load_cpu(&init, next + 1, 2, page)?;
    let ms = (crate::arch::cpu::nanos() - t0) / 1_000_000;
    nic.note(format!("INIT image loaded in {} ms; waiting for ALIVE", ms));
    let sb = |n: &Nic| (n.read_prph(SB_CPU_1_STATUS).unwrap_or(0xFFFF_FFFF), n.read_prph(SB_CPU_2_STATUS).unwrap_or(0xFFFF_FFFF));
    let (a0, b0) = sb(nic);
    delay_us(20_000);
    let (a1, b1) = sb(nic);
    nic.note(format!("secure boot just after load: CPU1 {:#x} CPU2 {:#x}; +20 ms: CPU1 {:#x} CPU2 {:#x}", a0, b0, a1, b1));

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
            nic.firmware_state(stts, bufs[0].1);
            nic.verify_sram(&init);
            return Err(format!("no ALIVE within 2 s (CSR_INT alive bit {})", int & INT_ALIVE != 0));
        }
        delay_us(100);
        t += 100;
    }
    nic.w(CSR_INT, INT_FH_RX | INT_ALIVE);
    let mut rx = Rx { stts, bufs, rb: 0, off: 0, consumed: 0, backlog: Vec::new() };
    let pkt = rx.next(nic, 100_000).ok_or("RX closed a buffer but holds no packet")?;
    let rd = |o: usize| pkt.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(0);
    let cmd = pkt[4];
    let group = pkt[5];
    let words: Vec<String> = (0..12).map(|i| format!("{:08x}", rd(i * 4))).collect();
    nic.note(format!("packet: len {} cmd {:#x} group {:#x}: {}", rd(0) & 0x3FFF, cmd, group, words.join(" ")));
    let status = rd(8) & 0xFFFF;
    if cmd != 0x1 {
        return Err(format!("first packet is cmd {:#x}, not ALIVE (0x1)", cmd));
    }
    if status != 0xCAFE {
        return Err(format!("ALIVE status {:#x} (expected 0xcafe)", status));
    }
    // iwl_alive_ntf_v3: status, lmac (major, minor, ver, timestamp, error
    // table, log table, cpu regs, dbgm, alive counter, scd base, ...).
    let scd_alive = rd(8 + 4 + 40);
    nic.note(format!("ALIVE ok (status 0xcafe, ucode {}.{}), SCD base {:#x}", rd(12), rd(16), scd_alive));
    crate::println!("[kernel] MILESTONE: IWLWIFI ALIVE OK (Intel {:04x}, INIT firmware in {} ms)", dev.device_id, ms);

    // ---- step 2: scheduler + command queue (iwl_pcie_tx_start) ----
    let ring = CmdRing::new()?;
    nic.tx_start(scd_alive, &ring)?;
    let mut tx = Tx { ring, wr: 0 };

    // The OTP MAC address (what Linux falls back to without an override).
    let m0 = nic.read_prph(WFMP_MAC_ADDR_0).unwrap_or(0);
    let m1 = nic.read_prph(WFMP_MAC_ADDR_1).unwrap_or(0);
    let otp = [(m0 >> 24) as u8, (m0 >> 16) as u8, (m0 >> 8) as u8, m0 as u8, (m1 >> 8) as u8, m1 as u8];
    nic.note(format!("OTP MAC address {}", mac_str(&otp)));

    // iwl_nvm_init: read every section the 8000 family uses.
    let mut mac = otp;
    let mut sections_ok = 0;
    for (kind, name) in [(10u16, "HW"), (1, "SW"), (11, "MAC_OVERRIDE"), (12, "PHY_SKU"), (3, "REGULATORY"), (4, "CALIBRATION")] {
        match nvm_read(nic, &mut tx, &mut rx, kind) {
            Ok(data) => {
                sections_ok += 1;
                nic.note(format!("NVM section {} ({}): {} bytes", kind, name, data.len()));
                if kind == 11 && data.len() >= 8 {
                    // MAC_ADDRESS_OVERRIDE_EXT_NVM: word 1, bytes swapped in pairs.
                    let h = &data[2..8];
                    let m = [h[3], h[2], h[1], h[0], h[5], h[4]];
                    let valid = m != [0; 6] && m != [0xFF; 6] && m[0] & 1 == 0 && m != [2, 0, 0, 0, 0, 0];
                    nic.note(format!("  override MAC {} ({})", mac_str(&m), if valid { "used" } else { "not valid, OTP kept" }));
                    if valid {
                        mac = m;
                    }
                }
                if kind == 12 && data.len() >= 8 {
                    // SKU_FAMILY_8000 (word offset 2 of PHY_SKU, as a u32).
                    let sku = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                    nic.note(format!(
                        "  SKU {:#x}: 2.4 GHz {}, 5 GHz {}, 11n {}, 11ac {}",
                        sku,
                        sku & 1 != 0,
                        sku & 2 != 0,
                        sku & 4 != 0,
                        sku & 8 != 0
                    ));
                }
            }
            Err(e) => nic.note(format!("NVM section {} ({}): {}", kind, name, e)),
        }
    }
    if sections_ok == 0 {
        return Err(String::from("no NVM section could be read (command queue not working)"));
    }
    nic.note(format!("MAC address {}", mac_str(&mac)));
    crate::println!("[kernel] MILESTONE: IWLWIFI NVM OK (MAC {}, {} section(s))", mac_str(&mac), sections_ok);

    // ---- INIT calibration: TX antennas, PHY config, wait INIT_COMPLETE ----
    let phy = tlv_u32(&fw, 23).unwrap_or(0);
    let tx_ant = (phy >> 16) & 0xF;
    nic.send_cmd(&mut tx, &mut rx, TX_ANT_CONFIGURATION_CMD, &tx_ant.to_le_bytes())?;
    let (flow, event) = def_calib(&fw, 1);
    let mut cfg = Vec::with_capacity(12);
    cfg.extend_from_slice(&phy.to_le_bytes());
    cfg.extend_from_slice(&flow.to_le_bytes());
    cfg.extend_from_slice(&event.to_le_bytes());
    nic.note(format!("PHY config {:#x}, TX antennas {:#x}, calibration flow {:#x} event {:#x}", phy, tx_ant, flow, event));
    let t0 = crate::arch::cpu::nanos();
    nic.send_cmd(&mut tx, &mut rx, PHY_CONFIGURATION_CMD, &cfg)?;
    let mut phy_db = 0usize;
    loop {
        let waited = (crate::arch::cpu::nanos() - t0) / 1000;
        if waited > 2_000_000 {
            nic.dump("no INIT_COMPLETE");
            return Err(format!("no INIT_COMPLETE within 2 s ({} PHY DB entries)", phy_db));
        }
        let Some(p) = rx.next(nic, 2_000_000 - waited) else { continue };
        match p[4] {
            CALIB_RES_NOTIF_PHY_DB => phy_db += 1,
            INIT_COMPLETE_NOTIF => break,
            other => nic.note(format!("  notification {:#x} ({} bytes)", other, p.len())),
        }
    }
    let ms = (crate::arch::cpu::nanos() - t0) / 1_000_000;
    nic.note(format!("INIT calibration complete in {} ms, {} PHY DB entries", ms, phy_db));
    nic.note(String::from("next step: runtime image + paging, scan, association, WPA2"));
    crate::println!("[kernel] MILESTONE: IWLWIFI CALIB OK ({} PHY DB entries)", phy_db);
    Ok(())
}

fn mac_str(m: &[u8; 6]) -> String {
    format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", m[0], m[1], m[2], m[3], m[4], m[5])
}

/// The DEF_CALIB TLV (22) for one ucode type (1 = INIT): flow and event triggers.
fn def_calib(fw: &[u8], ucode_type: u32) -> (u32, u32) {
    let mut p = 88;
    while p + 8 <= fw.len() {
        let t = u32::from_le_bytes([fw[p], fw[p + 1], fw[p + 2], fw[p + 3]]);
        let l = u32::from_le_bytes([fw[p + 4], fw[p + 5], fw[p + 6], fw[p + 7]]) as usize;
        if t == 22 && l >= 12 && p + 20 <= fw.len() {
            let w = |o: usize| u32::from_le_bytes([fw[p + o], fw[p + o + 1], fw[p + o + 2], fw[p + o + 3]]);
            if w(8) == ucode_type {
                return (w(12), w(16));
            }
        }
        p += 8 + ((l + 3) & !3);
    }
    (0, 0)
}

/// NVM_ACCESS_CMD reads of one section, 2 KiB at a time.
fn nvm_read(nic: &mut Nic, tx: &mut Tx, rx: &mut Rx, kind: u16) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let mut c = [0u8; 8];
        // op 0 = read, target 0 = NVM_ACCESS_TARGET_CACHE
        c[2..4].copy_from_slice(&kind.to_le_bytes());
        c[4..6].copy_from_slice(&(out.len() as u16).to_le_bytes());
        c[6..8].copy_from_slice(&(NVM_CHUNK as u16).to_le_bytes());
        let r = nic.send_cmd(tx, rx, NVM_ACCESS_CMD, &c)?;
        // iwl_nvm_access_resp: offset, length, type, status, data
        if r.len() < 8 {
            return Err(format!("short reply ({} bytes)", r.len()));
        }
        let len = u16::from_le_bytes([r[2], r[3]]) as usize;
        let status = u16::from_le_bytes([r[6], r[7]]);
        if status != 0 {
            return if out.is_empty() { Err(format!("status {}", status)) } else { Ok(out) };
        }
        let data = r.get(8..8 + len).ok_or("reply shorter than its length")?;
        out.extend_from_slice(data);
        if len < NVM_CHUNK || out.len() > 64 * 1024 {
            return Ok(out);
        }
    }
}

/// The legacy RX queue: buffers in order, several packets per buffer.
struct Rx {
    stts: *mut u8,
    bufs: Vec<(u64, *mut u8)>,
    rb: usize,
    off: usize,
    consumed: usize,
    /// Packets that arrived while a command waited for its reply.
    backlog: Vec<Vec<u8>>,
}

impl Rx {
    /// The next packet (len_n_flags + header + payload), waiting up to `us`.
    fn next(&mut self, nic: &Nic, us: u64) -> Option<Vec<u8>> {
        if !self.backlog.is_empty() {
            return Some(self.backlog.remove(0));
        }
        let mut t = 0u64;
        loop {
            crate::arch::cpu::dma_sync(self.stts as u64, 16);
            let closed = (unsafe { read_volatile(self.stts as *const u16) } & 0xFFF) as usize;
            if self.rb % RX_QUEUE != closed % RX_QUEUE {
                let buf = self.bufs[self.rb % RX_QUEUE].1;
                if self.off == 0 {
                    crate::arch::cpu::dma_sync(buf as u64, 4096);
                }
                let lf = if self.off + 8 <= 4096 { unsafe { read_volatile(buf.add(self.off) as *const u32) } } else { 0 };
                let len = (lf & 0x3FFF) as usize;
                if len == 0 || lf == 0x5555_0000 || self.off + 4 + len > 4096 {
                    self.advance(nic);
                    continue;
                }
                let mut v = alloc::vec![0u8; len + 4];
                for (i, b) in v.iter_mut().enumerate() {
                    *b = unsafe { read_volatile(buf.add(self.off + i)) };
                }
                self.off += (len + 4 + 0x3F) & !0x3F;
                if self.off >= 4096 {
                    self.advance(nic);
                }
                return Some(v);
            }
            if t >= us {
                return None;
            }
            delay_us(50);
            t += 50;
        }
    }
    /// Done with the current buffer: hand buffers back eight at a time.
    fn advance(&mut self, nic: &Nic) {
        self.rb += 1;
        self.off = 0;
        self.consumed += 1;
        if self.consumed % 8 == 0 {
            nic.write_direct(FH_RX_WPTR, ((RX_POSTED as usize + self.consumed) % RX_QUEUE) as u32);
        }
        nic.w(CSR_INT, INT_FH_RX);
    }
}

const TFD_SIZE: usize = 128;
const CMD_SLOTS: usize = 32;
const CMD_BUF: usize = 512;

/// TFD ring of the command queue, the scheduler's byte-count tables and
/// one command buffer per slot.
struct CmdRing {
    tfd_phys: u64,
    tfd: *mut u8,
    bc_phys: u64,
    cmd_phys: u64,
    cmd: *mut u8,
}

impl CmdRing {
    fn new() -> Result<CmdRing, String> {
        let (tfd_phys, tfd) = crate::memory::alloc_dma_contiguous(8).ok_or("out of memory (TFD ring)")?;
        // byte-count tables: 31 queues x 640 bytes, 1 KiB aligned.
        let (bc_phys, _) = crate::memory::alloc_dma_contiguous(5).ok_or("out of memory (byte counts)")?;
        let (cmd_phys, cmd) =
            crate::memory::alloc_dma_contiguous(CMD_SLOTS * CMD_BUF / 4096).ok_or("out of memory (commands)")?;
        Ok(CmdRing { tfd_phys, tfd, bc_phys, cmd_phys, cmd })
    }
}

struct Tx {
    ring: CmdRing,
    wr: usize,
}

impl Nic {
    fn write_mem(&self, addr: u32, v: u32) -> bool {
        if !self.grab() {
            return false;
        }
        self.w(HBUS_MEM_WADDR, addr);
        self.w(HBUS_MEM_WDAT, v);
        self.release();
        true
    }

    /// iwl_pcie_tx_start + iwl_trans_ac_txq_enable(command queue).
    fn tx_start(&mut self, scd_alive: u32, ring: &CmdRing) -> Result<(), String> {
        let scd = self.read_prph(SCD_SRAM_BASE_ADDR).ok_or("no NIC access (SCD base)")?;
        if scd != scd_alive {
            self.note(format!("SCD base: register {:#x}, ALIVE {:#x} (using the register)", scd, scd_alive));
        }
        let mut a = scd + SCD_CONTEXT_MEM_LOWER;
        while a < scd + SCD_TRANS_TBL_MEM_UPPER {
            self.write_mem(a, 0);
            a += 4;
        }
        self.write_prph(SCD_DRAM_BASE_ADDR, (ring.bc_phys >> 10) as u32);
        self.write_prph(SCD_CHAINEXT_EN, 0);
        // command queue 0 -> FIFO 7
        let q = CMD_QUEUE;
        self.write_direct(fh_cbbc(q as usize), (ring.tfd_phys >> 8) as u32);
        self.write_prph(scd_queue_status(q), 1 << 19);
        let aggr = self.read_prph(SCD_AGGR_SEL).unwrap_or(0);
        self.write_prph(SCD_AGGR_SEL, aggr & !(1 << q));
        self.write_direct(HBUS_TARG_WRPTR, q << 8);
        self.write_prph(scd_queue_rdptr(q), 0);
        let ctx = scd + SCD_CONTEXT_MEM_LOWER + q * 8;
        self.write_mem(ctx, 0);
        self.write_mem(ctx + 4, (64 & 0x7F) | ((64 << 16) & 0x7F_0000));
        // active | FIFO | WSL | SCD_QUEUE_STTS_REG_MSK
        self.write_prph(scd_queue_status(q), (1 << 3) | CMD_FIFO | (1 << 4) | 0x017F_0000);
        // all TX FIFOs + DMA channels on, scheduler auto-retry
        self.write_prph(SCD_TXFACT, 0xFF);
        for ch in 0..8 {
            self.write_direct(fh_tcsr_config(ch), 0x8000_0000 | 0x8);
        }
        let chick = self.read_direct(FH_TX_CHICKEN_BITS).unwrap_or(0);
        self.write_direct(FH_TX_CHICKEN_BITS, chick | 0x2);
        self.note(format!("scheduler at SRAM {:#x}; command queue {} on FIFO {}", scd, q, CMD_FIFO));
        Ok(())
    }

    /// Send one host command (wide header, group 1) and wait for its reply;
    /// notifications that arrive first are logged. Returns the reply payload.
    fn send_cmd(&mut self, tx: &mut Tx, rx: &mut Rx, id: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
        let idx = tx.wr % CMD_SLOTS;
        let hw = tx.wr % 256;
        let total = 8 + payload.len();
        if total > CMD_BUF {
            return Err("command too long".into());
        }
        let seq = ((CMD_QUEUE as u16 & 0x1F) << 8) | hw as u16;
        let buf = unsafe { tx.ring.cmd.add(idx * CMD_BUF) };
        let phys = tx.ring.cmd_phys + (idx * CMD_BUF) as u64;
        let mut hdr = [0u8; 8];
        hdr[0] = id;
        hdr[1] = 1; // IWL_ALWAYS_LONG_GROUP
        hdr[2..4].copy_from_slice(&seq.to_le_bytes());
        hdr[4..6].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        unsafe {
            for (i, &b) in hdr.iter().chain(payload.iter()).enumerate() {
                write_volatile(buf.add(i), b);
            }
        }
        crate::arch::cpu::dma_sync(buf as u64, total);
        // TFD: TB0 = the first 20 bytes, TB1 = the rest (as iwlwifi splits it).
        let tfd = unsafe { tx.ring.tfd.add(hw * TFD_SIZE) };
        unsafe { core::ptr::write_bytes(tfd, 0, TFD_SIZE) };
        let put = |n: usize, addr: u64, len: usize| unsafe {
            let p = tfd.add(4 + n * 6);
            write_volatile(p as *mut u32, addr as u32);
            write_volatile(p.add(4) as *mut u16, (((addr >> 32) & 0xF) as u16) | ((len as u16) << 4));
        };
        let first = total.min(20);
        put(0, phys, first);
        let mut tbs = 1u8;
        if total > first {
            put(1, phys + first as u64, total - first);
            tbs += 1;
        }
        unsafe { write_volatile(tfd.add(3), tbs) };
        crate::arch::cpu::dma_sync(tfd as u64, TFD_SIZE);
        tx.wr += 1;
        if !self.grab() {
            return Err("no NIC access to post a command".into());
        }
        self.w(HBUS_TARG_WRPTR, ((tx.wr % 256) as u32) | (CMD_QUEUE << 8));
        let t0 = crate::arch::cpu::nanos();
        let mut others = Vec::new();
        let result = loop {
            let waited = (crate::arch::cpu::nanos() - t0) / 1000;
            if waited > 1_000_000 {
                break Err(format!("command {:#x}: no reply within 1 s (CSR_INT {:#010x})", id, self.r(CSR_INT)));
            }
            if self.r(CSR_INT) & (INT_SW_ERR | INT_HW_ERR) != 0 {
                break Err(format!("command {:#x}: firmware error (CSR_INT {:#010x})", id, self.r(CSR_INT)));
            }
            let Some(p) = rx.next(self, (1_000_000 - waited).min(50_000)) else { continue };
            let pseq = u16::from_le_bytes([p[6], p[7]]);
            if p[4] == id && pseq == seq {
                break Ok(p[8..].to_vec());
            }
            others.push(p);
        };
        self.release();
        rx.backlog.splice(0..0, others);
        result
    }
}
