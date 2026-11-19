//! Screenshots: the desktop as a 24-bit BMP in `/disk/LAYAR` (Print Screen,
//! shell `layar`). Generic code: the picture is composed by the window
//! manager (`wm::snapshot`, without the pointer), not read back from the
//! framebuffer, so it is the same on every architecture and display.

use alloc::{format, string::String, vec::Vec};

pub const DIR: &str = "/disk/LAYAR";

/// A 24-bit bottom-up BMP of 0x00RRGGBB pixels.
pub fn bmp(px: &[u32], w: usize, h: usize) -> Vec<u8> {
    let row = (w * 3 + 3) & !3;
    let size = 54 + row * h;
    let mut b = Vec::with_capacity(size);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&(size as u32).to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&(w as i32).to_le_bytes());
    b.extend_from_slice(&(h as i32).to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&((row * h) as u32).to_le_bytes());
    b.extend_from_slice(&2835u32.to_le_bytes());
    b.extend_from_slice(&2835u32.to_le_bytes());
    b.extend_from_slice(&[0u8; 8]);
    for y in (0..h).rev() {
        for &p in &px[y * w..(y + 1) * w] {
            b.extend_from_slice(&[p as u8, (p >> 8) as u8, (p >> 16) as u8]);
        }
        b.resize(b.len() + (row - w * 3), 0);
    }
    b
}

/// The next free `LAYARnnn.BMP` in `dir`.
fn next_name(dir: &str) -> Option<String> {
    (1..1000).map(|n| format!("{}/LAYAR{:03}.BMP", dir, n)).find(|p| crate::vfs::stat(p).is_err())
}

/// Save the screen into `dir` (created when missing); the file's path.
pub fn save_to(dir: &str) -> Result<String, &'static str> {
    if crate::vfs::stat(dir).is_err() {
        crate::vfs::mkdir(dir)?;
    }
    let (px, w, h) = crate::wm::snapshot().ok_or("no desktop")?;
    let path = next_name(dir).ok_or("folder full (999 pictures)")?;
    let data = bmp(&px, w, h);
    drop(px);
    crate::vfs::write(&path, &data)?;
    Ok(path)
}

/// Print Screen: save into [`DIR`].
pub fn save_screen() -> Result<String, &'static str> {
    save_to(DIR)
}

/// The whole screen as a BMP in memory (header, size, not one flat colour),
/// then a 64x64 crop of it written to a scratch folder, read back and
/// removed -- a full-size file may not fit on a nearly full test image.
pub fn self_test() -> Result<String, String> {
    let (px, w, h) = crate::wm::snapshot().ok_or("no desktop")?;
    let data = bmp(&px, w, h);
    let bw = u32::from_le_bytes([data[18], data[19], data[20], data[21]]) as usize;
    let bh = u32::from_le_bytes([data[22], data[23], data[24], data[25]]) as usize;
    let want = 54 + ((w * 3 + 3) & !3) * h;
    if &data[..2] != b"BM" || data[28] != 24 || bw != w || bh != h || data.len() != want {
        return Err(format!("bad BMP: {} bytes, {}x{}", data.len(), bw, bh));
    }
    if px.iter().all(|&p| p == px[0]) {
        return Err(String::from("the screenshot is one flat colour"));
    }
    let size = data.len();
    drop(data);
    let (cw, ch) = (64.min(w), 64.min(h));
    let crop: Vec<u32> = (0..ch).flat_map(|y| px[y * w..y * w + cw].iter().copied()).collect();
    drop(px);
    let small = bmp(&crop, cw, ch);
    let dir = "/disk/LAYARTES";
    if crate::vfs::stat(dir).is_err() {
        crate::vfs::mkdir(dir).map_err(String::from)?;
    }
    let path = format!("{}/UJI.BMP", dir);
    let wrote = crate::vfs::write(&path, &small);
    let back = crate::vfs::read(&path);
    let _ = crate::vfs::remove(&path);
    let _ = crate::vfs::remove(dir);
    wrote.map_err(String::from)?;
    if back.map_err(String::from)? != small {
        return Err(String::from("the saved picture reads back differently"));
    }
    Ok(format!("screenshot {}x{} = {} KB BMP, a crop saved and read back", w, h, size / 1024))
}
