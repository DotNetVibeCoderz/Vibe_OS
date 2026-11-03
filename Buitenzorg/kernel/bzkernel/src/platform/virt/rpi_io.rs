//! Raspberry Pi 4 board I/O for applications (the HW_* syscalls): GPIO, I2C
//! (BSC), SPI0, the extra PL011 UARTs 2-5, the two built-in LEDs, and the
//! board sensors the VideoCore firmware reports (temperature, clocks,
//! voltages, throttling). Every bus is polled -- no interrupts, no DMA.
//!
//! Peripheral addresses come from the GPIO node of the Device Tree: on the
//! BCM2711 every block sits at a fixed offset from it (GPIO at base + 0x200000).
//! Pins are switched to their bus function on first use. Applications may use
//! the header pins GPIO 0-27 except 14/15 (the serial console) and GPIO 42 (the
//! activity LED); anything else is refused.

use bz_abi::{hw_gpio, hw_query, hw_status};
use spin::Mutex;

use crate::arch::aarch64::rpi::firmware_property;
use crate::arch::cpu;

const GPFSEL0: u64 = 0x00;
const GPSET0: u64 = 0x1c;
const GPCLR0: u64 = 0x28;
const GPLEV0: u64 = 0x34;
const GPIO_PUP_PDN0: u64 = 0xe4; // BCM2711 pull control, 2 bits per pin

const ACT_LED: u32 = 42;
const PWR_LED_FW_GPIO: u32 = 128 + 2; // firmware expander GPIO 2, active low

const UART_CLOCK: u32 = 48_000_000; // the Pi 4 firmware's PL011 clock

struct Io {
    gpio: u64,
    i2c: [u64; 2],      // BSC0, BSC1
    spi0: u64,
    uart: [u64; 4],     // UART2..UART5
    core_hz: u32,
    i2c_ready: [bool; 2],
    spi_ready: bool,
    uart_open: [bool; 4],
    cm: u64,      // clock manager
    pwm1: u64,    // PWM1: the headphone jack (GPIO 40 / 41); 0 = absent
    audio_ready: bool,
}

/// Headphone-jack state (outside `IO` so AUDIO_STAT never waits on a tone).
static VOLUME: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(70);

static IO: Mutex<Option<Io>> = Mutex::new(None);

fn r(a: u64) -> u32 {
    unsafe { (a as *const u32).read_volatile() }
}
fn w(a: u64, v: u32) {
    unsafe { (a as *mut u32).write_volatile(v) }
}

fn delay_us(us: u64) {
    let hz = cpu::counter_hz().max(1);
    let t0 = cpu::timestamp();
    while cpu::timestamp() - t0 < us * hz / 1_000_000 {
        core::hint::spin_loop();
    }
}

/// Elapsed microseconds since `t0` (a `cpu::timestamp`).
fn since_us(t0: u64) -> u64 {
    (cpu::timestamp() - t0) * 1_000_000 / cpu::counter_hz().max(1)
}

/// One firmware property with a single 32-bit argument; the first `n` words
/// of the answer.
fn fw(tag: u32, arg: u32, n: usize) -> Option<[u32; 2]> {
    let mut v = [arg, 0];
    if !firmware_property(tag, &mut v) {
        return None;
    }
    let _ = n;
    Some(v)
}

/// Pins applications may touch.
fn allowed(pin: u32) -> bool {
    (pin <= 27 && pin != 14 && pin != 15) || pin == ACT_LED
}

impl Io {
    fn fsel(&self, pin: u32, code: u32) {
        let reg = self.gpio + GPFSEL0 + (pin / 10) as u64 * 4;
        let shift = (pin % 10) * 3;
        w(reg, (r(reg) & !(7 << shift)) | (code << shift));
    }
    fn fsel_get(&self, pin: u32) -> u32 {
        (r(self.gpio + GPFSEL0 + (pin / 10) as u64 * 4) >> ((pin % 10) * 3)) & 7
    }
    fn set(&self, pin: u32, high: bool) {
        let reg = if high { GPSET0 } else { GPCLR0 } + (pin / 32) as u64 * 4;
        w(self.gpio + reg, 1 << (pin % 32));
    }
    fn level(&self, pin: u32) -> bool {
        r(self.gpio + GPLEV0 + (pin / 32) as u64 * 4) & (1 << (pin % 32)) != 0
    }
    fn pull(&self, pin: u32, mode: u32) {
        let reg = self.gpio + GPIO_PUP_PDN0 + (pin / 16) as u64 * 4;
        let shift = (pin % 16) * 2;
        w(reg, (r(reg) & !(3 << shift)) | ((mode & 3) << shift));
    }
}

/// The FSEL code of "alternate function n".
fn alt(n: u32) -> u32 {
    [4, 5, 6, 7, 3, 2][n as usize]
}

/// API mode (0 input, 1 output, 2+n ALTn) <-> FSEL code.
fn mode_to_fsel(mode: u64) -> Option<u32> {
    match mode {
        0 => Some(0),
        1 => Some(1),
        2..=7 => Some(alt(mode as u32 - 2)),
        _ => None,
    }
}
fn fsel_to_mode(code: u32) -> u64 {
    match code {
        0 => 0,
        1 => 1,
        c => (0..6).find(|&n| alt(n) == c).map_or(0, |n| 2 + n as u64),
    }
}

/// Find the peripherals (the GPIO node of the DTB) and map them.
pub fn init(gpio_phys: u64) -> bool {
    let periph = gpio_phys - 0x20_0000;
    let map = |off: u64| crate::memory::map_mmio(periph + off, 0x1000);
    let (Some(gpio), Some(i0), Some(i1), Some(spi0)) = (map(0x20_0000), map(0x20_5000), map(0x80_4000), map(0x20_4000)) else {
        return false;
    };
    let mut uart = [0u64; 4];
    for (n, u) in uart.iter_mut().enumerate() {
        // UART2..5 share one page: 0x201400, 0x201600, 0x201800, 0x201a00
        let pa = periph + 0x20_1400 + n as u64 * 0x200;
        match crate::memory::map_mmio(pa & !0xFFF, 0x1000) {
            Some(va) => *u = va + (pa & 0xFFF),
            None => return false,
        }
    }
    // The BSC / SPI dividers run off the VPU ("core") clock.
    let core_hz = fw(0x0003_0047, 4, 2).map(|v| v[1]).filter(|&h| h > 1_000_000).unwrap_or(500_000_000);
    let (Some(cm), Some(pwm)) = (map(0x10_1000), map(0x20_c000)) else { return false };
    // The PWM block may be missing (QEMU raspi4b does not model it: any access
    // is an external abort), so probe it once before using the jack.
    let pwm1 = match crate::arch::aarch64::exceptions::probe_read32(pwm + 0x800 + PWM_STA) {
        Some(_) => pwm + 0x800,
        None => {
            crate::println!("[rpi-io] no PWM block answers: headphone jack unavailable");
            0
        }
    };
    *IO.lock() = Some(Io {
        gpio,
        i2c: [i0, i1],
        spi0,
        uart,
        core_hz,
        i2c_ready: [false; 2],
        spi_ready: false,
        uart_open: [false; 4],
        cm,
        pwm1,
        audio_ready: false,
    });
    // HDMI audio on HDMI0 (CTS from the pixel clock the firmware set up)
    let pixel = fw(0x0003_0047, 9, 2).map(|v| v[1]).filter(|&h| h > 1_000_000)
        .or_else(|| fw(0x0003_0002, 9, 2).map(|v| v[1]).filter(|&h| h > 1_000_000))
        .unwrap_or(148_500_000) as u64;
    super::hdmi_audio::init(pixel);
    true
}

pub fn present() -> bool {
    IO.lock().is_some()
}

// ------------------------------------------------------------------ board

pub fn board(query: u64, arg: u64) -> u64 {
    use bz_abi::syserr::{INVAL, NOSYS};
    if !present() {
        return NOSYS;
    }
    let a = arg as u32;
    let v = match query {
        hw_query::BOARD => Some(1),
        hw_query::TEMPERATURE => fw(0x0003_0006, 0, 2).map(|v| v[1] as u64),
        hw_query::TEMPERATURE_MAX => fw(0x0003_000a, 0, 2).map(|v| v[1] as u64),
        // the measured rate; the configured one if the firmware cannot measure it
        hw_query::CLOCK => fw(0x0003_0047, a, 2).map(|v| v[1]).filter(|&h| h != 0).or_else(|| fw(0x0003_0002, a, 2).map(|v| v[1])).map(|h| h as u64),
        hw_query::VOLTAGE => fw(0x0003_0003, a, 2).map(|v| v[1] as u64),
        hw_query::THROTTLED => fw(0x0003_0046, 0, 1).map(|v| v[0] as u64),
        hw_query::REVISION => fw(0x0001_0002, 0, 1).map(|v| v[0] as u64),
        hw_query::SERIAL => fw(0x0001_0004, 0, 2).map(|v| v[0] as u64 | (v[1] as u64) << 32),
        hw_query::MEMORY => fw(0x0001_0005, 0, 2).map(|v| v[1] as u64),
        hw_query::GPIO_COUNT => Some(58),
        _ => None,
    };
    v.unwrap_or(INVAL)
}

// ------------------------------------------------------------------ GPIO

pub fn gpio(op: u64, pin: u64, value: u64) -> u64 {
    use bz_abi::syserr::{INVAL, NOSYS};
    let guard = IO.lock();
    let Some(io) = guard.as_ref() else { return NOSYS };
    let pin = pin as u32;
    if !allowed(pin) {
        return INVAL;
    }
    match op {
        hw_gpio::MODE => match mode_to_fsel(value) {
            Some(code) => {
                io.fsel(pin, code);
                0
            }
            None => INVAL,
        },
        hw_gpio::WRITE => {
            io.set(pin, value != 0);
            0
        }
        hw_gpio::READ => io.level(pin) as u64,
        hw_gpio::PULL if value <= 2 => {
            io.pull(pin, value as u32);
            0
        }
        hw_gpio::GET_MODE => fsel_to_mode(io.fsel_get(pin)),
        _ => INVAL,
    }
}

// ------------------------------------------------------------------ LEDs

pub fn led(which: u64, on: bool) -> u64 {
    use bz_abi::syserr::{INVAL, NOSYS};
    let guard = IO.lock();
    let Some(io) = guard.as_ref() else { return NOSYS };
    match which {
        0 => {
            io.fsel(ACT_LED, 1);
            io.set(ACT_LED, on); // active high
            0
        }
        1 => {
            drop(guard);
            // firmware SET_GPIO_STATE on the expander; the LED is active low
            let mut v = [PWR_LED_FW_GPIO, if on { 0 } else { 1 }];
            if firmware_property(0x0003_8041, &mut v) { 0 } else { INVAL }
        }
        _ => INVAL,
    }
}

// ------------------------------------------------------------------ I2C (BSC)

const BSC_C: u64 = 0x00;
const BSC_S: u64 = 0x04;
const BSC_DLEN: u64 = 0x08;
const BSC_A: u64 = 0x0c;
const BSC_FIFO: u64 = 0x10;
const BSC_DIV: u64 = 0x14;
const C_I2CEN: u32 = 1 << 15;
const C_ST: u32 = 1 << 7;
const C_CLEAR: u32 = 3 << 4;
const C_READ: u32 = 1;
const S_DONE: u32 = 1 << 1;
const S_TXD: u32 = 1 << 4;
const S_RXD: u32 = 1 << 5;
const S_ERR: u32 = 1 << 8;
const S_CLKT: u32 = 1 << 9;

/// One I2C phase (write `tx` or read into `rx`); `Err(hw_status)`.
fn bsc_phase(b: u64, addr: u32, tx: &[u8], rx: &mut [u8]) -> Result<(), u64> {
    let reading = !rx.is_empty();
    let len = if reading { rx.len() } else { tx.len() };
    w(b + BSC_S, S_DONE | S_ERR | S_CLKT);
    w(b + BSC_A, addr & 0x7F);
    w(b + BSC_DLEN, len as u32);
    w(b + BSC_C, C_I2CEN | C_CLEAR);
    w(b + BSC_C, C_I2CEN | C_ST | if reading { C_READ } else { 0 });
    let t0 = cpu::timestamp();
    let mut i = 0;
    loop {
        let s = r(b + BSC_S);
        if reading {
            while i < len && r(b + BSC_S) & S_RXD != 0 {
                rx[i] = r(b + BSC_FIFO) as u8;
                i += 1;
            }
        } else {
            while i < len && r(b + BSC_S) & S_TXD != 0 {
                w(b + BSC_FIFO, tx[i] as u32);
                i += 1;
            }
        }
        if s & (S_ERR | S_CLKT) != 0 {
            // The failed transfer is not over yet: wait for the controller to
            // finish it (DONE, TA clear) before clearing, else the *next*
            // transfer sees this one's DONE and reports success (a scan on a
            // real board then "found" every second address).
            let t1 = cpu::timestamp();
            while r(b + BSC_S) & (S_DONE | 1) != S_DONE && since_us(t1) < 10_000 {}
            w(b + BSC_C, C_I2CEN | C_CLEAR);
            w(b + BSC_S, S_DONE | S_ERR | S_CLKT);
            return Err(if s & S_ERR != 0 { hw_status::NACK } else { hw_status::CLOCK_STRETCH });
        }
        if s & S_DONE != 0 {
            // a read may leave the last bytes in the FIFO
            while reading && i < len && r(b + BSC_S) & S_RXD != 0 {
                rx[i] = r(b + BSC_FIFO) as u8;
                i += 1;
            }
            w(b + BSC_S, S_DONE);
            return Ok(());
        }
        if since_us(t0) > 100_000 {
            w(b + BSC_C, C_I2CEN | C_CLEAR);
            return Err(hw_status::TIMEOUT);
        }
    }
}

pub fn i2c(bus: u32, addr: u32, speed_hz: u32, tx: &[u8], rx: &mut [u8]) -> u64 {
    use bz_abi::syserr::{INVAL, NOSYS};
    let mut guard = IO.lock();
    let Some(io) = guard.as_mut() else { return NOSYS };
    if bus > 1 || addr > 0x7F {
        return INVAL;
    }
    let b = io.i2c[bus as usize];
    if !io.i2c_ready[bus as usize] {
        // bus 0: GPIO 0/1, bus 1: GPIO 2/3 (SDA/SCL), ALT0
        let (sda, scl) = if bus == 0 { (0, 1) } else { (2, 3) };
        io.fsel(sda, alt(0));
        io.fsel(scl, alt(0));
        io.i2c_ready[bus as usize] = true;
    }
    let hz = if speed_hz == 0 { 100_000 } else { speed_hz.clamp(10_000, 1_000_000) };
    let mut div = io.core_hz.div_ceil(hz);
    if div & 1 != 0 {
        div += 1;
    }
    w(b + BSC_DIV, div.min(0xFFFE));
    if !tx.is_empty() {
        if let Err(e) = bsc_phase(b, addr, tx, &mut []) {
            return e;
        }
    }
    if !rx.is_empty() {
        if let Err(e) = bsc_phase(b, addr, &[], rx) {
            return e;
        }
    }
    if tx.is_empty() && rx.is_empty() {
        // a zero-length write: just the address, to see who answers
        if let Err(e) = bsc_phase(b, addr, &[], &mut []) {
            return e;
        }
    }
    hw_status::OK
}

// ------------------------------------------------------------------ SPI0

const SPI_CS: u64 = 0x00;
const SPI_FIFO: u64 = 0x04;
const SPI_CLK: u64 = 0x08;
const CS_TA: u32 = 1 << 7;
const CS_DONE: u32 = 1 << 16;
const CS_RXD: u32 = 1 << 17;
const CS_TXD: u32 = 1 << 18;

pub fn spi(bus: u32, cs: u32, speed_hz: u32, mode: u32, tx: &[u8], rx: &mut [u8]) -> u64 {
    use bz_abi::syserr::{INVAL, NOSYS};
    let mut guard = IO.lock();
    let Some(io) = guard.as_mut() else { return NOSYS };
    if bus != 0 || cs > 1 || mode > 3 {
        return INVAL;
    }
    let b = io.spi0;
    if !io.spi_ready {
        // CE1 7, CE0 8, MISO 9, MOSI 10, SCLK 11 -- ALT0
        for p in 7..=11 {
            io.fsel(p, alt(0));
        }
        io.spi_ready = true;
    }
    let hz = if speed_hz == 0 { 1_000_000 } else { speed_hz };
    let mut div = io.core_hz.div_ceil(hz.max(1));
    if div & 1 != 0 {
        div += 1;
    }
    w(b + SPI_CLK, div.clamp(2, 65534));
    let len = tx.len().max(rx.len());
    let base_cs = cs | (mode << 2);
    w(b + SPI_CS, base_cs | C_CLEAR);
    w(b + SPI_CS, base_cs | CS_TA);
    let (mut ti, mut ri) = (0usize, 0usize);
    let t0 = cpu::timestamp();
    while ri < len {
        if ti < len && r(b + SPI_CS) & CS_TXD != 0 {
            w(b + SPI_FIFO, *tx.get(ti).unwrap_or(&0) as u32);
            ti += 1;
        }
        while ri < len && r(b + SPI_CS) & CS_RXD != 0 {
            let v = r(b + SPI_FIFO) as u8;
            if ri < rx.len() {
                rx[ri] = v;
            }
            ri += 1;
        }
        if since_us(t0) > 200_000 {
            w(b + SPI_CS, base_cs | C_CLEAR);
            return hw_status::TIMEOUT;
        }
    }
    let t1 = cpu::timestamp();
    while r(b + SPI_CS) & CS_DONE == 0 && since_us(t1) < 10_000 {}
    w(b + SPI_CS, base_cs); // TA off: chip select released
    hw_status::OK
}

// ------------------------------------------------------------------ UART2-5

const DR: u64 = 0x00;
const FR: u64 = 0x18;
const IBRD: u64 = 0x24;
const FBRD: u64 = 0x28;
const LCRH: u64 = 0x2c;
const CR: u64 = 0x30;
const ICR: u64 = 0x44;
const FR_RXFE: u32 = 1 << 4;
const FR_TXFF: u32 = 1 << 5;

/// TX/RX pins of UART2..5 (ALT4).
const UART_PINS: [(u32, u32); 4] = [(0, 1), (4, 5), (8, 9), (12, 13)];

pub fn uart_open(port: u32, baud: u32) -> u64 {
    use bz_abi::syserr::{INVAL, NOSYS};
    let mut guard = IO.lock();
    let Some(io) = guard.as_mut() else { return NOSYS };
    if !(2..=5).contains(&port) || baud == 0 {
        return INVAL;
    }
    let n = (port - 2) as usize;
    let b = io.uart[n];
    let (txp, rxp) = UART_PINS[n];
    io.fsel(txp, alt(4));
    io.fsel(rxp, alt(4));
    io.pull(rxp, 1);
    w(b + CR, 0);
    w(b + ICR, 0x7FF);
    // divisor = clock / (16 * baud), 6 fractional bits
    let div64 = (UART_CLOCK as u64 * 4).div_ceil(baud as u64); // = 64 * clock / (16 * baud)
    w(b + IBRD, (div64 >> 6) as u32);
    w(b + FBRD, (div64 & 63) as u32);
    w(b + LCRH, (3 << 5) | (1 << 4)); // 8 bits, FIFOs on, no parity, 1 stop
    w(b + CR, (1 << 9) | (1 << 8) | 1); // RX, TX, enable
    // drain anything already received (bounded: an absent UART -- QEMU
    // models none of UART2-5 -- reads as "never empty")
    for _ in 0..64 {
        if r(b + FR) & FR_RXFE != 0 {
            break;
        }
        let _ = r(b + DR);
    }
    io.uart_open[n] = true;
    0
}

pub fn uart_close(port: u32) -> u64 {
    let mut guard = IO.lock();
    let Some(io) = guard.as_mut() else { return bz_abi::syserr::NOSYS };
    if !(2..=5).contains(&port) {
        return bz_abi::syserr::INVAL;
    }
    let n = (port - 2) as usize;
    w(io.uart[n] + CR, 0);
    io.uart_open[n] = false;
    0
}

pub fn uart_write(port: u32, data: &[u8]) -> u64 {
    let guard = IO.lock();
    let Some(io) = guard.as_ref() else { return bz_abi::syserr::NOSYS };
    if !(2..=5).contains(&port) || !io.uart_open[(port - 2) as usize] {
        return bz_abi::syserr::INVAL;
    }
    let b = io.uart[(port - 2) as usize];
    let mut n = 0;
    for &c in data {
        let t0 = cpu::timestamp();
        while r(b + FR) & FR_TXFF != 0 {
            if since_us(t0) > 50_000 {
                return n;
            }
        }
        w(b + DR, c as u32);
        n += 1;
    }
    n
}

pub fn uart_read(port: u32, out: &mut [u8]) -> u64 {
    let guard = IO.lock();
    let Some(io) = guard.as_ref() else { return bz_abi::syserr::NOSYS };
    if !(2..=5).contains(&port) || !io.uart_open[(port - 2) as usize] {
        return bz_abi::syserr::INVAL;
    }
    let b = io.uart[(port - 2) as usize];
    let mut n = 0;
    while n < out.len() && r(b + FR) & FR_RXFE == 0 {
        out[n] = r(b + DR) as u8;
        n += 1;
    }
    n as u64
}

// ------------------------------------------------------------------ headphone jack (PWM)
//
// The Pi 4's 3.5 mm jack is PWM1 (GPIO 40 = left, 41 = right, ALT0) behind an
// RC filter. Its clock comes from the clock manager: the 54 MHz oscillator
// divided by 2 = 27 MHz; a range of 562 gives 27 MHz / 562 = 48.04 kHz
// samples of ~9 bits. Both channels read one FIFO, alternately (left, right).

pub const SAMPLE_RATE: u32 = 48_000;
const CM_PASSWD: u32 = 0x5a00_0000;
const CM_PWMCTL: u64 = 0xa0;
const CM_PWMDIV: u64 = 0xa4;
const PWM_CTL: u64 = 0x00;
const PWM_STA: u64 = 0x04;
const PWM_RNG1: u64 = 0x10;
const PWM_FIF1: u64 = 0x18;
const PWM_RNG2: u64 = 0x20;
const PWM_RANGE: u32 = 562;

fn audio_setup(io: &mut Io) {
    if io.audio_ready {
        return;
    }
    io.fsel(40, alt(0));
    io.fsel(41, alt(0));
    let cm = io.cm;
    // stop the PWM clock, wait until it is idle, then oscillator / 2
    w(cm + CM_PWMCTL, CM_PASSWD | (r(cm + CM_PWMCTL) & !0x10));
    let t0 = cpu::timestamp();
    while r(cm + CM_PWMCTL) & 0x80 != 0 && since_us(t0) < 10_000 {}
    w(cm + CM_PWMDIV, CM_PASSWD | (2 << 12));
    w(cm + CM_PWMCTL, CM_PASSWD | 1);
    w(cm + CM_PWMCTL, CM_PASSWD | 0x10 | 1);
    let t1 = cpu::timestamp();
    while r(cm + CM_PWMCTL) & 0x80 == 0 && since_us(t1) < 10_000 {}
    let p = io.pwm1;
    w(p + PWM_CTL, 0);
    delay_us(10);
    w(p + PWM_RNG1, PWM_RANGE);
    w(p + PWM_RNG2, PWM_RANGE);
    w(p + PWM_CTL, 1 << 6); // clear the FIFO
    delay_us(10);
    w(p + PWM_STA, 0x1FE); // clear error flags
    // both channels enabled, fed from the FIFO, PWM (not mark/space) algorithm
    w(p + PWM_CTL, 1 | (1 << 5) | (1 << 8) | (1 << 13));
    io.audio_ready = true;
}

/// Queue one stereo frame (0..PWM_RANGE per channel); false if the FIFO stalled.
fn audio_frame(p: u64, l: u32, rr: u32) -> bool {
    for v in [l, rr] {
        let t0 = cpu::timestamp();
        while r(p + PWM_STA) & 1 != 0 {
            if since_us(t0) > 5_000 {
                return false; // FIFO stuck full: no PWM clock
            }
        }
        w(p + PWM_FIF1, v);
    }
    true
}

pub fn audio_present() -> bool {
    super::hdmi_audio::present() || IO.lock().as_ref().map_or(false, |io| io.pwm1 != 0)
}

/// The jack's PWM FIFO, set up on first use (0 = no jack).
fn jack() -> u64 {
    let mut guard = IO.lock();
    match guard.as_mut().filter(|io| io.pwm1 != 0) {
        Some(io) => {
            audio_setup(io);
            io.pwm1
        }
        None => 0,
    }
}

/// Play `frames` stereo samples from `next` on every output there is (the
/// headphone jack and HDMI); an output whose FIFO stalls is dropped for the
/// rest of this sound. False if nothing played.
fn play_frames(frames: usize, mut next: impl FnMut(usize) -> (i16, i16)) -> bool {
    let mut pwm = jack();
    let mut hdmi = super::hdmi_audio::present();
    if pwm == 0 && !hdmi {
        return false;
    }
    let conv = |s: i16| ((s as i32 + 32768) as u32 * PWM_RANGE / 65536).min(PWM_RANGE);
    let mut hdmi_sent = 0usize;
    let mut ok = true;
    for i in 0..frames {
        let (l, r) = next(i);
        if pwm != 0 && !audio_frame(pwm, conv(l), conv(r)) {
            pwm = 0;
        }
        if hdmi {
            if super::hdmi_audio::frame(l, r) {
                hdmi_sent += 1;
            } else {
                hdmi = false;
            }
        }
        if pwm == 0 && !hdmi {
            ok = false;
            break;
        }
    }
    super::hdmi_audio::report(frames, hdmi_sent);
    ok
}
pub fn audio_volume() -> u32 {
    VOLUME.load(core::sync::atomic::Ordering::Relaxed)
}
pub fn audio_set_volume(pct: u32) -> bool {
    VOLUME.store(pct.min(100), core::sync::atomic::Ordering::Relaxed);
    audio_present()
}

/// A sine-ish tone (a 32-step table) for `ms` milliseconds.
pub fn audio_tone(freq: u32, ms: u32) -> bool {
    const SINE: [i32; 32] = [0, 98, 195, 290, 383, 471, 556, 634, 707, 773, 831, 882, 924, 957, 981, 995, 1000, 995, 981, 957, 924, 882, 831, 773, 707, 634, 556, 471, 383, 290, 195, 98];
    let amp = 32_000 * audio_volume() as i32 / 100;
    let frames = SAMPLE_RATE as usize * ms.min(5000) as usize / 1000;
    let step = (freq.clamp(20, 20_000) as u64) * 64 * 65536 / SAMPLE_RATE as u64; // 64 half-steps per period, 16.16
    play_frames(frames, |n| {
        let i = (((n as u64 * step) >> 16) % 64) as usize;
        let s = (if i < 32 { SINE[i] } else { -SINE[i - 32] } * amp / 1000) as i16;
        (s, s)
    })
}

/// Interleaved 16-bit stereo PCM at 48 kHz (`len` bytes, already in kernel memory).
pub fn audio_play(pcm: &[u8]) -> bool {
    let vol = audio_volume() as i32;
    let s = |b: &[u8]| (i16::from_le_bytes([b[0], b[1]]) as i32 * vol / 100) as i16;
    play_frames(pcm.len() / 4, |n| (s(&pcm[n * 4..n * 4 + 2]), s(&pcm[n * 4 + 2..n * 4 + 4])))
}

// ------------------------------------------------------------------ self-test

/// Boot check: sensors answer, the activity LED pin toggles and reads back,
/// and both I2C buses can be scanned (devices found are listed).
pub fn self_test() -> bool {
    if !present() {
        return false;
    }
    let temp = board(hw_query::TEMPERATURE, 0);
    let arm = board(hw_query::CLOCK, 3);
    let volt = board(hw_query::VOLTAGE, 1);
    let thr = board(hw_query::THROTTLED, 0);
    let ok_sensor = temp != bz_abi::syserr::INVAL && temp > 0 && temp < 150_000;
    let mut blink = true;
    for i in 0..6 {
        let on = i % 2 == 0;
        led(0, on);
        delay_us(50_000);
        let lvl = gpio(hw_gpio::READ, ACT_LED as u64, 0) == 1;
        blink &= lvl == on;
    }
    led(0, false);
    let mut found = alloc::vec::Vec::new();
    let mut scanned = false;
    for addr in 0x08..0x78u32 {
        let mut b = [0u8; 1];
        match i2c(1, addr, 100_000, &[], &mut b) {
            hw_status::OK => found.push(addr),
            hw_status::TIMEOUT => break,
            _ => scanned = true,
        }
    }
    // serial port 3: open, write, read back what (if anything) arrived, close
    let t_uart = cpu::timestamp();
    let opened = uart_open(3, 115_200) == 0;
    let wrote = uart_write(3, b"bz");
    let mut back = [0u8; 8];
    let read = uart_read(3, &mut back);
    uart_close(3);
    crate::println!("[rpi-io] UART3: open {}, wrote {}, read {} in {} us", opened, wrote, read, since_us(t_uart));
    // headphone jack: 20 ms of silence must drain through the PWM FIFO
    let jack = {
        let mut g = IO.lock();
        match g.as_mut().filter(|io| io.pwm1 != 0) {
            Some(io) => {
                audio_setup(io);
                let p = io.pwm1;
                drop(g);
                let mid = PWM_RANGE / 2;
                (0..SAMPLE_RATE / 50).all(|_| audio_frame(p, mid, mid))
            }
            None => false,
        }
    };
    crate::println!("[rpi-io] headphone jack (PWM1, GPIO 40/41): {}", if jack { "PWM clock running, 48 kHz" } else { "not available" });
    let fmt = |v: u64| if v == bz_abi::syserr::INVAL { alloc::string::String::from("n/a") } else { alloc::format!("{}", v) };
    crate::println!(
        "[rpi-io] SoC {} mC, ARM {} Hz, core {} uV, throttled {:#x}; ACT LED toggles: {}; I2C1 {} {:?}",
        fmt(temp),
        fmt(arm),
        fmt(volt),
        thr,
        blink,
        if scanned { "scanned, devices at" } else { "not answering" },
        found
    );
    ok_sensor && blink
}

// --- Hardware cursor (G2) ---------------------------------------------------
//
// The VideoCore firmware can show a cursor image in its own HVS plane over
// the framebuffer: mailbox tags SET_CURSOR_INFO (0x00008010: width, height,
// unused, bus address of 32-bit ARGB pixels, hot spot x, y) and
// SET_CURSOR_STATE (0x00008011: enable, x, y, flags -- 1 = framebuffer
// coordinates). Each answers 0 for "valid". QEMU's `raspi4b` does not model
// them (it answers without touching the values), so there the desktop keeps
// its software cursor.

/// Edge of the firmware cursor image.
pub const FW_CURSOR: usize = 32;

/// (physical address, kernel address) of the cursor image frame.
static CURSOR_FRAME: Mutex<Option<(u64, u64)>> = Mutex::new(None);

/// Upload a `FW_CURSOR`^2 ARGB image as the firmware cursor.
pub fn cursor_image(px: &[u32]) -> bool {
    if px.len() < FW_CURSOR * FW_CURSOR {
        return false;
    }
    let mut frame = CURSOR_FRAME.lock();
    if frame.is_none() {
        // the VideoCore reaches the first GiB only
        let mut rejected = alloc::vec::Vec::new();
        for _ in 0..64 {
            let Some((phys, va)) = crate::memory::alloc_dma_frame() else { break };
            if phys + 4096 <= 0x4000_0000 {
                *frame = Some((phys, va as u64));
                break;
            }
            rejected.push(phys);
        }
        for phys in rejected {
            crate::memory::free_frame(phys);
        }
    }
    let Some((phys, va)) = *frame else { return false };
    unsafe { core::ptr::copy_nonoverlapping(px.as_ptr(), va as *mut u32, FW_CURSOR * FW_CURSOR) };
    cpu::dma_sync(va, 4096);
    let bus = (phys as u32) | 0xC000_0000; // the uncached alias
    let n = FW_CURSOR as u32;
    let mut v = [n, n, 0, bus, 0, 0];
    firmware_property(0x0000_8010, &mut v) && v[0] == 0
}

/// Show (or hide) the firmware cursor at (x, y) in framebuffer coordinates.
pub fn cursor_state(visible: bool, x: i32, y: i32) -> bool {
    let mut v = [visible as u32, x.max(0) as u32, y.max(0) as u32, 1];
    firmware_property(0x0000_8011, &mut v) && v[0] == 0
}
