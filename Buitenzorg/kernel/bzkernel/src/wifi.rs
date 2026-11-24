//! `wifi` on every kernel. The Raspberry Pi 4's CYW43455 has a driver
//! (`platform/virt/cyw43.rs`); on PCs the Intel 8260/8265 gets as far as its
//! firmware's ALIVE (`wifi up`, `platform/pc/iwlwifi.rs`), and the
//! command lists the PCI network controllers it sees (vendor:device) -- the
//! information a driver for that laptop needs.

use alloc::string::String;
use alloc::vec::Vec;

/// The Wi-Fi adapter this machine has and how far its driver goes:
/// (description, can scan + join).
pub fn adapter() -> Option<(String, bool)> {
    #[cfg(target_arch = "aarch64")]
    {
        crate::platform::virt::cyw43::present().then(|| (String::from("Broadcom CYW43455 (Raspberry Pi)"), true))
    }
    #[cfg(target_arch = "x86_64")]
    {
        let d = crate::pci::scan().into_iter().find(|d| d.class == 0x02 && d.subclass == 0x80)?;
        Some(if crate::iwlwifi::supported(&d) {
            (String::from("Intel Wireless 8265 (driver tahap awal: belum bisa memindai)"), false)
        } else {
            (alloc::format!("{:04x}:{:04x} (belum ada driver)", d.vendor_id, d.device_id), false)
        })
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        None
    }
}

/// The saved Wi-Fi network ("" = none).
pub fn saved_ssid() -> String {
    #[cfg(target_arch = "aarch64")]
    {
        crate::platform::virt::cyw43::saved_ssid()
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        String::new()
    }
}

pub fn command(args: &[&str]) -> Vec<String> {
    #[cfg(target_arch = "aarch64")]
    {
        crate::platform::virt::cyw43::command(args)
    }
    #[cfg(target_arch = "x86_64")]
    {
        let mut out = Vec::new();
        let found: Vec<_> = crate::pci::scan().into_iter().filter(|d| d.class == 0x02 && d.subclass == 0x80).collect();
        if args.first() == Some(&"up") {
            match found.iter().find(|d| crate::iwlwifi::supported(d)) {
                Some(d) => {
                    out.push(String::from("wifi up: Intel 8000-family driver, step 1 (firmware + ALIVE; no scan/join yet)"));
                    out.extend(crate::iwlwifi::bring_up(d).into_iter().map(|l| alloc::format!("  {}", l)));
                }
                None => out.push(String::from("wifi up: no adapter with a driver (supported: Intel 8260/8265)")),
            }
            return out;
        }
        if found.is_empty() {
            out.push(String::from("wifi: no wireless network controller (PCI class 02/80) found"));
        }
        for d in found {
            let who = match (d.vendor_id, d.device_id) {
                (0x8086, 0x24fd) => "Intel Wireless 8265 (`wifi up`: driver step 1)",
                (0x8086, 0x24f3) => "Intel Wireless 8260",
                (0x8086, 0x095a) | (0x8086, 0x095b) => "Intel Wireless 7265",
                (0x8086, 0x2526) => "Intel Wireless-AC 9260",
                (0x8086, 0x2723) => "Intel Wi-Fi 6 AX200",
                (0x168c, 0x003e) => "Qualcomm Atheros QCA6174",
                (0x10ec, _) => "Realtek wireless",
                (0x14e4, _) => "Broadcom wireless",
                _ => "wireless controller",
            };
            out.push(alloc::format!(
                "wifi: {:02x}:{:02x}.{} {:04x}:{:04x} {} -- no scan/join yet",
                d.bus, d.slot, d.function, d.vendor_id, d.device_id, who
            ));
        }
        out
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = args;
        alloc::vec![String::from("wifi: no Wi-Fi driver on this platform")]
    }
}
