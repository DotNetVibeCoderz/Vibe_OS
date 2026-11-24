//! An NES (Famicom) emulator core for managed programs: the 6502 CPU (every
//! official opcode plus the common unofficial NOPs / LAX / SAX / DCP / ISB /
//! SLO / RLA / SRE / RRA), the 2C02 PPU (background with loopy scrolling,
//! 8x8 / 8x16 sprites, sprite 0 hit, sprite overflow, vblank NMI, the left
//! 8-pixel masks), the APU (two pulse channels with envelope / sweep /
//! length counter, triangle with linear counter, noise; no DMC) mixed to
//! 48 kHz mono, two controllers, and mappers 0 (NROM), 2 (UxROM) and
//! 3 (CNROM).
//!
//! Timing is per scanline: each of the 262 lines renders its pixels, then
//! runs ~113.67 CPU cycles. Enough for games that change scroll in hblank;
//! mid-line effects are not modelled.
//!
//! Pure code (alloc only, libm for the mixer), no clrhost types: a PC test
//! crate includes it with `#[path]`. The natives are in `nesapi.rs`.

use alloc::vec::Vec;

pub const WIDTH: usize = 256;
pub const HEIGHT: usize = 240;
pub const SAMPLE_RATE: u32 = 48_000;
const CPU_HZ: u32 = 1_789_773;

/// 2C02 colours (0xRRGGBB).
pub const PALETTE: [u32; 64] = [
    0x666666, 0x002A88, 0x1412A7, 0x3B00A4, 0x5C007E, 0x6E0040, 0x6C0600, 0x561D00, 0x333500, 0x0B4800, 0x005200, 0x004F08, 0x00404D, 0x000000, 0x000000, 0x000000,
    0xADADAD, 0x155FD9, 0x4240FF, 0x7527FE, 0xA01ACC, 0xB71E7B, 0xB53120, 0x994E00, 0x6B6D00, 0x388700, 0x0C9300, 0x008F32, 0x007C8D, 0x000000, 0x000000, 0x000000,
    0xFFFEFF, 0x64B0FF, 0x9290FF, 0xC676FF, 0xF36AFF, 0xFE6ECC, 0xFE8170, 0xEA9E22, 0xBCBE00, 0x88D800, 0x5CE430, 0x45E082, 0x48CDDE, 0x4F4F4F, 0x000000, 0x000000,
    0xFFFEFF, 0xC0DFFF, 0xD3D2FF, 0xE8C8FF, 0xFBC2FF, 0xFEC4EA, 0xFECCC5, 0xF7D8A5, 0xE4E594, 0xCFEF96, 0xBDF4AB, 0xB3F3CC, 0xB5EBF2, 0xB8B8B8, 0x000000, 0x000000,
];

/// Controller buttons (bit order of the shift register).
pub mod button {
    pub const A: u8 = 1;
    pub const B: u8 = 2;
    pub const SELECT: u8 = 4;
    pub const START: u8 = 8;
    pub const UP: u8 = 16;
    pub const DOWN: u8 = 32;
    pub const LEFT: u8 = 64;
    pub const RIGHT: u8 = 128;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
}

/// Why a ROM did not load.
#[derive(Debug, PartialEq, Eq)]
pub enum LoadError {
    NotINes,
    Truncated,
    Mapper(u8),
}

struct Cart {
    prg: Vec<u8>,
    chr: Vec<u8>,
    chr_ram: bool,
    prg_ram: [u8; 8192],
    mapper: u8,
    mirroring: Mirroring,
    /// UxROM: the 16 KiB bank at $8000; CNROM: the 8 KiB CHR bank.
    bank: usize,
}

impl Cart {
    fn parse(rom: &[u8]) -> Result<Cart, LoadError> {
        if rom.len() < 16 || &rom[0..4] != b"NES\x1A" {
            return Err(LoadError::NotINes);
        }
        let prg_len = rom[4] as usize * 16384;
        let chr_len = rom[5] as usize * 8192;
        let trainer = if rom[6] & 4 != 0 { 512 } else { 0 };
        let mapper = (rom[6] >> 4) | (rom[7] & 0xF0);
        if !matches!(mapper, 0 | 2 | 3) {
            return Err(LoadError::Mapper(mapper));
        }
        let start = 16 + trainer;
        if prg_len == 0 || rom.len() < start + prg_len + chr_len {
            return Err(LoadError::Truncated);
        }
        let prg = rom[start..start + prg_len].to_vec();
        let (chr, chr_ram) = if chr_len == 0 { (alloc::vec![0u8; 8192], true) } else { (rom[start + prg_len..start + prg_len + chr_len].to_vec(), false) };
        let mirroring = if rom[6] & 8 != 0 {
            Mirroring::FourScreen
        } else if rom[6] & 1 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };
        Ok(Cart { prg, chr, chr_ram, prg_ram: [0; 8192], mapper, mirroring, bank: 0 })
    }

    fn read_prg(&self, a: u16) -> u8 {
        match a {
            0x6000..=0x7FFF => self.prg_ram[(a - 0x6000) as usize],
            0x8000..=0xFFFF => {
                let n = self.prg.len();
                let off = (a - 0x8000) as usize;
                if self.mapper == 2 {
                    let banks = n / 16384;
                    if off < 0x4000 {
                        self.prg[(self.bank % banks) * 16384 + off]
                    } else {
                        self.prg[(banks - 1) * 16384 + off - 0x4000]
                    }
                } else {
                    self.prg[off % n]
                }
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, a: u16, v: u8) {
        match a {
            0x6000..=0x7FFF => self.prg_ram[(a - 0x6000) as usize] = v,
            0x8000..=0xFFFF => {
                if self.mapper == 2 || self.mapper == 3 {
                    // bus conflicts ignored
                    self.bank = v as usize;
                }
            }
            _ => {}
        }
    }

    fn chr_addr(&self, a: u16) -> usize {
        let a = a as usize & 0x1FFF;
        if self.mapper == 3 {
            (self.bank * 8192 + a) % self.chr.len()
        } else {
            a % self.chr.len()
        }
    }

    fn read_chr(&self, a: u16) -> u8 {
        self.chr[self.chr_addr(a)]
    }

    fn write_chr(&mut self, a: u16, v: u8) {
        if self.chr_ram {
            let i = self.chr_addr(a);
            self.chr[i] = v;
        }
    }
}

// ---- PPU --------------------------------------------------------------------

struct Ppu {
    ctrl: u8,
    mask: u8,
    status: u8,
    oam_addr: u8,
    oam: [u8; 256],
    vram: [u8; 4096],
    palette: [u8; 32],
    /// loopy registers
    v: u16,
    t: u16,
    x: u8,
    w: bool,
    read_buf: u8,
    nmi: bool,
    /// palette indices of the frame (0..63)
    frame: Vec<u8>,
}

impl Ppu {
    fn new() -> Ppu {
        Ppu {
            ctrl: 0,
            mask: 0,
            status: 0,
            oam_addr: 0,
            oam: [0; 256],
            vram: [0; 4096],
            palette: [0x0F; 32],
            v: 0,
            t: 0,
            x: 0,
            w: false,
            read_buf: 0,
            nmi: false,
            frame: alloc::vec![0u8; WIDTH * HEIGHT],
        }
    }

    fn rendering(&self) -> bool {
        self.mask & 0x18 != 0
    }

    fn nt_index(&self, cart: &Cart, a: u16) -> usize {
        let a = (a as usize - 0x2000) & 0x0FFF;
        let table = a / 0x400;
        let off = a & 0x3FF;
        let t = match cart.mirroring {
            Mirroring::Vertical => table & 1,
            Mirroring::Horizontal => table >> 1,
            Mirroring::FourScreen => table,
        };
        t * 0x400 + off
    }

    fn pal_index(a: u16) -> usize {
        let mut i = (a & 0x1F) as usize;
        if i >= 16 && i % 4 == 0 {
            i -= 16;
        }
        i
    }

    fn read_vram(&self, cart: &Cart, a: u16) -> u8 {
        let a = a & 0x3FFF;
        match a {
            0..=0x1FFF => cart.read_chr(a),
            0x2000..=0x3EFF => self.vram[self.nt_index(cart, a)],
            _ => self.palette[Self::pal_index(a)],
        }
    }

    fn write_vram(&mut self, cart: &mut Cart, a: u16, v: u8) {
        let a = a & 0x3FFF;
        match a {
            0..=0x1FFF => cart.write_chr(a, v),
            0x2000..=0x3EFF => {
                let i = self.nt_index(cart, a);
                self.vram[i] = v;
            }
            _ => self.palette[Self::pal_index(a)] = v & 0x3F,
        }
    }

    fn read_reg(&mut self, cart: &Cart, r: u16) -> u8 {
        match r & 7 {
            2 => {
                let s = self.status;
                self.status &= 0x7F;
                self.w = false;
                s
            }
            4 => self.oam[self.oam_addr as usize],
            7 => {
                let a = self.v & 0x3FFF;
                let v = if a >= 0x3F00 {
                    self.read_buf = self.read_vram(cart, a - 0x1000);
                    self.read_vram(cart, a)
                } else {
                    let old = self.read_buf;
                    self.read_buf = self.read_vram(cart, a);
                    old
                };
                self.v = self.v.wrapping_add(if self.ctrl & 4 != 0 { 32 } else { 1 });
                v
            }
            _ => 0,
        }
    }

    fn write_reg(&mut self, cart: &mut Cart, r: u16, v: u8) {
        match r & 7 {
            0 => {
                let was = self.ctrl & 0x80;
                self.ctrl = v;
                self.t = (self.t & !0x0C00) | ((v as u16 & 3) << 10);
                // enabling NMI during vblank raises one at once
                if was == 0 && v & 0x80 != 0 && self.status & 0x80 != 0 {
                    self.nmi = true;
                }
            }
            1 => self.mask = v,
            3 => self.oam_addr = v,
            4 => {
                self.oam[self.oam_addr as usize] = v;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            5 => {
                if !self.w {
                    self.t = (self.t & !0x001F) | (v as u16 >> 3);
                    self.x = v & 7;
                } else {
                    self.t = (self.t & !0x73E0) | ((v as u16 & 7) << 12) | ((v as u16 >> 3) << 5);
                }
                self.w = !self.w;
            }
            6 => {
                if !self.w {
                    self.t = (self.t & 0x00FF) | ((v as u16 & 0x3F) << 8);
                } else {
                    self.t = (self.t & 0xFF00) | v as u16;
                    self.v = self.t;
                }
                self.w = !self.w;
            }
            7 => {
                let a = self.v;
                self.write_vram(cart, a, v);
                self.v = self.v.wrapping_add(if self.ctrl & 4 != 0 { 32 } else { 1 });
            }
            _ => {}
        }
    }

    fn incr_y(&mut self) {
        if self.v & 0x7000 != 0x7000 {
            self.v += 0x1000;
        } else {
            self.v &= !0x7000;
            let mut y = (self.v & 0x03E0) >> 5;
            if y == 29 {
                y = 0;
                self.v ^= 0x0800;
            } else if y == 31 {
                y = 0;
            } else {
                y += 1;
            }
            self.v = (self.v & !0x03E0) | (y << 5);
        }
    }

    /// Draw scanline `line` (0..240) from the current scroll.
    fn render_line(&mut self, cart: &Cart, line: usize) {
        let mut bg = [0u8; WIDTH]; // 0 = transparent, else palette slot 1..15
        let show_bg = self.mask & 0x08 != 0;
        let show_sp = self.mask & 0x10 != 0;
        if show_bg {
            let fine_y = (self.v >> 12) & 7;
            let pt = if self.ctrl & 0x10 != 0 { 0x1000 } else { 0 };
            let mut v = self.v;
            let mut px: i32 = -(self.x as i32);
            for _ in 0..33 {
                let nt_addr = 0x2000 | (v & 0x0FFF);
                let tile = self.vram[self.nt_index(cart, nt_addr)] as u16;
                let at_addr = 0x23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07);
                let at = self.vram[self.nt_index(cart, at_addr)];
                let shift = ((v >> 4) & 4) | (v & 2);
                let pal = (at >> shift) & 3;
                let lo = cart.read_chr(pt + tile * 16 + fine_y);
                let hi = cart.read_chr(pt + tile * 16 + fine_y + 8);
                for b in 0..8 {
                    let xx = px + b;
                    if (0..WIDTH as i32).contains(&xx) {
                        let bit = 7 - b;
                        let c = ((lo >> bit) & 1) | (((hi >> bit) & 1) << 1);
                        bg[xx as usize] = if c == 0 { 0 } else { pal * 4 + c };
                    }
                }
                px += 8;
                // coarse x increment
                if v & 0x001F == 31 {
                    v &= !0x001F;
                    v ^= 0x0400;
                } else {
                    v += 1;
                }
            }
            if self.mask & 0x02 == 0 {
                bg[..8].fill(0);
            }
        }
        let row = &mut self.frame[line * WIDTH..(line + 1) * WIDTH];
        let backdrop = self.palette[0];
        for x in 0..WIDTH {
            row[x] = if bg[x] != 0 { self.palette[bg[x] as usize] } else { backdrop };
        }
        if !show_sp {
            return;
        }
        let tall = self.ctrl & 0x20 != 0;
        let h = if tall { 16 } else { 8 };
        let mut found = 0;
        // pixel already taken by a higher-priority sprite
        let mut taken = [false; WIDTH];
        for i in 0..64 {
            let y = self.oam[i * 4] as usize + 1;
            if line < y || line >= y + h {
                continue;
            }
            found += 1;
            if found > 8 {
                self.status |= 0x20; // overflow
                break;
            }
            let tile = self.oam[i * 4 + 1] as u16;
            let attr = self.oam[i * 4 + 2];
            let sx = self.oam[i * 4 + 3] as usize;
            let mut r = (line - y) as u16;
            if attr & 0x80 != 0 {
                r = h as u16 - 1 - r;
            }
            let addr = if tall {
                let bank = (tile & 1) * 0x1000;
                bank + (tile & 0xFE) * 16 + if r >= 8 { 16 + r - 8 } else { r }
            } else {
                (if self.ctrl & 0x08 != 0 { 0x1000 } else { 0 }) + tile * 16 + r
            };
            let lo = cart.read_chr(addr);
            let hi = cart.read_chr(addr + 8);
            for b in 0..8 {
                let x = sx + b;
                if x >= WIDTH {
                    break;
                }
                if x < 8 && self.mask & 0x04 == 0 {
                    continue;
                }
                let bit = if attr & 0x40 != 0 { b } else { 7 - b };
                let c = ((lo >> bit) & 1) | (((hi >> bit) & 1) << 1);
                if c == 0 {
                    continue;
                }
                if i == 0 && bg[x] != 0 && x != 255 {
                    self.status |= 0x40; // sprite 0 hit
                }
                if taken[x] {
                    continue;
                }
                taken[x] = true;
                if attr & 0x20 == 0 || bg[x] == 0 {
                    row[x] = self.palette[16 + (attr as usize & 3) * 4 + c as usize];
                }
            }
        }
    }
}

// ---- APU --------------------------------------------------------------------

const LENGTHS: [u8; 32] = [10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30];
const DUTY: [[u8; 8]; 4] = [[0, 1, 0, 0, 0, 0, 0, 0], [0, 1, 1, 0, 0, 0, 0, 0], [0, 1, 1, 1, 1, 0, 0, 0], [1, 0, 0, 1, 1, 1, 1, 1]];
const TRIANGLE: [u8; 32] = [15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const NOISE_PERIOD: [u16; 16] = [4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068];

#[derive(Default, Clone, Copy)]
struct Envelope {
    start: bool,
    divider: u8,
    decay: u8,
    volume: u8,
    constant: bool,
    looped: bool,
}

impl Envelope {
    fn clock(&mut self) {
        if self.start {
            self.start = false;
            self.decay = 15;
            self.divider = self.volume;
        } else if self.divider == 0 {
            self.divider = self.volume;
            if self.decay > 0 {
                self.decay -= 1;
            } else if self.looped {
                self.decay = 15;
            }
        } else {
            self.divider -= 1;
        }
    }
    fn level(&self) -> u8 {
        if self.constant { self.volume } else { self.decay }
    }
}

#[derive(Default, Clone, Copy)]
struct Pulse {
    enabled: bool,
    duty: u8,
    env: Envelope,
    length: u8,
    timer: u16,
    sweep_on: bool,
    sweep_period: u8,
    sweep_neg: bool,
    sweep_shift: u8,
    sweep_div: u8,
    sweep_reload: bool,
    /// step in the 8-step sequence, cycles towards the next one
    phase: u32,
    acc: u32,
    second: bool,
}

impl Pulse {
    fn target(&self) -> u16 {
        let d = self.timer >> self.sweep_shift;
        if self.sweep_neg {
            self.timer.wrapping_sub(d).wrapping_sub(if self.second { 0 } else { 1 })
        } else {
            self.timer + d
        }
    }
    fn muted(&self) -> bool {
        self.timer < 8 || self.target() > 0x7FF
    }
    fn clock_sweep(&mut self) {
        if self.sweep_div == 0 && self.sweep_on && self.sweep_shift > 0 && !self.muted() {
            self.timer = self.target();
        }
        if self.sweep_div == 0 || self.sweep_reload {
            self.sweep_div = self.sweep_period;
            self.sweep_reload = false;
        } else {
            self.sweep_div -= 1;
        }
    }
    /// `cycles` CPU cycles have passed: one sequence step every 2 * (timer + 1).
    fn sample(&mut self, cycles: u32) -> u8 {
        if !self.enabled || self.length == 0 || self.muted() {
            return 0;
        }
        let period = 2 * (self.timer as u32 + 1);
        self.acc += cycles;
        if self.acc >= period {
            self.phase = (self.phase + self.acc / period) & 7;
            self.acc %= period;
        }
        if DUTY[self.duty as usize][self.phase as usize] != 0 { self.env.level() } else { 0 }
    }
}

#[derive(Default, Clone, Copy)]
struct Triangle {
    enabled: bool,
    control: bool,
    linear_load: u8,
    linear: u8,
    reload: bool,
    length: u8,
    timer: u16,
    phase: u32,
    acc: u32,
}

impl Triangle {
    fn sample(&mut self, cycles: u32) -> u8 {
        if self.enabled && self.length > 0 && self.linear > 0 && self.timer >= 2 {
            let period = self.timer as u32 + 1;
            self.acc += cycles;
            if self.acc >= period {
                self.phase = (self.phase + self.acc / period) & 31;
                self.acc %= period;
            }
        }
        TRIANGLE[self.phase as usize]
    }
}

#[derive(Clone, Copy)]
struct Noise {
    enabled: bool,
    env: Envelope,
    mode: bool,
    period: u16,
    length: u8,
    lfsr: u16,
    acc: u32,
}

impl Noise {
    fn sample(&mut self, cycles: u32) -> u8 {
        self.acc += cycles;
        let p = self.period as u32;
        while self.acc >= p {
            self.acc -= p;
            let bit = if self.mode { 6 } else { 1 };
            let fb = (self.lfsr ^ (self.lfsr >> bit)) & 1;
            self.lfsr = (self.lfsr >> 1) | (fb << 14);
        }
        if !self.enabled || self.length == 0 || self.lfsr & 1 != 0 { 0 } else { self.env.level() }
    }
}

struct Apu {
    pulse: [Pulse; 2],
    tri: Triangle,
    noise: Noise,
    five_step: bool,
    irq_inhibit: bool,
    /// samples of the frame being built, and the fraction owed
    out: Vec<i16>,
    /// CPU cycles * 48000 not turned into a sample yet, cycles since the last one
    owed: u32,
    since: u32,
    /// DC blocker state (sample << 9)
    filter: i32,
    /// non-linear mixer tables (Nesdev), scaled so 1.0 = 40000
    pulse_mix: [i32; 31],
    tnd_mix: [i32; 203],
}

impl Apu {
    fn new() -> Apu {
        let mut p2 = Pulse::default();
        p2.second = true;
        Apu {
            pulse: [Pulse::default(), p2],
            tri: Triangle::default(),
            noise: Noise { enabled: false, env: Envelope::default(), mode: false, period: 4, length: 0, lfsr: 1, acc: 0 },
            five_step: false,
            irq_inhibit: true,
            out: Vec::new(),
            owed: 0,
            since: 0,
            filter: 0,
            pulse_mix: core::array::from_fn(|n| if n == 0 { 0 } else { (95.52 / (8128.0 / n as f64 + 100.0) * 40000.0) as i32 }),
            tnd_mix: core::array::from_fn(|n| if n == 0 { 0 } else { (163.67 / (24329.0 / n as f64 + 100.0) * 40000.0) as i32 }),
        }
    }

    fn write(&mut self, a: u16, v: u8) {
        match a {
            0x4000 | 0x4004 => {
                let p = &mut self.pulse[((a - 0x4000) / 4) as usize];
                p.duty = v >> 6;
                p.env.looped = v & 0x20 != 0;
                p.env.constant = v & 0x10 != 0;
                p.env.volume = v & 15;
            }
            0x4001 | 0x4005 => {
                let p = &mut self.pulse[((a - 0x4001) / 4) as usize];
                p.sweep_on = v & 0x80 != 0;
                p.sweep_period = (v >> 4) & 7;
                p.sweep_neg = v & 8 != 0;
                p.sweep_shift = v & 7;
                p.sweep_reload = true;
            }
            0x4002 | 0x4006 => {
                let p = &mut self.pulse[((a - 0x4002) / 4) as usize];
                p.timer = (p.timer & 0x700) | v as u16;
            }
            0x4003 | 0x4007 => {
                let p = &mut self.pulse[((a - 0x4003) / 4) as usize];
                p.timer = (p.timer & 0xFF) | ((v as u16 & 7) << 8);
                if p.enabled {
                    p.length = LENGTHS[(v >> 3) as usize];
                }
                p.env.start = true;
                p.phase = 0;
                p.acc = 0;
            }
            0x4008 => {
                self.tri.control = v & 0x80 != 0;
                self.tri.linear_load = v & 0x7F;
            }
            0x400A => self.tri.timer = (self.tri.timer & 0x700) | v as u16,
            0x400B => {
                self.tri.timer = (self.tri.timer & 0xFF) | ((v as u16 & 7) << 8);
                if self.tri.enabled {
                    self.tri.length = LENGTHS[(v >> 3) as usize];
                }
                self.tri.reload = true;
            }
            0x400C => {
                self.noise.env.looped = v & 0x20 != 0;
                self.noise.env.constant = v & 0x10 != 0;
                self.noise.env.volume = v & 15;
            }
            0x400E => {
                self.noise.mode = v & 0x80 != 0;
                self.noise.period = NOISE_PERIOD[(v & 15) as usize];
            }
            0x400F => {
                if self.noise.enabled {
                    self.noise.length = LENGTHS[(v >> 3) as usize];
                }
                self.noise.env.start = true;
            }
            0x4015 => {
                for (k, p) in self.pulse.iter_mut().enumerate() {
                    p.enabled = v & (1 << k) != 0;
                    if !p.enabled {
                        p.length = 0;
                    }
                }
                self.tri.enabled = v & 4 != 0;
                if !self.tri.enabled {
                    self.tri.length = 0;
                }
                self.noise.enabled = v & 8 != 0;
                if !self.noise.enabled {
                    self.noise.length = 0;
                }
            }
            0x4017 => {
                self.five_step = v & 0x80 != 0;
                self.irq_inhibit = v & 0x40 != 0;
            }
            _ => {}
        }
    }

    fn status(&self) -> u8 {
        (self.pulse[0].length > 0) as u8 | ((self.pulse[1].length > 0) as u8) << 1 | ((self.tri.length > 0) as u8) << 2 | ((self.noise.length > 0) as u8) << 3
    }

    fn quarter(&mut self) {
        self.pulse[0].env.clock();
        self.pulse[1].env.clock();
        self.noise.env.clock();
        let t = &mut self.tri;
        if t.reload {
            t.linear = t.linear_load;
        } else if t.linear > 0 {
            t.linear -= 1;
        }
        if !t.control {
            t.reload = false;
        }
    }

    fn half(&mut self) {
        for p in self.pulse.iter_mut() {
            if !p.env.looped && p.length > 0 {
                p.length -= 1;
            }
            p.clock_sweep();
        }
        if !self.tri.control && self.tri.length > 0 {
            self.tri.length -= 1;
        }
        if !self.noise.env.looped && self.noise.length > 0 {
            self.noise.length -= 1;
        }
    }

    /// Produce the samples `cycles` CPU cycles are worth.
    fn run(&mut self, cycles: u32) {
        self.since += cycles;
        self.owed += cycles * SAMPLE_RATE;
        while self.owed >= CPU_HZ {
            self.owed -= CPU_HZ;
            let c = self.since;
            self.since = 0;
            let p = self.pulse[0].sample(c) as usize + self.pulse[1].sample(c) as usize;
            let t = self.tri.sample(c) as usize;
            let n = self.noise.sample(c) as usize;
            let s = self.pulse_mix[p] + self.tnd_mix[3 * t + 2 * n];
            // DC blocker (the NES output is AC coupled): filter += (s - filter) / 512
            self.filter += s - (self.filter >> 9);
            let v = (s - (self.filter >> 9)).clamp(-32767, 32767);
            self.out.push(v as i16);
        }
    }
}

// ---- the machine --------------------------------------------------------------

pub struct Nes {
    cart: Cart,
    ppu: Ppu,
    apu: Apu,
    ram: [u8; 2048],
    a: u8,
    x: u8,
    y: u8,
    s: u8,
    p: u8,
    pc: u16,
    cycles: u64,
    stall: u32,
    pads: [u8; 2],
    shift: [u8; 2],
    strobe: bool,
    pub frames: u64,
    /// The program hit an opcode this core does not know (KIL/JAM).
    pub jammed: bool,
    pub instructions: u64,
}

const C: u8 = 0x01;
const Z: u8 = 0x02;
const I: u8 = 0x04;
const D: u8 = 0x08;
const B: u8 = 0x10;
const U: u8 = 0x20;
const V: u8 = 0x40;
const N: u8 = 0x80;

#[derive(Clone, Copy)]
enum Mode {
    Imp,
    Acc,
    Imm,
    Zp,
    Zpx,
    Zpy,
    Abs,
    Abx,
    Aby,
    Ind,
    Izx,
    Izy,
    Rel,
}

impl Nes {
    pub fn new(rom: &[u8]) -> Result<Nes, LoadError> {
        let cart = Cart::parse(rom)?;
        let mut n = Nes {
            cart,
            ppu: Ppu::new(),
            apu: Apu::new(),
            ram: [0; 2048],
            a: 0,
            x: 0,
            y: 0,
            s: 0xFD,
            p: I | U,
            pc: 0,
            cycles: 7,
            stall: 0,
            pads: [0; 2],
            shift: [0; 2],
            strobe: false,
            frames: 0,
            jammed: false,
            instructions: 0,
        };
        n.pc = n.read16(0xFFFC);
        Ok(n)
    }

    pub fn mapper(&self) -> u8 {
        self.cart.mapper
    }

    pub fn sizes(&self) -> (usize, usize) {
        (self.cart.prg.len(), if self.cart.chr_ram { 0 } else { self.cart.chr.len() })
    }

    /// Start a test at `pc` (nestest's automated mode starts at $C000).
    pub fn set_pc(&mut self, pc: u16) {
        self.pc = pc;
    }

    pub fn registers(&self) -> (u16, u8, u8, u8, u8, u8, u64) {
        (self.pc, self.a, self.x, self.y, self.p, self.s, self.cycles)
    }

    pub fn peek(&mut self, a: u16) -> u8 {
        self.read(a)
    }

    pub fn set_buttons(&mut self, pad: usize, b: u8) {
        self.pads[pad & 1] = b;
    }

    fn read(&mut self, a: u16) -> u8 {
        match a {
            0x0000..=0x1FFF => self.ram[(a & 0x7FF) as usize],
            0x2000..=0x3FFF => self.ppu.read_reg(&self.cart, a),
            0x4015 => self.apu.status(),
            0x4016 | 0x4017 => {
                let k = (a - 0x4016) as usize;
                if self.strobe {
                    return 0x40 | (self.pads[k] & 1);
                }
                let v = self.shift[k] & 1;
                self.shift[k] = (self.shift[k] >> 1) | 0x80;
                0x40 | v
            }
            0x4000..=0x5FFF => 0,
            _ => self.cart.read_prg(a),
        }
    }

    fn write(&mut self, a: u16, v: u8) {
        match a {
            0x0000..=0x1FFF => self.ram[(a & 0x7FF) as usize] = v,
            0x2000..=0x3FFF => self.ppu.write_reg(&mut self.cart, a, v),
            0x4014 => {
                // OAM DMA: 256 bytes from page v, 513 cycles
                let base = (v as u16) << 8;
                for i in 0..256u16 {
                    let b = self.read(base + i);
                    let at = self.ppu.oam_addr.wrapping_add(i as u8) as usize;
                    self.ppu.oam[at] = b;
                }
                self.stall += 513 + (self.cycles & 1) as u32;
            }
            0x4016 => {
                self.strobe = v & 1 != 0;
                if self.strobe {
                    self.shift = self.pads;
                }
            }
            0x4000..=0x4017 => self.apu.write(a, v),
            0x4018..=0x5FFF => {}
            _ => self.cart.write_prg(a, v),
        }
    }

    fn read16(&mut self, a: u16) -> u16 {
        self.read(a) as u16 | (self.read(a.wrapping_add(1)) as u16) << 8
    }

    fn push(&mut self, v: u8) {
        self.write(0x100 | self.s as u16, v);
        self.s = self.s.wrapping_sub(1);
    }

    fn pop(&mut self) -> u8 {
        self.s = self.s.wrapping_add(1);
        self.read(0x100 | self.s as u16)
    }

    fn zn(&mut self, v: u8) {
        self.p = (self.p & !(Z | N)) | if v == 0 { Z } else { 0 } | (v & N);
    }

    fn flag(&mut self, f: u8, on: bool) {
        if on {
            self.p |= f;
        } else {
            self.p &= !f;
        }
    }

    fn nmi(&mut self) {
        self.push((self.pc >> 8) as u8);
        self.push(self.pc as u8);
        self.push((self.p | U) & !B);
        self.p |= I;
        self.pc = self.read16(0xFFFA);
        self.cycles += 7;
    }

    /// Effective address (and whether a page was crossed).
    fn addr(&mut self, m: Mode) -> (u16, bool) {
        match m {
            Mode::Imm => {
                let a = self.pc;
                self.pc = self.pc.wrapping_add(1);
                (a, false)
            }
            Mode::Zp => {
                let a = self.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                (a, false)
            }
            Mode::Zpx => {
                let a = self.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                (a, false)
            }
            Mode::Zpy => {
                let a = self.read(self.pc).wrapping_add(self.y) as u16;
                self.pc = self.pc.wrapping_add(1);
                (a, false)
            }
            Mode::Abs => {
                let a = self.read16(self.pc);
                self.pc = self.pc.wrapping_add(2);
                (a, false)
            }
            Mode::Abx => {
                let b = self.read16(self.pc);
                self.pc = self.pc.wrapping_add(2);
                let a = b.wrapping_add(self.x as u16);
                (a, a & 0xFF00 != b & 0xFF00)
            }
            Mode::Aby => {
                let b = self.read16(self.pc);
                self.pc = self.pc.wrapping_add(2);
                let a = b.wrapping_add(self.y as u16);
                (a, a & 0xFF00 != b & 0xFF00)
            }
            Mode::Ind => {
                let p = self.read16(self.pc);
                self.pc = self.pc.wrapping_add(2);
                // the 6502 bug: the high byte wraps within the page
                let lo = self.read(p) as u16;
                let hi = self.read((p & 0xFF00) | (p.wrapping_add(1) & 0x00FF)) as u16;
                (lo | hi << 8, false)
            }
            Mode::Izx => {
                let z = self.read(self.pc).wrapping_add(self.x);
                self.pc = self.pc.wrapping_add(1);
                let lo = self.read(z as u16) as u16;
                let hi = self.read(z.wrapping_add(1) as u16) as u16;
                (lo | hi << 8, false)
            }
            Mode::Izy => {
                let z = self.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                let lo = self.read(z as u16) as u16;
                let hi = self.read(z.wrapping_add(1) as u16) as u16;
                let b = lo | hi << 8;
                let a = b.wrapping_add(self.y as u16);
                (a, a & 0xFF00 != b & 0xFF00)
            }
            Mode::Rel => {
                let off = self.read(self.pc) as i8;
                self.pc = self.pc.wrapping_add(1);
                (self.pc.wrapping_add(off as u16), false)
            }
            Mode::Imp | Mode::Acc => (0, false),
        }
    }

    fn adc(&mut self, v: u8) {
        let sum = self.a as u16 + v as u16 + (self.p & C) as u16;
        let r = sum as u8;
        self.flag(C, sum > 0xFF);
        self.flag(V, (!(self.a ^ v) & (self.a ^ r) & 0x80) != 0);
        self.a = r;
        self.zn(r);
    }

    fn cmp(&mut self, reg: u8, v: u8) {
        self.flag(C, reg >= v);
        self.zn(reg.wrapping_sub(v));
    }

    fn branch(&mut self, cond: bool) -> u32 {
        let (t, _) = self.addr(Mode::Rel);
        if !cond {
            return 0;
        }
        let extra = if t & 0xFF00 != self.pc & 0xFF00 { 2 } else { 1 };
        self.pc = t;
        extra
    }

    /// Read-modify-write helper: the value at `m`, or A.
    fn rmw(&mut self, m: Mode, f: fn(&mut Nes, u8) -> u8) {
        if let Mode::Acc = m {
            let a = self.a;
            self.a = f(self, a);
            return;
        }
        let (a, _) = self.addr(m);
        let v = self.read(a);
        let r = f(self, v);
        self.write(a, r);
    }

    fn asl(&mut self, v: u8) -> u8 {
        self.flag(C, v & 0x80 != 0);
        let r = v << 1;
        self.zn(r);
        r
    }
    fn lsr(&mut self, v: u8) -> u8 {
        self.flag(C, v & 1 != 0);
        let r = v >> 1;
        self.zn(r);
        r
    }
    fn rol(&mut self, v: u8) -> u8 {
        let c = self.p & C;
        self.flag(C, v & 0x80 != 0);
        let r = v << 1 | c;
        self.zn(r);
        r
    }
    fn ror(&mut self, v: u8) -> u8 {
        let c = self.p & C;
        self.flag(C, v & 1 != 0);
        let r = v >> 1 | c << 7;
        self.zn(r);
        r
    }
    fn inc(&mut self, v: u8) -> u8 {
        let r = v.wrapping_add(1);
        self.zn(r);
        r
    }
    fn dec(&mut self, v: u8) -> u8 {
        let r = v.wrapping_sub(1);
        self.zn(r);
        r
    }

    /// Execute one instruction; returns its cycles.
    pub fn step(&mut self) -> u32 {
        if self.stall > 0 {
            let s = self.stall;
            self.stall = 0;
            self.cycles += s as u64;
            return s;
        }
        if self.ppu.nmi {
            self.ppu.nmi = false;
            self.nmi();
            return 7;
        }
        self.instructions += 1;
        let op = self.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        use Mode::*;
        // (mode, base cycles, page-cross penalty) from the opcode
        let (mode, cyc, pen) = OPS[op as usize];
        let mode = match mode {
            0 => Imp,
            1 => Acc,
            2 => Imm,
            3 => Zp,
            4 => Zpx,
            5 => Zpy,
            6 => Abs,
            7 => Abx,
            8 => Aby,
            9 => Ind,
            10 => Izx,
            11 => Izy,
            _ => Rel,
        };
        let mut extra = 0u32;
        macro_rules! load {
            () => {{
                let (a, crossed) = self.addr(mode);
                if crossed && pen {
                    extra += 1;
                }
                self.read(a)
            }};
        }
        match op {
            // loads / stores
            0xA9 | 0xA5 | 0xB5 | 0xAD | 0xBD | 0xB9 | 0xA1 | 0xB1 => {
                let v = load!();
                self.a = v;
                self.zn(v);
            }
            0xA2 | 0xA6 | 0xB6 | 0xAE | 0xBE => {
                let v = load!();
                self.x = v;
                self.zn(v);
            }
            0xA0 | 0xA4 | 0xB4 | 0xAC | 0xBC => {
                let v = load!();
                self.y = v;
                self.zn(v);
            }
            0x85 | 0x95 | 0x8D | 0x9D | 0x99 | 0x81 | 0x91 => {
                let (a, _) = self.addr(mode);
                self.write(a, self.a);
            }
            0x86 | 0x96 | 0x8E => {
                let (a, _) = self.addr(mode);
                self.write(a, self.x);
            }
            0x84 | 0x94 | 0x8C => {
                let (a, _) = self.addr(mode);
                self.write(a, self.y);
            }
            // transfers
            0xAA => {
                self.x = self.a;
                self.zn(self.x);
            }
            0xA8 => {
                self.y = self.a;
                self.zn(self.y);
            }
            0x8A => {
                self.a = self.x;
                self.zn(self.a);
            }
            0x98 => {
                self.a = self.y;
                self.zn(self.a);
            }
            0xBA => {
                self.x = self.s;
                self.zn(self.x);
            }
            0x9A => self.s = self.x,
            // stack
            0x48 => self.push(self.a),
            0x08 => self.push(self.p | B | U),
            0x68 => {
                self.a = self.pop();
                self.zn(self.a);
            }
            0x28 => self.p = (self.pop() & !B) | U,
            // logic / arithmetic
            0x29 | 0x25 | 0x35 | 0x2D | 0x3D | 0x39 | 0x21 | 0x31 => {
                let v = load!();
                self.a &= v;
                self.zn(self.a);
            }
            0x09 | 0x05 | 0x15 | 0x0D | 0x1D | 0x19 | 0x01 | 0x11 => {
                let v = load!();
                self.a |= v;
                self.zn(self.a);
            }
            0x49 | 0x45 | 0x55 | 0x4D | 0x5D | 0x59 | 0x41 | 0x51 => {
                let v = load!();
                self.a ^= v;
                self.zn(self.a);
            }
            0x69 | 0x65 | 0x75 | 0x6D | 0x7D | 0x79 | 0x61 | 0x71 => {
                let v = load!();
                self.adc(v);
            }
            0xE9 | 0xEB | 0xE5 | 0xF5 | 0xED | 0xFD | 0xF9 | 0xE1 | 0xF1 => {
                let v = load!();
                self.adc(!v);
            }
            0xC9 | 0xC5 | 0xD5 | 0xCD | 0xDD | 0xD9 | 0xC1 | 0xD1 => {
                let v = load!();
                self.cmp(self.a, v);
            }
            0xE0 | 0xE4 | 0xEC => {
                let v = load!();
                self.cmp(self.x, v);
            }
            0xC0 | 0xC4 | 0xCC => {
                let v = load!();
                self.cmp(self.y, v);
            }
            0x24 | 0x2C => {
                let v = load!();
                self.flag(Z, self.a & v == 0);
                self.p = (self.p & !(N | V)) | (v & (N | V));
            }
            // shifts / increments
            0x0A | 0x06 | 0x16 | 0x0E | 0x1E => self.rmw(mode, Nes::asl),
            0x4A | 0x46 | 0x56 | 0x4E | 0x5E => self.rmw(mode, Nes::lsr),
            0x2A | 0x26 | 0x36 | 0x2E | 0x3E => self.rmw(mode, Nes::rol),
            0x6A | 0x66 | 0x76 | 0x6E | 0x7E => self.rmw(mode, Nes::ror),
            0xE6 | 0xF6 | 0xEE | 0xFE => self.rmw(mode, Nes::inc),
            0xC6 | 0xD6 | 0xCE | 0xDE => self.rmw(mode, Nes::dec),
            0xE8 => {
                self.x = self.x.wrapping_add(1);
                self.zn(self.x);
            }
            0xC8 => {
                self.y = self.y.wrapping_add(1);
                self.zn(self.y);
            }
            0xCA => {
                self.x = self.x.wrapping_sub(1);
                self.zn(self.x);
            }
            0x88 => {
                self.y = self.y.wrapping_sub(1);
                self.zn(self.y);
            }
            // jumps
            0x4C | 0x6C => self.pc = self.addr(mode).0,
            0x20 => {
                let t = self.read16(self.pc);
                let ret = self.pc.wrapping_add(1);
                self.push((ret >> 8) as u8);
                self.push(ret as u8);
                self.pc = t;
            }
            0x60 => {
                let lo = self.pop() as u16;
                let hi = self.pop() as u16;
                self.pc = (lo | hi << 8).wrapping_add(1);
            }
            0x40 => {
                self.p = (self.pop() & !B) | U;
                let lo = self.pop() as u16;
                let hi = self.pop() as u16;
                self.pc = lo | hi << 8;
            }
            0x00 => {
                let ret = self.pc.wrapping_add(1);
                self.push((ret >> 8) as u8);
                self.push(ret as u8);
                self.push(self.p | B | U);
                self.p |= I;
                self.pc = self.read16(0xFFFE);
            }
            // branches
            0x10 => extra += self.branch(self.p & N == 0),
            0x30 => extra += self.branch(self.p & N != 0),
            0x50 => extra += self.branch(self.p & V == 0),
            0x70 => extra += self.branch(self.p & V != 0),
            0x90 => extra += self.branch(self.p & C == 0),
            0xB0 => extra += self.branch(self.p & C != 0),
            0xD0 => extra += self.branch(self.p & Z == 0),
            0xF0 => extra += self.branch(self.p & Z != 0),
            // flags
            0x18 => self.p &= !C,
            0x38 => self.p |= C,
            0x58 => self.p &= !I,
            0x78 => self.p |= I,
            0xB8 => self.p &= !V,
            0xD8 => self.p &= !D,
            0xF8 => self.p |= D,
            0xEA => {}
            // unofficial NOPs (with operands)
            0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => {}
            0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 | 0x04 | 0x44 | 0x64 | 0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 | 0x0C | 0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => {
                let _ = load!();
            }
            // unofficial combined operations
            0xA7 | 0xB7 | 0xAF | 0xBF | 0xA3 | 0xB3 => {
                let v = load!();
                self.a = v;
                self.x = v;
                self.zn(v);
            }
            0x87 | 0x97 | 0x8F | 0x83 => {
                let (a, _) = self.addr(mode);
                self.write(a, self.a & self.x);
            }
            0xC7 | 0xD7 | 0xCF | 0xDF | 0xDB | 0xC3 | 0xD3 => {
                let (a, _) = self.addr(mode);
                let v = self.read(a).wrapping_sub(1);
                self.write(a, v);
                self.cmp(self.a, v);
            }
            0xE7 | 0xF7 | 0xEF | 0xFF | 0xFB | 0xE3 | 0xF3 => {
                let (a, _) = self.addr(mode);
                let v = self.read(a).wrapping_add(1);
                self.write(a, v);
                self.adc(!v);
            }
            0x07 | 0x17 | 0x0F | 0x1F | 0x1B | 0x03 | 0x13 => {
                let (a, _) = self.addr(mode);
                let v = self.read(a);
                let r = self.asl(v);
                self.write(a, r);
                self.a |= r;
                self.zn(self.a);
            }
            0x27 | 0x37 | 0x2F | 0x3F | 0x3B | 0x23 | 0x33 => {
                let (a, _) = self.addr(mode);
                let v = self.read(a);
                let r = self.rol(v);
                self.write(a, r);
                self.a &= r;
                self.zn(self.a);
            }
            0x47 | 0x57 | 0x4F | 0x5F | 0x5B | 0x43 | 0x53 => {
                let (a, _) = self.addr(mode);
                let v = self.read(a);
                let r = self.lsr(v);
                self.write(a, r);
                self.a ^= r;
                self.zn(self.a);
            }
            0x67 | 0x77 | 0x6F | 0x7F | 0x7B | 0x63 | 0x73 => {
                let (a, _) = self.addr(mode);
                let v = self.read(a);
                let r = self.ror(v);
                self.write(a, r);
                self.adc(r);
            }
            _ => {
                // KIL / JAM and the unstable ones: stop the CPU
                self.jammed = true;
                self.pc = self.pc.wrapping_sub(1);
            }
        }
        let c = cyc as u32 + extra;
        self.cycles += c as u64;
        c
    }

    /// Run until `budget` PPU dots (3 per CPU cycle) are used up.
    fn run_cycles(&mut self, budget: &mut i32) {
        while *budget > 0 && !self.jammed {
            let c = self.step();
            *budget -= 3 * c as i32;
            self.apu.run(c);
        }
    }

    /// Run one frame (262 scanlines). The pixels are in `frame_rgb`, the
    /// audio of the frame in `take_audio`.
    pub fn run_frame(&mut self) {
        let mut budget = 0i32;
        let mut quarter = 0;
        for line in 0..262usize {
            if line < 240 {
                if self.ppu.rendering() {
                    self.ppu.render_line(&self.cart, line);
                    self.ppu.incr_y();
                    // dot 257: horizontal bits of t -> v
                    self.ppu.v = (self.ppu.v & !0x041F) | (self.ppu.t & 0x041F);
                } else {
                    let bd = self.ppu.palette[0];
                    self.ppu.frame[line * WIDTH..(line + 1) * WIDTH].fill(bd);
                }
            } else if line == 241 {
                self.ppu.status |= 0x80;
                if self.ppu.ctrl & 0x80 != 0 {
                    self.ppu.nmi = true;
                }
            } else if line == 261 {
                self.ppu.status &= !(0x80 | 0x40 | 0x20);
                if self.ppu.rendering() {
                    // dots 280-304: vertical bits of t -> v
                    self.ppu.v = (self.ppu.v & 0x041F) | (self.ppu.t & !0x041F);
                }
            }
            budget += 341;
            self.run_cycles(&mut budget);
            // the frame counter's quarter frames, ~4 per frame
            if line % 66 == 65 && quarter < 4 {
                quarter += 1;
                self.apu.quarter();
                if quarter % 2 == 0 {
                    self.apu.half();
                }
            }
        }
        self.frames += 1;
    }

    /// The last frame as 0xRRGGBB.
    pub fn frame_rgb(&self, out: &mut [u32]) {
        for (o, &c) in out.iter_mut().zip(self.ppu.frame.iter()) {
            *o = PALETTE[c as usize & 63];
        }
    }

    /// Palette index at (x, y) of the last frame (tests).
    pub fn pixel(&self, x: usize, y: usize) -> u8 {
        self.ppu.frame[y * WIDTH + x]
    }

    /// Samples produced since the last call (48 kHz mono).
    pub fn take_audio(&mut self) -> Vec<i16> {
        core::mem::take(&mut self.apu.out)
    }

    pub fn oam(&self, i: usize) -> u8 {
        self.ppu.oam[i & 255]
    }
}

/// (addressing mode, base cycles, +1 on a page cross) per opcode. Modes:
/// 0 imp 1 acc 2 imm 3 zp 4 zpx 5 zpy 6 abs 7 abx 8 aby 9 ind 10 izx 11 izy 12 rel.
const OPS: [(u8, u8, bool); 256] = {
    let mut t = [(0u8, 2u8, false); 256];
    macro_rules! set {
        ($($op:expr => $m:expr, $c:expr, $p:expr);* $(;)?) => { $( t[$op] = ($m, $c, $p); )* };
    }
    set! {
        0x00 => 0, 7, false; 0x01 => 10, 6, false; 0x03 => 10, 8, false; 0x04 => 3, 3, false; 0x05 => 3, 3, false; 0x06 => 3, 5, false; 0x07 => 3, 5, false;
        0x08 => 0, 3, false; 0x09 => 2, 2, false; 0x0A => 1, 2, false; 0x0C => 6, 4, false; 0x0D => 6, 4, false; 0x0E => 6, 6, false; 0x0F => 6, 6, false;
        0x10 => 12, 2, false; 0x11 => 11, 5, true; 0x13 => 11, 8, false; 0x14 => 4, 4, false; 0x15 => 4, 4, false; 0x16 => 4, 6, false; 0x17 => 4, 6, false;
        0x18 => 0, 2, false; 0x19 => 8, 4, true; 0x1A => 0, 2, false; 0x1B => 8, 7, false; 0x1C => 7, 4, true; 0x1D => 7, 4, true; 0x1E => 7, 7, false; 0x1F => 7, 7, false;
        0x20 => 6, 6, false; 0x21 => 10, 6, false; 0x23 => 10, 8, false; 0x24 => 3, 3, false; 0x25 => 3, 3, false; 0x26 => 3, 5, false; 0x27 => 3, 5, false;
        0x28 => 0, 4, false; 0x29 => 2, 2, false; 0x2A => 1, 2, false; 0x2C => 6, 4, false; 0x2D => 6, 4, false; 0x2E => 6, 6, false; 0x2F => 6, 6, false;
        0x30 => 12, 2, false; 0x31 => 11, 5, true; 0x33 => 11, 8, false; 0x34 => 4, 4, false; 0x35 => 4, 4, false; 0x36 => 4, 6, false; 0x37 => 4, 6, false;
        0x38 => 0, 2, false; 0x39 => 8, 4, true; 0x3A => 0, 2, false; 0x3B => 8, 7, false; 0x3C => 7, 4, true; 0x3D => 7, 4, true; 0x3E => 7, 7, false; 0x3F => 7, 7, false;
        0x40 => 0, 6, false; 0x41 => 10, 6, false; 0x43 => 10, 8, false; 0x44 => 3, 3, false; 0x45 => 3, 3, false; 0x46 => 3, 5, false; 0x47 => 3, 5, false;
        0x48 => 0, 3, false; 0x49 => 2, 2, false; 0x4A => 1, 2, false; 0x4C => 6, 3, false; 0x4D => 6, 4, false; 0x4E => 6, 6, false; 0x4F => 6, 6, false;
        0x50 => 12, 2, false; 0x51 => 11, 5, true; 0x53 => 11, 8, false; 0x54 => 4, 4, false; 0x55 => 4, 4, false; 0x56 => 4, 6, false; 0x57 => 4, 6, false;
        0x58 => 0, 2, false; 0x59 => 8, 4, true; 0x5A => 0, 2, false; 0x5B => 8, 7, false; 0x5C => 7, 4, true; 0x5D => 7, 4, true; 0x5E => 7, 7, false; 0x5F => 7, 7, false;
        0x60 => 0, 6, false; 0x61 => 10, 6, false; 0x63 => 10, 8, false; 0x64 => 3, 3, false; 0x65 => 3, 3, false; 0x66 => 3, 5, false; 0x67 => 3, 5, false;
        0x68 => 0, 4, false; 0x69 => 2, 2, false; 0x6A => 1, 2, false; 0x6C => 9, 5, false; 0x6D => 6, 4, false; 0x6E => 6, 6, false; 0x6F => 6, 6, false;
        0x70 => 12, 2, false; 0x71 => 11, 5, true; 0x73 => 11, 8, false; 0x74 => 4, 4, false; 0x75 => 4, 4, false; 0x76 => 4, 6, false; 0x77 => 4, 6, false;
        0x78 => 0, 2, false; 0x79 => 8, 4, true; 0x7A => 0, 2, false; 0x7B => 8, 7, false; 0x7C => 7, 4, true; 0x7D => 7, 4, true; 0x7E => 7, 7, false; 0x7F => 7, 7, false;
        0x80 => 2, 2, false; 0x81 => 10, 6, false; 0x82 => 2, 2, false; 0x83 => 10, 6, false; 0x84 => 3, 3, false; 0x85 => 3, 3, false; 0x86 => 3, 3, false; 0x87 => 3, 3, false;
        0x88 => 0, 2, false; 0x89 => 2, 2, false; 0x8A => 0, 2, false; 0x8C => 6, 4, false; 0x8D => 6, 4, false; 0x8E => 6, 4, false; 0x8F => 6, 4, false;
        0x90 => 12, 2, false; 0x91 => 11, 6, false; 0x94 => 4, 4, false; 0x95 => 4, 4, false; 0x96 => 5, 4, false; 0x97 => 5, 4, false;
        0x98 => 0, 2, false; 0x99 => 8, 5, false; 0x9A => 0, 2, false; 0x9D => 7, 5, false;
        0xA0 => 2, 2, false; 0xA1 => 10, 6, false; 0xA2 => 2, 2, false; 0xA3 => 10, 6, false; 0xA4 => 3, 3, false; 0xA5 => 3, 3, false; 0xA6 => 3, 3, false; 0xA7 => 3, 3, false;
        0xA8 => 0, 2, false; 0xA9 => 2, 2, false; 0xAA => 0, 2, false; 0xAC => 6, 4, false; 0xAD => 6, 4, false; 0xAE => 6, 4, false; 0xAF => 6, 4, false;
        0xB0 => 12, 2, false; 0xB1 => 11, 5, true; 0xB3 => 11, 5, true; 0xB4 => 4, 4, false; 0xB5 => 4, 4, false; 0xB6 => 5, 4, false; 0xB7 => 5, 4, false;
        0xB8 => 0, 2, false; 0xB9 => 8, 4, true; 0xBA => 0, 2, false; 0xBC => 7, 4, true; 0xBD => 7, 4, true; 0xBE => 8, 4, true; 0xBF => 8, 4, true;
        0xC0 => 2, 2, false; 0xC1 => 10, 6, false; 0xC2 => 2, 2, false; 0xC3 => 10, 8, false; 0xC4 => 3, 3, false; 0xC5 => 3, 3, false; 0xC6 => 3, 5, false; 0xC7 => 3, 5, false;
        0xC8 => 0, 2, false; 0xC9 => 2, 2, false; 0xCA => 0, 2, false; 0xCC => 6, 4, false; 0xCD => 6, 4, false; 0xCE => 6, 6, false; 0xCF => 6, 6, false;
        0xD0 => 12, 2, false; 0xD1 => 11, 5, true; 0xD3 => 11, 8, false; 0xD4 => 4, 4, false; 0xD5 => 4, 4, false; 0xD6 => 4, 6, false; 0xD7 => 4, 6, false;
        0xD8 => 0, 2, false; 0xD9 => 8, 4, true; 0xDA => 0, 2, false; 0xDB => 8, 7, false; 0xDC => 7, 4, true; 0xDD => 7, 4, true; 0xDE => 7, 7, false; 0xDF => 7, 7, false;
        0xE0 => 2, 2, false; 0xE1 => 10, 6, false; 0xE2 => 2, 2, false; 0xE3 => 10, 8, false; 0xE4 => 3, 3, false; 0xE5 => 3, 3, false; 0xE6 => 3, 5, false; 0xE7 => 3, 5, false;
        0xE8 => 0, 2, false; 0xE9 => 2, 2, false; 0xEA => 0, 2, false; 0xEB => 2, 2, false; 0xEC => 6, 4, false; 0xED => 6, 4, false; 0xEE => 6, 6, false; 0xEF => 6, 6, false;
        0xF0 => 12, 2, false; 0xF1 => 11, 5, true; 0xF3 => 11, 8, false; 0xF4 => 4, 4, false; 0xF5 => 4, 4, false; 0xF6 => 4, 6, false; 0xF7 => 4, 6, false;
        0xF8 => 0, 2, false; 0xF9 => 8, 4, true; 0xFA => 0, 2, false; 0xFB => 8, 7, false; 0xFC => 7, 4, true; 0xFD => 7, 4, true; 0xFE => 7, 7, false; 0xFF => 7, 7, false;
    }
    t
};
