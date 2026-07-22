//! Build pipeline entry point (requirements.md §17 "Pipeline boot QEMU otomatis").
//!
//! ```text
//! bzimage --out <dir>            copy UEFI+BIOS images to <dir> and print paths
//! bzimage --run [--uefi]         boot the image in QEMU (BIOS default)
//! bzimage --smoke [--uefi]       headless boot, serial to stdout (for CI grep)
//!         --media ide|ahci|nvme|usb   controller the boot disk is attached to
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

const UEFI_IMAGE: &str = env!("UEFI_IMAGE");
const BIOS_IMAGE: &str = env!("BIOS_IMAGE");

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
    println!("usage: bzimage [--out <dir>] [--run|--smoke] [--uefi] [--media ide|ahci|nvme|usb]");
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
        other => {
            eprintln!("unknown --media '{other}' (expected ide|ahci|nvme|usb)");
            std::process::exit(1);
        }
    }
    cmd.args(["-m", "512M"]);
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
