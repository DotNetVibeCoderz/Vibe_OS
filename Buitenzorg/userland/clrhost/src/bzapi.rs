//! The native half of `Buitenzorg.App` (`/disk/BZAPP.DLL`): every
//! `[DllImport("buitenzorg")] static extern ...` in that library reaches
//! RustCLR as the binding key `pinvoke:buitenzorg!<Name>`, registered here.
//!
//! The marshalling happens on this side. A pointer in RustCLR is a
//! (buffer, offset) reference into the managed heap, not an address the
//! kernel could read, so a managed string is copied out as UTF-8, an `int[]`
//! as a pixel buffer, and the `DrawCmd` the WIN_CMD syscall takes is built
//! here, in the host's own memory.

use alloc::string::String;
use alloc::vec::Vec;

use rustclr_core::{ArrayStorage, ClrArray, ExecResult, Interpreter, Value};

use crate::sys;

const FB_INFO: u64 = 5;
const WIN_CREATE: u64 = 6;
const WIN_CMD: u64 = 7;
const WIN_PRESENT: u64 = 8;
const KEY_READ: u64 = 9;
const IS_INTERACTIVE: u64 = 31;
const PROC_LIST: u64 = 10;
const PROC_KILL: u64 = 11;
const SYS_STAT: u64 = 12;
const FS_LIST: u64 = 29;
const FS_READ: u64 = 30;
const FS_WRITE: u64 = 32;
const PROC_SPAWN: u64 = 40;
const PROC_WAIT: u64 = 41;
const WIN_EVENT: u64 = 45;
const TEXT_METRICS: u64 = 46;
const HW_BOARD: u64 = 47;
const HW_GPIO: u64 = 48;
const HW_I2C: u64 = 49;
const HW_SPI: u64 = 50;
const HW_UART: u64 = 51;
const HW_LED: u64 = 52;
const SETTING: u64 = 53;
const FS_OP: u64 = 54;
const SHELL_RUN: u64 = 58;
const KEY_EVENT: u64 = 59;
const KEY_STATE: u64 = 60;
const SLEEP_UNTIL: u64 = 61;
const AUDIO_WRITE: u64 = 62;
const AUDIO_APP_VOLUME: u64 = 63;
const USER_OP: u64 = 55;
const AUDIO_STAT: u64 = 23;
const AUDIO_SET_VOLUME: u64 = 24;
const AUDIO_TONE: u64 = 25;

/// `bz_abi::HwTransfer`.
#[repr(C)]
struct HwTransfer {
    bus: u32,
    addr: u32,
    speed_hz: u32,
    mode: u32,
    tx_ptr: u64,
    tx_len: u64,
    rx_ptr: u64,
    rx_len: u64,
}
/// `bz_abi::win_flags::CLIENT_SIZE`.
const CLIENT_SIZE: u64 = 1 << 63;

/// `bz_abi::draw_op::BLIT`.
const BLIT: u64 = 7;

/// `bz_abi::DrawCmd`.
#[repr(C)]
struct DrawCmd {
    op: u64,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: u32,
    _pad: u32,
    text_ptr: u64,
    text_len: u64,
}

pub(crate) fn int(a: &[Value], i: usize) -> i64 {
    match a.get(i) {
        Some(Value::I32(v)) => *v as i64,
        Some(Value::I64(v)) | Some(Value::NativeInt(v)) => *v,
        _ => 0,
    }
}

pub(crate) fn string(interp: &Interpreter, a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::Obj(h)) if !h.is_null() => interp.string_value(*h).unwrap_or_default(),
        _ => String::new(),
    }
}

/// An `int[]` (or `uint[]`) argument as 32-bit pixels.
fn pixels(interp: &Interpreter, a: &[Value], i: usize) -> Vec<u32> {
    let Some(Value::Obj(h)) = a.get(i) else { return Vec::new() };
    interp
        .heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::I32(v) => v.iter().map(|&p| p as u32).collect(),
            ArrayStorage::U32(v) => v.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

/// A `byte[]` argument's contents (empty for null or another element type).
fn bytes(interp: &Interpreter, a: &[Value], i: usize) -> Vec<u8> {
    let Some(Value::Obj(h)) = a.get(i) else { return Vec::new() };
    if h.is_null() {
        return Vec::new();
    }
    interp
        .heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::U8(v) => v.clone(),
            ArrayStorage::I8(v) => v.iter().map(|&b| b as u8).collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

/// Copy `data` into the `byte[]` argument `i` (as far as it is long).
fn store_bytes(interp: &mut Interpreter, a: &[Value], i: usize, data: &[u8]) {
    let Some(Value::Obj(h)) = a.get(i) else { return };
    if h.is_null() {
        return;
    }
    let _ = interp.heap.with_mut::<ClrArray, _>(*h, |arr| match &mut arr.storage {
        ArrayStorage::U8(v) => {
            let n = v.len().min(data.len());
            v[..n].copy_from_slice(&data[..n]);
        }
        ArrayStorage::I8(v) => {
            for (d, s) in v.iter_mut().zip(data) {
                *d = *s as i8;
            }
        }
        _ => {}
    });
}

/// Run a bus syscall with a `HwTransfer` over host buffers.
fn hw_transfer(nr: u64, op: u64, bus: i64, addr: i64, speed: i64, mode: i64, tx: &[u8], rx: &mut [u8]) -> u64 {
    let t = HwTransfer {
        bus: bus as u32,
        addr: addr as u32,
        speed_hz: speed as u32,
        mode: mode as u32,
        tx_ptr: if tx.is_empty() { 0 } else { tx.as_ptr() as u64 },
        tx_len: tx.len() as u64,
        rx_ptr: if rx.is_empty() { 0 } else { rx.as_mut_ptr() as u64 },
        rx_len: rx.len() as u64,
    };
    let p = &t as *const HwTransfer as u64;
    if nr == HW_UART { sys::syscall(nr, op, p, 0) } else { sys::syscall(nr, p, 0, 0) }
}

/// `HwBoard(int query, int arg)` -> value, -1 when unavailable.
fn hw_board(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = sys::syscall(HW_BOARD, int(a, 0) as u64, int(a, 1) as u64, 0);
    Ok(Some(Value::I64(if sys::is_err(r) { -1 } else { r as i64 })))
}

/// `HwGpio(int op, int pin, int value)` -> result, -1 when refused.
fn hw_gpio(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = sys::syscall(HW_GPIO, int(a, 0) as u64, int(a, 1) as u64, int(a, 2) as u64);
    Ok(Some(Value::I64(if sys::is_err(r) { -1 } else { r as i64 })))
}

/// `HwLed(int led, int on)` -> 0 / -1.
fn hw_led(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = sys::syscall(HW_LED, int(a, 0) as u64, int(a, 1) as u64, 0);
    ret_i32(if sys::is_err(r) { -1 } else { r as i64 })
}

/// `HwI2c(int bus, int address, int speedHz, byte[] tx, int txLen, byte[] rx, int rxLen)` -> status.
fn hw_i2c(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let mut tx = bytes(i, a, 3);
    tx.truncate(int(a, 4).max(0) as usize);
    let mut rx = alloc::vec![0u8; int(a, 6).clamp(0, 4096) as usize];
    let r = hw_transfer(HW_I2C, 0, int(a, 0), int(a, 1), int(a, 2), 0, &tx, &mut rx);
    if !sys::is_err(r) && !rx.is_empty() {
        store_bytes(i, a, 5, &rx);
    }
    ret_i32(if sys::is_err(r) { -1 } else { r as i64 })
}

/// `HwSpi(int bus, int cs, int speedHz, int mode, byte[] tx, byte[] rx, int len)` -> status.
fn hw_spi(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let len = int(a, 6).clamp(0, 4096) as usize;
    let mut tx = bytes(i, a, 4);
    tx.resize(len, 0);
    let mut rx = alloc::vec![0u8; len];
    let r = hw_transfer(HW_SPI, 0, int(a, 0), int(a, 1), int(a, 2), int(a, 3), &tx, &mut rx);
    if !sys::is_err(r) {
        store_bytes(i, a, 5, &rx);
    }
    ret_i32(if sys::is_err(r) { -1 } else { r as i64 })
}

/// `HwUart(int op, int port, int baud, byte[] buffer, int len)` -> bytes / 0 / -1.
fn hw_uart(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let op = int(a, 0) as u64;
    let len = int(a, 4).clamp(0, 4096) as usize;
    let (r, rx) = match op {
        1 => {
            let mut tx = bytes(i, a, 3);
            tx.truncate(len);
            (hw_transfer(HW_UART, op, int(a, 1), 0, int(a, 2), 0, &tx, &mut []), Vec::new())
        }
        2 => {
            let mut rx = alloc::vec![0u8; len];
            let r = hw_transfer(HW_UART, op, int(a, 1), 0, int(a, 2), 0, &[], &mut rx);
            (r, rx)
        }
        _ => (hw_transfer(HW_UART, op, int(a, 1), 0, int(a, 2), 0, &[], &mut []), Vec::new()),
    };
    if op == 2 && !sys::is_err(r) {
        store_bytes(i, a, 3, &rx[..(r as usize).min(rx.len())]);
    }
    ret_i32(if sys::is_err(r) { -1 } else { r as i64 })
}

/// `SettingGet(int key)` -> value, -1 when unknown.
fn setting_get(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = sys::syscall(SETTING, 0, int(a, 0) as u64, 0);
    ret_i32(if sys::is_err(r) { -1 } else { r as i64 })
}

/// `SettingSet(int key, int value)` -> 0 / -1.
fn setting_set(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = sys::syscall(SETTING, 1, int(a, 0) as u64, int(a, 1) as u64);
    ret_i32(if sys::is_err(r) { -1 } else { 0 })
}

/// `SettingCount(int key)` -> number of choices.
fn setting_count(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = sys::syscall(SETTING, 2, int(a, 0) as u64, 0);
    ret_i32(if sys::is_err(r) { 0 } else { r as i64 })
}

/// `SettingName(int key, int index, int description)` -> string ("" when none).
fn setting_name(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let mut buf = [0u8; 64];
    let op = if int(a, 2) != 0 { 4 } else { 3 };
    let k = (int(a, 0) as u64 & 0xFFFF) | ((int(a, 1) as u64 & 0xFFFF) << 16);
    let r = sys::syscall(SETTING, op, k, buf.as_mut_ptr() as u64);
    let n = if sys::is_err(r) { 0 } else { (r as usize).min(63) };
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    ret_string(i, text)
}

/// `AudioStat(int field)` -> AudioInfo field (0 present, 1 rate, 2 channels,
/// 3 bits, 4 volume, 5 muted), -1 on failure.
fn audio_stat(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let mut info = [0u64; 6];
    let r = sys::syscall(AUDIO_STAT, info.as_mut_ptr() as u64, 0, 0);
    let f = int(a, 0) as usize;
    Ok(Some(Value::I64(if sys::is_err(r) || f >= 6 { -1 } else { info[f] as i64 })))
}

/// `AudioSetVolume(int percent)` -> 0 / -1.
fn audio_set_volume(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = sys::syscall(AUDIO_SET_VOLUME, int(a, 0).clamp(0, 100) as u64, 0, 0);
    ret_i32(if sys::is_err(r) { -1 } else { 0 })
}

/// `AudioTone(int hz, int ms)` -> 0 / -1.
fn audio_tone(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (hz, ms) = (int(a, 0).clamp(20, 20_000) as u64, int(a, 1).clamp(1, 5000) as u64);
    let r = i.blocking(|_| sys::syscall(AUDIO_TONE, hz, ms, 0));
    ret_i32(if sys::is_err(r) { -1 } else { 0 })
}

/// A NUL-terminated copy of `s` (PROC_SPAWN takes C strings).
fn cstr(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s.as_bytes());
    v.push(0);
    v
}

fn ret_i32(v: i64) -> ExecResult<Option<Value>> {
    Ok(Some(Value::I32(v as i32)))
}

fn win_cmd(win: u64, cmd: &DrawCmd) -> i64 {
    let r = sys::syscall(WIN_CMD, win, cmd as *const DrawCmd as u64, 0);
    if r == 0 { 0 } else { -1 }
}

/// `WinCreate(string title, int width, int height)` -> window id (0: failed).
fn win_create(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let title = string(i, a, 0);
    let (w, h) = (int(a, 1).clamp(1, 4096) as u64, int(a, 2).clamp(1, 4096) as u64);
    let id = sys::syscall(WIN_CREATE, title.as_ptr() as u64, title.len() as u64, (w << 32) | h);
    ret_i32(if sys::is_err(id) { 0 } else { id as i64 })
}

/// `WinDraw(int window, int op, int x, int y, int w, int h, int color, string text)`.
fn win_draw(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let text = string(i, a, 7);
    let cmd = DrawCmd {
        op: int(a, 1) as u64,
        x: int(a, 2) as i32,
        y: int(a, 3) as i32,
        w: int(a, 4) as i32,
        h: int(a, 5) as i32,
        color: int(a, 6) as u32 & 0x00FF_FFFF,
        _pad: 0,
        text_ptr: if text.is_empty() { 0 } else { text.as_ptr() as u64 },
        text_len: text.len() as u64,
    };
    ret_i32(win_cmd(int(a, 0) as u64, &cmd))
}

/// `WinBlit(int window, int x, int y, int w, int h, int[] pixels)`.
fn win_blit(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let px = pixels(i, a, 5);
    let (w, h) = (int(a, 3), int(a, 4));
    if w <= 0 || h <= 0 || (px.len() as i64) < w * h {
        return ret_i32(-1);
    }
    let cmd = DrawCmd {
        op: BLIT,
        x: int(a, 1) as i32,
        y: int(a, 2) as i32,
        w: w as i32,
        h: h as i32,
        color: 0,
        _pad: 0,
        text_ptr: px.as_ptr() as u64,
        text_len: (px.len() * 4) as u64,
    };
    ret_i32(win_cmd(int(a, 0) as u64, &cmd))
}

/// `WinCreateEx(string title, int width, int height, int flags)`: flags bit 0
/// = width/height are the client area (the window adds its title bar).
fn win_create_ex(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let title = string(i, a, 0);
    let (w, h) = (int(a, 1).clamp(1, 4096) as u64, int(a, 2).clamp(1, 4096) as u64);
    let flags = if int(a, 3) & 1 != 0 { CLIENT_SIZE } else { 0 };
    let id = sys::syscall(WIN_CREATE, title.as_ptr() as u64, title.len() as u64, flags | (w << 32) | h);
    ret_i32(if sys::is_err(id) { 0 } else { id as i64 })
}

/// `WinDrawEx(int window, int op, int x, int y, int w, int h, int color,
/// int param, int param2, string text)`: the toolkit ops (8-13), where
/// `param` is the second parameter (`_pad`) and `param2` the radius of
/// FILL_ROUND_GRADIENT_V (carried in `text_len`, as no text goes with it).
fn win_draw_ex(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let text = string(i, a, 9);
    let cmd = DrawCmd {
        op: int(a, 1) as u64,
        x: int(a, 2) as i32,
        y: int(a, 3) as i32,
        w: int(a, 4) as i32,
        h: int(a, 5) as i32,
        color: int(a, 6) as u32 & 0x00FF_FFFF,
        _pad: int(a, 7) as u32,
        text_ptr: if text.is_empty() { 0 } else { text.as_ptr() as u64 },
        text_len: if text.is_empty() { int(a, 8) as u64 } else { text.len() as u64 },
    };
    ret_i32(win_cmd(int(a, 0) as u64, &cmd))
}

/// `WinEvent(int window)` -> the next packed pointer event, 0 = none.
fn win_event(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let ev = sys::syscall(WIN_EVENT, int(a, 0) as u64, 0, 0);
    Ok(Some(Value::I64(if sys::is_err(ev) { 0 } else { ev as i64 })))
}

/// `TextMetrics(int style)` -> (glyph width << 32) | line height.
fn text_metrics(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    Ok(Some(Value::I64(sys::syscall(TEXT_METRICS, int(a, 0) as u64, 0, 0) as i64)))
}

fn ret_string(i: &mut Interpreter, s: &str) -> ExecResult<Option<Value>> {
    Ok(Some(Value::Obj(i.alloc_string(s))))
}

/// `FsList(string path)` -> one entry per line, `name` + `/` for a directory
/// ("" lists the mounts).
fn fs_list(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let path = cstr(&string(i, a, 0));
    // bz_abi::FsEntry: 24-byte name + u64 is_dir.
    let mut buf = alloc::vec![0u8; 32 * 128];
    let n = sys::syscall(FS_LIST, path.as_ptr() as u64, buf.as_mut_ptr() as u64, 128);
    let mut out = String::new();
    if !sys::is_err(n) {
        for e in buf.chunks(32).take(n as usize) {
            let len = e[..24].iter().position(|&b| b == 0).unwrap_or(24);
            out.push_str(core::str::from_utf8(&e[..len]).unwrap_or("?"));
            if e[24] != 0 {
                out.push('/');
            }
            out.push('\n');
        }
    }
    ret_string(i, &out)
}

/// `FsReadText(string path)` -> the file as text (invalid UTF-8 replaced),
/// at most 256 KiB; "" if missing.
fn fs_read_text(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let path = cstr(&string(i, a, 0));
    let mut buf = alloc::vec![0u8; 256 * 1024];
    let n = sys::syscall(FS_READ, path.as_ptr() as u64, buf.as_mut_ptr() as u64, buf.len() as u64);
    let n = if sys::is_err(n) { 0 } else { n as usize };
    let text = String::from_utf8_lossy(&buf[..n]).into_owned();
    ret_string(i, &text)
}

/// `FsReadInto(string path, byte[] buffer)` -> bytes read into the buffer
/// (at most its length, 16 MiB), -1 if unreadable.
fn fs_read_into(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let path = cstr(&string(i, a, 0));
    let cap = match a.get(1) {
        Some(Value::Obj(h)) if !h.is_null() => i.heap.with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::U8(v) => v.len(),
            _ => 0,
        }).unwrap_or(0),
        _ => 0,
    }
    .min(16 << 20);
    if cap == 0 {
        return ret_i32(-1);
    }
    let mut buf = alloc::vec![0u8; cap];
    let n = sys::syscall(FS_READ, path.as_ptr() as u64, buf.as_mut_ptr() as u64, cap as u64);
    if sys::is_err(n) {
        return ret_i32(-1);
    }
    let n = (n as usize).min(cap);
    store_bytes(i, a, 1, &buf[..n]);
    ret_i32(n as i64)
}

/// `FsSize(string path)` -> the file's size in bytes (FS_OP stat), -1 if
/// it does not exist.
fn fs_size(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let path = cstr(&string(i, a, 0));
    let mut st = [0u64; 4];
    let r = sys::syscall(FS_OP, 3, path.as_ptr() as u64, st.as_mut_ptr() as u64);
    Ok(Some(Value::I64(if sys::is_err(r) { -1 } else { st[0] as i64 })))
}

/// `FsOp(int op, string path, string path2)` -> 0, or -1 when refused
/// (FS_OP: 0 mkdir, 1 remove, 2 rename / move).
fn fs_op(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let op = match a.first() {
        Some(Value::I32(v)) => *v as u64,
        Some(Value::I64(v)) => *v as u64,
        _ => return ret_i32(-1),
    };
    if op > 2 {
        return ret_i32(-1);
    }
    let (path, path2) = (cstr(&string(i, a, 1)), cstr(&string(i, a, 2)));
    let r = sys::syscall(FS_OP, op, path.as_ptr() as u64, path2.as_ptr() as u64);
    ret_i32(if sys::is_err(r) { -1 } else { 0 })
}

/// `UserOp(int op, string request)` -> 0, or -1 when refused (USER_OP 1-6;
/// the request is the '\n'-separated fields, see bz_abi::user_op).
fn user_op(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let op = match a.first() {
        Some(Value::I32(v)) => *v as u64,
        Some(Value::I64(v)) => *v as u64,
        _ => return ret_i32(-1),
    };
    if op == 0 || op > 6 {
        return ret_i32(-1);
    }
    let req = cstr(&string(i, a, 1));
    let r = sys::syscall(USER_OP, op, req.as_ptr() as u64, 0);
    ret_i32(if sys::is_err(r) { -1 } else { 0 })
}

/// `UserList()` -> one `name\tadmin\tavatar` line per account.
fn user_list(i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    let mut buf = alloc::vec![0u8; 2048];
    let n = sys::syscall(USER_OP, 0, 0, buf.as_mut_ptr() as u64);
    let n = if sys::is_err(n) { 0 } else { (n as usize).min(buf.len()) };
    let text = String::from_utf8_lossy(&buf[..n]).into_owned();
    ret_string(i, &text)
}

/// `FsStat(string path)` -> "size,isFolder,fatDate,fatTime", "" if missing.
fn fs_stat(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let path = cstr(&string(i, a, 0));
    let mut st = [0u64; 4];
    let r = sys::syscall(FS_OP, 3, path.as_ptr() as u64, st.as_mut_ptr() as u64);
    let text = if sys::is_err(r) {
        String::new()
    } else {
        alloc::format!("{},{},{},{}", st[0], st[1], st[2] >> 16, st[2] & 0xFFFF)
    };
    ret_string(i, &text)
}

/// `FsWriteText(string path, string text)` -> bytes written (0 = failed).
fn fs_write_text(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (path, text) = (cstr(&string(i, a, 0)), string(i, a, 1));
    let n = sys::syscall(FS_WRITE, path.as_ptr() as u64, text.as_ptr() as u64, text.len() as u64);
    ret_i32(if sys::is_err(n) { 0 } else { n as i64 })
}

/// `ProcList()` -> one process per line: `pid,state,cpu_ticks,kind,name`.
fn proc_list(i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    // bz_abi::ProcInfo: 4 x u64 + a 32-byte name = 8 words.
    let mut buf = alloc::vec![0u64; 8 * 128];
    let n = sys::syscall(PROC_LIST, buf.as_mut_ptr() as u64, 128, 0);
    let mut out = String::new();
    if !sys::is_err(n) {
        for e in buf.chunks(8).take(n as usize) {
            let name_bytes: Vec<u8> = e[4..].iter().flat_map(|w| w.to_le_bytes()).collect();
            let len = name_bytes.iter().position(|&b| b == 0).unwrap_or(32);
            let name = core::str::from_utf8(&name_bytes[..len]).unwrap_or("?");
            out.push_str(&alloc::format!("{},{},{},{},{}\n", e[0], e[1], e[2], e[3], name));
        }
    }
    ret_string(i, &out)
}

/// `ProcKill(long pid)` -> 0 on success.
fn proc_kill(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = sys::syscall(PROC_KILL, int(a, 0) as u64, 0, 0);
    ret_i32(if r == 0 { 0 } else { -1 })
}

/// `SysStat(int field)` -> one field of `bz_abi::SysStat` (0 uptime ticks,
/// 1 tick Hz, 2 heap used, 3 heap total, 4 tasks, 5 memory MiB).
fn sys_stat(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let mut st = [0u64; 6];
    sys::syscall(SYS_STAT, st.as_mut_ptr() as u64, 0, 0);
    Ok(Some(Value::I64(st.get(int(a, 0) as usize).copied().unwrap_or(0) as i64)))
}

fn win_present(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    sys::syscall(WIN_PRESENT, int(a, 0) as u64, 0, 0);
    ret_i32(0)
}

fn key_read(_i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    ret_i32(sys::syscall(KEY_READ, 0, 0, 0) as i64)
}

/// `KeyEvent()` -> 0, or code | pressed << 16 | 1 << 17 (KEY_EVENT).
fn key_event(_i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    ret_i32(sys::syscall(KEY_EVENT, 0, 0, 0) as i64)
}

/// `KeyState(int code)` -> 1 while held down (KEY_STATE).
fn key_state(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    ret_i32(sys::syscall(KEY_STATE, int(a, 0) as u64, 0, 0) as i64)
}

/// `AudioWrite(byte[] data, int offset, int count, int format)` -> bytes the
/// mixer took, -1 without a streaming card (AUDIO_WRITE). Copies only the
/// range (a song is megabytes; it is written a quarter second at a time).
fn audio_write(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (off, cnt, fmt) = (int(a, 1).max(0) as usize, int(a, 2).max(0) as usize, int(a, 3) as u64);
    let chunk: Vec<u8> = match a.first() {
        Some(Value::Obj(h)) if !h.is_null() => i
            .heap
            .with::<ClrArray, _>(*h, |arr| match &arr.storage {
                ArrayStorage::U8(v) => v.get(off.min(v.len())..(off + cnt).min(v.len())).map(|s| s.to_vec()).unwrap_or_default(),
                _ => Vec::new(),
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let r = sys::syscall(AUDIO_WRITE, chunk.as_ptr() as u64, chunk.len() as u64, fmt);
    ret_i32(if sys::is_err(r) { -1 } else { r as i64 })
}

/// `AudioQueuedMs()` -> the caller's queued milliseconds, -1 without a streaming card.
fn audio_queued(_i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    let r = sys::syscall(AUDIO_WRITE, 0, 0, 0);
    ret_i32(if sys::is_err(r) { -1 } else { r as i64 })
}

/// `AudioAppVolume(long pid, int volume)` -> the volume (volume < 0 = read).
fn audio_app_volume(_i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let v = int(a, 1);
    let r = sys::syscall(AUDIO_APP_VOLUME, int(a, 0) as u64, if v < 0 { u64::MAX } else { v as u64 }, 0);
    ret_i32(r as i64)
}

/// `ImageInfo(byte[] data)` -> width | height << 16 | frames << 32, or -1
/// (BMP, PNG, JPEG, GIF -- imgdecode.rs).
fn image_info(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let data = bytes(i, a, 0);
    let r = crate::imgdecode::with_picture(&data, |p| (p.width as i64) | (p.height as i64) << 16 | (p.frames.len() as i64) << 32);
    Ok(Some(Value::I64(r.unwrap_or(-1))))
}

/// `ImageDecodeInto(byte[] data, int frame, int[] pixels)` -> the frame's
/// delay in ms (0 for a still picture), or -1.
fn image_decode_into(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let data = bytes(i, a, 0);
    let frame = int(a, 1).max(0) as usize;
    let Some((px, delay)) = crate::imgdecode::with_picture(&data, |p| p.frames.get(frame).map(|(f, d)| (f.clone(), *d))).flatten() else {
        return ret_i32(-1);
    };
    let Some(Value::Obj(h)) = a.get(2) else { return ret_i32(-1) };
    let ok = i
        .heap
        .with_mut::<ClrArray, _>(*h, |arr| {
            if let ArrayStorage::I32(v) = &mut arr.storage {
                for (d, s) in v.iter_mut().zip(px.iter()) {
                    *d = *s as i32;
                }
                true
            } else {
                false
            }
        })
        .unwrap_or(false);
    ret_i32(if ok { delay as i64 } else { -1 })
}

/// `ImageDecodeScaled(byte[] data, int frame, int[] pixels, int dw, int dh,
/// int sx, int sy, int sw, int sh, int bg1, int bg2)`: the frame's source
/// rectangle (sx, sy, sw, sh) sampled (nearest) into dw x dh pixels, alpha
/// composited over an 8-pixel checkerboard of bg1/bg2 -- so the interpreter
/// never touches single pixels. Returns the frame's delay in ms, or -1.
fn image_decode_scaled(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let data = bytes(i, a, 0);
    let frame = int(a, 1).max(0) as usize;
    let (dw, dh) = (int(a, 3).max(0) as usize, int(a, 4).max(0) as usize);
    let (sx, sy, sw, sh) = (int(a, 5), int(a, 6), int(a, 7).max(1), int(a, 8).max(1));
    let (bg1, bg2) = (int(a, 9) as u32 & 0xFF_FFFF, int(a, 10) as u32 & 0xFF_FFFF);
    if dw == 0 || dh == 0 || dw * dh > 16 << 20 {
        return ret_i32(-1);
    }
    let out = crate::imgdecode::with_picture(&data, |p| {
        let (f, delay) = p.frames.get(frame)?;
        let (w, h) = (p.width as i64, p.height as i64);
        let mut px = alloc::vec![0u32; dw * dh];
        for y in 0..dh {
            let ty = sy + (y as i64 * sh) / dh as i64;
            for x in 0..dw {
                let tx = sx + (x as i64 * sw) / dw as i64;
                let bg = if ((x >> 3) ^ (y >> 3)) & 1 == 0 { bg1 } else { bg2 };
                px[y * dw + x] = if tx < 0 || ty < 0 || tx >= w || ty >= h {
                    bg
                } else {
                    let c = f[(ty * w + tx) as usize];
                    let al = c >> 24;
                    if al == 255 {
                        c & 0xFF_FFFF
                    } else {
                        let mix = |s: u32| -> u32 {
                            let (cs, bs) = ((c >> s) & 255, (bg >> s) & 255);
                            ((cs * al + bs * (255 - al)) / 255) << s
                        };
                        mix(16) | mix(8) | mix(0)
                    }
                };
            }
        }
        Some((px, *delay))
    })
    .flatten();
    let Some((px, delay)) = out else { return ret_i32(-1) };
    let Some(Value::Obj(h)) = a.get(2) else { return ret_i32(-1) };
    let ok = i
        .heap
        .with_mut::<ClrArray, _>(*h, |arr| {
            if let ArrayStorage::I32(v) = &mut arr.storage {
                for (d, s) in v.iter_mut().zip(px.iter()) {
                    *d = *s as i32;
                }
                true
            } else {
                false
            }
        })
        .unwrap_or(false);
    ret_i32(if ok { delay as i64 } else { -1 })
}

/// `SleepUntil(long ns)`: until CLOCK_NANOS reaches ns (other managed threads
/// and the collector go on meanwhile).
fn sleep_until(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let ns = int(a, 0) as u64;
    i.blocking(|_| sys::syscall(SLEEP_UNTIL, ns, 0, 0));
    ret_i32(0)
}

/// `bz_abi::FramebufferInfo`: [address, size, width, height, stride, bpp, format].
fn fb_info() -> [u64; 7] {
    let mut fb = [0u64; 7];
    sys::syscall(FB_INFO, fb.as_mut_ptr() as u64, 0, 0);
    fb
}

fn screen_width(_i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    ret_i32(fb_info()[2] as i64)
}

fn screen_height(_i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    ret_i32(fb_info()[3] as i64)
}

fn is_interactive(_i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    ret_i32(sys::syscall(IS_INTERACTIVE, 0, 0, 0) as i64)
}

/// `Spawn(string path, string args)` -> pid (negative: failed).
fn spawn(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let (path, args) = (cstr(&string(i, a, 0)), cstr(&string(i, a, 1)));
    let pid = sys::syscall(PROC_SPAWN, path.as_ptr() as u64, args.as_ptr() as u64, 0);
    Ok(Some(Value::I64(if sys::is_err(pid) { -1 } else { pid as i64 })))
}

/// `Wait(long pid)` -> exit code. A blocking call: other managed threads run
/// meanwhile, and a collection must not wait for this one -- it is announced
/// blocked, with its roots handed over.
fn wait(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let pid = int(a, 0) as u64;
    let code = i.blocking(|_| sys::syscall(PROC_WAIT, pid, 0, 0));
    Ok(Some(Value::I64(code as i64)))
}

/// `ShellRun(string line)` -> "0\n<output>" (succeeded) or "1\n<output>"
/// (failed): the line run by the built-in shell (SHELL_RUN). Blocking, so
/// other managed threads and a collection go on meanwhile.
fn shell_run(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let line = cstr(&string(i, a, 0));
    let mut buf = alloc::vec![0u8; 256 * 1024];
    let (ptr, cap) = (buf.as_mut_ptr() as u64, buf.len() as u64);
    let r = i.blocking(|_| sys::syscall(SHELL_RUN, line.as_ptr() as u64, ptr, cap));
    if sys::is_err(r) {
        return ret_string(i, "1\nshell: permintaan ditolak");
    }
    let n = ((r & 0xFFFF_FFFF) as usize).min(buf.len());
    let failed = r >> 32 & 1 == 1;
    let mut text = String::from(if failed { "1\n" } else { "0\n" });
    text.push_str(&String::from_utf8_lossy(&buf[..n]));
    ret_string(i, &text)
}

/// Register the bindings.
pub fn install(interp: &mut Interpreter) {
    let natives: [(&str, rustclr_core::interp::NativeFn); 49] = [
        ("WinCreate", win_create),
        ("WinDraw", win_draw),
        ("WinBlit", win_blit),
        ("WinPresent", win_present),
        ("KeyRead", key_read),
        ("KeyEvent", key_event),
        ("KeyState", key_state),
        ("SleepUntil", sleep_until),
        ("AudioWrite", audio_write),
        ("AudioQueuedMs", audio_queued),
        ("AudioAppVolume", audio_app_volume),
        ("ImageInfo", image_info),
        ("ImageDecodeInto", image_decode_into),
        ("ImageDecodeScaled", image_decode_scaled),
        ("ScreenWidth", screen_width),
        ("ScreenHeight", screen_height),
        ("IsInteractive", is_interactive),
        ("Spawn", spawn),
        ("Wait", wait),
        ("WinCreateEx", win_create_ex),
        ("WinDrawEx", win_draw_ex),
        ("WinEvent", win_event),
        ("TextMetrics", text_metrics),
        ("FsList", fs_list),
        ("FsReadText", fs_read_text),
        ("FsSize", fs_size),
        ("FsWriteText", fs_write_text),
        ("ProcList", proc_list),
        ("ProcKill", proc_kill),
        ("SysStat", sys_stat),
        ("HwBoard", hw_board),
        ("HwGpio", hw_gpio),
        ("HwLed", hw_led),
        ("HwI2c", hw_i2c),
        ("HwSpi", hw_spi),
        ("HwUart", hw_uart),
        ("SettingGet", setting_get),
        ("SettingSet", setting_set),
        ("SettingCount", setting_count),
        ("SettingName", setting_name),
        ("AudioStat", audio_stat),
        ("AudioSetVolume", audio_set_volume),
        ("AudioTone", audio_tone),
        ("FsReadInto", fs_read_into),
        ("FsOp", fs_op),
        ("FsStat", fs_stat),
        ("UserOp", user_op),
        ("UserList", user_list),
        ("ShellRun", shell_run),
    ];
    for (name, f) in natives {
        interp.register_native(alloc::format!("pinvoke:buitenzorg!{}", name), f);
    }
    crate::netapi::register(interp);
}
