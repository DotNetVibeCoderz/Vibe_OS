//! Build pipeline entry point (requirements.md §17 "Pipeline boot QEMU otomatis").
//!
//! ```text
//! bzimage --out <dir>            copy UEFI+BIOS (+ Limine UEFI) images to <dir>
//! bzimage --run [--uefi]         boot the image in QEMU (BIOS default)
//! bzimage --smoke [--uefi]       headless boot, serial to stdout (for CI grep)
//!         --media ide|ahci|nvme|usb|virtio   controller the boot disk is attached to
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

const UEFI_IMAGE: &str = env!("UEFI_IMAGE");
const BIOS_IMAGE: &str = env!("BIOS_IMAGE");
/// The same kernel booted by Limine under UEFI; empty when the Limine
/// binaries were not fetched at build time (scripts/fetch-limine.*).
const LIMINE_IMAGE: &str = env!("LIMINE_IMAGE");
/// The ARM64 kernel on a Limine UEFI image (empty unless it was built).
const ARM64_IMAGE: &str = env!("ARM64_IMAGE");
/// The RISC-V kernel on a Limine UEFI image (empty unless it was built).
const RISCV64_IMAGE: &str = env!("RISCV64_IMAGE");
const KERNEL8_IMAGE: &str = env!("KERNEL8_IMAGE");
const RPI4_IMAGE: &str = env!("RPI4_IMAGE");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);

    if let Some(pos) = args.iter().position(|a| a == "--out") {
        let dir = PathBuf::from(args.get(pos + 1).map(String::as_str).unwrap_or("dist"));
        std::fs::create_dir_all(&dir).expect("create out dir");
        let uefi = dir.join("buitenzorg-uefi.img");
        let bios = dir.join("buitenzorg-bios.img");
        std::fs::copy(UEFI_IMAGE, &uefi).expect("copy uefi image");
        std::fs::copy(BIOS_IMAGE, &bios).expect("copy bios image");
        println!("UEFI image: {}", uefi.display());
        println!("BIOS image: {}", bios.display());
        if !LIMINE_IMAGE.is_empty() {
            let limine = dir.join("buitenzorg-limine-uefi.img");
            std::fs::copy(LIMINE_IMAGE, &limine).expect("copy limine image");
            println!("Limine (UEFI) image: {}", limine.display());
        }
        if !ARM64_IMAGE.is_empty() {
            let arm = dir.join("buitenzorg-arm64.img");
            std::fs::copy(ARM64_IMAGE, &arm).expect("copy arm64 image");
            println!("ARM64 (Limine, UEFI) image: {}", arm.display());
        }
        if !RISCV64_IMAGE.is_empty() {
            let rv = dir.join("buitenzorg-riscv64.img");
            std::fs::copy(RISCV64_IMAGE, &rv).expect("copy riscv64 image");
            println!("RISC-V (Limine, UEFI) image: {}", rv.display());
        }
        if !KERNEL8_IMAGE.is_empty() {
            let k = dir.join("kernel8.img");
            std::fs::copy(KERNEL8_IMAGE, &k).expect("copy kernel8.img");
            println!("Raspberry Pi 4 kernel: {}", k.display());
        }
        if !RPI4_IMAGE.is_empty() {
            let rpi = dir.join("buitenzorg-rpi4.img");
            std::fs::copy(RPI4_IMAGE, &rpi).expect("copy rpi4 image");
            println!("Raspberry Pi 4 SD image: {}", rpi.display());
        }
        return;
    }

    let media = args
        .iter()
        .position(|a| a == "--media")
        .and_then(|pos| args.get(pos + 1))
        .cloned()
        .unwrap_or_else(|| "ide".into());

    if has("--run") || has("--smoke") {
        run_qemu(has("--uefi"), has("--smoke"), &media);
        return;
    }

    println!("UEFI image: {UEFI_IMAGE}");
    println!("BIOS image: {BIOS_IMAGE}");
    println!("usage: bzimage [--out <dir>] [--run|--smoke] [--uefi] [--media ide|ahci|nvme|usb|virtio]");
}

fn qemu_binary() -> PathBuf {
    if let Ok(path) = std::env::var("QEMU") {
        return PathBuf::from(path);
    }
    // PATH first, then the default Windows install location.
    let candidates = [
        "qemu-system-x86_64",
        r"C:\Program Files\qemu\qemu-system-x86_64.exe",
    ];
    for c in candidates {
        if Path::new(c).exists() || which(c) {
            return PathBuf::from(c);
        }
    }
    PathBuf::from("qemu-system-x86_64")
}

fn which(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_qemu(uefi: bool, smoke: bool, media: &str) {
    let mut cmd = Command::new(qemu_binary());
    let image = if uefi { UEFI_IMAGE } else { BIOS_IMAGE };
    if uefi {
        use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};
        let prebuilt = Prebuilt::fetch(Source::LATEST, "target/ovmf")
            .expect("failed to download OVMF firmware");
        let code = prebuilt.get_file(Arch::X64, FileType::Code);
        let vars = prebuilt.get_file(Arch::X64, FileType::Vars);
        cmd.arg("-drive")
            .arg(format!("if=pflash,format=raw,readonly=on,file={}", code.display()));
        cmd.arg("-drive")
            .arg(format!("if=pflash,format=raw,readonly=on,file={}", vars.display()));
    }
    // Attach the boot disk to the requested controller (v0.3 "Batang":
    // boot dari NVMe / SATA / IDE / USB).
    match media {
        "ide" => {
            cmd.arg("-drive").arg(format!("format=raw,file={image}"));
        }
        "ahci" => {
            cmd.arg("-drive")
                .arg(format!("id=bzdisk,format=raw,file={image},if=none"));
            cmd.args(["-device", "ahci,id=ahci0"]);
            cmd.args(["-device", "ide-hd,drive=bzdisk,bus=ahci0.0"]);
        }
        "nvme" => {
            cmd.arg("-drive")
                .arg(format!("id=bzdisk,format=raw,file={image},if=none"));
            cmd.args(["-device", "nvme,drive=bzdisk,serial=bz0001"]);
        }
        "usb" => {
            cmd.arg("-drive")
                .arg(format!("id=bzdisk,format=raw,file={image},if=none"));
            cmd.args(["-usb", "-device", "usb-storage,drive=bzdisk"]);
        }
        "virtio" => {
            cmd.arg("-drive")
                .arg(format!("id=bzdisk,format=raw,file={image},if=none"));
            cmd.args(["-device", "virtio-blk-pci,drive=bzdisk,bootindex=0,disable-legacy=on"]);
        }
        other => {
            eprintln!("unknown --media '{other}' (expected ide|ahci|nvme|usb|virtio)");
            std::process::exit(1);
        }
    }
    cmd.args(["-m", "512M"]);
    // v0.16 "Panen": an Intel AC'97 sound card (null host backend so it is
    // always present headless; the kernel driver still drives its registers/DMA).
    cmd.args(["-audiodev", "none,id=snd0", "-device", "AC97,audiodev=snd0"]);
    if smoke {
        cmd.args(["-display", "none", "-serial", "stdio", "-no-reboot"]);
    } else {
        cmd.args(["-serial", "stdio"]);
    }
    // Extra QEMU args, e.g. QEMU_EXTRA="-s -S" for GDB debugging.
    if let Ok(extra) = std::env::var("QEMU_EXTRA") {
        cmd.args(extra.split_whitespace());
    }
    println!("[bzimage] launching: {:?}", cmd);
    let status = cmd.status().expect("failed to launch QEMU");
    std::process::exit(status.code().unwrap_or(1));
}
