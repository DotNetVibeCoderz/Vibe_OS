//! `wifi` on every kernel. The Raspberry Pi 4's CYW43455 has a driver
//! (`platform/virt/cyw43.rs`); on PCs no Wi-Fi adapter is driven yet, so the
//! command lists the PCI network controllers it sees (vendor:device) -- the
//! information a driver for that laptop needs.

use alloc::string::String;
use alloc::vec::Vec;

pub fn command(args: &[&str]) -> Vec<String> {
    #[cfg(target_arch = "aarch64")]
    {
        crate::platform::virt::cyw43::command(args)
    }
    #[cfg(target_arch = "x86_64")]
    {
        let _ = args;
        let mut out = Vec::new();
        let found: Vec<_> = crate::pci::scan().into_iter().filter(|d| d.class == 0x02 && d.subclass == 0x80).collect();
        if found.is_empty() {
            out.push(String::from("wifi: no wireless network controller (PCI class 02/80) found"));
        }
        for d in found {
            let who = match (d.vendor_id, d.device_id) {
                (0x8086, 0x24fd) => "Intel Wireless 8265",
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
                "wifi: {:02x}:{:02x}.{} {:04x}:{:04x} {} -- no driver yet (Intel/Qualcomm need their firmware + a large command protocol)",
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
