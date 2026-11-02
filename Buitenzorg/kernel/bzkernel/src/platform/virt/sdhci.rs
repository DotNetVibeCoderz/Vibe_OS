//! SD cards on an SD Host Controller Interface (SDHCI v3) -- the Raspberry Pi
//! 4's card slot (EMMC2, `brcm,bcm2711-emmc2`) and the generic SDHCI QEMU
//! models (roadmap R3). Programmed I/O, one 512-byte block per command: no
//! DMA, so there is no cache maintenance to get wrong, and no interrupts.
//!
//! The Broadcom controllers want 32-bit register accesses only, so every
//! 8/16-bit register is changed with a 32-bit read-modify-write.
//!
//! Card start-up follows the SD Physical Layer spec: CMD0 (idle), CMD8
//! (voltage check: a v2 card answers), ACMD41 until the card is ready (CCS
//! says high capacity: block addressing), CMD2 + CMD3 (its address), CMD7
//! (select), ACMD6 (4-bit bus), then a 25 MHz clock.

use alloc::string::String;

use crate::driver::{BlockDevice, SECTOR_SIZE};

const ARG2: u64 = 0x00;
const BLKSIZECNT: u64 = 0x04;
const ARG1: u64 = 0x08;
const CMDTM: u64 = 0x0C;
const RESP0: u64 = 0x10;
const DATA: u64 = 0x20;
const STATUS: u64 = 0x24;
const CONTROL0: u64 = 0x28; // host control 1 | power | block gap | wake-up
const CONTROL1: u64 = 0x2C; // clock control | timeout | software reset
const INTERRUPT: u64 = 0x30;
const IRPT_EN: u64 = 0x34;
const IRPT_SIGNAL: u64 = 0x38;
const CAPS: u64 = 0x40;

// STATUS
const CMD_INHIBIT: u32 = 1 << 0;
const DAT_INHIBIT: u32 = 1 << 1;
const CARD_INSERTED: u32 = 1 << 16;
// INTERRUPT
const CMD_DONE: u32 = 1 << 0;
const DATA_DONE: u32 = 1 << 1;
const WRITE_READY: u32 = 1 << 4;
const READ_READY: u32 = 1 << 5;
const ERROR: u32 = 1 << 15;
// CONTROL1
const CLK_INTLEN: u32 = 1 << 0;
const CLK_STABLE: u32 = 1 << 1;
const CLK_EN: u32 = 1 << 2;
const SRST_HC: u32 = 1 << 24;
const SRST_CMD: u32 = 1 << 25;
const SRST_DATA: u32 = 1 << 26;

// Command register flags (upper half of CMDTM).
const RESP_NONE: u32 = 0;
const RESP_136: u32 = 1;
const RESP_48: u32 = 2;
const RESP_48_BUSY: u32 = 3;
const CRC_CHECK: u32 = 1 << 3;
const INDEX_CHECK: u32 = 1 << 4;
const DATA_PRESENT: u32 = 1 << 5;
// Transfer mode (lower half).
const TM_READ: u32 = 1 << 4;

const R1: u32 = RESP_48 | CRC_CHECK | INDEX_CHECK;
const R1B: u32 = RESP_48_BUSY | CRC_CHECK | INDEX_CHECK;
const R2: u32 = RESP_136 | CRC_CHECK;
const R3: u32 = RESP_48;
const R6: u32 = RESP_48 | CRC_CHECK | INDEX_CHECK;
const R7: u32 = RESP_48 | CRC_CHECK | INDEX_CHECK;

pub struct SdCard {
    base: u64,
    rca: u32,
    high_capacity: bool,
    sectors: u64,
    label: &'static str,
    /// The controller's base clock in Hz (firmware or capabilities register).
    pub base_hz: u64,
}

fn r32(a: u64) -> u32 {
    unsafe { (a as *const u32).read_volatile() }
}
fn w32(a: u64, v: u32) {
    unsafe { (a as *mut u32).write_volatile(v) }
}

/// Microseconds of counter time.
fn delay_us(us: u64) {
    let hz = crate::arch::cpu::counter_hz().max(1);
    let t0 = crate::arch::cpu::timestamp();
    while crate::arch::cpu::timestamp() - t0 < us * hz / 1_000_000 {
        core::hint::spin_loop();
    }
}

/// Wait until `cond` holds, up to `us` microseconds.
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

impl SdCard {
    fn reg(&self, off: u64) -> u64 {
        self.base + off
    }

    /// Bring up the card on the controller mapped at `base`, whose base
    /// clock is `base_hz` (0 = take it from the capabilities register).
    pub fn init(base: u64, base_hz: u64, label: &'static str) -> Result<SdCard, &'static str> {
        let mut c = SdCard { base, rca: 0, high_capacity: false, sectors: 0, label, base_hz: 0 };
        // Full reset.
        w32(c.reg(CONTROL1), r32(c.reg(CONTROL1)) | SRST_HC);
        if !wait(100_000, || r32(c.reg(CONTROL1)) & SRST_HC == 0) {
            return Err("sdhci: controller reset timed out");
        }
        // 3.3 V, bus power on.
        w32(c.reg(CONTROL0), (r32(c.reg(CONTROL0)) & !0xFF00) | (0x0F << 8));
        delay_us(2_000);
        // Every status bit visible, none signalled (we poll).
        w32(c.reg(IRPT_EN), 0xFFFF_FFFF);
        w32(c.reg(IRPT_SIGNAL), 0);
        let caps_hz = ((r32(c.reg(CAPS)) >> 8) & 0xFF) as u64 * 1_000_000;
        let base_hz = if base_hz != 0 { base_hz } else { caps_hz };
        if base_hz == 0 {
            return Err("sdhci: unknown base clock");
        }
        c.base_hz = base_hz;
        c.set_clock(base_hz, 400_000)?;
        if !wait(100_000, || r32(c.reg(STATUS)) & CARD_INSERTED != 0) {
            // EMMC2 on a Pi has "broken-cd": carry on and let the card answer or not.
        }

        c.cmd(0, 0, RESP_NONE, 0)?; // GO_IDLE_STATE
        delay_us(1_000);
        let v2 = c.cmd(8, 0x1AA, R7, 0).map(|r| r & 0xFFF == 0x1AA).unwrap_or(false);
        // ACMD41 until ready (bit 31), asking for high capacity on a v2 card.
        let ocr_arg = 0x00FF_8000 | if v2 { 1 << 30 } else { 0 };
        let mut ocr = 0;
        let ready = wait(1_500_000, || {
            if c.cmd(55, 0, R1, 0).is_err() {
                return false;
            }
            match c.cmd(41, ocr_arg, R3, 0) {
                Ok(r) => {
                    ocr = r;
                    if r & (1 << 31) == 0 {
                        delay_us(10_000);
                    }
                    r & (1 << 31) != 0
                }
                Err(_) => false,
            }
        });
        if !ready {
            return Err("sdhci: no card answered ACMD41");
        }
        c.high_capacity = ocr & (1 << 30) != 0;
        c.cmd(2, 0, R2, 0)?; // ALL_SEND_CID
        let r6 = c.cmd(3, 0, R6, 0)?; // SEND_RELATIVE_ADDR
        c.rca = r6 & 0xFFFF_0000;
        let csd = c.cmd_r2(9, c.rca)?; // SEND_CSD (before selecting)
        c.sectors = csd_sectors(csd);
        c.cmd(7, c.rca, R1B, 0)?; // SELECT_CARD
        if !c.high_capacity {
            c.cmd(16, 512, R1, 0)?; // SET_BLOCKLEN
        }
        // 4-bit bus.
        c.cmd(55, c.rca, R1, 0)?;
        c.cmd(6, 2, R1, 0)?;
        w32(c.reg(CONTROL0), r32(c.reg(CONTROL0)) | (1 << 1));
        c.set_clock(base_hz, 25_000_000)?;
        Ok(c)
    }

    fn set_clock(&mut self, base_hz: u64, want_hz: u64) -> Result<(), &'static str> {
        // SD clock = base / (2 * N), N in 10 bits (0 = base itself).
        let n = if base_hz <= want_hz { 0 } else { base_hz.div_ceil(2 * want_hz).min(0x3FF) as u32 };
        let mut v = r32(self.reg(CONTROL1));
        v &= !(CLK_EN | 0xFFC0 | (0xF << 16));
        w32(self.reg(CONTROL1), v);
        delay_us(100);
        v |= ((n & 0xFF) << 8) | (((n >> 8) & 3) << 6) | CLK_INTLEN | (0xE << 16);
        w32(self.reg(CONTROL1), v);
        if !wait(100_000, || r32(self.reg(CONTROL1)) & CLK_STABLE != 0) {
            return Err("sdhci: clock did not settle");
        }
        w32(self.reg(CONTROL1), r32(self.reg(CONTROL1)) | CLK_EN);
        delay_us(2_000);
        Ok(())
    }

    fn reset_lines(&mut self) {
        w32(self.reg(CONTROL1), r32(self.reg(CONTROL1)) | SRST_CMD | SRST_DATA);
        wait(100_000, || r32(self.reg(CONTROL1)) & (SRST_CMD | SRST_DATA) == 0);
    }

    /// Send a command; returns response word 0. `tm` = transfer-mode bits.
    fn cmd(&mut self, index: u32, arg: u32, flags: u32, tm: u32) -> Result<u32, &'static str> {
        let busy = if flags & 3 == RESP_48_BUSY || flags & DATA_PRESENT != 0 { CMD_INHIBIT | DAT_INHIBIT } else { CMD_INHIBIT };
        if !wait(500_000, || r32(self.reg(STATUS)) & busy == 0) {
            self.reset_lines();
            return Err("sdhci: controller busy");
        }
        w32(self.reg(INTERRUPT), 0xFFFF_FFFF);
        w32(self.reg(ARG2), 0);
        w32(self.reg(ARG1), arg);
        w32(self.reg(CMDTM), (index << 24) | (flags << 16) | tm);
        let mut st = 0;
        if !wait(500_000, || {
            st = r32(self.reg(INTERRUPT));
            st & (CMD_DONE | ERROR) != 0
        }) || st & ERROR != 0
        {
            w32(self.reg(INTERRUPT), 0xFFFF_FFFF);
            self.reset_lines();
            return Err("sdhci: command failed");
        }
        w32(self.reg(INTERRUPT), CMD_DONE);
        if flags & 3 == RESP_48_BUSY {
            wait(1_000_000, || r32(self.reg(INTERRUPT)) & (DATA_DONE | ERROR) != 0);
            w32(self.reg(INTERRUPT), DATA_DONE);
        }
        Ok(r32(self.reg(RESP0)))
    }

    /// A 136-bit response (CID/CSD), as the controller stores it: bits 8..135
    /// of the response, shifted down by 8, in RESP0..RESP3.
    fn cmd_r2(&mut self, index: u32, arg: u32) -> Result<[u32; 4], &'static str> {
        self.cmd(index, arg, R2, 0)?;
        Ok([r32(self.reg(RESP0)), r32(self.reg(RESP0 + 4)), r32(self.reg(RESP0 + 8)), r32(self.reg(RESP0 + 12))])
    }

    fn address(&self, lba: u64) -> u32 {
        if self.high_capacity { lba as u32 } else { (lba * 512) as u32 }
    }

    fn finish_data(&mut self) -> Result<(), &'static str> {
        let mut st = 0;
        if !wait(1_000_000, || {
            st = r32(self.reg(INTERRUPT));
            st & (DATA_DONE | ERROR) != 0
        }) || st & ERROR != 0
        {
            w32(self.reg(INTERRUPT), 0xFFFF_FFFF);
            self.reset_lines();
            return Err("sdhci: data transfer failed");
        }
        w32(self.reg(INTERRUPT), DATA_DONE);
        Ok(())
    }
}

/// Card capacity in 512-byte sectors from the CSD (v1 and v2 layouts).
fn csd_sectors(r: [u32; 4]) -> u64 {
    // The controller drops the CRC byte: CSD bit n lives at bit (n - 8).
    let bits = |hi: u32, lo: u32| -> u64 {
        let mut v = 0u64;
        for b in (lo..=hi).rev() {
            let n = b - 8;
            v = (v << 1) | ((r[(n / 32) as usize] >> (n % 32)) & 1) as u64;
        }
        v
    };
    match bits(127, 126) {
        1 => (bits(69, 48) + 1) * 1024, // v2: C_SIZE in 512 KiB units
        0 => {
            let c_size = bits(73, 62);
            let mult = bits(49, 47);
            let read_bl_len = bits(83, 80);
            ((c_size + 1) << (mult + 2)) << read_bl_len >> 9
        }
        _ => 0,
    }
}

impl BlockDevice for SdCard {
    fn name(&self) -> String {
        alloc::format!("{} ({})", self.label, if self.high_capacity { "SDHC/SDXC" } else { "SDSC" })
    }

    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read_sector(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        w32(self.reg(BLKSIZECNT), (1 << 16) | 512);
        let addr = self.address(lba);
        self.cmd(17, addr, R1 | DATA_PRESENT, TM_READ)?;
        if !wait(1_000_000, || r32(self.reg(INTERRUPT)) & (READ_READY | ERROR) != 0) || r32(self.reg(INTERRUPT)) & ERROR != 0 {
            self.reset_lines();
            return Err("sdhci: no data from the card");
        }
        w32(self.reg(INTERRUPT), READ_READY);
        for i in 0..128 {
            let v = r32(self.reg(DATA));
            buf[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        self.finish_data()
    }

    fn write_sector(&mut self, lba: u64, buf: &[u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        w32(self.reg(BLKSIZECNT), (1 << 16) | 512);
        let addr = self.address(lba);
        self.cmd(24, addr, R1 | DATA_PRESENT, 0)?;
        if !wait(1_000_000, || r32(self.reg(INTERRUPT)) & (WRITE_READY | ERROR) != 0) || r32(self.reg(INTERRUPT)) & ERROR != 0 {
            self.reset_lines();
            return Err("sdhci: card not ready for data");
        }
        w32(self.reg(INTERRUPT), WRITE_READY);
        for i in 0..128 {
            w32(self.reg(DATA), u32::from_le_bytes(buf[i * 4..i * 4 + 4].try_into().unwrap()));
        }
        self.finish_data()?;
        // Wait for the card to finish programming (DAT0 busy).
        wait(1_000_000, || r32(self.reg(STATUS)) & DAT_INHIBIT == 0);
        Ok(())
    }
}
