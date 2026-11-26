//! Raspberry Pi 4 Wi-Fi: the Cypress/Broadcom CYW43455 on the legacy Arasan
//! SDHCI (`brcm,bcm2835-sdhci`, the "mmcnr" controller, GPIO 34-39 ALT3),
//! powered by WL_REG_ON on the firmware's GPIO expander (pin 1 = 129).
//!
//! Follows Linux `brcmfmac` (sdio.c, chip.c, bcdc.c, fwil):
//!
//! 1. SDIO: CMD0, CMD5 until the card is ready, CMD3, CMD7, 4-bit bus,
//!    function 1 (backplane) on, block sizes F1 64 / F2 512.
//! 2. Backplane: a 32 KiB window set through SBADDRLOW/MID/HIGH; the ALP
//!    clock is requested in CHIPCLKCSR; the cores come from the enumeration
//!    ROM (chip id at 0x18000000).
//! 3. Firmware: the ARM CR4 is halted, the firmware is written to its TCM
//!    at `rambase`, the NVRAM (key=value lines -> NUL-separated + length
//!    token) at the end of RAM, the reset vector to address 0, the CR4
//!    released; then HT clock, F2 on (ready = the firmware booted).
//! 4. Frames on F2: a 4-byte tag (length, ~length) + an 8-byte SDPCM header
//!    (sequence, channel 0 control / 1 event / 2 data, data offset, credit).
//!    Control = BCDC ioctls (cmd, len, flags with an id, status) incl.
//!    GET_VAR/SET_VAR iovars; data = a 4-byte BCDC header + an Ethernet frame.
//! 5. WPA2-PSK through the firmware's own supplicant ("sup_wpa" + SET_WSEC_PMK
//!    with the passphrase), then SET_SSID; events (SET_SSID, LINK, PSK_SUP)
//!    say how the join went. The CLM blob (regulatory data) is loaded with
//!    the "clmload" iovar.
//!
//! Firmware files (Cypress licence, not in git): `scripts/fetch-wifi-firmware.*`
//! downloads them (SHA-256 pinned) and bzimage puts them on the Pi image as
//! `/disk/WIFI/FW.BIN`, `FW.CLM`, `FW.TXT`.
//!
//! **Untested on hardware** when written: QEMU `raspi4b` has no Wi-Fi chip
//! (its SD card sits on this controller, so the driver stays off there).
//! Everything is polled; the chip is kept awake (HT forced, PM off).

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;

// ---- the SDIO host (Arasan SDHCI, 32-bit register access only) --------------

const ARG2: u64 = 0x00;
const BLKSIZECNT: u64 = 0x04;
const ARG1: u64 = 0x08;
const CMDTM: u64 = 0x0C;
const RESP0: u64 = 0x10;
const DATA: u64 = 0x20;
const STATUS: u64 = 0x24;
const CONTROL0: u64 = 0x28;
const CONTROL1: u64 = 0x2C;
const INTERRUPT: u64 = 0x30;
const IRPT_EN: u64 = 0x34;
const IRPT_SIGNAL: u64 = 0x38;
const CAPS: u64 = 0x40;

const CMD_INHIBIT: u32 = 1 << 0;
const DAT_INHIBIT: u32 = 1 << 1;
const CMD_DONE: u32 = 1 << 0;
const DATA_DONE: u32 = 1 << 1;
const WRITE_READY: u32 = 1 << 4;
const READ_READY: u32 = 1 << 5;
const ERROR: u32 = 1 << 15;
const CLK_INTLEN: u32 = 1 << 0;
const CLK_STABLE: u32 = 1 << 1;
const CLK_EN: u32 = 1 << 2;
const SRST_HC: u32 = 1 << 24;
const SRST_CMD: u32 = 1 << 25;
const SRST_DATA: u32 = 1 << 26;

const RESP_NONE: u32 = 0;
const RESP_48: u32 = 2;
const RESP_48_BUSY: u32 = 3;
const CRC_CHECK: u32 = 1 << 3;
const INDEX_CHECK: u32 = 1 << 4;
const DATA_PRESENT: u32 = 1 << 5;
const R4: u32 = RESP_48;
const R5: u32 = RESP_48 | CRC_CHECK | INDEX_CHECK;
const R6: u32 = RESP_48 | CRC_CHECK | INDEX_CHECK;
const R1B: u32 = RESP_48_BUSY | CRC_CHECK | INDEX_CHECK;
const TM_BLKCNT: u32 = 1 << 1;
const TM_READ: u32 = 1 << 4;
const TM_MULTI: u32 = 1 << 5;

fn r32(a: u64) -> u32 {
    unsafe { (a as *const u32).read_volatile() }
}
fn w32(a: u64, v: u32) {
    unsafe { (a as *mut u32).write_volatile(v) }
}

fn delay_us(us: u64) {
    let hz = crate::arch::cpu::counter_hz().max(1);
    let t0 = crate::arch::cpu::timestamp();
    while crate::arch::cpu::timestamp() - t0 < us * hz / 1_000_000 {
        core::hint::spin_loop();
    }
}

fn wait(us: u64, mut cond: impl FnMut() -> bool) -> bool {
    let hz = crate::arch::cpu::counter_hz().max(1);
    let t0 = crate::arch::cpu::timestamp();
    while !cond() {
        if crate::arch::cpu::timestamp() - t0 > us * hz / 1_000_000 {
            return false;
        }
        core::hint::spin_loop();
    }
    true
}

struct Host {
    base: u64,
    /// microseconds after each register write (the Arasan wants two SD
    /// clock cycles between writes; 10 us at 400 kHz)
    write_gap: u64,
}

impl Host {
    fn w(&self, off: u64, v: u32) {
        w32(self.base + off, v);
        if self.write_gap > 0 {
            delay_us(self.write_gap);
        }
    }
    fn r(&self, off: u64) -> u32 {
        r32(self.base + off)
    }

    fn reset(&mut self) -> Result<(), &'static str> {
        self.w(CONTROL1, self.r(CONTROL1) | SRST_HC);
        if !wait(100_000, || self.r(CONTROL1) & SRST_HC == 0) {
            return Err("SDHCI reset timed out");
        }
        // 3.3 V bus power, every status visible, nothing signalled (polled)
        self.w(CONTROL0, (self.r(CONTROL0) & !0xFF00) | (0x0F << 8));
        delay_us(2_000);
        self.w(IRPT_EN, 0xFFFF_FFFF);
        self.w(IRPT_SIGNAL, 0);
        Ok(())
    }

    fn set_clock(&mut self, base_hz: u64, want_hz: u64) -> Result<(), &'static str> {
        let n = if base_hz <= want_hz { 0 } else { base_hz.div_ceil(2 * want_hz).min(0x3FF) as u32 };
        let mut v = self.r(CONTROL1);
        v &= !(CLK_EN | 0xFFC0 | (0xF << 16));
        self.w(CONTROL1, v);
        delay_us(100);
        v |= ((n & 0xFF) << 8) | (((n >> 8) & 3) << 6) | CLK_INTLEN | (0xE << 16);
        self.w(CONTROL1, v);
        if !wait(100_000, || self.r(CONTROL1) & CLK_STABLE != 0) {
            return Err("SDIO clock did not settle");
        }
        self.w(CONTROL1, self.r(CONTROL1) | CLK_EN);
        delay_us(2_000);
        Ok(())
    }

    fn reset_lines(&mut self) {
        self.w(CONTROL1, self.r(CONTROL1) | SRST_CMD | SRST_DATA);
        wait(100_000, || self.r(CONTROL1) & (SRST_CMD | SRST_DATA) == 0);
    }

    fn cmd(&mut self, index: u32, arg: u32, flags: u32, tm: u32) -> Result<u32, &'static str> {
        let busy = if flags & 3 == RESP_48_BUSY || flags & DATA_PRESENT != 0 { CMD_INHIBIT | DAT_INHIBIT } else { CMD_INHIBIT };
        if !wait(200_000, || self.r(STATUS) & busy == 0) {
            self.reset_lines();
            return Err("SDIO host busy");
        }
        self.w(INTERRUPT, 0xFFFF_FFFF);
        self.w(ARG2, 0);
        self.w(ARG1, arg);
        self.w(CMDTM, (index << 24) | (flags << 16) | tm);
        let mut st = 0;
        if !wait(200_000, || {
            st = self.r(INTERRUPT);
            st & (CMD_DONE | ERROR) != 0
        }) || st & ERROR != 0
        {
            self.w(INTERRUPT, 0xFFFF_FFFF);
            self.reset_lines();
            return Err("SDIO command failed");
        }
        self.w(INTERRUPT, CMD_DONE);
        if flags & 3 == RESP_48_BUSY {
            wait(500_000, || self.r(INTERRUPT) & (DATA_DONE | ERROR) != 0);
            self.w(INTERRUPT, DATA_DONE);
        }
        Ok(self.r(RESP0))
    }

    /// CMD52: one byte of function `func`'s register space.
    fn cmd52(&mut self, write: bool, func: u32, addr: u32, val: u8) -> Result<u8, &'static str> {
        let arg = (write as u32) << 31 | (func & 7) << 28 | (write as u32) << 27 | (addr & 0x1FFFF) << 9 | val as u32;
        let r = self.cmd(52, arg, R5, 0)?;
        if (r >> 8) & 0xCB != 0 {
            return Err("CMD52: the card reported an error");
        }
        Ok(r as u8)
    }

    /// CMD53: `buf` to / from function `func` at `addr`, incrementing the
    /// address when `inc`. Lengths are multiples of 4; above `blk` bytes the
    /// transfer is in blocks of `blk`.
    fn cmd53(&mut self, write: bool, func: u32, addr: u32, inc: bool, buf: &mut [u8], blk: usize) -> Result<(), &'static str> {
        let len = buf.len();
        if len == 0 || len % 4 != 0 {
            return Err("CMD53: length must be a multiple of 4");
        }
        let (block_mode, count, bsize, nblocks) = if len <= blk {
            (false, (len % 512) as u32, len, 1usize)
        } else {
            if len % blk != 0 {
                return Err("CMD53: block transfer not a whole number of blocks");
            }
            (true, (len / blk) as u32, blk, len / blk)
        };
        let arg = (write as u32) << 31 | (func & 7) << 28 | (block_mode as u32) << 27 | (inc as u32) << 26 | (addr & 0x1FFFF) << 9 | (count & 0x1FF);
        self.w(BLKSIZECNT, (nblocks as u32) << 16 | bsize as u32);
        let tm = TM_BLKCNT | if write { 0 } else { TM_READ } | if nblocks > 1 { TM_MULTI } else { 0 };
        self.cmd(53, arg, R5 | DATA_PRESENT, tm)?;
        for b in 0..nblocks {
            let want = if write { WRITE_READY } else { READ_READY };
            let mut st = 0;
            if !wait(500_000, || {
                st = self.r(INTERRUPT);
                st & (want | ERROR) != 0
            }) || st & ERROR != 0
            {
                self.w(INTERRUPT, 0xFFFF_FFFF);
                self.reset_lines();
                return Err("CMD53: data phase failed");
            }
            self.w(INTERRUPT, want);
            let chunk = &mut buf[b * bsize..(b + 1) * bsize];
            for word in chunk.chunks_exact_mut(4) {
                if write {
                    w32(self.base + DATA, u32::from_le_bytes([word[0], word[1], word[2], word[3]]));
                } else {
                    word.copy_from_slice(&r32(self.base + DATA).to_le_bytes());
                }
            }
        }
        let mut st = 0;
        if !wait(500_000, || {
            st = self.r(INTERRUPT);
            st & (DATA_DONE | ERROR) != 0
        }) || st & ERROR != 0
        {
            self.w(INTERRUPT, 0xFFFF_FFFF);
            self.reset_lines();
            return Err("CMD53: transfer did not complete");
        }
        self.w(INTERRUPT, DATA_DONE);
        Ok(())
    }
}

// ---- the chip ------------------------------------------------------------------

// CCCR (function 0)
const CCCR_IOEN: u32 = 0x02;
const CCCR_IORDY: u32 = 0x03;
const CCCR_INTEN: u32 = 0x04;
const CCCR_BUS_IF: u32 = 0x07;
const CCCR_BRCM_CARDCTRL: u32 = 0xF1;
// function 1 (backplane) registers
const SBSDIO_WATERMARK: u32 = 0x10008;
const SBADDRLOW: u32 = 0x1000A;
const CHIPCLKCSR: u32 = 0x1000E;
const SDIOPULLUP: u32 = 0x1000F;
const FORCE_ALP: u8 = 0x01;
const FORCE_HT: u8 = 0x02;
const ALP_AVAIL_REQ: u8 = 0x08;
const HT_AVAIL_REQ: u8 = 0x10;
const FORCE_HW_CLKREQ_OFF: u8 = 0x20;
const ALP_AVAIL: u8 = 0x40;
const HT_AVAIL: u8 = 0x80;
const SB_WINDOW: u32 = 0x8000;
const SB_ACCESS_4B: u32 = 0x8000;

// cores
const CORE_CHIPCOMMON: u32 = 0x800;
const CORE_80211: u32 = 0x812;
const CORE_SDIO_DEV: u32 = 0x829;
const CORE_ARM_CR4: u32 = 0x83E;
// AI wrapper registers
const IOCTL: u32 = 0x408;
const RESET_CTL: u32 = 0x800;
const IOCTL_CLK: u32 = 0x01;
const IOCTL_FGC: u32 = 0x02;
const CR4_CPUHALT: u32 = 0x20;
const D11_PHYCLOCKEN: u32 = 0x04;
const D11_PHYRESET: u32 = 0x08;
// SDIO device core registers
const SD_INTSTATUS: u32 = 0x20;
const SD_HOSTINTMASK: u32 = 0x24;
const SD_TOSBMAILBOX: u32 = 0x40;
const SD_TOSBMAILBOXDATA: u32 = 0x48;
const SD_TOHOSTMAILBOXDATA: u32 = 0x4C;
const I_HMB_FC_CHANGE: u32 = 1 << 5;
const I_HMB_FRAME_IND: u32 = 1 << 6;
const I_HMB_HOST_INT: u32 = 1 << 7;
const I_CHIPACTIVE: u32 = 1 << 29;
const SMB_INT_ACK: u32 = 1 << 1;
const SDPCM_PROT_VERSION: u32 = 4;

// BCDC
const WLC_UP: u32 = 2;
const WLC_SET_INFRA: u32 = 20;
const WLC_SET_AUTH: u32 = 22;
const WLC_SET_SSID: u32 = 26;
const WLC_DISASSOC: u32 = 52;
const WLC_SET_PM: u32 = 86;
const WLC_SET_WSEC: u32 = 134;
const WLC_SET_WPA_AUTH: u32 = 165;
const WLC_GET_VAR: u32 = 262;
const WLC_SET_VAR: u32 = 263;
const WLC_SET_WSEC_PMK: u32 = 268;
const BCDC_SET: u32 = 0x02;
const BCDC_ERROR: u32 = 0x01;

// events
const E_SET_SSID: u32 = 0;
const E_DEAUTH: u32 = 5;
const E_DEAUTH_IND: u32 = 6;
const E_DISASSOC: u32 = 11;
const E_DISASSOC_IND: u32 = 12;
const E_LINK: u32 = 16;
const E_PSK_SUP: u32 = 46;
const E_ESCAN_RESULT: u32 = 69;

#[derive(Clone, Copy, Default)]
struct Core {
    id: u32,
    base: u32,
    wrap: u32,
}

/// One network found by a scan.
#[derive(Clone)]
pub struct Bss {
    pub ssid: String,
    pub bssid: [u8; 6],
    pub channel: u8,
    pub rssi: i16,
    pub secured: bool,
}

/// What the firmware said about the join.
#[derive(Default, Clone, Copy)]
pub struct JoinState {
    pub ssid_set: Option<u32>,
    pub link: Option<bool>,
    pub keyed: Option<u32>,
    pub dropped: bool,
}

pub struct Cyw43 {
    h: Host,
    window: u32,
    chip: u32,
    rev: u32,
    cores: Vec<Core>,
    rambase: u32,
    ramsize: u32,
    sd_core: u32,
    tx_seq: u8,
    tx_max: u8,
    ioctl_id: u16,
    pub mac: [u8; 6],
    pub firmware: String,
    pub rx: alloc::collections::VecDeque<Vec<u8>>,
    pub scan: Vec<Bss>,
    pub scan_done: bool,
    pub join: JoinState,
    pub frames_rx: u64,
    pub frames_tx: u64,
    pub events: u64,
    /// control replies that arrived while waiting for another one
    pending_ctl: Vec<Vec<u8>>,
}

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn be32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn round4(n: usize) -> usize {
    (n + 3) & !3
}

impl Cyw43 {
    // -- backplane access ------------------------------------------------------

    fn set_window(&mut self, addr: u32) -> Result<(), &'static str> {
        let w = addr & !(SB_WINDOW - 1);
        if w != self.window {
            let v = w >> 8;
            for i in 0..3 {
                self.h.cmd52(true, 1, SBADDRLOW + i, (v >> (8 * i)) as u8)?;
            }
            self.window = w;
        }
        Ok(())
    }

    fn bp_read32(&mut self, addr: u32) -> Result<u32, &'static str> {
        self.set_window(addr)?;
        let mut b = [0u8; 4];
        self.h.cmd53(false, 1, (addr & (SB_WINDOW - 1)) | SB_ACCESS_4B, true, &mut b, 64)?;
        Ok(u32::from_le_bytes(b))
    }

    fn bp_write32(&mut self, addr: u32, v: u32) -> Result<(), &'static str> {
        self.set_window(addr)?;
        let mut b = v.to_le_bytes();
        self.h.cmd53(true, 1, (addr & (SB_WINDOW - 1)) | SB_ACCESS_4B, true, &mut b, 64)
    }

    /// Write (or read back) a block of chip memory, window by window, in
    /// 64-byte F1 blocks where possible.
    fn bp_rw(&mut self, write: bool, mut addr: u32, data: &mut [u8]) -> Result<(), &'static str> {
        let mut off = 0;
        while off < data.len() {
            let room = (SB_WINDOW - (addr & (SB_WINDOW - 1))) as usize;
            let mut n = (data.len() - off).min(room).min(2048);
            if n > 64 {
                n -= n % 64;
            }
            let mut chunk = [0u8; 2048];
            let len = round4(n);
            if write {
                chunk[..n].copy_from_slice(&data[off..off + n]);
            }
            self.set_window(addr)?;
            self.h.cmd53(write, 1, (addr & (SB_WINDOW - 1)) | SB_ACCESS_4B, true, &mut chunk[..len], 64)?;
            if !write {
                data[off..off + n].copy_from_slice(&chunk[..n]);
            }
            off += n;
            addr += n as u32;
        }
        Ok(())
    }

    fn core(&self, id: u32) -> Option<Core> {
        self.cores.iter().copied().find(|c| c.id == id)
    }

    // -- enumeration ROM (brcmf_chip_dmp_erom_scan) ------------------------------

    fn scan_cores(&mut self) -> Result<(), &'static str> {
        let mut erom = self.bp_read32(0x1800_00FC)?;
        let end = erom + 0x1000;
        let mut cores = Vec::new();
        let desc = |s: &mut Self, at: &mut u32| -> Result<(u32, u32), &'static str> {
            let v = s.bp_read32(*at)?;
            *at += 4;
            let mut t = v & 0xF;
            if t & !0x8 == 0x5 {
                t = 0x5;
            }
            Ok((v, t))
        };
        while erom < end {
            let (v, t) = desc(self, &mut erom)?;
            if t == 0xF {
                break; // end of table
            }
            if v & 1 == 0 || t != 0x1 {
                continue;
            }
            let id = (v & 0x000F_FF00) >> 8;
            let (v2, t2) = desc(self, &mut erom)?;
            if t2 != 0x1 {
                return Err("enumeration ROM: broken component entry");
            }
            let nmw = (v2 & 0x0007_C000) >> 14;
            let nsw = (v2 & 0x00F8_0000) >> 19;
            if nmw + nsw == 0 {
                continue;
            }
            // addresses: the first slave address = registers, the first
            // wrapper of the right type = the AI wrapper
            let (v3, t3) = desc(self, &mut erom)?;
            let wraptype = if t3 == 0x3 {
                2
            } else if t3 == 0x5 {
                erom -= 4;
                3
            } else {
                continue;
            };
            let _ = v3;
            let (mut base, mut wrap) = (0u32, 0u32);
            while base == 0 || wrap == 0 {
                let (v, t) = loop {
                    let (v, t) = desc(self, &mut erom)?;
                    if t == 0xF || t == 0x5 || t == 0x1 {
                        break (v, t);
                    }
                };
                if t != 0x5 {
                    erom -= 4;
                    break;
                }
                if v & 0x8 != 0 {
                    desc(self, &mut erom)?; // upper 32 address bits
                }
                let sz = (v & 0x30) >> 4;
                if sz == 3 {
                    let (s, _) = desc(self, &mut erom)?;
                    if s & 0x8 != 0 {
                        desc(self, &mut erom)?;
                    }
                }
                if sz != 0 && sz != 1 {
                    continue; // not a 4 or 8 KiB region
                }
                let st = (v & 0xC0) >> 6;
                if base == 0 && st == 0 {
                    base = v & 0xFFFF_F000;
                }
                if wrap == 0 && st == wraptype {
                    wrap = v & 0xFFFF_F000;
                }
            }
            if base != 0 {
                cores.push(Core { id, base, wrap });
            }
        }
        self.cores = cores;
        Ok(())
    }

    // -- core reset (brcmf_chip_ai_resetcore) ------------------------------------

    fn reset_core(&mut self, c: Core, prereset: u32, reset: u32, postreset: u32) -> Result<(), &'static str> {
        if self.bp_read32(c.wrap + RESET_CTL)? & 1 == 0 {
            self.bp_write32(c.wrap + IOCTL, prereset)?;
            self.bp_read32(c.wrap + IOCTL)?;
            self.bp_write32(c.wrap + RESET_CTL, 1)?;
            delay_us(20);
            for _ in 0..30 {
                if self.bp_read32(c.wrap + RESET_CTL)? == 1 {
                    break;
                }
                delay_us(10);
            }
        }
        self.bp_write32(c.wrap + IOCTL, reset | IOCTL_FGC | IOCTL_CLK)?;
        self.bp_read32(c.wrap + IOCTL)?;
        for _ in 0..50 {
            if self.bp_read32(c.wrap + RESET_CTL)? & 1 == 0 {
                break;
            }
            self.bp_write32(c.wrap + RESET_CTL, 0)?;
            delay_us(40);
        }
        self.bp_write32(c.wrap + IOCTL, postreset | IOCTL_CLK)?;
        self.bp_read32(c.wrap + IOCTL)?;
        Ok(())
    }

    /// The CR4's TCM size: the sum of its banks (brcmf_chip_tcm_ramsize).
    fn cr4_ramsize(&mut self, cr4: Core) -> Result<u32, &'static str> {
        let cap = self.bp_read32(cr4.base + 0x04)?;
        let banks = (cap & 0xF) + ((cap & 0xF0) >> 4);
        let mut size = 0;
        for i in 0..banks {
            self.bp_write32(cr4.base + 0x40, i)?;
            let info = self.bp_read32(cr4.base + 0x44)?;
            size += ((info & 0x7F) + 1) * 8192;
        }
        Ok(size)
    }

    // -- bring-up ---------------------------------------------------------------

    /// Bring the chip up and boot `fw` with `nvram` (text) and `clm`.
    pub fn start(base: u64, base_hz: u64, fw: &[u8], nvram: &[u8], clm: &[u8]) -> Result<Cyw43, String> {
        let mut h = Host { base, write_gap: 10 };
        h.reset()?;
        let base_hz = if base_hz != 0 { base_hz } else { ((h.r(CAPS) >> 8) & 0xFF) as u64 * 1_000_000 };
        if base_hz == 0 {
            return Err("SDIO: unknown base clock".into());
        }
        h.set_clock(base_hz, 400_000)?;
        h.cmd(0, 0, RESP_NONE, 0)?;
        delay_us(2_000);
        // CMD5 with no voltage first (asks the OCR), then with the window until ready
        let ocr = h.cmd(5, 0, R4, 0).map_err(|_| String::from("SDIO: no answer to CMD5 (chip off or not SDIO)"))?;
        let mut ready = false;
        for _ in 0..100 {
            let r = h.cmd(5, ocr & 0x00FF_FF00, R4, 0)?;
            if r & (1 << 31) != 0 {
                ready = true;
                break;
            }
            delay_us(10_000);
        }
        if !ready {
            return Err("SDIO: the card never became ready (CMD5)".into());
        }
        let rca = h.cmd(3, 0, R6, 0)? & 0xFFFF_0000;
        h.cmd(7, rca, R1B, 0)?;
        // 4-bit bus on both sides, then 25 MHz
        let bus = h.cmd52(false, 0, CCCR_BUS_IF, 0)?;
        h.cmd52(true, 0, CCCR_BUS_IF, (bus & !3) | 2)?;
        h.w(CONTROL0, h.r(CONTROL0) | (1 << 1));
        h.set_clock(base_hz, 25_000_000)?;
        h.write_gap = 1;
        // block sizes: F1 64, F2 512
        h.cmd52(true, 0, 0x110, 64)?;
        h.cmd52(true, 0, 0x111, 0)?;
        h.cmd52(true, 0, 0x210, 0)?;
        h.cmd52(true, 0, 0x211, 2)?;
        // function 1 on
        h.cmd52(true, 0, CCCR_IOEN, 0x02)?;
        if !wait(500_000, || h.cmd52(false, 0, CCCR_IORDY, 0).map_or(false, |v| v & 0x02 != 0)) {
            return Err("SDIO: function 1 never became ready".into());
        }

        let mut c = Cyw43 {
            h,
            window: u32::MAX,
            chip: 0,
            rev: 0,
            cores: Vec::new(),
            rambase: 0x198000,
            ramsize: 0,
            sd_core: 0,
            tx_seq: 0,
            tx_max: 4,
            ioctl_id: 1,
            mac: [0; 6],
            firmware: String::new(),
            rx: alloc::collections::VecDeque::new(),
            scan: Vec::new(),
            scan_done: false,
            join: JoinState::default(),
            frames_rx: 0,
            frames_tx: 0,
            events: 0,
            pending_ctl: Vec::new(),
        };
        // ALP clock
        c.h.cmd52(true, 1, CHIPCLKCSR, FORCE_HW_CLKREQ_OFF | ALP_AVAIL_REQ)?;
        if !wait(500_000, || c.h.cmd52(false, 1, CHIPCLKCSR, 0).map_or(false, |v| v & ALP_AVAIL != 0)) {
            return Err("chip: no ALP clock".into());
        }
        c.h.cmd52(true, 1, CHIPCLKCSR, FORCE_HW_CLKREQ_OFF | FORCE_ALP)?;
        c.h.cmd52(true, 1, SDIOPULLUP, 0)?;
        let id = c.bp_read32(0x1800_0000)?;
        c.chip = id & 0xFFFF;
        c.rev = (id >> 16) & 0xF;
        if (id >> 28) & 0xF != 1 {
            return Err(format!("chip {:#x}: not an AXI backplane", c.chip));
        }
        c.scan_cores()?;
        let cr4 = c.core(CORE_ARM_CR4).ok_or_else(|| format!("chip {:x} rev {}: no ARM CR4 in the enumeration ROM", c.chip, c.rev))?;
        let sd = c.core(CORE_SDIO_DEV).ok_or("no SDIO device core")?;
        c.sd_core = sd.base;
        crate::println!(
            "[wifi] chip {:x} rev {}: {} cores, CR4 at {:#x} (wrapper {:#x}), SDIO core {:#x}",
            c.chip, c.rev, c.cores.len(), cr4.base, cr4.wrap, sd.base
        );
        // halt the CR4, reset the 802.11 core (brcmf_chip_cr4_set_passive)
        c.reset_core(cr4, CR4_CPUHALT, CR4_CPUHALT, CR4_CPUHALT)?;
        if let Some(d11) = c.core(CORE_80211) {
            c.reset_core(d11, D11_PHYRESET | D11_PHYCLOCKEN, D11_PHYCLOCKEN, D11_PHYCLOCKEN)?;
        }
        // an SDIO card reset also resets the WLAN backplane
        let cc = c.h.cmd52(false, 0, CCCR_BRCM_CARDCTRL, 0)?;
        c.h.cmd52(true, 0, CCCR_BRCM_CARDCTRL, cc | 0x02)?;
        c.ramsize = c.cr4_ramsize(cr4)?;
        if c.ramsize == 0 || fw.len() as u32 > c.ramsize {
            return Err(format!("TCM {} bytes is too small for {} bytes of firmware", c.ramsize, fw.len()));
        }

        // HT clock for the download
        c.h.cmd52(true, 1, CHIPCLKCSR, HT_AVAIL_REQ)?;
        let ht_dl = wait(500_000, || c.h.cmd52(false, 1, CHIPCLKCSR, 0).map_or(false, |v| v & HT_AVAIL != 0));
        crate::println!(
            "[wifi] download clock: HT {} (CHIPCLKCSR {:#x})",
            if ht_dl { "up" } else { "NOT up" },
            c.h.cmd52(false, 1, CHIPCLKCSR, 0).unwrap_or(0xEE)
        );
        // firmware, checked by reading the first 256 bytes back
        let t0 = crate::arch::cpu::timestamp();
        let mut image = fw.to_vec();
        c.bp_rw(true, c.rambase, &mut image)?;
        let mut back = [0u8; 256];
        c.bp_rw(false, c.rambase, &mut back)?;
        if back[..] != fw[..256.min(fw.len())] {
            return Err("firmware read back from the TCM differs".into());
        }
        let ms = (crate::arch::cpu::timestamp() - t0) * 1000 / crate::arch::cpu::counter_hz().max(1);
        // NVRAM at the end of RAM
        let mut nv = nvram_blob(nvram);
        let at = c.rambase + c.ramsize - nv.len() as u32;
        c.bp_rw(true, at, &mut nv)?;
        crate::println!("[wifi] firmware {} bytes in {} ms, NVRAM {} bytes at {:#x}, TCM {} KiB at {:#x}", fw.len(), ms, nv.len(), at, c.ramsize / 1024, c.rambase);
        // reset vector to address 0, clear the SDIO core's interrupts, release the CR4
        let rstvec = le32(fw, 0);
        c.bp_write32(0, rstvec)?;
        c.bp_write32(c.sd_core + SD_INTSTATUS, 0xFFFF_FFFF)?;
        c.reset_core(cr4, CR4_CPUHALT, 0, 0)?;

        // Like brcmf_sdio_download_firmware + brcmf_sdio_firmware_callback:
        // drop the clock request (CLK_SDONLY), then request HT again
        // (CLK_AVAIL) and wait for it; the CR4 release does not keep it.
        c.h.cmd52(true, 1, CHIPCLKCSR, 0)?;
        delay_us(1_000);
        c.h.cmd52(true, 1, CHIPCLKCSR, HT_AVAIL_REQ)?;
        if !wait(2_000_000, || c.h.cmd52(false, 1, CHIPCLKCSR, 0).map_or(false, |v| v & HT_AVAIL != 0)) {
            let csr = c.h.cmd52(false, 1, CHIPCLKCSR, 0).unwrap_or(0xEE);
            // the firmware replaces the NVRAM length token at the end of the
            // TCM with its shared-area pointer once it runs
            let last = c.bp_read32(c.rambase + c.ramsize - 4).unwrap_or(0xDEAD_BEEF);
            let vec0 = c.bp_read32(0).unwrap_or(0xDEAD_BEEF);
            crate::println!(
                "[wifi] HT clock not up after the CR4 release: CHIPCLKCSR {:#x}, TCM end word {:#x}, word 0 {:#x} (rstvec {:#x}); trying ALP + F2",
                csr, last, vec0, rstvec
            );
            // fall back: ALP only, then see whether the firmware still readies F2
            c.h.cmd52(true, 1, CHIPCLKCSR, ALP_AVAIL_REQ)?;
            if !wait(500_000, || c.h.cmd52(false, 1, CHIPCLKCSR, 0).map_or(false, |v| v & ALP_AVAIL != 0)) {
                return Err(format!("firmware did not bring the HT clock up (CHIPCLKCSR {:#x})", csr));
            }
        }
        c.h.cmd52(true, 1, CHIPCLKCSR, FORCE_HT)?;
        c.bp_write32(c.sd_core + SD_TOSBMAILBOXDATA, SDPCM_PROT_VERSION << 16)?;
        c.h.cmd52(true, 0, CCCR_IOEN, 0x06)?;
        if !wait(3_000_000, || c.h.cmd52(false, 0, CCCR_IORDY, 0).map_or(false, |v| v & 0x04 != 0)) {
            return Err("firmware did not start (function 2 never became ready)".into());
        }
        c.bp_write32(c.sd_core + SD_HOSTINTMASK, I_HMB_FC_CHANGE | I_HMB_FRAME_IND | I_HMB_HOST_INT | 0x10 | I_CHIPACTIVE)?;
        c.h.cmd52(true, 1, SBSDIO_WATERMARK, 8)?;
        c.h.cmd52(true, 0, CCCR_INTEN, 0x07)?;

        // the firmware answers ioctls now
        c.poll();
        if !clm.is_empty() {
            c.load_clm(clm)?;
        }
        let mut ver = [0u8; 128];
        if let Ok(n) = c.get_var("ver", &mut ver) {
            let end = ver[..n].iter().position(|&b| b == 0 || b == b'\n').unwrap_or(n);
            c.firmware = String::from_utf8_lossy(&ver[..end]).into_owned();
        }
        let mut mac = [0u8; 6];
        c.get_var("cur_etheraddr", &mut mac).map_err(|e| format!("cur_etheraddr: {}", e))?;
        c.mac = mac;
        // events we care about, awake all the time, interface up
        let mut mask = [0u8; 18];
        for e in [E_SET_SSID, E_DEAUTH, E_DEAUTH_IND, E_DISASSOC, E_DISASSOC_IND, E_LINK, E_PSK_SUP, E_ESCAN_RESULT, 1, 3] {
            mask[(e / 8) as usize] |= 1 << (e % 8);
        }
        c.set_var("event_msgs", &mask)?;
        let _ = c.set_var_u32("mpc", 0);
        c.ioctl(WLC_UP, true, &mut [])?;
        let _ = c.set_u32(WLC_SET_PM, 0);
        Ok(c)
    }

    /// The CLM (regulatory) blob in 1400-byte chunks through "clmload".
    fn load_clm(&mut self, clm: &[u8]) -> Result<(), String> {
        let chunks = clm.chunks(1400).count();
        for (k, part) in clm.chunks(1400).enumerate() {
            let mut flag = 1u16 << 12; // handler version 1
            if k == 0 {
                flag |= 0x2;
            }
            if k + 1 == chunks {
                flag |= 0x4;
            }
            let mut b = Vec::with_capacity(12 + part.len());
            b.extend_from_slice(&flag.to_le_bytes());
            b.extend_from_slice(&2u16.to_le_bytes()); // CLM
            b.extend_from_slice(&(part.len() as u32).to_le_bytes());
            b.extend_from_slice(&0u32.to_le_bytes());
            b.extend_from_slice(part);
            self.set_var("clmload", &b).map_err(|e| format!("clmload: {}", e))?;
        }
        let mut st = [0u8; 4];
        if self.get_var("clmload_status", &mut st).is_ok() && le32(&st, 0) != 0 {
            return Err(format!("the firmware rejected the CLM blob (status {})", le32(&st, 0)));
        }
        Ok(())
    }

    // -- frames ------------------------------------------------------------------

    /// The firmware grants sequence numbers up to `tx_max` (exclusive).
    fn has_credit(&self) -> bool {
        let d = self.tx_max.wrapping_sub(self.tx_seq);
        d != 0 && d & 0x80 == 0
    }

    fn send_frame(&mut self, channel: u8, payload: &[u8]) -> Result<(), &'static str> {
        // wait a little for credit (the reply headers carry it); send anyway after 20 ms
        if !self.has_credit() {
            let hz = crate::arch::cpu::counter_hz().max(1);
            let t0 = crate::arch::cpu::timestamp();
            while !self.has_credit() && (crate::arch::cpu::timestamp() - t0) * 1000 / hz < 20 {
                self.poll_frames();
            }
        }
        let total = 12 + payload.len();
        let mut f = Vec::with_capacity(total + 512);
        f.extend_from_slice(&(total as u16).to_le_bytes());
        f.extend_from_slice(&(!(total as u16)).to_le_bytes());
        f.extend_from_slice(&[self.tx_seq, channel & 0xF, 0, 12, 0, 0, 0, 0]);
        f.extend_from_slice(payload);
        let mut len = round4(f.len());
        if len > 512 {
            len = len.div_ceil(512) * 512;
        }
        f.resize(len, 0);
        self.tx_seq = self.tx_seq.wrapping_add(1);
        self.h.cmd53(true, 2, 0, false, &mut f, 512)
    }

    /// Read one frame from F2; None when nothing is waiting.
    fn read_frame(&mut self) -> Result<Option<(u8, Vec<u8>)>, &'static str> {
        let mut head = [0u8; 64];
        self.h.cmd53(false, 2, 0, false, &mut head, 512)?;
        let len = u16::from_le_bytes([head[0], head[1]]) as usize;
        let check = u16::from_le_bytes([head[2], head[3]]) as usize;
        if len == 0 && check == 0 {
            return Ok(None);
        }
        if len ^ check != 0xFFFF || len < 12 {
            return Err("SDPCM: bad frame tag");
        }
        let mut f = head.to_vec();
        if len > 64 {
            let mut rest_len = round4(len - 64);
            if rest_len > 512 {
                rest_len = rest_len.div_ceil(512) * 512;
            }
            let mut rest = alloc::vec![0u8; rest_len];
            self.h.cmd53(false, 2, 0, false, &mut rest, 512)?;
            f.extend_from_slice(&rest);
        }
        f.truncate(len);
        let chan = f[5] & 0xF;
        let doff = f[7] as usize;
        let credit = f[9];
        // brcmf_sdio_update_credit: an implausible window is clamped to 2
        self.tx_max = if credit.wrapping_sub(self.tx_seq) > 0x40 { self.tx_seq.wrapping_add(2) } else { credit };
        if doff < 12 || doff > len {
            return Err("SDPCM: bad data offset");
        }
        Ok(Some((chan, f[doff..].to_vec())))
    }

    /// Take everything the chip has for us: data frames to `rx`, events
    /// handled, control replies kept for `ioctl`.
    pub fn poll(&mut self) {
        let Ok(st) = self.bp_read32(self.sd_core + SD_INTSTATUS) else { return };
        if st & (I_HMB_FRAME_IND | I_HMB_HOST_INT | I_HMB_FC_CHANGE) != 0 {
            let _ = self.bp_write32(self.sd_core + SD_INTSTATUS, st & (I_HMB_FRAME_IND | I_HMB_HOST_INT | I_HMB_FC_CHANGE));
        }
        if st & I_HMB_HOST_INT != 0 {
            let _ = self.bp_read32(self.sd_core + SD_TOHOSTMAILBOXDATA);
            let _ = self.bp_write32(self.sd_core + SD_TOSBMAILBOX, SMB_INT_ACK);
        }
        self.poll_frames();
    }

    /// Read frames whether or not an indication was seen (everything is polled).
    fn poll_frames(&mut self) {
        for _ in 0..16 {
            match self.read_frame() {
                Ok(Some((chan, body))) => self.dispatch(chan, body),
                _ => break,
            }
        }
    }

    fn dispatch(&mut self, chan: u8, body: Vec<u8>) {
        match chan {
            0 => self.pending_ctl.push(body),
            1 | 2 => {
                // BCDC data header: flags, priority, flags2, data offset (words)
                if body.len() < 4 {
                    return;
                }
                let off = 4 + body[3] as usize * 4;
                if body.len() < off + 14 {
                    return;
                }
                let frame = &body[off..];
                if u16::from_be_bytes([frame[12], frame[13]]) == 0x886C {
                    self.event(frame);
                } else if chan == 2 {
                    self.frames_rx += 1;
                    if self.rx.len() < 64 {
                        self.rx.push_back(frame.to_vec());
                    }
                }
            }
            _ => {}
        }
    }

    /// A Broadcom event (big-endian header after Ethernet + bcmeth).
    fn event(&mut self, f: &[u8]) {
        let m = 14 + 10;
        if f.len() < m + 48 {
            return;
        }
        self.events += 1;
        let ty = be32(f, m + 4);
        let status = be32(f, m + 8);
        let flags = u16::from_be_bytes([f[m + 2], f[m + 3]]);
        let datalen = be32(f, m + 20) as usize;
        let data = &f[(m + 48).min(f.len())..(m + 48 + datalen).min(f.len())];
        match ty {
            E_SET_SSID => self.join.ssid_set = Some(status),
            E_LINK => {
                let up = flags & 1 != 0;
                self.join.link = Some(up);
                if !up {
                    self.join.dropped = true;
                }
            }
            E_PSK_SUP => self.join.keyed = Some(status),
            E_DEAUTH | E_DEAUTH_IND | E_DISASSOC | E_DISASSOC_IND => self.join.dropped = true,
            E_ESCAN_RESULT => {
                if status == 0 {
                    self.scan_done = true;
                } else if data.len() >= 12 + 80 {
                    self.add_bss(&data[12..]);
                }
            }
            _ => {}
        }
    }

    /// One `brcmf_bss_info_le` from an escan result.
    fn add_bss(&mut self, b: &[u8]) {
        let mut bssid = [0u8; 6];
        bssid.copy_from_slice(&b[8..14]);
        let cap = u16::from_le_bytes([b[16], b[17]]);
        let n = (b[18] as usize).min(32);
        let ssid = String::from_utf8_lossy(&b[19..19 + n]).into_owned();
        let chanspec = u16::from_le_bytes([b[72], b[73]]);
        let rssi = i16::from_le_bytes([b[78], b[79]]);
        if let Some(e) = self.scan.iter_mut().find(|e| e.bssid == bssid) {
            e.rssi = e.rssi.max(rssi);
            return;
        }
        self.scan.push(Bss { ssid, bssid, channel: (chanspec & 0xFF) as u8, rssi, secured: cap & 0x10 != 0 });
    }

    // -- ioctls ------------------------------------------------------------------

    /// One BCDC ioctl; `data` goes out and the reply comes back in it.
    pub fn ioctl(&mut self, cmd: u32, set: bool, data: &mut [u8]) -> Result<usize, String> {
        let id = self.ioctl_id;
        self.ioctl_id = self.ioctl_id.wrapping_add(1).max(1);
        let mut p = Vec::with_capacity(16 + data.len());
        p.extend_from_slice(&cmd.to_le_bytes());
        p.extend_from_slice(&(data.len() as u32).to_le_bytes());
        p.extend_from_slice(&((id as u32) << 16 | if set { BCDC_SET } else { 0 }).to_le_bytes());
        p.extend_from_slice(&0u32.to_le_bytes());
        p.extend_from_slice(data);
        self.send_frame(0, &p).map_err(String::from)?;
        let hz = crate::arch::cpu::counter_hz().max(1);
        let t0 = crate::arch::cpu::timestamp();
        loop {
            if let Some(k) = self.pending_ctl.iter().position(|r| r.len() >= 16 && (le32(r, 8) >> 16) as u16 == id) {
                let r = self.pending_ctl.remove(k);
                let flags = le32(&r, 8);
                let status = le32(&r, 12) as i32;
                if flags & BCDC_ERROR != 0 {
                    return Err(format!("ioctl {} failed: firmware error {}", cmd, status));
                }
                let n = (r.len() - 16).min(data.len());
                data[..n].copy_from_slice(&r[16..16 + n]);
                return Ok(n);
            }
            if crate::arch::cpu::timestamp() - t0 > hz * 2 {
                return Err(format!("ioctl {}: no answer from the firmware", cmd));
            }
            self.poll();
            delay_us(200);
        }
    }

    pub fn get_var(&mut self, name: &str, out: &mut [u8]) -> Result<usize, String> {
        let mut b = alloc::vec![0u8; (name.len() + 1).max(out.len())];
        b[..name.len()].copy_from_slice(name.as_bytes());
        let n = self.ioctl(WLC_GET_VAR, false, &mut b)?;
        let k = n.min(out.len());
        out[..k].copy_from_slice(&b[..k]);
        Ok(k)
    }

    pub fn set_var(&mut self, name: &str, value: &[u8]) -> Result<(), String> {
        let mut b = Vec::with_capacity(name.len() + 1 + value.len());
        b.extend_from_slice(name.as_bytes());
        b.push(0);
        b.extend_from_slice(value);
        self.ioctl(WLC_SET_VAR, true, &mut b).map(|_| ())
    }

    fn set_var_u32(&mut self, name: &str, v: u32) -> Result<(), String> {
        self.set_var(name, &v.to_le_bytes())
    }

    fn set_u32(&mut self, cmd: u32, v: u32) -> Result<(), String> {
        let mut b = v.to_le_bytes();
        self.ioctl(cmd, true, &mut b).map(|_| ())
    }

    // -- scan and join ---------------------------------------------------------------

    /// An active scan of every channel; waits up to `ms` for the end.
    pub fn scan(&mut self, ms: u64) -> Result<usize, String> {
        self.scan.clear();
        self.scan_done = false;
        let mut p = [0u8; 8 + 64];
        p[0..4].copy_from_slice(&1u32.to_le_bytes()); // version
        p[4..6].copy_from_slice(&1u16.to_le_bytes()); // action: start
        p[6..8].copy_from_slice(&0x1234u16.to_le_bytes()); // sync id
        let s = &mut p[8..];
        s[36..42].fill(0xFF); // any BSSID
        s[42] = 2; // any BSS type
        s[43] = 0; // active
        for o in [44, 48, 52, 56] {
            s[o..o + 4].copy_from_slice(&(-1i32).to_le_bytes());
        }
        self.set_var("escan", &p)?;
        let hz = crate::arch::cpu::counter_hz().max(1);
        let t0 = crate::arch::cpu::timestamp();
        while !self.scan_done && (crate::arch::cpu::timestamp() - t0) * 1000 / hz < ms {
            self.poll();
            delay_us(2_000);
        }
        self.scan.sort_by(|a, b| b.rssi.cmp(&a.rssi));
        Ok(self.scan.len())
    }

    /// Join `ssid`: WPA2-PSK with `pass` (8..63 characters) through the
    /// firmware supplicant, or an open network when `pass` is empty.
    pub fn join(&mut self, ssid: &str, pass: &str, ms: u64) -> Result<(), String> {
        if ssid.is_empty() || ssid.len() > 32 {
            return Err("SSID must be 1..32 bytes".into());
        }
        if !pass.is_empty() && (pass.len() < 8 || pass.len() > 64) {
            return Err("a WPA2 passphrase has 8..63 characters (or a 64-digit hex PSK)".into());
        }
        self.join = JoinState::default();
        let _ = self.ioctl(WLC_DISASSOC, true, &mut []);
        self.set_u32(WLC_SET_INFRA, 1)?;
        self.set_u32(WLC_SET_AUTH, 0)?;
        if pass.is_empty() {
            self.set_u32(WLC_SET_WSEC, 0)?;
            self.set_u32(WLC_SET_WPA_AUTH, 0)?;
            let _ = self.set_var_u32("sup_wpa", 0);
        } else {
            self.set_u32(WLC_SET_WSEC, 4)?; // AES
            self.set_u32(WLC_SET_WPA_AUTH, 0x80)?; // WPA2-PSK
            self.set_var_u32("sup_wpa", 1)?;
            let mut pmk = [0u8; 68];
            let hex = pass.len() == 64 && pass.bytes().all(|b| b.is_ascii_hexdigit());
            pmk[0..2].copy_from_slice(&(pass.len() as u16).to_le_bytes());
            // flags: 1 = a passphrase; 0 = the 256-bit PSK in hex
            pmk[2..4].copy_from_slice(&(if hex { 0u16 } else { 1 }).to_le_bytes());
            pmk[4..4 + pass.len()].copy_from_slice(pass.as_bytes());
            // the supplicant may need a moment after sup_wpa
            delay_us(5_000);
            self.ioctl(WLC_SET_WSEC_PMK, true, &mut pmk)?;
        }
        let mut s = [0u8; 36];
        s[0..4].copy_from_slice(&(ssid.len() as u32).to_le_bytes());
        s[4..4 + ssid.len()].copy_from_slice(ssid.as_bytes());
        self.ioctl(WLC_SET_SSID, true, &mut s)?;
        let hz = crate::arch::cpu::counter_hz().max(1);
        let t0 = crate::arch::cpu::timestamp();
        loop {
            self.poll();
            let j = self.join;
            if let Some(st) = j.ssid_set {
                if st != 0 {
                    return Err(format!("join failed (SET_SSID status {}{})", st, match st {
                        1 => ": failed",
                        2 => ": network not found",
                        3 => ": timed out",
                        _ => "",
                    }));
                }
            }
            let linked = j.link == Some(true);
            if linked && (pass.is_empty() || j.keyed == Some(6)) {
                return Ok(());
            }
            if let Some(k) = j.keyed {
                if k != 6 && k != 0 {
                    return Err(format!("WPA2 handshake failed (supplicant status {}): wrong password?", k));
                }
            }
            if (crate::arch::cpu::timestamp() - t0) * 1000 / hz > ms {
                return Err(format!(
                    "no association within {} s (ssid {:?}, link {:?}, keyed {:?})",
                    ms / 1000, j.ssid_set, j.link, j.keyed
                ));
            }
            delay_us(5_000);
        }
    }

    /// Send an Ethernet frame (BCDC v2 header, data channel).
    pub fn send_ethernet(&mut self, frame: &[u8]) -> Result<(), &'static str> {
        let mut p = Vec::with_capacity(4 + frame.len());
        p.extend_from_slice(&[0x20, 0, 0, 0]);
        p.extend_from_slice(frame);
        self.frames_tx += 1;
        self.send_frame(2, &p)
    }

    pub fn summary(&self) -> String {
        format!(
            "CYW{:x} rev {}, firmware {}, MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}, frames rx {} tx {}, events {}, credit {}/{}",
            self.chip, self.rev, if self.firmware.is_empty() { "?" } else { self.firmware.as_str() },
            self.mac[0], self.mac[1], self.mac[2], self.mac[3], self.mac[4], self.mac[5],
            self.frames_rx, self.frames_tx, self.events, self.tx_seq, self.tx_max
        )
    }
}

/// NVRAM text -> the blob brcmf writes: `key=value` entries NUL-terminated
/// (comments and blank lines dropped), padded to 4 bytes with at least one
/// NUL, then the length token `~words << 16 | words`.
pub fn nvram_blob(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in text.split(|&b| b == b'\n') {
        let line = match line.iter().position(|&b| b == b'#') {
            Some(k) => &line[..k],
            None => line,
        };
        let t: &[u8] = {
            let s = line.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(line.len());
            let e = line.iter().rposition(|b| !b.is_ascii_whitespace()).map_or(s, |e| e + 1);
            &line[s..e.max(s)]
        };
        if t.is_empty() || !t.contains(&b'=') {
            continue;
        }
        out.extend_from_slice(t);
        out.push(0);
    }
    let len = (out.len() + 1 + 3) & !3;
    out.resize(len, 0);
    let words = (len / 4) as u32;
    out.extend_from_slice(&((!words << 16) | (words & 0xFFFF)).to_le_bytes());
    out
}

// ---- the board glue: state, commands, the network device ---------------------

use core::sync::atomic::{AtomicU64, Ordering};

/// The physical base of the SDHCI that carries /disk (QEMU puts the SD card
/// on the Wi-Fi controller), set by `arm_main::sd_card_init`.
pub static SD_DISK_BASE: AtomicU64 = AtomicU64::new(0);

static WIFI: spin::Mutex<Option<Cyw43>> = spin::Mutex::new(None);
/// Why there is no Wi-Fi, or what it is doing (shown by `wifi`).
static WIFI_STATUS: spin::Mutex<String> = spin::Mutex::new(String::new());
const CONFIG: &str = "/disk/WIFI.CFG";

fn set_status(s: String) {
    crate::println!("[wifi] {}", s);
    *WIFI_STATUS.lock() = s;
}

/// The WPA2 key: PBKDF2-HMAC-SHA1(passphrase, SSID, 4096) as 64 hex digits
/// (IEEE 802.11i), so WIFI.CFG never holds the passphrase itself.
pub fn psk_hex(ssid: &str, pass: &str) -> String {
    let mut psk = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(pass.as_bytes(), ssid.as_bytes(), 4096, &mut psk);
    psk.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Bring the chip up (once): DT node, pins, power, firmware from /disk/WIFI.
pub fn start() -> Result<(), String> {
    if WIFI.lock().is_some() {
        return Ok(());
    }
    let Some(phys) = crate::boot::info().firmware.dtb else { return Err("no device tree".into()) };
    let f = unsafe { crate::dtb::Fdt::from_phys(phys) }.map_err(|_| String::from("device tree unreadable"))?;
    // The Pi 4 / Pi 400 carry the chip on "mmcnr" (the firmware's DTB has no
    // brcm,bcm4329-fmac child, so the board model decides)
    let model_ok = f.nodes[0].strings("compatible").any(|c| c == "raspberrypi,4-model-b" || c == "raspberrypi,400")
        || f.compatible("brcm,bcm4329-fmac").next().is_some();
    if !model_ok {
        return Err("this board has no CYW43455 Wi-Fi (only the Raspberry Pi 4 / 400 are supported)".into());
    }
    let node = (0..f.nodes.len())
        .find(|&k| f.nodes[k].name.starts_with("mmcnr"))
        .or_else(|| f.compatible("brcm,bcm2835-sdhci").next());
    let Some(i) = node else { return Err("no SDIO controller (mmcnr / brcm,bcm2835-sdhci)".into()) };
    let Some(&(pa, _)) = f.reg(i).first() else { return Err("SDIO controller without registers".into()) };
    if SD_DISK_BASE.load(Ordering::Relaxed) == pa {
        return Err("the SDIO controller carries the SD card here (QEMU raspi4b has no Wi-Fi chip)".into());
    }
    let fw = crate::vfs::read("/disk/WIFI/FW.BIN").map_err(|_| String::from("firmware /disk/WIFI/FW.BIN missing (scripts/fetch-wifi-firmware, then rebuild the image)"))?;
    let nvram = crate::vfs::read("/disk/WIFI/FW.TXT").map_err(|_| String::from("NVRAM /disk/WIFI/FW.TXT missing"))?;
    let clm = crate::vfs::read("/disk/WIFI/FW.CLM").unwrap_or_default();
    super::rpi_io::wifi_power_on()?;
    let va = crate::memory::map_mmio(pa, 0x1000).ok_or("cannot map the SDIO controller")?;
    let chip = Cyw43::start(va, super::rpi_io::emmc_clock(), &fw, &nvram, &clm)?;
    set_status(format!("up: {}", chip.summary()));
    *WIFI.lock() = Some(chip);
    Ok(())
}

/// The network device the IP stack uses once a network is joined.
pub struct WifiNet;

impl crate::net::NetDevice for WifiNet {
    fn mac(&self) -> crate::net::MacAddr {
        WIFI.lock().as_ref().map_or([0; 6], |c| c.mac)
    }
    fn transmit(&mut self, frame: &[u8]) {
        if let Some(c) = WIFI.lock().as_mut() {
            let _ = c.send_ethernet(frame);
        }
    }
    fn receive(&mut self) -> Option<Vec<u8>> {
        let mut g = WIFI.lock();
        let c = g.as_mut()?;
        if c.rx.is_empty() {
            c.poll();
        }
        c.rx.pop_front()
    }
    fn diag(&self) -> Option<String> {
        WIFI.lock().as_ref().map(|c| {
            let j = c.join;
            format!("wifi: {}; join ssid {:?} link {:?} keyed {:?} dropped {}", c.summary(), j.ssid_set, j.link, j.keyed, j.dropped)
        })
    }
    fn wireless(&self) -> bool {
        true
    }
}

/// The network saved in WIFI.CFG ("" = none).
pub fn saved_ssid() -> String {
    let Ok(cfg) = crate::vfs::read(CONFIG) else { return String::new() };
    String::from_utf8_lossy(&cfg).lines().find_map(|l| l.strip_prefix("ssid=")).unwrap_or("").trim().to_string()
}

/// Is there a CYW43455 on this board (Raspberry Pi 4 / 400)?
pub fn present() -> bool {
    let Some(phys) = crate::boot::info().firmware.dtb else { return false };
    let Ok(f) = (unsafe { crate::dtb::Fdt::from_phys(phys) }) else { return false };
    f.nodes[0].strings("compatible").any(|c| c == "raspberrypi,4-model-b" || c == "raspberrypi,400")
}

/// Join, take a DHCP lease and make Wi-Fi the network device.
pub fn connect(ssid: &str, key: &str) -> Result<String, String> {
    start()?;
    {
        let mut g = WIFI.lock();
        let c = g.as_mut().ok_or("Wi-Fi is not up")?;
        c.join(ssid, key, 20_000)?;
    }
    let mut dev = WifiNet;
    let lease = crate::dhcp::lease(&mut dev, 10).ok_or_else(|| String::from("joined, but no DHCP answer"))?;
    crate::net::attach(alloc::boxed::Box::new(WifiNet), lease.ip);
    crate::net::set_route(lease.mask, lease.router);
    crate::net::set_dns(lease.dns);
    let (ip, gw) = (lease.ip, lease.router);
    let msg = format!(
        "connected to {:?}: {}.{}.{}.{} (router {}.{}.{}.{})",
        ssid, ip[0], ip[1], ip[2], ip[3], gw[0], gw[1], gw[2], gw[3]
    );
    set_status(msg.clone());
    crate::println!(
        "[kernel] MILESTONE: WIFI OK (CYW43455 over SDIO, firmware supplicant, DHCP {}.{}.{}.{})",
        ip[0], ip[1], ip[2], ip[3]
    );
    Ok(msg)
}

/// `wifi` shell command.
pub fn command(args: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    match args.first().copied().unwrap_or("") {
        "" | "status" => {
            let st = WIFI_STATUS.lock().clone();
            out.push(if st.is_empty() {
                String::from("wifi: not started (`wifi scan` or `wifi connect SSID PASSWORD`)")
            } else {
                format!("wifi: {}", st)
            });
            if let Some(c) = WIFI.lock().as_ref() {
                out.push(c.summary());
            }
        }
        "scan" => match start() {
            Err(e) => {
                set_status(e.clone());
                out.push(format!("wifi: {}", e));
            }
            Ok(()) => {
                let mut g = WIFI.lock();
                if let Some(c) = g.as_mut() {
                    match c.scan(6_000) {
                        Err(e) => out.push(format!("wifi scan: {}", e)),
                        Ok(_) if args.get(1) == Some(&"--raw") => {
                            for b in c.scan.iter().take(40) {
                                out.push(format!("@bss\t{}\t{}\t{}\t{}", b.rssi, b.channel, if b.secured { "wpa" } else { "open" }, b.ssid));
                            }
                        }
                        Ok(n) => {
                            out.push(format!("{} network(s){}:", n, if c.scan_done { "" } else { " (the scan did not finish)" }));
                            for b in c.scan.iter().take(30) {
                                out.push(format!(
                                    "  {:>4} dBm  ch {:>3}  {}  {}",
                                    b.rssi,
                                    b.channel,
                                    if b.secured { "WPA " } else { "open" },
                                    if b.ssid.is_empty() { "(hidden)" } else { b.ssid.as_str() }
                                ));
                            }
                        }
                    }
                }
            }
        },
        "connect" => {
            let Some(ssid) = args.get(1) else {
                out.push(String::from("usage: wifi connect SSID [PASSWORD]"));
                return out;
            };
            let pass = args.get(2).copied().unwrap_or("");
            let key = if pass.is_empty() { String::new() } else { psk_hex(ssid, pass) };
            match connect(ssid, &key) {
                Ok(m) => {
                    out.push(format!("wifi: {}", m));
                    let cfg = format!("ssid={}\npsk={}\n", ssid, key);
                    match crate::vfs::write(CONFIG, cfg.as_bytes()) {
                        Ok(()) => out.push(format!("saved in {} (the derived key, not the password); joins at every start", CONFIG)),
                        Err(e) => out.push(format!("not saved: {}", e)),
                    }
                }
                Err(e) => {
                    set_status(format!("join failed: {}", e));
                    out.push(format!("wifi: {}", e));
                }
            }
        }
        "forget" => {
            let _ = crate::vfs::remove(CONFIG);
            out.push(format!("wifi: {} removed", CONFIG));
        }
        _ => out.push(String::from("usage: wifi [status | scan | connect SSID [PASSWORD] | forget]")),
    }
    out
}

/// Boot (background): when WIFI.CFG exists and no wired network came up,
/// start the chip and join.
pub fn autostart() {
    let Ok(cfg) = crate::vfs::read(CONFIG) else { return };
    if crate::net::info().map_or(false, |i| i.attached && i.ip != [0; 4] && i.ip[0] != 127) {
        set_status(String::from("a wired network is up; Wi-Fi left off (`wifi connect` switches)"));
        return;
    }
    let text = String::from_utf8_lossy(&cfg).into_owned();
    let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(k)).unwrap_or("").trim().to_string();
    let (ssid, psk) = (get("ssid="), get("psk="));
    if ssid.is_empty() {
        return;
    }
    if let Err(e) = connect(&ssid, &psk) {
        set_status(format!("autostart: {}", e));
    }
}

/// What can be checked without the chip: the WPA2 key derivation against
/// the IEEE 802.11i test vector, and the NVRAM blob format.
pub fn self_test() -> bool {
    let psk = psk_hex("IEEE", "password");
    let key_ok = psk == "f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e";
    let nv = nvram_blob(b"# comment\nsromrev=11\n\n  boardtype=0x6e4  # trailing\nnot a pair\n");
    let body = b"sromrev=11\0boardtype=0x6e4\0";
    let words = (nv.len() - 4) / 4;
    let token = le32(&nv, nv.len() - 4);
    let nv_ok = nv.starts_with(body)
        && (nv.len() - 4) % 4 == 0
        && nv[body.len()..nv.len() - 4].iter().all(|&b| b == 0)
        && nv.len() - 4 > body.len()
        && token == ((!(words as u32)) << 16 | words as u32);
    if key_ok && nv_ok {
        crate::println!("[kernel] MILESTONE: WIFI KEY OK (WPA2 PSK = IEEE 802.11i vector, NVRAM blob + length token)");
    } else {
        crate::println!("[wifi] self-test FAILED: psk {} ({}), nvram {}", psk, key_ok, nv_ok);
    }
    key_ok && nv_ok
}
