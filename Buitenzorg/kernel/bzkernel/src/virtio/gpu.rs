//! virtio-gpu, 2D (§5.7): a display the compositor presents into. The driver
//! creates one host resource the size of the desktop (format B8G8R8X8 — the
//! same bytes as the compositor's `0x00RRGGBB` pixels), backs it with
//! physically contiguous guest memory, and binds it to scanout 0. Each present
//! copies the frame into the backing, then TRANSFER_TO_HOST_2D + RESOURCE_FLUSH.
//!
//! On PCs it mirrors the boot framebuffer (the VGA device stays the console);
//! on ARM64 / RISC-V `virt`, which have no VGA, it will be the only display.
//!
//! G2 (2D on the GPU): presents transfer and flush only the rows the
//! compositor's damage names (`present_bands`), and the pointer is a
//! **hardware cursor** -- a 64x64 ARGB resource on the cursor queue (queue 1:
//! UPDATE_CURSOR sets the image, MOVE_CURSOR moves it), so moving the mouse
//! costs one small command and no pixel transfer. (2D virtio-gpu has no
//! host-side composition: one resource per window only pays off with
//! 3D/virgl, G3.)

use alloc::boxed::Box;
use spin::Mutex;

use super::{Buf, Transport, Virtqueue};

const GET_DISPLAY_INFO: u32 = 0x0100;
const RESOURCE_CREATE_2D: u32 = 0x0101;
const SET_SCANOUT: u32 = 0x0103;
const RESOURCE_FLUSH: u32 = 0x0104;
const TRANSFER_TO_HOST_2D: u32 = 0x0105;
const RESOURCE_ATTACH_BACKING: u32 = 0x0106;
const UPDATE_CURSOR: u32 = 0x0300;
const MOVE_CURSOR: u32 = 0x0301;
const FORMAT_B8G8R8A8: u32 = 1;
const CURSOR_RESOURCE: u32 = 2;
/// Edge of the cursor image (virtio-gpu cursors are 64x64).
pub const CURSOR_SIZE: usize = 64;
const RESP_OK_NODATA: u32 = 0x1100;
const RESP_OK_DISPLAY_INFO: u32 = 0x1101;
const FORMAT_B8G8R8X8: u32 = 2;
const RESOURCE: u32 = 1;

pub struct VirtioGpu {
    t: Box<dyn Transport>,
    q: Virtqueue,
    /// Command (page 0: request at 0, response at 2048) DMA page.
    cmd_phys: u64,
    cmd: *mut u8,
    width: u32,
    height: u32,
    fb: *mut u32,
    pub presents: u64,
    pub display: (u32, u32),
    /// The cursor queue and the cursor image's backing, when the device has one.
    cq: Option<Virtqueue>,
    cursor_px: *mut u32,
    /// An image was uploaded: the hardware cursor is in use.
    cursor_set: bool,
    pub cursor_moves: u64,
    pub band_presents: u64,
}

unsafe impl Send for VirtioGpu {}

static GPU: Mutex<Option<VirtioGpu>> = Mutex::new(None);

impl VirtioGpu {
    /// Send the command already written at the start of the command page
    /// (`len` bytes); return the response type.
    fn command(&mut self, len: usize, resp_len: usize) -> Result<u32, &'static str> {
        let req = Buf { phys: self.cmd_phys, len: len as u32, device_writes: false };
        let resp = Buf { phys: self.cmd_phys + 2048, len: resp_len as u32, device_writes: true };
        self.q.submit_wait(self.t.as_ref(), &[req, resp])?;
        Ok(unsafe { (self.cmd.add(2048) as *const u32).read_volatile() })
    }

    /// Write a command: 24-byte control header with `kind`, then `args` (u32s).
    fn put(&mut self, kind: u32, args: &[u32]) -> usize {
        unsafe {
            core::ptr::write_bytes(self.cmd, 0, 2048 + 512);
            (self.cmd as *mut u32).write_volatile(kind);
            for (i, a) in args.iter().enumerate() {
                (self.cmd.add(24 + i * 4) as *mut u32).write_volatile(*a);
            }
        }
        24 + args.len() * 4
    }

    fn ok(&mut self, kind: u32, args: &[u32]) -> Result<(), &'static str> {
        let n = self.put(kind, args);
        match self.command(n, 24)? {
            RESP_OK_NODATA => Ok(()),
            _ => Err("virtio-gpu: command failed"),
        }
    }

    fn new(mut t: Box<dyn Transport>, width: u32, height: u32) -> Result<VirtioGpu, &'static str> {
        super::begin_init(t.as_mut(), 0)?;
        let q = Virtqueue::new(t.as_mut(), 0)?;
        // queue 1 = the cursor queue (set up before DRIVER_OK like every queue)
        let cq = Virtqueue::new(t.as_mut(), 1).ok();
        super::finish_init(t.as_mut());
        let (cmd_phys, cmd) = crate::memory::alloc_dma_frame().ok_or("virtio-gpu: out of memory")?;
        let pages = (width as usize * height as usize * 4).div_ceil(4096);
        let (fb_phys, fb) = crate::memory::alloc_dma_contiguous(pages).ok_or("virtio-gpu: no contiguous memory for the framebuffer")?;
        let mut g = VirtioGpu {
            t,
            q,
            cmd_phys,
            cmd,
            width,
            height,
            fb: fb as *mut u32,
            presents: 0,
            display: (0, 0),
            cq: None,
            cursor_px: core::ptr::null_mut(),
            cursor_set: false,
            cursor_moves: 0,
            band_presents: 0,
        };

        // The host's preferred mode for scanout 0 (informational).
        let n = g.put(GET_DISPLAY_INFO, &[]);
        if g.command(n, 24 + 16 * 24)? != RESP_OK_DISPLAY_INFO {
            return Err("virtio-gpu: no display info");
        }
        let d = unsafe { g.cmd.add(2048 + 24) as *const u32 };
        g.display = unsafe { (d.add(2).read_volatile(), d.add(3).read_volatile()) };

        g.ok(RESOURCE_CREATE_2D, &[RESOURCE, FORMAT_B8G8R8X8, width, height])?;
        // One backing entry: { addr u64, length u32, padding u32 }.
        let len = (width * height * 4) as u32;
        g.ok(RESOURCE_ATTACH_BACKING, &[RESOURCE, 1, fb_phys as u32, (fb_phys >> 32) as u32, len, 0])?;
        g.ok(SET_SCANOUT, &[0, 0, width, height, 0, RESOURCE])?;
        // The hardware cursor: a 64x64 ARGB resource for the cursor queue.
        if let Some(cq) = cq {
            let bytes = CURSOR_SIZE * CURSOR_SIZE * 4;
            if let Some((cphys, cva)) = crate::memory::alloc_dma_contiguous(bytes / 4096) {
                let sz = CURSOR_SIZE as u32;
                if g.ok(RESOURCE_CREATE_2D, &[CURSOR_RESOURCE, FORMAT_B8G8R8A8, sz, sz]).is_ok()
                    && g.ok(RESOURCE_ATTACH_BACKING, &[CURSOR_RESOURCE, 1, cphys as u32, (cphys >> 32) as u32, bytes as u32, 0]).is_ok()
                {
                    g.cq = Some(cq);
                    g.cursor_px = cva as *mut u32;
                }
            }
        }
        crate::println!(
            "[virtio] gpu at {}: scanout 0 = {}x{} resource (host display {}x{}), {} KiB contiguous backing",
            g.t.location(),
            width,
            height,
            g.display.0,
            g.display.1,
            pages * 4
        );
        Ok(g)
    }

    /// Copy rows `y0..y1` of `back` into the backing, transfer and flush them.
    fn present_rows(&mut self, back: &[u32], y0: u32, y1: u32) -> Result<(), &'static str> {
        let w = self.width as usize;
        let y1 = y1.min(self.height).min((back.len() / w.max(1)) as u32);
        if y1 <= y0 {
            return Ok(());
        }
        let (a, n) = (y0 as usize * w, (y1 - y0) as usize * w);
        unsafe { core::ptr::copy_nonoverlapping(back.as_ptr().add(a), self.fb.add(a), n) };
        let off = (a * 4) as u64;
        let h = y1 - y0;
        // rect (x, y, w, h), offset into the backing (u64), resource, padding
        self.ok(TRANSFER_TO_HOST_2D, &[0, y0, self.width, h, off as u32, (off >> 32) as u32, RESOURCE, 0])?;
        self.ok(RESOURCE_FLUSH, &[0, y0, self.width, h, RESOURCE, 0])
    }

    /// One command on the cursor queue (56 bytes: header, position, resource,
    /// hot spot), in the second half of the command page.
    fn cursor_cmd(&mut self, kind: u32, x: i32, y: i32, resource: u32) -> Result<(), &'static str> {
        let Some(mut cq) = self.cq.take() else { return Err("virtio-gpu: no cursor queue") };
        let at = 3072;
        unsafe {
            let c = self.cmd.add(at);
            core::ptr::write_bytes(c, 0, 56);
            let w = |i: usize, v: u32| (c.add(i * 4) as *mut u32).write_volatile(v);
            w(0, kind);
            // pos (at 24): scanout, x, y, padding
            w(6, 0);
            w(7, x.max(0) as u32);
            w(8, y.max(0) as u32);
            // (at 40): resource id, hot_x, hot_y, padding
            w(10, resource);
        }
        let req = Buf { phys: self.cmd_phys + at as u64, len: 56, device_writes: false };
        let r = cq.submit_wait(self.t.as_ref(), &[req]).map(|_| ());
        self.cq = Some(cq);
        r
    }

    fn present(&mut self, back: &[u32]) -> Result<(), &'static str> {
        let n = (self.width as usize * self.height as usize).min(back.len());
        unsafe { core::ptr::copy_nonoverlapping(back.as_ptr(), self.fb, n) };
        let (w, h) = (self.width, self.height);
        // rect, offset (u64), resource id, padding
        self.ok(TRANSFER_TO_HOST_2D, &[0, 0, w, h, 0, 0, RESOURCE, 0])?;
        self.ok(RESOURCE_FLUSH, &[0, 0, w, h, RESOURCE, 0])?;
        self.presents += 1;
        Ok(())
    }
}

/// Bring up the first virtio GPU with a `width` x `height` desktop. Prints the
/// milestone once a first frame went through it.
pub fn init(width: usize, height: usize) {
    for t in super::find(super::device::GPU) {
        let loc = t.location();
        match VirtioGpu::new(t, width as u32, height as u32) {
            Ok(g) => {
                *GPU.lock() = Some(g);
                return;
            }
            Err(e) => crate::println!("[virtio] gpu at {}: {}", loc, e),
        }
    }
}

/// Present a desktop frame (`0x00RRGGBB`, desktop-sized) on the virtio GPU.
pub fn present(back: &[u32]) {
    let Some(mut guard) = GPU.try_lock() else { return };
    let Some(g) = guard.as_mut() else { return };
    match g.present(back) {
        Ok(()) if g.presents == 1 => crate::println!(
            "[kernel] MILESTONE: VIRTIO GPU OK (2D resource {}x{} on scanout 0: create + attach backing + set scanout, first frame transferred + flushed)",
            g.width, g.height
        ),
        Ok(()) => {}
        Err(e) => crate::println!("[virtio] gpu present: {}", e),
    }
}

/// Present only the row bands `(y0, y1)` of a desktop frame (damage).
pub fn present_bands(back: &[u32], bands: &[(usize, usize)]) {
    let Some(mut guard) = GPU.try_lock() else { return };
    let Some(g) = guard.as_mut() else { return };
    if g.presents == 0 {
        // the first frame goes whole (and prints the milestone)
        drop(guard);
        present(back);
        return;
    }
    for &(y0, y1) in bands {
        if let Err(e) = g.present_rows(back, y0 as u32, y1 as u32) {
            crate::println!("[virtio] gpu present: {}", e);
            return;
        }
    }
    g.band_presents += 1;
}

/// Whether a virtio GPU with a cursor queue is up (it can show a hardware
/// cursor).
pub fn cursor_capable() -> bool {
    GPU.try_lock().is_some_and(|g| g.as_ref().is_some_and(|g| g.cq.is_some()))
}

/// Whether a virtio GPU is up at all.
pub fn active() -> bool {
    GPU.try_lock().is_some_and(|g| g.is_some())
}

/// Set the cursor image (`CURSOR_SIZE`^2 ARGB pixels, hot spot top-left) and
/// show it at (x, y).
pub fn set_cursor(image: &[u32], x: i32, y: i32) -> bool {
    let Some(mut guard) = GPU.try_lock() else { return false };
    let Some(g) = guard.as_mut() else { return false };
    if g.cq.is_none() || image.len() < CURSOR_SIZE * CURSOR_SIZE {
        return false;
    }
    unsafe { core::ptr::copy_nonoverlapping(image.as_ptr(), g.cursor_px, CURSOR_SIZE * CURSOR_SIZE) };
    let sz = CURSOR_SIZE as u32;
    let ok = g.ok(TRANSFER_TO_HOST_2D, &[0, 0, sz, sz, 0, 0, CURSOR_RESOURCE, 0]).is_ok()
        && g.cursor_cmd(UPDATE_CURSOR, x, y, CURSOR_RESOURCE).is_ok();
    if ok && !g.cursor_set {
        crate::println!("[virtio] gpu: hardware cursor on (64x64 ARGB resource on the cursor queue)");
    }
    g.cursor_set = ok;
    ok
}

/// Move the hardware cursor.
pub fn move_cursor(x: i32, y: i32) {
    let Some(mut guard) = GPU.try_lock() else { return };
    let Some(g) = guard.as_mut() else { return };
    if g.cursor_set && g.cursor_cmd(MOVE_CURSOR, x, y, CURSOR_RESOURCE).is_ok() {
        g.cursor_moves += 1;
    }
}

/// Hide the hardware cursor (resource 0); the next `set_cursor` shows it.
pub fn hide_cursor() {
    let Some(mut guard) = GPU.try_lock() else { return };
    let Some(g) = guard.as_mut() else { return };
    if g.cursor_set {
        let _ = g.cursor_cmd(UPDATE_CURSOR, 0, 0, 0);
        g.cursor_set = false;
    }
}

/// (hardware cursor moves, damage presents) so far.
pub fn stats() -> (u64, u64) {
    GPU.try_lock().and_then(|g| g.as_ref().map(|g| (g.cursor_moves, g.band_presents))).unwrap_or((0, 0))
}

/// Frames presented through the virtio GPU.
#[allow(dead_code)]
pub fn presents() -> u64 {
    GPU.lock().as_ref().map(|g| g.presents).unwrap_or(0)
}
