//! Zero-copy surfaces, vsync and exclusive full screen (WIN_CTL), on RISC-V:
//! a window gets two shared buffers mapped into this process (SURFACE), the
//! program draws into the back one and flips (SURFACE_PRESENT) once per
//! vblank (VSYNC_WAIT) -- the compositor reads the pixels where they are, no
//! BLIT copy. The surface stays pinned (MUNMAP refuses it). A second window
//! goes full screen (FULLSCREEN) and back.
#![no_std]
#![no_main]
mod rt;
use rt::*;

const WIN_CREATE: u64 = 6;
const WIN_PRESENT: u64 = 8;
const MUNMAP: u64 = 15;
const WIN_CTL: u64 = 74;

const FULLSCREEN: u64 = 0;
const SURFACE: u64 = 1;
const SURFACE_PRESENT: u64 = 2;
const VSYNC_WAIT: u64 = 3;
const VSYNC_INFO: u64 = 4;
const SURFACE_SIZE: u64 = 5;

fn ctl(op: u64, win: u64, arg: u64) -> u64 {
    syscall(WIN_CTL, op | win << 8, arg, 0)
}

fn fail(what: &str) -> u64 {
    print("surface: FAIL ");
    print(what);
    print("\n");
    1
}

fn main(_arg: u64) -> u64 {
    let title = "Permukaan";
    let win = syscall(WIN_CREATE, title.as_ptr() as u64, title.len() as u64, (240u64 << 32) | 140);
    if win == 0 || win > u32::MAX as u64 {
        return fail("WIN_CREATE");
    }
    let size = ctl(SURFACE_SIZE, win, 0);
    let (w, h) = ((size & 0xFFFF) as usize, (size >> 16 & 0xFFFF) as usize);
    if w == 0 || h == 0 {
        return fail("SURFACE_SIZE");
    }
    let va = ctl(SURFACE, win, 0);
    if va == 0 || va >= 0x8000_0000 {
        return fail("SURFACE");
    }
    let buf_bytes = (w * h * 4 + 4095) & !4095;
    let info = ctl(VSYNC_INFO, 0, 0);
    let refresh = info & 0xFFFF_FFFF;
    if !(20_000..=240_000).contains(&refresh) {
        return fail("VSYNC_INFO");
    }
    // eight frames: draw a moving bar into the back buffer, wait for the
    // vblank, flip
    let mut last = ctl(VSYNC_WAIT, 0, 0);
    for frame in 0..8usize {
        let back = frame & 1;
        let px = (va as usize + back * buf_bytes) as *mut u32;
        let bar = frame * (w / 8);
        for y in 0..h {
            for x in 0..w {
                let c = if x >= bar && x < bar + w / 8 { 0x00F0_C040 } else { 0x0010_2A1C + ((y * 255 / h) as u32) };
                unsafe { px.add(y * w + x).write_volatile(c) };
            }
        }
        let now = ctl(VSYNC_WAIT, 0, 0);
        if now <= last {
            return fail("VSYNC_WAIT did not advance");
        }
        last = now;
        if ctl(SURFACE_PRESENT, win, back as u64) != 0 {
            return fail("SURFACE_PRESENT");
        }
        syscall(WIN_PRESENT, win, 0, 0);
    }
    // the buffers belong to the window: unmapping them is refused
    if syscall(MUNMAP, va, 4096, 0) == 0 {
        return fail("MUNMAP of a surface succeeded");
    }
    // a second window goes full screen: its client area is the screen
    let t2 = "Layar penuh";
    let full = syscall(WIN_CREATE, t2.as_ptr() as u64, t2.len() as u64, (160u64 << 32) | 100);
    let before = ctl(SURFACE_SIZE, full, 0);
    let fs = ctl(FULLSCREEN, full, 1);
    let (fw, fh) = (fs & 0xFFFF, fs >> 16 & 0xFFFF);
    if fw < 320 || fh < 200 || fs > 0xFFFF_FFFF {
        return fail("FULLSCREEN on");
    }
    // a window with a surface cannot also take the screen (its buffers are
    // the window's size)
    if ctl(FULLSCREEN, win, 1) < 0x1_0000_0000 {
        return fail("FULLSCREEN allowed on a surface window");
    }
    let back = ctl(FULLSCREEN, full, 0);
    if back != before {
        return fail("FULLSCREEN off");
    }
    print("surface: 8 frames flipped on vblank, pinned buffers, full screen ");
    print_u64(fw);
    print("x");
    print_u64(fh);
    print(" and back\n");
    0
}
