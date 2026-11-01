//! virtio-gpu, 2D (§5.7): a display the compositor presents into. The driver
//! creates one host resource the size of the desktop (format B8G8R8X8 — the
//! same bytes as the compositor's `0x00RRGGBB` pixels), backs it with
//! physically contiguous guest memory, and binds it to scanout 0. Each present
//! copies the frame into the backing, then TRANSFER_TO_HOST_2D + RESOURCE_FLUSH.
//!
//! On PCs it mirrors the boot framebuffer (the VGA device stays the console);
//! on ARM64 / RISC-V `virt`, which have no VGA, it will be the only display.

use alloc::boxed::Box;
use spin::Mutex;

use super::{Buf, Transport, Virtqueue};

const GET_DISPLAY_INFO: u32 = 0x0100;
const RESOURCE_CREATE_2D: u32 = 0x0101;
const SET_SCANOUT: u32 = 0x0103;
const RESOURCE_FLUSH: u32 = 0x0104;
const TRANSFER_TO_HOST_2D: u32 = 0x0105;
const RESOURCE_ATTACH_BACKING: u32 = 0x0106;
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
        super::finish_init(t.as_mut());
        let (cmd_phys, cmd) = crate::memory::alloc_dma_frame().ok_or("virtio-gpu: out of memory")?;
        let pages = (width as usize * height as usize * 4).div_ceil(4096);
        let (fb_phys, fb) = crate::memory::alloc_dma_contiguous(pages).ok_or("virtio-gpu: no contiguous memory for the framebuffer")?;
        let mut g = VirtioGpu { t, q, cmd_phys, cmd, width, height, fb: fb as *mut u32, presents: 0, display: (0, 0) };

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
    for t in super::pci::find(super::device::GPU) {
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

/// Frames presented through the virtio GPU.
#[allow(dead_code)]
pub fn presents() -> u64 {
    GPU.lock().as_ref().map(|g| g.presents).unwrap_or(0)
}
