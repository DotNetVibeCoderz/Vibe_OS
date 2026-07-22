//! Syscall ABI v1 dispatcher (requirements.md §4, §10.1).
//!
//! The numbered contract lives in the shared `bz-abi` crate and is mirrored
//! by `Buitenzorg.Runtime` (C#). Today the dispatcher is exercised from
//! kernel context; wiring it to a ring-3 `syscall` instruction entry point is
//! the remaining v0.2/v0.4 work tracked in requirements.md §17.

use bz_abi::{pixel_format, syserr, sysno, FramebufferInfo, ABI_VERSION};

use crate::{framebuffer, interrupts, print, task};

/// Dispatch a syscall by number. Arguments and result follow the C ABI:
/// unused arguments are ignored, results are returned as `u64`.
pub fn dispatch(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    match nr {
        sysno::ABI_VERSION => ABI_VERSION,
        sysno::DEBUG_WRITE => sys_debug_write(a0, a1),
        sysno::EXIT => sys_exit(a0),
        sysno::YIELD => {
            task::yield_now();
            0
        }
        sysno::TICKS => interrupts::ticks(),
        sysno::FB_INFO => sys_fb_info(a0),
        sysno::WIN_CREATE => sys_win_create(a0, a1, a2),
        sysno::WIN_CMD => sys_win_cmd(a0, a1),
        sysno::WIN_PRESENT => {
            crate::wm::present_now();
            0
        }
        sysno::KEY_READ => crate::keyboard::pop().map(|c| c as u64).unwrap_or(0),
        sysno::PROC_LIST => sys_proc_list(a0, a1),
        sysno::PROC_KILL => {
            if crate::process::kill(a0) {
                0
            } else {
                syserr::INVAL
            }
        }
        sysno::SYS_STAT => sys_stat(a0),
        _ => syserr::NOSYS,
    }
}

fn sys_proc_list(buf_ptr: u64, max: u64) -> u64 {
    if buf_ptr == 0 || max == 0 || max > 256 {
        return syserr::INVAL;
    }
    let out = unsafe {
        core::slice::from_raw_parts_mut(buf_ptr as *mut bz_abi::ProcInfo, max as usize)
    };
    crate::process::list(out) as u64
}

fn sys_stat(out_ptr: u64) -> u64 {
    if out_ptr == 0 {
        return syserr::INVAL;
    }
    let stat = crate::process::stat();
    unsafe { core::ptr::write(out_ptr as *mut bz_abi::SysStat, stat) };
    0
}

fn sys_win_create(title_ptr: u64, title_len: u64, dims: u64) -> u64 {
    if title_ptr == 0 || title_len == 0 || title_len > 128 {
        return 0;
    }
    // Safety: v1 callers are single-process ring 3 launched by the kernel;
    // full address-space validation arrives with multi-process isolation.
    let bytes = unsafe { core::slice::from_raw_parts(title_ptr as *const u8, title_len as usize) };
    let Ok(title) = core::str::from_utf8(bytes) else {
        return 0;
    };
    let w = ((dims >> 32) as i32).clamp(80, 2000);
    let h = ((dims & 0xFFFF_FFFF) as i32).clamp(60, 2000);
    crate::wm::create_app_window(title, w, h) as u64
}

fn sys_win_cmd(win_id: u64, cmd_ptr: u64) -> u64 {
    if cmd_ptr == 0 {
        return syserr::INVAL;
    }
    let cmd = unsafe { core::ptr::read(cmd_ptr as *const bz_abi::DrawCmd) };
    let text = if cmd.op == bz_abi::draw_op::DRAW_TEXT {
        if cmd.text_ptr == 0 || cmd.text_len == 0 || cmd.text_len > 4096 {
            return syserr::INVAL;
        }
        let bytes =
            unsafe { core::slice::from_raw_parts(cmd.text_ptr as *const u8, cmd.text_len as usize) };
        match core::str::from_utf8(bytes) {
            Ok(s) => Some(s),
            Err(_) => return syserr::INVAL,
        }
    } else {
        None
    };
    match crate::wm::draw_on_window(win_id as u32, &cmd, text) {
        Ok(()) => 0,
        Err(_) => syserr::INVAL,
    }
}

fn sys_debug_write(ptr: u64, len: u64) -> u64 {
    if ptr == 0 || len == 0 {
        return syserr::INVAL;
    }
    // Safety: v1 callers are kernel-context only; once ring 3 lands this must
    // validate the range against the caller's address space.
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    match core::str::from_utf8(bytes) {
        Ok(s) => {
            print!("{}", s);
            len
        }
        Err(_) => syserr::INVAL,
    }
}

fn sys_exit(code: u64) -> u64 {
    if crate::usermode::user_active() {
        crate::usermode::exit_user(code);
    }
    crate::println!("[kernel] task exited with code {}", code);
    task::exit_current()
}

fn sys_fb_info(out_ptr: u64) -> u64 {
    if out_ptr == 0 {
        return syserr::INVAL;
    }
    let Some(console) = framebuffer::CONSOLE.get() else {
        return syserr::INVAL;
    };
    let mut console = console.lock();
    let info = console.info();
    let (addr, size) = console.buffer_addr();
    let fb = FramebufferInfo {
        address: addr,
        size,
        width: info.width as u64,
        height: info.height as u64,
        stride: info.stride as u64,
        bytes_per_pixel: info.bytes_per_pixel as u64,
        pixel_format: match info.pixel_format {
            bootloader_api::info::PixelFormat::Rgb => pixel_format::RGB,
            bootloader_api::info::PixelFormat::Bgr => pixel_format::BGR,
            bootloader_api::info::PixelFormat::U8 => pixel_format::GRAY,
            _ => pixel_format::UNKNOWN,
        },
    };
    unsafe { core::ptr::write(out_ptr as *mut FramebufferInfo, fb) };
    0
}
