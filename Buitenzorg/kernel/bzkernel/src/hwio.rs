//! Board I/O for applications (the HW_* syscalls): GPIO, I2C, SPI, serial
//! ports, LEDs and built-in sensors. Only the Raspberry Pi 4 has a driver
//! (`platform::virt::rpi_io`); everywhere else every call answers NOSYS, so
//! the same program runs on any machine and can tell whether the board is
//! there (`HW_BOARD` query `BOARD`).

#[cfg(target_arch = "aarch64")]
use crate::platform::virt::rpi_io as dev;

use bz_abi::syserr::NOSYS;

pub fn board(query: u64, arg: u64) -> u64 {
    #[cfg(target_arch = "aarch64")]
    return dev::board(query, arg);
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = (query, arg);
        NOSYS
    }
}

pub fn gpio(op: u64, pin: u64, value: u64) -> u64 {
    #[cfg(target_arch = "aarch64")]
    return dev::gpio(op, pin, value);
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = (op, pin, value);
        NOSYS
    }
}

pub fn led(which: u64, on: bool) -> u64 {
    #[cfg(target_arch = "aarch64")]
    return dev::led(which, on);
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = (which, on);
        NOSYS
    }
}

pub fn i2c(bus: u32, addr: u32, speed_hz: u32, tx: &[u8], rx: &mut [u8]) -> u64 {
    #[cfg(target_arch = "aarch64")]
    return dev::i2c(bus, addr, speed_hz, tx, rx);
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = (bus, addr, speed_hz, tx, rx);
        NOSYS
    }
}

pub fn spi(bus: u32, cs: u32, speed_hz: u32, mode: u32, tx: &[u8], rx: &mut [u8]) -> u64 {
    #[cfg(target_arch = "aarch64")]
    return dev::spi(bus, cs, speed_hz, mode, tx, rx);
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = (bus, cs, speed_hz, mode, tx, rx);
        NOSYS
    }
}

pub fn uart_open(port: u32, baud: u32) -> u64 {
    #[cfg(target_arch = "aarch64")]
    return dev::uart_open(port, baud);
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = (port, baud);
        NOSYS
    }
}

pub fn uart_close(port: u32) -> u64 {
    #[cfg(target_arch = "aarch64")]
    return dev::uart_close(port);
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = port;
        NOSYS
    }
}

pub fn uart_write(port: u32, data: &[u8]) -> u64 {
    #[cfg(target_arch = "aarch64")]
    return dev::uart_write(port, data);
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = (port, data);
        NOSYS
    }
}

pub fn uart_read(port: u32, out: &mut [u8]) -> u64 {
    #[cfg(target_arch = "aarch64")]
    return dev::uart_read(port, out);
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = (port, out);
        NOSYS
    }
}

/// Edge of the board firmware's cursor image (0 = none).
pub fn fb_cursor_size() -> usize {
    #[cfg(target_arch = "aarch64")]
    return if dev::present() { dev::FW_CURSOR } else { 0 };
    #[cfg(not(target_arch = "aarch64"))]
    0
}

/// Give the board firmware a cursor image (Raspberry Pi: an HVS plane over
/// the framebuffer). False when there is no such cursor.
pub fn fb_cursor_image(px: &[u32]) -> bool {
    #[cfg(target_arch = "aarch64")]
    return dev::present() && dev::cursor_image(px);
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = px;
        false
    }
}

/// Show / move / hide the board firmware's cursor.
pub fn fb_cursor_state(visible: bool, x: i32, y: i32) -> bool {
    #[cfg(target_arch = "aarch64")]
    return dev::present() && dev::cursor_state(visible, x, y);
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = (visible, x, y);
        false
    }
}
