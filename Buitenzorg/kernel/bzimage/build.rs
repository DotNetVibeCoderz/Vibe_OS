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
const APP_ICONS: [&str; 21] = [
    "files", "editor", "calc", "store", "imgview", "clock", "piano", "2048", "taskmgr", "paint",
    "xox", "audiopanel", "wallpaper", "theme", "managed", "msetting", "mwaktu", "mgallery", "mhwlab",
    "mruang", "mbuah",
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
    jpeg_to_bmp_top(jpeg, tw, th, 1000)
}

/// Like [`jpeg_to_bmp`], but keep only the top `keep_permille` of the image
/// rows, center-cropping the width to the target aspect ratio.
fn jpeg_to_bmp_top(jpeg: &[u8], tw: usize, th: usize, keep_permille: usize) -> Option<Vec<u8>> {
    let (full_w, full_h, rgb) = decode_jpeg(jpeg)?;
    let sw_stride = full_w;
    let sh = full_h * keep_permille / 1000;
    let sw = (sh * tw / th).min(full_w);
    let x_off = (full_w - sw) / 2;
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
                    let p = (sy * sw_stride + x_off + sx) * 3;
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

/// Put a file on /disk (bootloader-crate images) and remember it for Limine.
fn add_file(builder: &mut bootloader::DiskImageBuilder, files: &mut Vec<(String, Vec<u8>)>, name: String, bytes: Vec<u8>) {
    builder.set_file_contents(name.clone().into(), bytes.clone());
    files.push((name, bytes));
}

/// A disk image Limine boots under UEFI: an MBR with one FAT partition (type
/// 0x0C, LBA 2048) holding `EFI/BOOT/BOOTX64.EFI` + `EFI/BOOT/limine.conf`,
/// the kernel ELF as `boot/bzkernel`, and every /disk file at the root in 8.3
/// upper case (the kernel's FAT driver reads the first MBR FAT partition, so
/// /disk works exactly as on the other images). Returns Ok(false) when the
/// Limine binaries are absent.
fn limine_image(limine: &std::path::Path, kernel: &std::path::Path, files: &[(String, Vec<u8>)], out: &std::path::Path) -> std::io::Result<bool> {
    limine_image_for("BOOTX64.EFI", limine, kernel, files, out)
}

/// [`limine_image`] for any architecture: `efi_name` is the removable-media
/// loader name (BOOTX64.EFI, BOOTAA64.EFI, BOOTRISCV64.EFI).
fn limine_image_for(efi_name: &str, limine: &std::path::Path, kernel: &std::path::Path, files: &[(String, Vec<u8>)], out: &std::path::Path) -> std::io::Result<bool> {
    limine_image_with(efi_name, limine, kernel, files, "", &[], out)
}

/// The general form: `extra_conf` is appended to the boot entry (e.g.
/// `module_path:` lines).
fn limine_image_with(efi_name: &str, limine: &std::path::Path, kernel: &std::path::Path, files: &[(String, Vec<u8>)], extra_conf: &str, raw: &[(String, Vec<u8>)], out: &std::path::Path) -> std::io::Result<bool> {
    // An SD card image (the Raspberry Pi's, raw files present) is 128 MiB in all,
    // a power of two as QEMU's SD card model requires; the others 1 + 64 MiB.
    let part_mib: u64 = if raw.is_empty() { 64 } else { 127 };
    use std::io::{Read, Seek, SeekFrom, Write};
    let Ok(efi) = std::fs::read(limine.join(efi_name)) else { return Ok(false) };
    let kernel_elf = std::fs::read(kernel)?;
    const START_LBA: u64 = 2048;
    let part_bytes = part_mib * 1024 * 1024;
    let part_path = out.with_extension("part");
    {
        let mut f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&part_path)?;
        f.set_len(part_bytes)?;
        let mut format = fatfs::FormatVolumeOptions::new().volume_label(*b"BUITENZORG ");
        if !raw.is_empty() {
            // The Raspberry Pi boot ROM goes by the partition type, which says FAT32.
            format = format.fat_type(fatfs::FatType::Fat32);
        }
        fatfs::format_volume(&mut f, format)?;
        let fs = fatfs::FileSystem::new(&mut f, fatfs::FsOptions::new())?;
        let root = fs.root_dir();
        let put = |dir: &fatfs::Dir<'_, &mut std::fs::File>, name: &str, bytes: &[u8]| -> std::io::Result<()> {
            let mut file = dir.create_file(name)?;
            file.truncate()?;
            file.write_all(bytes)
        };
        let boot = root.create_dir("EFI")?.create_dir("BOOT")?;
        put(&boot, efi_name, &efi)?;
        let conf = format!(
            "# Buitenzorg OS -- Limine boot (multi-arch M1)\ntimeout: 0\nserial: yes\n\n/Buitenzorg OS\n    protocol: limine\n    path: boot():/boot/bzkernel\n{extra_conf}"
        );
        put(&boot, "limine.conf", conf.as_bytes())?;
        put(&root.create_dir("boot")?, "bzkernel", &kernel_elf)?;
        for (name, bytes) in files {
            put(&root, &name.to_uppercase(), bytes)?;
        }
        // Files that keep their name and folder (the Raspberry Pi's).
        for (name, bytes) in raw {
            match name.split_once('/') {
                Some((dir, file)) => {
                    let d = root.create_dir(dir)?;
                    put(&d, file, bytes)?;
                }
                None => put(&root, name, bytes)?,
            }
        }
    }
    let mut part = Vec::new();
    std::fs::File::open(&part_path)?.read_to_end(&mut part)?;
    std::fs::remove_file(&part_path)?;

    let mut mbr = [0u8; 512];
    let sectors = part_bytes / 512;
    let e = &mut mbr[446..462];
    e[0] = 0x80; // active
    e[1..4].copy_from_slice(&[0xFE, 0xFF, 0xFF]); // CHS: use LBA
    e[4] = 0x0C; // FAT32 (LBA); also accepted for FAT16 by OVMF and the kernel
    e[5..8].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    e[8..12].copy_from_slice(&(START_LBA as u32).to_le_bytes());
    e[12..16].copy_from_slice(&(sectors as u32).to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    let mut disk = std::fs::File::create(out)?;
    disk.write_all(&mbr)?;
    disk.seek(SeekFrom::Start(START_LBA * 512))?;
    disk.write_all(&part)?;
    Ok(true)
}

/// Flatten a static ELF64 into the memory image of its PT_LOAD segments:
/// the first byte is the lowest load address.
fn flatten_elf(elf: &[u8]) -> Option<Vec<u8>> {
    let u16_at = |o: usize| Some(u16::from_le_bytes(elf.get(o..o + 2)?.try_into().ok()?));
    let u32_at = |o: usize| Some(u32::from_le_bytes(elf.get(o..o + 4)?.try_into().ok()?));
    let u64_at = |o: usize| Some(u64::from_le_bytes(elf.get(o..o + 8)?.try_into().ok()?));
    let (phoff, phentsize, phnum) = (u64_at(32)? as usize, u16_at(54)? as usize, u16_at(56)? as usize);
    let mut segments = Vec::new();
    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if u32_at(ph)? == 1 {
            segments.push((u64_at(ph + 16)?, u64_at(ph + 8)? as usize, u64_at(ph + 32)? as usize));
        }
    }
    let base = segments.iter().map(|s| s.0).min()?;
    let end = segments.iter().map(|s| s.0 + s.2 as u64).max()?;
    let mut image = vec![0u8; (end - base) as usize];
    for (vaddr, offset, filesz) in segments {
        let at = (vaddr - base) as usize;
        image[at..at + filesz].copy_from_slice(elf.get(offset..offset + filesz)?);
    }
    Some(image)
}

/// The boot route lives in config.txt. By default the Pi firmware starts
/// kernel8.img directly: the route `smoke [rpi4]` runs under QEMU. The block at
/// the end switches to UEFI + Limine (not testable under QEMU: see
/// docs/install-raspberry-pi.md).
const RPI4_CONFIG: &str = "# Buitenzorg OS on a Raspberry Pi 4
arm_64bit=1
kernel=kernel8.img
enable_gic=1
# Serial console: PL011 on GPIO 14 (TX) / 15 (RX), 115200 8N1.
enable_uart=1
uart_2ndstage=1
dtoverlay=miniuart-bt
# HDMI mode (not DVI) even if the display does not ask for it: HDMI audio needs it.
hdmi_drive=2

# UEFI + Limine instead: uncomment these four lines.
#armstub=RPI_EFI.fd
#device_tree_address=0x3e0000
#device_tree_end=0x400000
#dtoverlay=upstream-pi4
";

/// Pi UEFI enforces ACPI by default and hides the Device Tree, so Limine hands
/// the kernel this file instead.
const RPI4_LIMINE_CONF: &str = "    dtb_path: boot():/bcm2711-rpi-4-b.dtb\n";

/// What the SD card needs next to the kernel, from the fetched firmware
/// (scripts/fetch-rpi4-firmware.*); None when it was not fetched.
fn rpi4_files(dir: &std::path::Path) -> Option<Vec<(String, Vec<u8>)>> {
    println!("cargo:rerun-if-changed={}", dir.join("VERSION").display());
    let mut raw = vec![("config.txt".to_string(), RPI4_CONFIG.as_bytes().to_vec())];
    for name in [
        "start4.elf",
        "fixup4.dat",
        "RPI_EFI.fd",
        "bcm2711-rpi-4-b.dtb",
        "bcm2711-rpi-400.dtb",
        "bcm2711-rpi-cm4.dtb",
    ] {
        raw.push((name.to_string(), std::fs::read(dir.join(name)).ok()?));
    }
    for name in ["miniuart-bt.dtbo", "upstream-pi4.dtbo"] {
        raw.push((format!("overlays/{name}"), std::fs::read(dir.join("overlays").join(name)).ok()?));
    }
    Some(raw)
}

/// An ELF64 file without the contents of its `.symtab` / `.strtab` /
/// `.comment` sections (they stay as empty headers, so section indices --
/// and the loader's `.bootloader-config` lookup -- are unchanged). Everything
/// the loader maps comes before them in an lld-linked kernel; the section
/// name table and the section header table are appended after it. `None`
/// when the file is not laid out like that (then the original is used).
fn strip_symbols(f: &[u8]) -> Option<Vec<u8>> {
    let u16_at = |o: usize| u16::from_le_bytes([f[o], f[o + 1]]) as usize;
    let u32_at = |o: usize| u32::from_le_bytes(f[o..o + 4].try_into().unwrap());
    let u64_at = |o: usize| u64::from_le_bytes(f[o..o + 8].try_into().unwrap()) as usize;
    if f.len() < 64 || &f[..4] != b"\x7fELF" || f[4] != 2 || f[5] != 1 {
        return None;
    }
    let (shoff, shentsize, shnum, shstrndx) = (u64_at(0x28), u16_at(0x3A), u16_at(0x3C), u16_at(0x3E));
    let (phoff, phentsize, phnum) = (u64_at(0x20), u16_at(0x36), u16_at(0x38));
    if shentsize != 64 || shstrndx >= shnum || shoff + shnum * 64 > f.len() {
        return None;
    }
    let sh = |i: usize| shoff + i * 64;
    let name_of = |i: usize| -> String {
        let base = u64_at(sh(shstrndx) + 0x18) + u32_at(sh(i)) as usize;
        let end = f[base..].iter().position(|&b| b == 0).map(|e| base + e).unwrap_or(base);
        String::from_utf8_lossy(&f[base..end]).into_owned()
    };
    // the end of everything kept in place: every segment's file data and
    // every section except the dropped ones and the name table
    let drop = |i: usize| matches!(name_of(i).as_str(), ".symtab" | ".strtab" | ".comment");
    let mut keep_end = phoff + phnum * phentsize;
    for k in 0..phnum {
        let ph = phoff + k * phentsize;
        keep_end = keep_end.max(u64_at(ph + 8) + u64_at(ph + 0x20));
    }
    for i in 1..shnum {
        let kind = u32_at(sh(i) + 4);
        if i == shstrndx || drop(i) || kind == 8 {
            continue; // NOBITS has no file data
        }
        keep_end = keep_end.max(u64_at(sh(i) + 0x18) + u64_at(sh(i) + 0x20));
    }
    let mut out = f[..keep_end].to_vec();
    // the section name table, then the headers
    let (str_off, str_size) = (u64_at(sh(shstrndx) + 0x18), u64_at(sh(shstrndx) + 0x20));
    let new_str = out.len();
    out.extend_from_slice(&f[str_off..str_off + str_size]);
    while out.len() % 8 != 0 {
        out.push(0);
    }
    let new_shoff = out.len();
    for i in 0..shnum {
        let mut h = f[sh(i)..sh(i) + 64].to_vec();
        if i == shstrndx {
            h[0x18..0x20].copy_from_slice(&(new_str as u64).to_le_bytes());
        } else if i != 0 && drop(i) {
            h[0x18..0x20].copy_from_slice(&0u64.to_le_bytes());
            h[0x20..0x28].copy_from_slice(&0u64.to_le_bytes());
        }
        out.extend_from_slice(&h);
    }
    out[0x28..0x30].copy_from_slice(&(new_shoff as u64).to_le_bytes());
    Some(out)
}

fn main() {
    let kernel = PathBuf::from(std::env::var("CARGO_BIN_FILE_BZKERNEL_bzkernel").unwrap());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let uefi_path = out_dir.join("buitenzorg-uefi.img");
    let bios_path = out_dir.join("buitenzorg-bios.img");

    // The image gets the kernel without its symbol tables: the BIOS loader
    // reads the file through int 13h, and under QEMU every megabyte costs
    // ~1.2 s of boot (4.5 MB -> 3.0 MB). The un-stripped ELF stays in
    // target/ for the debugger.
    let stripped = out_dir.join("bzkernel-stripped");
    let kernel_img = match strip_symbols(&std::fs::read(&kernel).unwrap()) {
        Some(b) => {
            std::fs::write(&stripped, b).unwrap();
            stripped
        }
        None => kernel.clone(),
    };
    let mut builder = bootloader::DiskImageBuilder::new(kernel_img);
    // The loader prints every INFO step on the framebuffer (24-bit text
    // rendering + scrolling at 1280x720): under QEMU that took ~9 s before the
    // kernel started. Warnings only, and only on the serial port.
    let mut boot_config = bootloader::BootConfig::default();
    // The loader's own log on screen: if it stops before the kernel, that is
    // all a PC shows. Warnings and errors only (info cost ~1 s under QEMU).
    // TEMPORARY (first physical PC, black screen): every loader step on screen,
    // to tell a loader that stops from a kernel that does. It costs ~9 s per
    // BIOS boot under QEMU: set back to Warn once the PC boots.
    boot_config.frame_buffer_logging = true;
    boot_config.log_level = bootloader_boot_config::LevelFilter::Info;
    // never the serial port: a laptop has no COM1, and the loader's UART driver
    // waits for it without a limit
    boot_config.serial_logging = false;
    builder.set_boot_config(&boot_config);
    // Every /disk file, also written into the Limine image (see limine_image).
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    // The kernel only mounts a volume carrying this file as /disk (fat.rs
    // DISK_MARKER): a real PC has other FAT partitions, and /disk gets written.
    add_file(&mut builder, &mut files, "bzdisk.id".into(), b"Buitenzorg OS /disk volume
".to_vec());
    // Test file for the v0.3 "Batang" milestone: the kernel reads this back
    // through its own IDE PIO driver + FAT parser ("baca file dari disk").
    add_file(&mut builder, &mut files, 
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
        ("smpproc.elf", "smpproc.elf"),
        ("proctest.elf", "proctest.elf"),
        // Generic user-app slot (SDK / VS Code / MagicAppGen template output).
        // Optional: absent until a template app is built and deployed here.
        ("userapp.elf", "userapp.elf"),
    ] {
        let path = userland.join(src);
        println!("cargo:rerun-if-changed={}", path.display());
        if let Ok(bytes) = std::fs::read(&path) {
            add_file(&mut builder, &mut files, dest.into(), bytes);
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
            add_file(&mut builder, &mut files, dest.to_lowercase().into(), bmp);
        }
    }

    // Boot splash (`splash.rs`): assets/splash/splash.jpg -> 960x540 BMP; the
    // kernel crops + scales it to the screen once. The bottom 12 % of the
    // artwork is dropped (the generator painted a fake progress bar there).
    let splash = manifest_dir().join("..").join("..").join("assets").join("splash").join("splash.jpg");
    println!("cargo:rerun-if-changed={}", splash.display());
    if let Some(bmp) = std::fs::read(&splash).ok().and_then(|j| jpeg_to_bmp_top(&j, 960, 540, 880)) {
        add_file(&mut builder, &mut files, "splash.bmp".into(), bmp);
    }

    // Profile pictures for user accounts (users.rs): assets/avatars/avatarN.jpg
    // -> /disk/AVATARn.BMP, 96x96.
    let avatars = manifest_dir().join("..").join("..").join("assets").join("avatars");
    for n in 1..=6 {
        let path = avatars.join(format!("avatar{n}.jpg"));
        println!("cargo:rerun-if-changed={}", path.display());
        if let Some(bmp) = std::fs::read(&path).ok().and_then(|j| jpeg_to_bmp(&j, 96, 96)) {
            add_file(&mut builder, &mut files, format!("avatar{n}.bmp").into(), bmp);
        }
    }

    // App icons for the desktop and the start menu (assets/icons/*.jpg ->
    // anti-aliased rounded ARGB icons packed into /disk/ICONS.BIN).
    let icon_dir = manifest_dir().join("..").join("..").join("assets").join("icons");
    if let Some(pack) = build_icon_pack(&icon_dir) {
        add_file(&mut builder, &mut files, "icons.bin".into(), pack);
    }

    // v0.11: a generated 24-bit BMP so the kernel can demo loading a user
    // image as the desktop wallpaper.
    add_file(&mut builder, &mut files, "photo.bmp".into(), generate_bmp(320, 200));

    // v0.16: a baseline JPEG (64x64 red->blue gradient, committed) to exercise
    // the Buitenzorg.Drawing JPEG decoder from ring-3 C# (see jpgtest.cs).
    {
        let jpg = userland.join("grad.jpg");
        println!("cargo:rerun-if-changed={}", jpg.display());
        if let Ok(bytes) = std::fs::read(&jpg) {
            add_file(&mut builder, &mut files, "grad.jpg".into(), bytes);
        }
    }

    // The managed C# path (RustCLR): CLRHOST.ELF for this image's
    // architecture (scripts/build-clrhost.*) and the managed apps -- .NET
    // assemblies, identical on every architecture (userland/clr-apps/dist).
    if let Some(bytes) = clrhost_elf("x86_64-unknown-none") {
        add_file(&mut builder, &mut files, "clrhost.elf".into(), bytes);
    }
    let clr_apps = clr_assemblies();
    for (name, bytes) in &clr_apps {
        add_file(&mut builder, &mut files, name.to_lowercase(), bytes.clone());
    }

    builder.create_uefi_image(&uefi_path).unwrap();
    builder.create_bios_image(&bios_path).unwrap();

    // Multi-arch M1: the same kernel booted by Limine (UEFI), when the Limine
    // binaries have been fetched (scripts/fetch-limine.*). Optional.
    let limine_path = out_dir.join("buitenzorg-limine-uefi.img");
    let limine_dir = manifest_dir().join("..").join("target").join("limine").join("limine-binary");
    println!("cargo:rerun-if-changed={}", limine_dir.join("BOOTX64.EFI").display());
    match limine_image(&limine_dir, &kernel, &files, &limine_path) {
        Ok(true) => println!("cargo:rustc-env=LIMINE_IMAGE={}", limine_path.display()),
        Ok(false) => {
            println!("cargo:warning=Limine binaries not found (scripts/fetch-limine) -- no Limine image");
            println!("cargo:rustc-env=LIMINE_IMAGE=");
        }
        Err(e) => panic!("limine image: {e}"),
    }

    // Multi-arch M3: the ARM64 kernel (built separately for
    // aarch64-unknown-none-softfloat by scripts/build-arm64.*) on a Limine
    // image for QEMU `virt` + UEFI (AAVMF/edk2). Optional.
    let arm_kernel = manifest_dir().join("..").join("target").join("aarch64-unknown-none-softfloat").join("release").join("bzkernel");
    println!("cargo:rerun-if-changed={}", arm_kernel.display());
    let arm_path = out_dir.join("buitenzorg-arm64.img");
    // ARM64 user programs (scripts/build-hello-csharp-arm64.*): the x86 ELFs
    // are swapped for their AArch64 builds, and each is also loaded by Limine
    // as a module (the ARM64 kernel has no disk driver yet).
    let arm_progs = manifest_dir().join("..").join("..").join("userland").join("hello-csharp").join("arm64");
    let mut arm_files: Vec<(String, Vec<u8>)> = files.iter().filter(|(n, _)| !n.to_lowercase().ends_with(".elf")).cloned().collect();
    if let Some(bytes) = clrhost_elf("aarch64-unknown-none") {
        arm_files.push(("CLRHOST.ELF".into(), bytes));
    }
    // No Limine modules any more: the programs are read from /disk, and
    // Limine loading ~16 ELF modules through UEFI cost seconds of every boot
    // under QEMU. (The kernel still falls back to modules a loader hands over.)
    let modules = String::new();
    if let Ok(dir) = std::fs::read_dir(&arm_progs) {
        let mut elfs: Vec<_> = dir.flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "elf")).collect();
        elfs.sort();
        for path in elfs {
            println!("cargo:rerun-if-changed={}", path.display());
            let stem = path.file_stem().unwrap().to_string_lossy().to_string();
            if let Ok(bytes) = std::fs::read(&path) {
                let file = format!("{}.ELF", stem.to_uppercase());
                arm_files.push((file, bytes));
            }
        }
    }
    println!("cargo:rerun-if-changed={}", arm_progs.display());
    let arm_ok = arm_kernel.exists()
        && limine_image_with("BOOTAA64.EFI", &limine_dir, &arm_kernel, &arm_files, &modules, &[], &arm_path).unwrap_or(false);
    println!("cargo:rustc-env=ARM64_IMAGE={}", if arm_ok { arm_path.display().to_string() } else { String::new() });

    // Raspberry Pi 4 (roadmap R1): the same ARM64 kernel as a flat kernel8.img,
    // and the SD card image when the firmware is there.
    let kernel8_path = out_dir.join("kernel8.img");
    let kernel8 = std::fs::read(&arm_kernel).ok().and_then(|elf| flatten_elf(&elf));
    let kernel8_ok = kernel8.as_ref().is_some_and(|k| std::fs::write(&kernel8_path, k).is_ok());
    println!("cargo:rustc-env=KERNEL8_IMAGE={}", if kernel8_ok { kernel8_path.display().to_string() } else { String::new() });
    let rpi_path = out_dir.join("buitenzorg-rpi4.img");
    let rpi_ok = match (&kernel8, rpi4_files(&manifest_dir().join("..").join("target").join("rpi4-uefi"))) {
        (Some(k), Some(mut raw)) => {
            raw.push(("kernel8.img".into(), k.clone()));
            // The /disk files too (wallpapers, icons, programs, managed apps): the
            // kernel mounts the card's FAT partition as /disk (R3).
            limine_image_with("BOOTAA64.EFI", &limine_dir, &arm_kernel, &arm_files, RPI4_LIMINE_CONF, &raw, &rpi_path).unwrap_or(false)
        }
        _ => false,
    };
    println!("cargo:rustc-env=RPI4_IMAGE={}", if rpi_ok { rpi_path.display().to_string() } else { String::new() });

    // Multi-arch M4: the RISC-V kernel (riscv64imac-unknown-none-elf) on a
    // Limine image for QEMU `virt` + OpenSBI + edk2. Optional. The /disk
    // files without the x86 ELFs, plus the RISC-V user programs
    // (userland/riscv, Rust -- bflat has no RISC-V target), each also handed
    // over by Limine as a module.
    let rv_kernel = manifest_dir().join("..").join("target").join("riscv64imac-unknown-none-elf").join("release").join("bzkernel");
    println!("cargo:rerun-if-changed={}", rv_kernel.display());
    let rv_path = out_dir.join("buitenzorg-riscv64.img");
    let mut rv_files: Vec<(String, Vec<u8>)> = files.iter().filter(|(n, _)| !n.to_lowercase().ends_with(".elf")).cloned().collect();
    if let Some(bytes) = clrhost_elf("riscv64imac-unknown-none-elf") {
        rv_files.push(("CLRHOST.ELF".into(), bytes));
    }
    let rv_modules = String::new(); // no Limine modules: the programs come from /disk
    for (stem, bytes) in build_riscv_programs(&out_dir) {
        let file = format!("{}.ELF", stem.to_uppercase());
        rv_files.push((file, bytes));
    }
    let rv_ok = rv_kernel.exists()
        && limine_image_with("BOOTRISCV64.EFI", &limine_dir, &rv_kernel, &rv_files, &rv_modules, &[], &rv_path).unwrap_or(false);
    println!("cargo:rustc-env=RISCV64_IMAGE={}", if rv_ok { rv_path.display().to_string() } else { String::new() });

    println!("cargo:rustc-env=UEFI_IMAGE={}", uefi_path.display());
    println!("cargo:rustc-env=BIOS_IMAGE={}", bios_path.display());
}

/// The RISC-V user programs (`userland/riscv/*.rs`, sharing `rt.rs`): each is
/// compiled by this toolchain's rustc for riscv64imac-unknown-none-elf and
/// linked by rust-lld at 0x400000 (`user.ld`). Returns (name, ELF bytes);
/// a program that fails to build is skipped with a warning.
fn build_riscv_programs(out_dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let dir = manifest_dir().join("..").join("..").join("userland").join("riscv");
    println!("cargo:rerun-if-changed={}", dir.display());
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let ld = dir.join("user.ld");
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&dir) else { return out };
    let mut srcs: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "rs")).collect();
    srcs.sort();
    for src in srcs {
        println!("cargo:rerun-if-changed={}", src.display());
        let stem = src.file_stem().unwrap().to_string_lossy().to_string();
        if stem == "rt" {
            continue;
        }
        let elf = out_dir.join(format!("riscv-{stem}.elf"));
        let status = std::process::Command::new(&rustc)
            .args(["--edition", "2021", "--target", "riscv64imac-unknown-none-elf", "-C", "opt-level=2", "-C", "panic=abort"])
            .args(["-C", "relocation-model=static", "-C", "strip=symbols", "-C", "link-arg=--no-relax"])
            .arg("-C")
            .arg(format!("link-arg=-T{}", ld.display()))
            .arg("-o")
            .arg(&elf)
            .arg(&src)
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .status();
        match status.ok().filter(|s| s.success()).and_then(|_| std::fs::read(&elf).ok()) {
            Some(bytes) => out.push((stem, bytes)),
            None => println!("cargo:warning=RISC-V user program {stem} failed to build"),
        }
    }
    out
}

/// The managed-runtime host built for `target` (scripts/build-clrhost.*),
/// if it has been built.
fn clrhost_elf(target: &str) -> Option<Vec<u8>> {
    let path = manifest_dir().join("..").join("..").join("userland").join("clrhost").join("target").join(target).join("release").join("clrhost");
    println!("cargo:rerun-if-changed={}", path.display());
    std::fs::read(&path).ok()
}

/// The managed C# apps: .NET assemblies built by `dotnet build`
/// (scripts/build-clr-apps.*) and committed in userland/clr-apps/dist as 8.3
/// names (HELLOCLR.DLL, CONFORM.DLL, ...). Architecture-neutral.
fn clr_assemblies() -> Vec<(String, Vec<u8>)> {
    let dir = manifest_dir().join("..").join("..").join("userland").join("clr-apps").join("dist");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut paths: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x.eq_ignore_ascii_case("dll"))).collect();
        paths.sort();
        for p in paths {
            println!("cargo:rerun-if-changed={}", p.display());
            if let Ok(bytes) = std::fs::read(&p) {
                out.push((p.file_name().unwrap().to_string_lossy().to_uppercase(), bytes));
            }
        }
    }
    out
}
