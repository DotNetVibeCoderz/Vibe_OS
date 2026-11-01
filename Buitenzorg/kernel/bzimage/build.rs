use std::path::PathBuf;

fn manifest_dir() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
}

/// Generate a 24-bit uncompressed BMP with a "Bogor sunrise" gradient + hills,
/// so the OS can demonstrate loading a user image as wallpaper.
fn generate_bmp(w: i32, h: i32) -> Vec<u8> {
    let row_stride = (((w * 3) + 3) & !3) as usize;
    let pixel_bytes = row_stride * h as usize;
    let file_size = 54 + pixel_bytes;
    let mut buf = Vec::with_capacity(file_size);
    // BITMAPFILEHEADER (14) + BITMAPINFOHEADER (40).
    buf.extend_from_slice(b"BM");
    buf.extend_from_slice(&(file_size as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // reserved
    buf.extend_from_slice(&54u32.to_le_bytes()); // pixel data offset
    buf.extend_from_slice(&40u32.to_le_bytes()); // header size
    buf.extend_from_slice(&w.to_le_bytes());
    buf.extend_from_slice(&h.to_le_bytes()); // bottom-up
    buf.extend_from_slice(&1u16.to_le_bytes()); // planes
    buf.extend_from_slice(&24u16.to_le_bytes()); // bpp
    buf.extend_from_slice(&0u32.to_le_bytes()); // no compression
    buf.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    buf.extend_from_slice(&2835u32.to_le_bytes()); // x ppm
    buf.extend_from_slice(&2835u32.to_le_bytes()); // y ppm
    buf.extend_from_slice(&0u32.to_le_bytes()); // colors used
    buf.extend_from_slice(&0u32.to_le_bytes()); // important colors

    // Pixels, bottom-up rows, BGR.
    for row in 0..h {
        let y = h - 1 - row; // top-down y for the scene
        for x in 0..w {
            let t = y as f32 / h as f32; // 0 top .. 1 bottom
            // Sky gradient: warm orange top -> teal bottom.
            let mut r = (0xF2 as f32 * (1.0 - t) + 0x14 as f32 * t) as i32;
            let mut g = (0x9B as f32 * (1.0 - t) + 0x3A as f32 * t) as i32;
            let mut b = (0x3C as f32 * (1.0 - t) + 0x2C as f32 * t) as i32;
            // Sun disc.
            let (sx, sy) = (w * 3 / 4, h / 3);
            let dx = x - sx;
            let dy = y - sy;
            if dx * dx + dy * dy < 26 * 26 {
                r = 0xFF;
                g = 0xE0;
                b = 0x88;
            }
            // Rolling green hills near the bottom.
            let hill = h - h / 4 + ((x * 6 / w) % 2) * 8 - (((x / 8) % 3) * 4);
            if y > hill {
                r = 0x2E + (x % 20);
                g = 0x6A + (y % 30);
                b = 0x24;
            }
            let clamp = |v: i32| v.clamp(0, 255) as u8;
            buf.push(clamp(b));
            buf.push(clamp(g));
            buf.push(clamp(r));
        }
        // Row padding.
        for _ in (w * 3) as usize..row_stride {
            buf.push(0);
        }
    }
    buf
}

/// (asset name, 8.3 file on /disk) — keep in sync with `wallpaper::OFFICIAL`.
const OFFICIAL_WALLPAPERS: [(&str, &str); 8] = [
    ("nature", "WPNATURE.BMP"),
    ("futuristic", "WPFUTURE.BMP"),
    ("city", "WPCITY.BMP"),
    ("art", "WPART.BMP"),
    ("surreal", "WPSURREA.BMP"),
    ("imaginary", "WPIMAGIN.BMP"),
    ("space", "WPSPACE.BMP"),
    ("minimal", "WPMINIMA.BMP"),
];

/// App icons, in launcher order: (asset name in assets/icons, launch name used
/// by the kernel's `icons.rs` / `wm.rs` LAUNCH list).
const APP_ICONS: [&str; 14] = [
    "files", "editor", "calc", "store", "imgview", "clock", "piano", "2048", "taskmgr", "paint",
    "xox", "audiopanel", "wallpaper", "theme",
];
/// Icon sizes in ICONS.BIN: desktop tile and start-menu row.
const ICON_BIG: usize = 44;
const ICON_SMALL: usize = 18;

/// Decode a baseline JPEG to (width, height, RGB24 pixels).
fn decode_jpeg(jpeg: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut dec = jpeg_decoder::Decoder::new(jpeg);
    let rgb = dec.decode().ok()?;
    let info = dec.info()?;
    if info.pixel_format != jpeg_decoder::PixelFormat::RGB24 {
        return None;
    }
    Some((info.width as usize, info.height as usize, rgb))
}

/// Box-filter RGB24 `src` (sw x sh) down to tw x th; returns RGB24.
fn box_downscale(src: &[u8], sw: usize, sh: usize, tw: usize, th: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(tw * th * 3);
    for y in 0..th {
        let (y0, y1) = (y * sh / th, ((y + 1) * sh / th).max(y * sh / th + 1));
        for x in 0..tw {
            let (x0, x1) = (x * sw / tw, ((x + 1) * sw / tw).max(x * sw / tw + 1));
            let mut acc = [0u32; 3];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = (sy * sw + sx) * 3;
                    for c in 0..3 {
                        acc[c] += src[p + c] as u32;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            out.extend_from_slice(&[(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8]);
        }
    }
    out
}

/// Coverage (0..=255) of a rounded square of side `size` and corner radius `r`
/// at pixel (x, y), 4x4 supersampled — gives every icon the same anti-aliased
/// squircle silhouette regardless of what the artwork's edges look like.
fn rounded_alpha(x: usize, y: usize, size: usize, r: f32) -> u8 {
    let mut inside = 0;
    for sy in 0..4 {
        for sx in 0..4 {
            let px = x as f32 + (sx as f32 + 0.5) / 4.0;
            let py = y as f32 + (sy as f32 + 0.5) / 4.0;
            let s = size as f32;
            let cx = px.clamp(r, s - r);
            let cy = py.clamp(r, s - r);
            let (dx, dy) = (px - cx, py - cy);
            if dx * dx + dy * dy <= r * r {
                inside += 1;
            }
        }
    }
    (inside * 255 / 16) as u8
}

/// ICONS.BIN: "BZIC", version 1, count, big size, small size, then per icon a
/// 12-byte NUL-padded name, the big and the small icon as little-endian
/// 0xAARRGGBB words. Read by the kernel's icons.rs.
fn build_icon_pack(dir: &std::path::Path) -> Option<Vec<u8>> {
    let mut pack = Vec::new();
    pack.extend_from_slice(b"BZIC");
    pack.extend_from_slice(&[1, APP_ICONS.len() as u8, ICON_BIG as u8, ICON_SMALL as u8]);
    for name in APP_ICONS {
        let path = dir.join(format!("{name}.jpg"));
        println!("cargo:rerun-if-changed={}", path.display());
        let (w, h, rgb) = decode_jpeg(&std::fs::read(&path).ok()?)?;
        let mut tag = [0u8; 12];
        tag[..name.len()].copy_from_slice(name.as_bytes());
        pack.extend_from_slice(&tag);
        for (size, radius) in [(ICON_BIG, 10.0), (ICON_SMALL, 4.5)] {
            let px = box_downscale(&rgb, w, h, size, size);
            for y in 0..size {
                for x in 0..size {
                    let p = (y * size + x) * 3;
                    let a = rounded_alpha(x, y, size, radius) as u32;
                    let argb = a << 24 | (px[p] as u32) << 16 | (px[p + 1] as u32) << 8 | px[p + 2] as u32;
                    pack.extend_from_slice(&argb.to_le_bytes());
                }
            }
        }
    }
    Some(pack)
}

/// Decode a JPEG and box-filter it down to `tw` x `th` as a 24-bit BMP.
fn jpeg_to_bmp(jpeg: &[u8], tw: usize, th: usize) -> Option<Vec<u8>> {
    let (sw, sh, rgb) = decode_jpeg(jpeg)?;
    let row_stride = (tw * 3 + 3) & !3;
    let data_len = row_stride * th;
    let mut bmp = Vec::with_capacity(54 + data_len);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((54 + data_len) as u32).to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&54u32.to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&(tw as i32).to_le_bytes());
    bmp.extend_from_slice(&(th as i32).to_le_bytes()); // bottom-up rows
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&24u16.to_le_bytes());
    bmp.extend_from_slice(&[0; 24]);
    for y in (0..th).rev() {
        let (y0, y1) = (y * sh / th, ((y + 1) * sh / th).max(y * sh / th + 1));
        for x in 0..tw {
            let (x0, x1) = (x * sw / tw, ((x + 1) * sw / tw).max(x * sw / tw + 1));
            let mut acc = [0u32; 3];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = (sy * sw + sx) * 3;
                    for c in 0..3 {
                        acc[c] += rgb[p + c] as u32;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            bmp.extend_from_slice(&[(acc[2] / n) as u8, (acc[1] / n) as u8, (acc[0] / n) as u8]);
        }
        bmp.resize(bmp.len() + row_stride - tw * 3, 0);
    }
    Some(bmp)
}

fn main() {
    let kernel = PathBuf::from(std::env::var("CARGO_BIN_FILE_BZKERNEL_bzkernel").unwrap());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let uefi_path = out_dir.join("buitenzorg-uefi.img");
    let bios_path = out_dir.join("buitenzorg-bios.img");

    let mut builder = bootloader::DiskImageBuilder::new(kernel);
    // Test file for the v0.3 "Batang" milestone: the kernel reads this back
    // through its own IDE PIO driver + FAT parser ("baca file dari disk").
    builder.set_file_contents(
        "batang.txt".into(),
        b"Akar menembus tanah, batang menjulang: file ini dibaca kernel dari disk. (v0.3 Batang)\n"
            .to_vec(),
    );

    // v0.4/v0.5: embed the C# programs (built by scripts/build-hello-csharp)
    // so the kernel can load and run them in ring 3. Optional — the kernel
    // reports gracefully if they are absent.
    let userland = manifest_dir()
        .join("..")
        .join("..")
        .join("userland")
        .join("hello-csharp");
    for (src, dest) in [
        ("hello.elf", "hello.elf"),
        ("svc.elf", "svc.elf"),
        ("xox.elf", "xox.elf"),
        ("paint.elf", "paint.elf"),
        ("taskmgr.elf", "taskmgr.elf"),
        ("widget.elf", "widget.elf"),
        ("webview.elf", "webview.elf"),
        ("matang.elf", "matang.elf"),
        ("thread.elf", "thread.elf"),
        ("sync.elf", "sync.elf"),
        ("heap.elf", "heap.elf"),
        ("gcmem.elf", "gcmem.elf"),
        ("bcl.elf", "bcl.elf"),
        ("bcl2.elf", "bcl2.elf"),
        ("draw.elf", "draw.elf"),
        ("ui.elf", "ui.elf"),
        ("audio.elf", "audio.elf"),
        ("audioset.elf", "audioset.elf"),
        ("calc.elf", "calc.elf"),
        ("g2048.elf", "g2048.elf"),
        ("clock.elf", "clock.elf"),
        ("piano.elf", "piano.elf"),
        ("store.elf", "store.elf"),
        ("files.elf", "files.elf"),
        ("editor.elf", "editor.elf"),
        ("imgview.elf", "imgview.elf"),
        ("jpgtest.elf", "jpgtest.elf"),
        ("mproc.elf", "mproc.elf"),
        ("smpsys.elf", "smpsys.elf"),
        ("smpgui.elf", "smpgui.elf"),
        ("pthread.elf", "pthread.elf"),
        ("smpfs.elf", "smpfs.elf"),
        ("proctest.elf", "proctest.elf"),
        // Generic user-app slot (SDK / VS Code / MagicAppGen template output).
        // Optional: absent until a template app is built and deployed here.
        ("userapp.elf", "userapp.elf"),
    ] {
        let path = userland.join(src);
        println!("cargo:rerun-if-changed={}", path.display());
        if let Ok(bytes) = std::fs::read(&path) {
            builder.set_file_contents(dest.into(), bytes);
        }
    }

    // Official Buitenzorg OS wallpapers: JPEGs in assets/wallpapers are decoded
    // here (host side), downscaled to 800x450 and stored on /disk as 24-bit
    // BMP (the kernel reads BMP only and resamples to the screen once).
    let wallpapers = manifest_dir().join("..").join("..").join("assets").join("wallpapers");
    for (src, dest) in OFFICIAL_WALLPAPERS {
        let path = wallpapers.join(format!("{src}.jpg"));
        println!("cargo:rerun-if-changed={}", path.display());
        if let Some(bmp) = std::fs::read(&path).ok().and_then(|j| jpeg_to_bmp(&j, 800, 450)) {
            builder.set_file_contents(dest.to_lowercase().into(), bmp);
        }
    }

    // App icons for the desktop and the start menu (assets/icons/*.jpg ->
    // anti-aliased rounded ARGB icons packed into /disk/ICONS.BIN).
    let icon_dir = manifest_dir().join("..").join("..").join("assets").join("icons");
    if let Some(pack) = build_icon_pack(&icon_dir) {
        builder.set_file_contents("icons.bin".into(), pack);
    }

    // v0.11: a generated 24-bit BMP so the kernel can demo loading a user
    // image as the desktop wallpaper.
    builder.set_file_contents("photo.bmp".into(), generate_bmp(320, 200));

    // v0.16: a baseline JPEG (64x64 red->blue gradient, committed) to exercise
    // the Buitenzorg.Drawing JPEG decoder from ring-3 C# (see jpgtest.cs).
    {
        let jpg = userland.join("grad.jpg");
        println!("cargo:rerun-if-changed={}", jpg.display());
        if let Ok(bytes) = std::fs::read(&jpg) {
            builder.set_file_contents("grad.jpg".into(), bytes);
        }
    }

    builder.create_uefi_image(&uefi_path).unwrap();
    builder.create_bios_image(&bios_path).unwrap();

    println!("cargo:rustc-env=UEFI_IMAGE={}", uefi_path.display());
    println!("cargo:rustc-env=BIOS_IMAGE={}", bios_path.display());
}
