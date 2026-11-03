//! SNTP (RFC 4330) over the kernel's UDP: ask a time server for the current
//! time once the network is up -- the clock of boards without a real-time
//! clock (Raspberry Pi 4). The servers are anycast addresses that need no DNS
//! (Cloudflare, Google); traffic leaves through the DHCP router.

use crate::net::{self, Ipv4Addr};

/// time.cloudflare.com and time.google.com (anycast; no DNS needed).
pub const SERVERS: [Ipv4Addr; 2] = [[162, 159, 200, 1], [216, 239, 35, 0]];

/// The local time zone the clock shows: WIB (UTC+7). The software clock keeps
/// local time, like a PC's CMOS clock.
#[allow(dead_code)]
pub const LOCAL_OFFSET_S: u64 = 7 * 3600;

/// Seconds between 1900-01-01 (NTP era 0) and 1970-01-01.
const NTP_UNIX_OFFSET: u64 = 2_208_988_800;

/// Ask `server`; returns Unix seconds (UTC), or None after ~`seconds`.
pub fn query(server: Ipv4Addr, seconds: u64) -> Option<u64> {
    let h = net::udp_socket();
    if h == 0 {
        return None;
    }
    let _ = net::udp_bind(h, 12_300);
    let mut req = [0u8; 48];
    req[0] = 0x23; // LI 0, version 4, mode 3 (client)
    let hz = crate::arch::cpu::counter_hz().max(1);
    let t0 = crate::arch::cpu::timestamp();
    let mut sent_at = 0u64;
    let mut result = None;
    while crate::arch::cpu::timestamp() - t0 < seconds * hz {
        let now = crate::arch::cpu::timestamp();
        if sent_at == 0 || now - sent_at > hz {
            let _ = net::udp_send(h, server, 123, &req);
            sent_at = now;
        }
        net::poll();
        if let Some((from, port, data)) = net::udp_recv(h) {
            // server mode (4), from the server, a non-zero transmit timestamp
            if from == server && port == 123 && data.len() >= 48 && data[0] & 7 == 4 {
                let secs = u32::from_be_bytes([data[40], data[41], data[42], data[43]]) as u64;
                if secs > NTP_UNIX_OFFSET {
                    result = Some(secs - NTP_UNIX_OFFSET);
                    break;
                }
            }
        }
        crate::task::yield_now();
    }
    let _ = net::udp_close(h);
    result
}

/// Try each server; the first answer (UTC Unix seconds) wins.
pub fn sync(seconds_each: u64) -> Option<(Ipv4Addr, u64)> {
    SERVERS.iter().find_map(|&s| query(s, seconds_each).map(|t| (s, t)))
}
