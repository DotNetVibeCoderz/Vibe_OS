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
    ] {
        let path = userland.join(src);
        println!("cargo:rerun-if-changed={}", path.display());
        if let Ok(bytes) = std::fs::read(&path) {
            builder.set_file_contents(dest.into(), bytes);
        }
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
