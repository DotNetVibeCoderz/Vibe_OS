//! Intel Wireless 8265 / 8260 with the runtime ("regular") firmware: the
//! `iwlmvm` half of Linux's driver, enough to scan, join a WPA2-PSK (CCMP)
//! network and carry IP traffic. `iwlwifi.rs` is the transport underneath
//! (registers, firmware load, RX queue, command queue, INIT calibration).
//!
//! Bring-up after Linux `iwl_mvm_up`: INIT image + calibration, device
//! stopped, runtime image loaded, paging blocks handed over, PHY DB and PHY
//! configuration, TX antennas, BT coexistence, DQA, auxiliary station +
//! queue (scans), PHY context, device power (always on), UMAC scan config,
//! one station MAC context. Joining (after mac80211 + iwlmvm): PHY context
//! on the network's channel, binding + time quota, MAC context with the
//! BSSID, the AP station + its TX queue (DQA queue 4, FIFO VO), a session-
//! protection time event, then authentication (open system), association
//! (no HT / no WMM: 802.11a/b/g rates) and the WPA2 4-way handshake done
//! here (`wpa.rs`; iwlmvm has no firmware supplicant). Frames are encrypted
//! by the firmware (CCMP header built here, key in each TX command) and
//! decrypted by it (keys installed with ADD_STA_KEY).
//!
//! Not done: HT/VHT (11n/ac rates, aggregation), power save, roaming,
//! background scans while connected, WPA3/SAE, 802.11w, TKIP, enterprise.
//! There is no QEMU model of this chip: everything here was written against
//! the Linux sources and the firmware's API bits, and is verified only on
//! the laptop (`wifi scan`, `wifi connect`, `dmesg iwl`).

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::write_volatile;

use super::iwlwifi::{self as iwl, Hw, Nic, Rx, Tx};
use crate::pci::PciDevice;

// host commands (group 1 unless noted) and notifications
const PHY_CONTEXT_CMD: u8 = 0x08;
const SCAN_CFG_CMD: u8 = 0x0c;
const SCAN_REQ_UMAC: u8 = 0x0d;
const SCAN_COMPLETE_UMAC: u8 = 0x0f;
const ADD_STA_KEY: u8 = 0x17;
const ADD_STA: u8 = 0x18;
const REMOVE_STA: u8 = 0x19;
const TX_CMD: u8 = 0x1c;
const SCD_QUEUE_CFG: u8 = 0x1d;
const MAC_CONTEXT_CMD: u8 = 0x28;
const TIME_EVENT_CMD: u8 = 0x29;
const TIME_EVENT_NOTIFICATION: u8 = 0x2a;
const BINDING_CONTEXT_CMD: u8 = 0x2b;
const TIME_QUOTA_CMD: u8 = 0x2c;
const FW_PAGING_BLOCK_CMD: u8 = 0x4f;
const PHY_DB_CMD: u8 = 0x6c;
const POWER_TABLE_CMD: u8 = 0x77;
const BT_CONFIG: u8 = 0x9b;
const MAC_PM_POWER_TABLE: u8 = 0xa9;
const REPLY_RX_PHY_CMD: u8 = 0xc0;
const REPLY_RX_MPDU_CMD: u8 = 0xc1;
const REPLY_ERROR: u8 = 0x02;
/// DATA_PATH_GROUP, DQA_ENABLE_CMD
const DATA_PATH_GROUP: u8 = 5;
const DQA_ENABLE_CMD: u8 = 0x00;

const ACTION_ADD: u32 = 1;
const ACTION_MODIFY: u32 = 2;
const ACTION_REMOVE: u32 = 3;
const INVALID: u32 = 0xFFFF_FFFF;

/// DQA queues: 1 = auxiliary (scans), 4 = the BSS client queue.
const AUX_QUEUE: u32 = 1;
const DATA_QUEUE: u32 = 4;
const FIFO_VO: u8 = 3;
const FIFO_MCAST: u8 = 5;
/// The non-QoS TID (IWL_MAX_TID_COUNT): management and non-QoS data.
const TID_NON_QOS: u8 = 8;
const AUX_STA: u8 = 0;
const AP_STA: u8 = 1;
const MAC_INDEX_AUX: u32 = 4;
const DATA_SLOTS: usize = 64;

const CONFIG: &str = "/disk/WIFI.CFG";

fn w16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
fn w32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn r16(b: &[u8], o: usize) -> u16 {
    b.get(o..o + 2).map_or(0, |x| u16::from_le_bytes([x[0], x[1]]))
}
fn r32(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}
fn mac6(b: &[u8]) -> [u8; 6] {
    let mut m = [0u8; 6];
    m.copy_from_slice(&b[..6]);
    m
}
fn now_ms() -> u64 {
    crate::arch::cpu::nanos() / 1_000_000
}

/// One network seen in a scan.
#[derive(Clone)]
struct Bss {
    bssid: [u8; 6],
    ssid: String,
    channel: u8,
    rssi: i32,
    cap: u16,
    beacon_int: u16,
    /// Supported + extended rates as the AP lists them (basic bit kept).
    rates: Vec<u8>,
    rsn: Option<crate::wpa::Rsn>,
    wpa1: bool,
}

impl Bss {
    fn privacy(&self) -> bool {
        self.cap & 0x10 != 0
    }
    fn band24(&self) -> bool {
        self.channel <= 14
    }
}

/// The DQA BSS-client TX queue: TFD ring + one page per slot.
struct DataQ {
    tfd_phys: u64,
    tfd: *mut u8,
    slots: Vec<(u64, *mut u8)>,
    wr: usize,
}

impl DataQ {
    fn new() -> Result<DataQ, String> {
        let (tfd_phys, tfd) = crate::memory::alloc_dma_contiguous(8).ok_or("out of memory (data ring)")?;
        let mut slots = Vec::with_capacity(DATA_SLOTS);
        for _ in 0..DATA_SLOTS {
            slots.push(crate::memory::alloc_dma_frame().ok_or("out of memory (data buffers)")?);
        }
        let q = DataQ { tfd_phys, tfd, slots, wr: 0 };
        q.reset_ring();
        Ok(q)
    }
    fn reset_ring(&self) {
        unsafe { core::ptr::write_bytes(self.tfd, 0, 256 * iwl::TFD_SIZE) };
        crate::arch::cpu::dma_sync(self.tfd as u64, 256 * iwl::TFD_SIZE);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Mgmt,
    Assoc,
    Eapol,
    Data,
}

pub(super) struct Mvm {
    nic: Nic,
    hw: Hw,
    tx: Tx,
    rx: Rx,
    dq: DataQ,
    mac: [u8; 6],
    tx_ant: u32,
    rx_ant: u32,
    channels: Vec<u8>,
    n_scan_channels: usize,
    scan: Vec<Bss>,
    scan_done: bool,
    // last REPLY_RX_PHY: device time, channel, signal
    phy_gp2: u32,
    phy_chan: u8,
    phy_rssi: i32,
    // join state
    bss: Option<Bss>,
    bound: bool,
    sta: bool,
    connected: bool,
    keyed: bool,
    aid: u16,
    auth_status: Option<u16>,
    assoc_status: Option<(u16, u16)>,
    te_started: bool,
    lost: Option<u16>,
    beacon: Option<(u64, u32, u8, u8)>,
    sup: Option<crate::wpa::Supplicant>,
    eapol_error: Option<String>,
    tk: Option<[u8; 16]>,
    pn: u64,
    /// Ethernet frames for the IP stack.
    frames: VecDeque<Vec<u8>>,
    tx_ok: u64,
    tx_fail: u64,
    tx_drop: u64,
    rx_data: u64,
    notes: u32,
    dead: Option<String>,
}

// The raw pointers are DMA memory owned by this driver; all access goes
// through `DEV`'s lock.
unsafe impl Send for Mvm {}

static DEV: spin::Mutex<Option<Mvm>> = spin::Mutex::new(None);
static STATUS: spin::Mutex<String> = spin::Mutex::new(String::new());

fn set_status(s: String) {
    crate::println!("[wifi] {}", s);
    *STATUS.lock() = s;
}

/// The supported adapter on this PC, if any.
fn device() -> Option<PciDevice> {
    crate::pci::scan().into_iter().find(|d| iwl::supported(d))
}

pub fn present() -> bool {
    device().is_some()
}

/// INIT run, device stop, runtime image, paging, runtime configuration.
fn start(dev: &PciDevice, nic: &mut Nic) -> Result<Mvm, String> {
    let fw = iwl::open(dev, nic)?;
    let hw = Hw::new()?;
    let (init, mut tx) = match iwl::init_calibrate(dev, nic, &hw, &fw) {
        Ok(v) => v,
        Err(e) if e.starts_with("no ALIVE") => {
            // Whatever state the boot firmware (or an earlier attempt) left
            // the device in, a PCIe function-level reset clears it.
            nic.note(format!("failed: {}", e));
            crate::pci::function_level_reset(dev).map_err(|e| format!("no second attempt: {}", e))?;
            nic.note(String::from("PCIe function-level reset done; second attempt"));
            iwl::open(dev, nic)?;
            iwl::init_calibrate(dev, nic, &hw, &fw)?
        }
        Err(e) => return Err(e),
    };
    // iwl_mvm_up: stop the device, start again with the regular image
    iwl::stop_device(nic);
    let (_, rt) = iwl::sections(&fw, 19)?;
    nic.note(format!("runtime image: {} section(s)", rt.len()));
    let t0 = crate::arch::cpu::nanos();
    let (rx, scd) = iwl::start_fw(nic, &hw, &fw, &rt)?;
    tx.ring.clear();
    tx.wr = 0;
    nic.tx_start(scd, &tx.ring)?;
    let ms = (crate::arch::cpu::nanos() - t0) / 1_000_000;
    nic.note(format!("runtime firmware ALIVE after {} ms", ms));

    let phy = iwl::tlv_u32(&fw, 23).unwrap_or(0);
    let n_scan = iwl::tlv_u32(&fw, 31).unwrap_or(52).clamp(16, 64) as usize;
    let regs = core::mem::replace(&mut nic.regs, core::ptr::null_mut());
    let mut own = Nic::new();
    own.regs = regs;
    own.pci = nic.pci;
    let mut m = Mvm {
        nic: own,
        hw,
        tx,
        rx,
        dq: DataQ::new()?,
        mac: init.mac,
        tx_ant: (phy >> 16) & 0xF,
        rx_ant: (phy >> 20) & 0xF,
        channels: channel_list(&init.regulatory, init.sku),
        n_scan_channels: n_scan,
        scan: Vec::new(),
        scan_done: false,
        phy_gp2: 0,
        phy_chan: 0,
        phy_rssi: -127,
        bss: None,
        bound: false,
        sta: false,
        connected: false,
        keyed: false,
        aid: 0,
        auth_status: None,
        assoc_status: None,
        te_started: false,
        lost: None,
        beacon: None,
        sup: None,
        eapol_error: None,
        tk: None,
        pn: 0,
        frames: VecDeque::new(),
        tx_ok: 0,
        tx_fail: 0,
        tx_drop: 0,
        rx_data: 0,
        notes: 0,
        dead: None,
    };
    let r = m.paging(&rt).and_then(|_| m.configure(&init, &fw));
    // the transport's own lines go back to the caller's log
    nic.log.append(&mut m.nic.log);
    r?;
    Ok(m)
}

/// NVM_CHANNELS_EXTENDED (8000 family): the 51 channels of the regulatory
/// section, flags bit 0 = valid. With LAR the NVM may mark none valid and
/// the firmware applies the rules itself: then every channel is listed.
fn channel_list(reg: &[u8], sku: u32) -> Vec<u8> {
    let mut all: Vec<u8> = (1..=14).collect();
    all.extend((36..=144).step_by(4));
    all.extend((149..=181).step_by(4));
    let five = sku & 2 != 0 || sku == 0;
    let valid: Vec<u8> = all
        .iter()
        .enumerate()
        .filter(|&(i, _)| r16(reg, i * 2) & 1 != 0)
        .map(|(_, &c)| c)
        .collect();
    let list = if valid.is_empty() { all } else { valid };
    list.into_iter().filter(|&c| c <= 14 || five).collect()
}

impl Mvm {
    fn log(&mut self, s: String) {
        crate::println!("[iwl] {}", s);
    }

    fn cmd(&mut self, id: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
        self.nic.send_cmd(&mut self.tx, &mut self.rx, id, payload)
    }

    /// iwl_save_fw_paging + iwl_send_paging_cmd: the CSS block (4 KiB) and
    /// the paging image in 32 KiB blocks of 8 pages, handed to the
    /// firmware by physical address before anything else.
    fn paging(&mut self, rt: &[(u32, &[u8])]) -> Result<(), String> {
        let Some(sep) = rt.iter().position(|s| s.0 == iwl::PAGING_SEPARATOR) else {
            self.nic.note(String::from("no paging image"));
            return Ok(());
        };
        let css = rt.get(sep + 1).ok_or("paging: no CSS section")?.1;
        let img = rt.get(sep + 2).ok_or("paging: no paging section")?.1;
        if img.is_empty() || img.len() % 4096 != 0 || css.len() > 4096 {
            return Err(format!("paging: odd sizes (CSS {}, image {})", css.len(), img.len()));
        }
        let pages = img.len() / 4096;
        let blocks = (pages + 7) / 8;
        if blocks > 32 {
            return Err(format!("paging: {} blocks (at most 32)", blocks));
        }
        let last = pages - 8 * (blocks - 1);
        let (css_phys, css_va) = crate::memory::alloc_dma_frame().ok_or("out of memory (paging)")?;
        unsafe {
            core::ptr::write_bytes(css_va, 0, 4096);
            core::ptr::copy_nonoverlapping(css.as_ptr(), css_va, css.len());
        }
        crate::arch::cpu::dma_sync(css_va as u64, 4096);
        // blocks are naturally aligned in Linux (alloc_pages order 3)
        let (raw_phys, raw_va) = crate::memory::alloc_dma_contiguous(blocks * 8 + 8).ok_or("out of memory (paging blocks)")?;
        let skip = (((raw_phys + 0x7FFF) & !0x7FFF) - raw_phys) as usize;
        let base = raw_phys + skip as u64;
        let va = unsafe { raw_va.add(skip) };
        unsafe {
            core::ptr::write_bytes(va, 0, blocks * 0x8000);
            core::ptr::copy_nonoverlapping(img.as_ptr(), va, img.len());
        }
        crate::arch::cpu::dma_sync(va as u64, blocks * 0x8000);
        let mut c = [0u8; 12 + 33 * 4];
        // PAGING_CMD_IS_SECURED | PAGING_CMD_IS_ENABLED | pages in the last block
        w32(&mut c, 0, (1 << 9) | (1 << 8) | last as u32);
        w32(&mut c, 4, 15); // block size 2^15
        w32(&mut c, 8, blocks as u32);
        w32(&mut c, 12, (css_phys >> 12) as u32);
        for b in 0..blocks {
            w32(&mut c, 16 + 4 * b, ((base + (b as u64) * 0x8000) >> 12) as u32);
        }
        self.cmd(FW_PAGING_BLOCK_CMD, &c)?;
        self.nic.note(format!("paging: CSS {} bytes + {} block(s), {} page(s) in the last", css.len(), blocks, last));
        Ok(())
    }

    /// The rest of iwl_mvm_load_rt_fw + iwl_mvm_up.
    fn configure(&mut self, init: &iwl::Init, fw: &[u8]) -> Result<(), String> {
        // PHY DB: configuration, NCH, then every PAPD and TXP channel group
        let mut sent = 0;
        for ty in [1u16, 2, 4, 5] {
            let entries: Vec<Vec<u8>> = init.phy_db.iter().filter(|e| r16(e, 0) == ty).cloned().collect();
            for e in entries {
                self.cmd(PHY_DB_CMD, &e).map_err(|x| format!("PHY DB type {}: {}", ty, x))?;
                sent += 1;
            }
        }
        let phy = iwl::tlv_u32(fw, 23).unwrap_or(0);
        let (flow, event) = iwl::def_calib(fw, 0);
        let mut cfg = [0u8; 12];
        w32(&mut cfg, 0, phy);
        w32(&mut cfg, 4, flow);
        w32(&mut cfg, 8, event);
        self.cmd(iwl::PHY_CONFIGURATION_CMD, &cfg)?;
        let tx_ant = self.tx_ant;
        self.cmd(iwl::TX_ANT_CONFIGURATION_CMD, &tx_ant.to_le_bytes())?;
        // BT_COEX_NW, MPLUT + high-band retention
        let mut bt = [0u8; 8];
        w32(&mut bt, 0, 1);
        w32(&mut bt, 4, 0x11);
        self.cmd(BT_CONFIG, &bt)?;
        self.nic
            .send_cmd_group(&mut self.tx, &mut self.rx, DATA_PATH_GROUP, DQA_ENABLE_CMD, &0u32.to_le_bytes())?;
        self.nic.note(format!("PHY DB {} section(s) sent, PHY config {:#x}, DQA on", sent, phy));
        // auxiliary station + queue (scans)
        let tfd_zero = self.hw.tfd_zero;
        self.enable_queue(AUX_QUEUE, tfd_zero, FIFO_MCAST, AUX_STA, TID_NON_QOS)?;
        self.add_sta(false, AUX_STA, MAC_INDEX_AUX, [0; 6], 4, 1 << AUX_QUEUE, 0, 0)?;
        self.phy_ctx(ACTION_ADD, 1)?;
        // device power: always on (no POWER_SAVE_ENA)
        self.cmd(POWER_TABLE_CMD, &[0u8; 4])?;
        self.scan_config()?;
        self.mac_ctx(ACTION_ADD)?;
        self.nic.note(format!(
            "runtime configuration done: MAC {}, {} channel(s), antennas TX {:#x} RX {:#x}",
            iwl::mac_str(&self.mac),
            self.channels.len(),
            self.tx_ant,
            self.rx_ant
        ));
        crate::println!(
            "[kernel] MILESTONE: IWLWIFI RUNTIME OK (MAC {}, {} channel(s))",
            iwl::mac_str(&self.mac),
            self.channels.len()
        );
        Ok(())
    }

    /// iwl_mvm_enable_txq without the new TX API: the ring base, the write
    /// pointer at the start sequence number, then SCD_QUEUE_CFG.
    fn enable_queue(&mut self, q: u32, ring: u64, fifo: u8, sta: u8, tid: u8) -> Result<(), String> {
        self.nic.write_direct(iwl::fh_cbbc(q as usize), (ring >> 8) as u32);
        self.nic.write_direct(iwl::HBUS_TARG_WRPTR, q << 8);
        // token, sta, tid, queue, enable, aggregate, fifo, window, ssn, reserved
        let c = [0, sta, tid, q as u8, 1, 0, fifo, 64, 0, 0, 0, 0];
        self.cmd(SCD_QUEUE_CFG, &c)?;
        Ok(())
    }

    fn disable_queue(&mut self, q: u32, sta: u8, tid: u8) {
        let c = [0, sta, tid, q as u8, 0, 0, 0, 0, 0, 0, 0, 0];
        if let Err(e) = self.cmd(SCD_QUEUE_CFG, &c) {
            self.log(format!("queue {} disable: {}", q, e));
        }
        let tfd_zero = self.hw.tfd_zero;
        self.nic.write_direct(iwl::fh_cbbc(q as usize), (tfd_zero >> 8) as u32);
    }

    /// ADD_STA (API v10, 48 bytes).
    #[allow(clippy::too_many_arguments)]
    fn add_sta(&mut self, modify: bool, sta: u8, mac_id: u32, addr: [u8; 6], ty: u8, queues: u32, aid: u16, mask: u8) -> Result<(), String> {
        let mut c = [0u8; 48];
        c[0] = modify as u8;
        w16(&mut c, 2, 0xFFFF); // tid_disable_tx: no aggregation
        w32(&mut c, 4, mac_id);
        c[8..14].copy_from_slice(&addr);
        c[16] = sta;
        c[17] = mask;
        c[35] = ty;
        w16(&mut c, 36, aid);
        w32(&mut c, 40, queues);
        let r = self.cmd(ADD_STA, &c)?;
        let st = r32(&r, 0) & 0xFF;
        if st != 1 {
            return Err(format!("ADD_STA {} refused (status {:#x})", sta, st));
        }
        Ok(())
    }

    /// PHY context 0 (API v1): band, channel, 20 MHz, chains.
    fn phy_ctx(&mut self, action: u32, channel: u8) -> Result<(), String> {
        let mut c = [0u8; 36];
        w32(&mut c, 4, action);
        c[16] = if channel <= 14 { 1 } else { 0 }; // PHY_BAND_24 / PHY_BAND_5
        c[17] = channel;
        w32(&mut c, 20, self.tx_ant);
        // valid RX chains, 2 idle + 2 MIMO (both antennas for diversity)
        w32(&mut c, 24, (self.rx_ant << 1) | (2 << 10) | (2 << 12));
        self.cmd(PHY_CONTEXT_CMD, &c)?;
        Ok(())
    }

    /// SCAN_CFG_CMD (API v1).
    fn scan_config(&mut self) -> Result<(), String> {
        let n = self.channels.len().min(63);
        let mut c = alloc::vec![0u8; (36 + n + 3) & !3];
        let flags: u32 = 1 | (1 << 3) | (1 << 8) | (1 << 9) | (1 << 10) | (1 << 11) | (1 << 13) | (1 << 14) | (1 << 15) | (1 << 17) | ((n as u32) << 26);
        w32(&mut c, 0, flags);
        w32(&mut c, 4, self.tx_ant);
        w32(&mut c, 8, self.rx_ant);
        w32(&mut c, 12, 0x0FFF_0FFF); // every legacy rate, basic + supported
        c[24..28].copy_from_slice(&[10, 110, 44, 90]); // dwell: active, passive, fragmented, extended
        c[28..34].copy_from_slice(&self.mac);
        c[34] = AUX_STA;
        for (i, &ch) in self.channels.iter().take(n).enumerate() {
            c[36 + i] = ch;
        }
        self.cmd(SCAN_CFG_CMD, &c)?;
        Ok(())
    }

    /// MAC context 0 (station), 148 bytes.
    fn mac_ctx(&mut self, action: u32) -> Result<(), String> {
        let mut c = [0u8; 148];
        w32(&mut c, 4, action);
        w32(&mut c, 8, 5); // FW_MAC_TYPE_BSS_STA
        c[16..22].copy_from_slice(&self.mac);
        let bss = self.bss.clone();
        let band24 = bss.as_ref().map_or(true, |b| b.band24());
        if let Some(b) = &bss {
            c[24..30].copy_from_slice(&b.bssid);
        }
        w32(&mut c, 32, if band24 { 0xF } else { 0 }); // CCK ACK rates
        w32(&mut c, 36, 0x15); // OFDM 6/12/24
        let cap = bss.as_ref().map_or(0, |b| b.cap);
        if cap & 0x20 != 0 {
            w32(&mut c, 44, 1 << 5);
        }
        if !band24 || cap & 0x400 != 0 {
            w32(&mut c, 48, 1 << 4);
        }
        let assoc = self.connected && self.beacon.is_some();
        // ACCEPT_GRP, and beacons while not associated
        w32(&mut c, 52, (1 << 2) | if assoc { 0 } else { 1 << 6 });
        // EDCA per TX FIFO (BK, BE, VI, VO): cw_min, cw_max, aifsn, fifo mask, txop
        for (f, (cmin, cmax, aifs, txop)) in [(15u16, 1023u16, 7u8, 0u16), (15, 1023, 3, 0), (7, 15, 2, 94 * 32), (3, 7, 2, 47 * 32)].iter().enumerate() {
            let o = 60 + f * 8;
            w16(&mut c, o, *cmin);
            w16(&mut c, o + 2, *cmax);
            c[o + 4] = *aifs;
            c[o + 5] = 1 << f;
            w16(&mut c, o + 6, *txop);
        }
        if let (true, Some((tsf, gp2, count, period)), Some(b)) = (assoc, self.beacon, &bss) {
            let bi = b.beacon_int.max(1) as u64;
            let offs = count as u64 * bi * 1024;
            w32(&mut c, 100, 1);
            w32(&mut c, 104, gp2.wrapping_add(offs as u32));
            c[108..116].copy_from_slice(&(tsf + offs).to_le_bytes());
            w32(&mut c, 116, bi as u32);
            w32(&mut c, 124, (bi * period.max(1) as u64) as u32);
            w32(&mut c, 132, 10);
            w32(&mut c, 136, self.aid as u32);
        } else if let Some(b) = &bss {
            w32(&mut c, 116, b.beacon_int as u32);
            w32(&mut c, 132, 10);
        }
        self.cmd(MAC_CONTEXT_CMD, &c)?;
        Ok(())
    }

    fn binding(&mut self, action: u32) -> Result<(), String> {
        let mut c = [0u8; 24];
        w32(&mut c, 4, action);
        w32(&mut c, 8, if action == ACTION_REMOVE { INVALID } else { 0 });
        w32(&mut c, 12, INVALID);
        w32(&mut c, 16, INVALID);
        w32(&mut c, 20, 0);
        let r = self.cmd(BINDING_CONTEXT_CMD, &c)?;
        if r32(&r, 0) != 0 {
            self.log(format!("binding: status {:#x}", r32(&r, 0)));
        }
        Ok(())
    }

    /// TIME_QUOTA (API v2): the whole time for binding 0, or nobody.
    fn quota(&mut self, on: bool) -> Result<(), String> {
        let mut c = [0u8; 64];
        for i in 0..4 {
            w32(&mut c, i * 16, INVALID);
        }
        if on {
            w32(&mut c, 0, 0);
            w32(&mut c, 4, 128);
        }
        self.cmd(TIME_QUOTA_CMD, &c)?;
        Ok(())
    }

    /// Session protection (TE_BSS_STA_AGGRESSIVE_ASSOC): stay on the
    /// channel through authentication, association and the handshake.
    fn protect_session(&mut self, tu: u32) {
        self.te_started = false;
        let gp2 = self.nic.read_prph(0xA0206C).unwrap_or(0); // DEVICE_SYSTEM_TIME_REG
        let mut c = [0u8; 36];
        w32(&mut c, 4, ACTION_ADD);
        w32(&mut c, 12, gp2);
        w32(&mut c, 16, 500);
        w32(&mut c, 24, 1);
        w32(&mut c, 28, tu);
        c[32] = 1;
        w16(&mut c, 34, (1 << 0) | (1 << 1) | (1 << 11));
        match self.cmd(TIME_EVENT_CMD, &c) {
            Err(e) => self.log(format!("time event: {}", e)),
            Ok(r) => {
                let started = self.wait(600, |m| m.te_started);
                self.log(format!("time event: status {:#x}, started {}", r32(&r, 0), started));
            }
        }
    }

    /// Pump the RX queue: every packet goes to `handle`.
    fn poll(&mut self) {
        if self.dead.is_some() {
            return;
        }
        let int = self.nic.r(iwl::CSR_INT);
        if int & (iwl::INT_SW_ERR | iwl::INT_HW_ERR) != 0 {
            self.nic.dump("firmware error");
            let why = format!("firmware error (CSR_INT {:#010x})", int);
            self.log(why.clone());
            self.dead = Some(why);
            return;
        }
        for _ in 0..64 {
            match self.rx.next(&self.nic, 0) {
                Some(p) => self.handle(p),
                None => break,
            }
        }
    }

    /// Poll until `done` or `ms` pass; yields between polls so the rest of
    /// the system runs while a scan or a handshake takes its time.
    fn wait(&mut self, ms: u64, done: fn(&Mvm) -> bool) -> bool {
        let t0 = now_ms();
        loop {
            self.poll();
            if done(self) {
                return true;
            }
            if now_ms() - t0 >= ms || self.dead.is_some() {
                return false;
            }
            crate::task::yield_now();
        }
    }

    fn handle(&mut self, p: Vec<u8>) {
        if p.len() < 8 {
            return;
        }
        let (cmd, group) = (p[4], p[5]);
        let d = &p[8..];
        if group > 1 {
            return;
        }
        match cmd {
            REPLY_RX_PHY_CMD if d.len() >= 32 => {
                self.phy_gp2 = r32(d, 4);
                self.phy_chan = r16(d, 22) as u8;
                let e = r32(d, 28);
                let lowest = [e & 0xFF, (e >> 8) & 0xFF].iter().copied().filter(|&v| v != 0).min();
                self.phy_rssi = lowest.map_or(-127, |v| -(v as i32));
            }
            REPLY_RX_MPDU_CMD if d.len() >= 4 => {
                let n = r16(d, 0) as usize;
                if d.len() >= 4 + n + 4 {
                    let status = r32(d, 4 + n);
                    let frame = d[4..4 + n].to_vec();
                    self.on_frame(&frame, status);
                }
            }
            TX_CMD => {
                let st = d.get(36).copied().unwrap_or(0);
                if st == 1 || st == 2 {
                    self.tx_ok += 1;
                } else {
                    self.tx_fail += 1;
                    if self.tx_fail <= 10 {
                        self.log(format!("TX failed: status {:#06x}, rts {} retries {}", r16(d, 36), d[2], d[3]));
                    }
                }
            }
            SCAN_COMPLETE_UMAC => self.scan_done = true,
            TIME_EVENT_NOTIFICATION if d.len() >= 24 => {
                if r32(d, 16) & 1 != 0 && r32(d, 20) == 1 {
                    self.te_started = true;
                }
            }
            REPLY_ERROR => {
                let s = format!("firmware REPLY_ERROR: {:08x} {:08x} {:08x}", r32(d, 0), r32(d, 4), r32(d, 8));
                self.log(s);
            }
            _ => {
                if self.notes < 24 {
                    self.notes += 1;
                    self.log(format!("notification {:#x}/{} ({} bytes)", cmd, group, d.len()));
                }
            }
        }
    }

    fn on_frame(&mut self, f: &[u8], status: u32) {
        // RX_RES_STATUS_NO_CRC32_ERROR | NO_RXE_OVERFLOW
        if status & 3 != 3 || f.len() < 24 {
            return;
        }
        let fc = r16(f, 0);
        match (fc >> 2) & 3 {
            0 if fc & 0x4000 == 0 => self.on_mgmt((fc >> 4) & 0xF, f),
            2 => self.on_data(fc, f, status),
            _ => {}
        }
    }

    fn on_mgmt(&mut self, sub: u16, f: &[u8]) {
        let (a1, a2, a3) = (mac6(&f[4..]), mac6(&f[10..]), mac6(&f[16..]));
        let body = &f[24..];
        let ours = self.bss.as_ref().map_or(false, |b| b.bssid == a2) && a1 == self.mac;
        match sub {
            8 | 5 => self.on_beacon(a3, body, sub == 8),
            11 if ours && body.len() >= 6 && r16(body, 2) == 2 => self.auth_status = Some(r16(body, 4)),
            1 | 3 if ours && body.len() >= 6 => self.assoc_status = Some((r16(body, 2), r16(body, 4) & 0x3FFF)),
            10 | 12 if ours && self.connected => {
                let reason = r16(body, 0);
                self.log(format!("{} by the access point, reason {}", if sub == 12 { "deauthenticated" } else { "disassociated" }, reason));
                self.lost = Some(reason);
                self.connected = false;
                set_status(format!("connection lost (reason {})", reason));
            }
            _ => {}
        }
    }

    fn on_beacon(&mut self, bssid: [u8; 6], b: &[u8], beacon: bool) {
        if b.len() < 12 {
            return;
        }
        let tsf = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        let mut bss = Bss {
            bssid,
            ssid: String::new(),
            channel: self.phy_chan,
            rssi: self.phy_rssi,
            cap: r16(b, 10),
            beacon_int: r16(b, 8),
            rates: Vec::new(),
            rsn: None,
            wpa1: false,
        };
        let (mut dtim_count, mut dtim_period) = (0u8, 1u8);
        let mut p = 12;
        while p + 2 <= b.len() {
            let (id, len) = (b[p], b[p + 1] as usize);
            let Some(v) = b.get(p + 2..p + 2 + len) else { break };
            match id {
                0 => bss.ssid = v.iter().take_while(|&&c| c != 0).map(|&c| if (32..127).contains(&c) { c as char } else { '?' }).collect(),
                1 | 50 => bss.rates.extend_from_slice(v),
                3 if len >= 1 => bss.channel = v[0],
                5 if len >= 2 => {
                    dtim_count = v[0];
                    dtim_period = v[1];
                }
                48 => bss.rsn = crate::wpa::parse_rsn(v),
                221 if len >= 4 && v[..4] == [0x00, 0x50, 0xF2, 0x01] => bss.wpa1 = true,
                _ => {}
            }
            p += 2 + len;
        }
        if beacon && self.bss.as_ref().map_or(false, |x| x.bssid == bssid) {
            self.beacon = Some((tsf, self.phy_gp2, dtim_count, dtim_period));
        }
        let full = self.scan.len() >= 128;
        match self.scan.iter_mut().find(|x| x.bssid == bssid) {
            Some(x) => {
                if bss.ssid.is_empty() {
                    bss.ssid = core::mem::take(&mut x.ssid);
                }
                *x = bss;
            }
            None if !full => self.scan.push(bss),
            None => {}
        }
    }

    fn on_data(&mut self, fc: u16, f: &[u8], status: u32) {
        // from the distribution system only (FromDS = 1, ToDS = 0)
        if fc & 0x0300 != 0x0200 {
            return;
        }
        let (a1, a2, a3) = (mac6(&f[4..]), mac6(&f[10..]), mac6(&f[16..]));
        if self.bss.as_ref().map_or(true, |b| b.bssid != a2) || a3 == self.mac {
            return;
        }
        if a1 != self.mac && a1[0] & 1 == 0 {
            return;
        }
        let sub = (fc >> 4) & 0xF;
        if sub & 4 != 0 {
            return; // no data (null function)
        }
        let mut hdr = 24;
        if sub & 8 != 0 {
            hdr += 2; // QoS control
            if fc & 0x8000 != 0 {
                hdr += 4; // HT control
            }
        }
        let mut body = match f.get(hdr..) {
            Some(b) => b,
            None => return,
        };
        if fc & 0x4000 != 0 {
            // decrypted by the firmware: CCM, MIC ok; strip the CCMP header and the MIC
            if (status >> 8) & 7 != 2 || status & (1 << 6) == 0 || body.len() < 16 {
                return;
            }
            body = &body[8..body.len() - 8];
        }
        if body.len() < 8 || body[..3] != [0xAA, 0xAA, 0x03] {
            return;
        }
        let ethertype = u16::from_be_bytes([body[6], body[7]]);
        let payload = &body[8..];
        self.rx_data += 1;
        if ethertype == 0x888E {
            let p = payload.to_vec();
            self.on_eapol(&p);
            return;
        }
        if !self.connected || (self.bss.as_ref().map_or(false, |b| b.privacy()) && !self.keyed) {
            return;
        }
        let mut e = Vec::with_capacity(14 + payload.len());
        e.extend_from_slice(&a1);
        e.extend_from_slice(&a3);
        e.extend_from_slice(&ethertype.to_be_bytes());
        e.extend_from_slice(payload);
        if self.frames.len() >= 256 {
            self.frames.pop_front();
        }
        self.frames.push_back(e);
    }

    fn on_eapol(&mut self, f: &[u8]) {
        let Some(sup) = self.sup.as_mut() else { return };
        let st = sup.on_eapol(f);
        if let Some(e) = st.error {
            self.log(format!("WPA2: {}", e));
            self.eapol_error = Some(e);
        }
        if let Some(r) = st.reply {
            let bssid = self.bss.as_ref().map_or([0; 6], |b| b.bssid);
            self.send_data(bssid, 0x888E, &r, Kind::Eapol);
        }
        if let Some(tk) = st.ptk {
            // message 4 must leave unencrypted: wait for the queue to drain
            self.drain(200);
            match self.install_key(&tk, 0, 0, false) {
                Ok(()) => {
                    self.tk = Some(tk);
                    self.pn = 0;
                }
                Err(e) => {
                    self.log(format!("pairwise key: {}", e));
                    self.eapol_error = Some(e);
                }
            }
        }
        if let Some((id, k)) = st.gtk {
            if let Err(e) = self.install_key(&k, id, 1 + (id & 3), true) {
                self.log(format!("group key: {}", e));
                self.eapol_error = Some(e);
            }
        }
        if st.done && self.eapol_error.is_none() {
            self.keyed = true;
        }
    }

    /// ADD_STA_KEY (API v1, 64 bytes) on the AP station: CCMP, key map.
    fn install_key(&mut self, key: &[u8], keyidx: u8, offset: u8, group: bool) -> Result<(), String> {
        if key.len() < 16 {
            return Err(format!("{}-byte key (CCMP needs 16)", key.len()));
        }
        let mut c = [0u8; 64];
        c[0] = AP_STA;
        c[1] = offset;
        let flags: u16 = 2 | (1 << 3) | (((keyidx & 3) as u16) << 8) | if group { 1 << 14 } else { 0 };
        w16(&mut c, 2, flags);
        c[4..20].copy_from_slice(&key[..16]);
        let r = self.cmd(ADD_STA_KEY, &c)?;
        let st = r32(&r, 0) & 0xFF;
        if st != 1 {
            return Err(format!("ADD_STA_KEY refused (status {:#x})", st));
        }
        self.log(format!("{} key {} installed", if group { "group" } else { "pairwise" }, keyidx));
        Ok(())
    }

    fn queue_rdptr(&self) -> usize {
        (self.nic.read_prph(iwl::scd_queue_rdptr(DATA_QUEUE)).unwrap_or(0) & 0xFF) as usize
    }

    /// Wait (at most `ms`) until the data queue is empty.
    fn drain(&mut self, ms: u64) {
        let t0 = now_ms();
        while self.queue_rdptr() != self.dq.wr % 256 && now_ms() - t0 < ms {
            core::hint::spin_loop();
        }
    }

    /// Legacy rate_n_flags on antenna A.
    fn rate(&self, kind: Kind) -> u32 {
        let ant = 1 << 14;
        let cck = 1 << 9;
        let (band24, rates) = self.bss.as_ref().map_or((true, &[][..]), |b| (b.band24(), &b.rates[..]));
        let has = |r: u8| rates.iter().any(|&x| x & 0x7F == r);
        if kind != Kind::Data {
            return if band24 && (rates.is_empty() || has(2)) { 10 | cck | ant } else { 13 | ant };
        }
        if has(24) || !band24 {
            5 | ant // 12 Mbps OFDM
        } else if has(22) {
            110 | cck | ant
        } else {
            10 | cck | ant
        }
    }

    /// Queue one 802.11 frame (24-byte header + body) on the BSS queue.
    /// Data frames are protected with CCMP once a pairwise key exists.
    fn tx_frame(&mut self, hdr: &[u8; 24], body: &[u8], kind: Kind) -> bool {
        if self.dead.is_some() || !self.sta {
            return false;
        }
        let protect = matches!(kind, Kind::Data | Kind::Eapol) && self.tk.is_some();
        let tail = if protect { 8 } else { 0 } + body.len();
        if 84 + tail > 4096 {
            self.tx_drop += 1;
            return false;
        }
        let wr = self.dq.wr % 256;
        if (wr + 256 - self.queue_rdptr()) % 256 >= DATA_SLOTS - 1 {
            self.tx_drop += 1;
            return false;
        }
        let (phys, buf) = self.dq.slots[self.dq.wr % DATA_SLOTS];
        let mut v = Vec::with_capacity(84 + tail);
        v.resize(84, 0);
        // short command header: TX_CMD, group 0, sequence = queue | index
        v[0] = TX_CMD;
        w16(&mut v, 2, ((DATA_QUEUE as u16) << 8) | wr as u16);
        let t = 4;
        let len = 24 + tail;
        w16(&mut v, t, len as u16);
        w32(&mut v, t + 4, (1 << 3) | (1 << 13)); // ACK, firmware sequence numbers
        let rate = self.rate(kind);
        w32(&mut v, t + 12, rate);
        v[t + 16] = AP_STA;
        if let (true, Some(tk)) = (protect, self.tk) {
            v[t + 17] = 2; // TX_CMD_SEC_CCM
            v[t + 20..t + 36].copy_from_slice(&tk);
        }
        w32(&mut v, t + 40, 0xFFFF_FFFF); // life time: infinite
        let scratch = phys + 4 + 8;
        w32(&mut v, t + 44, scratch as u32);
        v[t + 48] = (scratch >> 32) as u8;
        v[t + 49] = 60;
        v[t + 50] = if kind == Kind::Data { 15 } else { 3 };
        v[t + 51] = TID_NON_QOS;
        w16(&mut v, t + 52, match kind {
            Kind::Assoc => 3,
            Kind::Mgmt | Kind::Eapol => 2,
            Kind::Data => 0,
        });
        v[60..84].copy_from_slice(hdr);
        if protect {
            v[61] |= 0x40; // Protected
            self.pn += 1;
            let pn = self.pn.to_le_bytes();
            v.extend_from_slice(&[pn[0], pn[1], 0, 0x20, pn[2], pn[3], pn[4], pn[5]]);
        }
        v.extend_from_slice(body);
        unsafe {
            for (i, &b) in v.iter().enumerate() {
                write_volatile(buf.add(i), b);
            }
        }
        crate::arch::cpu::dma_sync(buf as u64, v.len());
        // TFD: TB0 = 20 bytes, TB1 = rest of the command + 802.11 header, TB2 = the rest
        let tfd = unsafe { self.dq.tfd.add(wr * iwl::TFD_SIZE) };
        unsafe { core::ptr::write_bytes(tfd, 0, iwl::TFD_SIZE) };
        let put = |n: usize, addr: u64, l: usize| unsafe {
            let p = tfd.add(4 + n * 6);
            write_volatile(p as *mut u32, addr as u32);
            write_volatile(p.add(4) as *mut u16, (((addr >> 32) & 0xF) as u16) | ((l as u16) << 4));
        };
        put(0, phys, 20);
        put(1, phys + 20, 64);
        let mut tbs = 2u8;
        if v.len() > 84 {
            put(2, phys + 84, v.len() - 84);
            tbs = 3;
        }
        unsafe { write_volatile(tfd.add(3), tbs) };
        crate::arch::cpu::dma_sync(tfd as u64, iwl::TFD_SIZE);
        // scheduler byte count (bytes on the 8000 family): frame + CRC +
        // delimiter (+ MIC), station in the top 4 bits
        let bc = ((len + 8 + if protect { 8 } else { 0 }) as u16 & 0xFFF) | ((AP_STA as u16) << 12);
        let table = self.tx.ring.bc as *mut u16;
        let q = DATA_QUEUE as usize;
        unsafe {
            write_volatile(table.add(q * 320 + wr), bc);
            if wr < 64 {
                write_volatile(table.add(q * 320 + 256 + wr), bc);
            }
        }
        crate::arch::cpu::dma_sync(table as u64 + (q * 640) as u64, 640);
        self.dq.wr += 1;
        if !self.nic.grab() {
            self.tx_drop += 1;
            return false;
        }
        self.nic.w(iwl::HBUS_TARG_WRPTR, ((self.dq.wr % 256) as u32) | (DATA_QUEUE << 8));
        self.nic.release();
        true
    }

    fn mgmt_header(&self, subtype: u8) -> [u8; 24] {
        let bssid = self.bss.as_ref().map_or([0xFF; 6], |b| b.bssid);
        let mut h = [0u8; 24];
        h[0] = subtype << 4;
        h[4..10].copy_from_slice(&bssid);
        h[10..16].copy_from_slice(&self.mac);
        h[16..22].copy_from_slice(&bssid);
        h
    }

    fn send_data(&mut self, dst: [u8; 6], ethertype: u16, payload: &[u8], kind: Kind) -> bool {
        let bssid = self.bss.as_ref().map_or([0; 6], |b| b.bssid);
        let mut h = [0u8; 24];
        h[0] = 0x08; // data
        h[1] = 0x01; // ToDS
        h[4..10].copy_from_slice(&bssid);
        h[10..16].copy_from_slice(&self.mac);
        h[16..22].copy_from_slice(&dst);
        let mut body = Vec::with_capacity(8 + payload.len());
        body.extend_from_slice(&[0xAA, 0xAA, 0x03, 0, 0, 0]);
        body.extend_from_slice(&ethertype.to_be_bytes());
        body.extend_from_slice(payload);
        self.tx_frame(&h, &body, kind)
    }

    /// An Ethernet frame from the IP stack.
    fn send_ethernet(&mut self, frame: &[u8]) {
        if frame.len() < 14 || !self.connected || (self.bss.as_ref().map_or(false, |b| b.privacy()) && !self.keyed) {
            self.tx_drop += 1;
            return;
        }
        let dst = mac6(frame);
        let et = u16::from_be_bytes([frame[12], frame[13]]);
        self.send_data(dst, et, &frame[14..], Kind::Data);
    }

    /// UMAC scan (SCAN_REQ_UMAC API v7, adaptive dwell) of every channel;
    /// beacons and probe responses arrive as RX MPDUs.
    fn do_scan(&mut self) -> Result<usize, String> {
        if self.connected {
            return Ok(self.scan.len()); // no background scans while joined
        }
        let n = self.channels.len().min(self.n_scan_channels);
        let tail = 48 + self.n_scan_channels * 8;
        let mut c = alloc::vec![0u8; tail + 12 + 528 + 20 * 34];
        w32(&mut c, 8, 6); // ooc priority EXT_6
        w16(&mut c, 12, (1 << 2) | (1 << 13)); // PASS_ALL | ADAPTIVE_DWELL
        c[16] = 10; // active dwell
        c[17] = 110; // passive dwell
        c[18] = 44; // fragmented dwell
        c[19] = 2; // adaptive dwell: default APs
        c[20] = 10; // ... social channels
        w16(&mut c, 22, 300); // adaptive dwell budget
        w32(&mut c, 40, 6); // scan priority EXT_6
        c[45] = n as u8;
        for (i, &ch) in self.channels.iter().take(n).enumerate() {
            let o = 48 + i * 8;
            c[o + 4] = ch;
            c[o + 5] = 1; // iterations
        }
        c[tail + 2] = 1; // schedule[0]: one iteration, no interval
        // probe request template: header + empty SSID, then per-band rates
        let preq = tail + 12;
        let buf = preq + 16;
        let mut t: Vec<u8> = alloc::vec![0x40, 0, 0, 0];
        t.extend_from_slice(&[0xFF; 6]);
        t.extend_from_slice(&self.mac);
        t.extend_from_slice(&[0xFF; 6]);
        t.extend_from_slice(&[0, 0, 0, 0]);
        let b24: [u8; 16] = [1, 8, 0x02, 0x04, 0x0B, 0x16, 0x0C, 0x12, 0x18, 0x24, 50, 4, 0x30, 0x48, 0x60, 0x6C];
        let b5: [u8; 10] = [1, 8, 0x0C, 0x12, 0x18, 0x24, 0x30, 0x48, 0x60, 0x6C];
        w16(&mut c, preq, 0);
        w16(&mut c, preq + 2, 26);
        w16(&mut c, preq + 4, 26);
        w16(&mut c, preq + 6, b24.len() as u16);
        w16(&mut c, preq + 8, (26 + b24.len()) as u16);
        w16(&mut c, preq + 10, b5.len() as u16);
        w16(&mut c, preq + 12, (26 + b24.len() + b5.len()) as u16);
        t.extend_from_slice(&b24);
        t.extend_from_slice(&b5);
        c[buf..buf + t.len()].copy_from_slice(&t);
        self.scan.clear();
        self.scan_done = false;
        let t0 = now_ms();
        self.cmd(SCAN_REQ_UMAC, &c)?;
        let done = self.wait(12_000, |m| m.scan_done);
        let ms = now_ms() - t0;
        self.scan.sort_by(|a, b| b.rssi.cmp(&a.rssi));
        self.log(format!("scan: {} network(s) on {} channel(s) in {} ms{}", self.scan.len(), n, ms, if done { "" } else { " (no SCAN_COMPLETE)" }));
        if let Some(e) = &self.dead {
            return Err(e.clone());
        }
        Ok(self.scan.len())
    }

    /// Join `ssid` (WPA2-PSK with `pmk`, or open when `pmk` is None).
    fn join(&mut self, ssid: &str, pmk: Option<[u8; 32]>) -> Result<String, String> {
        self.leave();
        let pick = |m: &Mvm| m.scan.iter().filter(|b| b.ssid == ssid).max_by_key(|b| b.rssi).cloned();
        let mut cand = pick(self);
        if cand.is_none() {
            self.do_scan()?;
            cand = pick(self);
        }
        let bss = cand.ok_or_else(|| format!("network {:?} not found", ssid))?;
        let mut group = crate::wpa::CIPHER_CCMP;
        if bss.privacy() {
            if pmk.is_none() {
                return Err(String::from("this network needs a password"));
            }
            let Some(r) = bss.rsn else {
                return Err(String::from(if bss.wpa1 { "WPA (version 1) only: needs WPA2" } else { "WEP is not supported" }));
            };
            if r.sae_only || !r.psk {
                return Err(String::from("not WPA2-Personal (WPA3 / Enterprise are not supported yet)"));
            }
            if !r.ccmp {
                return Err(String::from("pairwise cipher is TKIP only (needs AES-CCMP)"));
            }
            if r.mfp_required {
                return Err(String::from("requires management frame protection (802.11w, not supported yet)"));
            }
            if r.group != crate::wpa::CIPHER_CCMP {
                return Err(String::from("group cipher TKIP is not supported"));
            }
            group = r.group;
        }
        self.log(format!(
            "joining {:?} {} channel {} ({} dBm, {})",
            bss.ssid,
            iwl::mac_str(&bss.bssid),
            bss.channel,
            bss.rssi,
            if bss.privacy() { "WPA2" } else { "open" }
        ));
        self.bss = Some(bss.clone());
        self.beacon = None;
        // channel, binding, time quota, MAC context with the BSSID
        self.phy_ctx(ACTION_MODIFY, bss.channel)?;
        self.binding(ACTION_ADD)?;
        self.bound = true;
        self.quota(true)?;
        self.mac_ctx(ACTION_MODIFY)?;
        // the AP station and its queue
        self.add_sta(false, AP_STA, 0, bss.bssid, 0, 0, 0, 0)?;
        self.sta = true;
        self.dq.reset_ring();
        self.dq.wr = 0;
        let ring = self.dq.tfd_phys;
        self.enable_queue(DATA_QUEUE, ring, FIFO_VO, AP_STA, TID_NON_QOS)?;
        self.add_sta(true, AP_STA, 0, bss.bssid, 0, 1 << DATA_QUEUE, 0, 1 << 7)?;
        self.protect_session(3000);
        // open-system authentication
        for _ in 0..3 {
            self.auth_status = None;
            let h = self.mgmt_header(0xB);
            self.tx_frame(&h, &[0, 0, 1, 0, 0, 0], Kind::Mgmt);
            if self.wait(500, |m| m.auth_status.is_some()) {
                break;
            }
        }
        match self.auth_status {
            Some(0) => {}
            Some(s) => return Err(format!("authentication refused (status {})", s)),
            None => return Err(format!("no answer to authentication (TX ok {}, failed {})", self.tx_ok, self.tx_fail)),
        }
        // the handshake may start right after the association response
        self.sup = pmk.map(|k| crate::wpa::Supplicant::new(k, bss.bssid, self.mac, group));
        self.eapol_error = None;
        self.keyed = false;
        let mut cap: u16 = 0x0001 | (bss.cap & 0x0420); // ESS + the AP's short preamble / slot
        if bss.privacy() {
            cap |= 0x0010;
        }
        let mut body = Vec::new();
        body.extend_from_slice(&cap.to_le_bytes());
        body.extend_from_slice(&10u16.to_le_bytes()); // listen interval
        body.push(0);
        body.push(bss.ssid.len() as u8);
        body.extend_from_slice(bss.ssid.as_bytes());
        let rates: Vec<u8> = if bss.rates.is_empty() {
            if bss.band24() { alloc::vec![0x82, 0x84, 0x8B, 0x96, 0x0C, 0x12, 0x18, 0x24] } else { alloc::vec![0x8C, 0x12, 0x98, 0x24, 0xB0, 0x48, 0x60, 0x6C] }
        } else {
            bss.rates.clone()
        };
        let first = rates.len().min(8);
        body.push(1);
        body.push(first as u8);
        body.extend_from_slice(&rates[..first]);
        if rates.len() > 8 {
            body.push(50);
            body.push((rates.len() - 8) as u8);
            body.extend_from_slice(&rates[8..]);
        }
        if bss.privacy() {
            body.extend_from_slice(&crate::wpa::rsn_ie(group));
        }
        for _ in 0..3 {
            self.assoc_status = None;
            let h = self.mgmt_header(0x0);
            self.tx_frame(&h, &body, Kind::Assoc);
            if self.wait(700, |m| m.assoc_status.is_some()) {
                break;
            }
        }
        let (st, aid) = self.assoc_status.ok_or_else(|| String::from("no answer to the association request"))?;
        if st != 0 {
            return Err(format!("association refused (status {})", st));
        }
        self.aid = aid;
        self.connected = true;
        self.lost = None;
        if let Err(e) = self.add_sta(true, AP_STA, 0, bss.bssid, 0, 1 << DATA_QUEUE, aid, 0) {
            self.log(format!("station update: {}", e));
        }
        self.log(format!("associated, AID {}", aid));
        if bss.privacy() {
            let ok = self.wait(8000, |m| m.keyed || m.eapol_error.is_some() || m.lost.is_some());
            if !ok || !self.keyed {
                let why = self.eapol_error.clone().unwrap_or_else(|| String::from("the access point did not complete it (wrong password?)"));
                return Err(format!("WPA2 handshake failed: {}", why));
            }
            self.log(String::from("WPA2 handshake complete"));
        }
        // DTIM timing from a beacon, then the MAC context as associated
        self.wait(400, |m| m.beacon.is_some());
        if let Err(e) = self.mac_ctx(ACTION_MODIFY) {
            self.log(format!("MAC context (associated): {}", e));
        }
        // MAC power: constantly awake (no POWER_MANAGEMENT_ENA), keep-alive 25 s
        let mut pm = [0u8; 40];
        w16(&mut pm, 6, 25);
        if let Err(e) = self.cmd(MAC_PM_POWER_TABLE, &pm) {
            self.log(format!("MAC power: {}", e));
        }
        Ok(format!(
            "{} channel {}, {} dBm{}",
            iwl::mac_str(&bss.bssid),
            bss.channel,
            bss.rssi,
            if bss.privacy() { ", WPA2-PSK CCMP" } else { ", open" }
        ))
    }

    /// Undo a join (best effort): deauthenticate, station, queue, binding.
    fn leave(&mut self) {
        if self.connected {
            let h = self.mgmt_header(0xC);
            self.tx_frame(&h, &3u16.to_le_bytes(), Kind::Mgmt);
            self.drain(100);
        }
        if self.sta {
            let mut r = [0u8; 4];
            r[0] = AP_STA;
            if let Err(e) = self.cmd(REMOVE_STA, &r) {
                self.log(format!("remove station: {}", e));
            }
            self.disable_queue(DATA_QUEUE, AP_STA, TID_NON_QOS);
        }
        if self.bound {
            let _ = self.quota(false);
            if let Err(e) = self.binding(ACTION_REMOVE) {
                self.log(format!("binding remove: {}", e));
            }
        }
        let was = self.bss.is_some();
        self.connected = false;
        self.keyed = false;
        self.sta = false;
        self.bound = false;
        self.tk = None;
        self.sup = None;
        self.bss = None;
        self.beacon = None;
        self.frames.clear();
        if was {
            if let Err(e) = self.mac_ctx(ACTION_MODIFY) {
                self.log(format!("MAC context reset: {}", e));
            }
        }
    }

    fn summary(&self) -> String {
        let link = match (&self.dead, &self.bss, self.connected) {
            (Some(e), _, _) => format!("stopped: {}", e),
            (None, Some(b), true) => format!("connected to {:?} ({} dBm, channel {})", b.ssid, b.rssi, b.channel),
            _ => String::from("not connected"),
        };
        format!(
            "Intel 8265, MAC {}; {}; TX ok {} failed {} dropped {}, RX data {}",
            iwl::mac_str(&self.mac),
            link,
            self.tx_ok,
            self.tx_fail,
            self.tx_drop,
            self.rx_data
        )
    }
}

/// Bring the adapter up once; later calls find it running.
fn ensure_up() -> Result<(), String> {
    if DEV.lock().is_some() {
        return Ok(());
    }
    let dev = device().ok_or("no Intel 8260/8265 Wi-Fi adapter on this PC")?;
    let mut nic = Nic::new();
    match start(&dev, &mut nic) {
        Ok(m) => {
            *DEV.lock() = Some(m);
            set_status(String::from("adapter up (runtime firmware)"));
            Ok(())
        }
        Err(e) => {
            set_status(format!("adapter start failed: {}", e));
            Err(e)
        }
    }
}

/// `wifi up`: start the adapter and show what the driver logged.
pub(super) fn up_command(dev: &PciDevice) -> Vec<String> {
    if DEV.lock().is_some() {
        return alloc::vec![String::from("already up"), DEV.lock().as_ref().map_or(String::new(), |m| m.summary())];
    }
    let mut nic = Nic::new();
    let r = start(dev, &mut nic);
    let mut out = core::mem::take(&mut nic.log);
    match r {
        Ok(m) => {
            out.push(m.summary());
            *DEV.lock() = Some(m);
            set_status(String::from("adapter up (runtime firmware)"));
        }
        Err(e) => {
            out.push(format!("failed: {}", e));
            set_status(format!("adapter start failed: {}", e));
        }
    }
    out
}

/// The network device the IP stack uses once a network is joined. It never
/// waits for the driver's lock: a join in progress holds it for seconds.
pub struct IwlNet;

impl crate::net::NetDevice for IwlNet {
    fn mac(&self) -> crate::net::MacAddr {
        DEV.try_lock().and_then(|g| g.as_ref().map(|m| m.mac)).unwrap_or([0; 6])
    }
    fn transmit(&mut self, frame: &[u8]) {
        if let Some(mut g) = DEV.try_lock() {
            if let Some(m) = g.as_mut() {
                m.send_ethernet(frame);
            }
        }
    }
    fn receive(&mut self) -> Option<Vec<u8>> {
        let mut g = DEV.try_lock()?;
        let m = g.as_mut()?;
        if m.frames.is_empty() {
            m.poll();
        }
        m.frames.pop_front()
    }
    fn diag(&self) -> Option<String> {
        DEV.try_lock().and_then(|g| g.as_ref().map(|m| format!("wifi: {}", m.summary())))
    }
    fn wireless(&self) -> bool {
        true
    }
}

fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    let s = s.trim();
    if s.len() != 64 {
        return None;
    }
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(k)
}

/// The WPA2 key: PBKDF2-HMAC-SHA1(passphrase, SSID, 4096) as 64 hex digits,
/// so WIFI.CFG never holds the passphrase itself.
fn psk_hex(ssid: &str, pass: &str) -> String {
    crate::wpa::psk(ssid, pass).iter().map(|b| format!("{:02x}", b)).collect()
}

/// Join, take a DHCP lease and make Wi-Fi the network device.
pub fn connect(ssid: &str, key_hex: &str) -> Result<String, String> {
    ensure_up()?;
    let pmk = if key_hex.is_empty() {
        None
    } else {
        Some(parse_hex32(key_hex).ok_or("the saved key is not 64 hex digits")?)
    };
    let info = {
        let mut g = DEV.lock();
        let m = g.as_mut().ok_or("Wi-Fi is not up")?;
        let r = m.join(ssid, pmk);
        if r.is_err() {
            m.leave();
        }
        r?
    };
    let mut dev = IwlNet;
    let lease = crate::dhcp::lease(&mut dev, 15).ok_or_else(|| format!("joined ({}), but no DHCP answer", info))?;
    crate::net::attach(Box::new(IwlNet), lease.ip);
    crate::net::set_route(lease.mask, lease.router);
    crate::net::set_dns(lease.dns);
    let (ip, gw) = (lease.ip, lease.router);
    let msg = format!(
        "connected to {:?}: {}.{}.{}.{} (router {}.{}.{}.{}; {})",
        ssid, ip[0], ip[1], ip[2], ip[3], gw[0], gw[1], gw[2], gw[3], info
    );
    set_status(msg.clone());
    crate::net::set_status(format!("Wi-Fi {:?}", ssid));
    crate::println!("[kernel] MILESTONE: WIFI OK (Intel 8265, host WPA2, DHCP {}.{}.{}.{})", ip[0], ip[1], ip[2], ip[3]);
    Ok(msg)
}

/// The network saved in WIFI.CFG ("" = none).
pub fn saved_ssid() -> String {
    let Ok(cfg) = crate::vfs::read(CONFIG) else { return String::new() };
    String::from_utf8_lossy(&cfg).lines().find_map(|l| l.strip_prefix("ssid=")).unwrap_or("").trim().into()
}

/// `wifi` on a PC with an Intel 8260/8265.
pub fn command(args: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    match args.first().copied().unwrap_or("") {
        "" | "status" => {
            let st = STATUS.lock().clone();
            out.push(if st.is_empty() {
                String::from("wifi: not started (`wifi scan` or `wifi connect SSID PASSWORD`)")
            } else {
                format!("wifi: {}", st)
            });
            if let Some(m) = DEV.lock().as_ref() {
                out.push(m.summary());
            }
        }
        "up" => match device() {
            Some(d) => out.extend(up_command(&d).into_iter().map(|l| format!("  {}", l))),
            None => out.push(String::from("wifi up: no Intel 8260/8265 adapter")),
        },
        "scan" => {
            if let Err(e) = ensure_up() {
                out.push(format!("wifi: {}", e));
                return out;
            }
            let mut g = DEV.lock();
            let Some(m) = g.as_mut() else { return out };
            let joined = m.connected;
            match m.do_scan() {
                Err(e) => out.push(format!("wifi scan: {}", e)),
                Ok(_) if args.get(1) == Some(&"--raw") => {
                    for b in m.scan.iter().filter(|b| !b.ssid.is_empty()).take(40) {
                        out.push(format!("@bss\t{}\t{}\t{}\t{}", b.rssi, b.channel, if b.privacy() { "wpa" } else { "open" }, b.ssid));
                    }
                }
                Ok(n) => {
                    out.push(format!("{} network(s){}:", n, if joined { " (from before joining)" } else { "" }));
                    for b in m.scan.iter().take(30) {
                        let sec = match (&b.rsn, b.privacy(), b.wpa1) {
                            (Some(r), _, _) if r.sae_only => "WPA3",
                            (Some(_), _, _) => "WPA2",
                            (None, true, true) => "WPA ",
                            (None, true, false) => "WEP ",
                            _ => "open",
                        };
                        out.push(format!(
                            "  {:>4} dBm  ch {:>3}  {}  {}",
                            b.rssi,
                            b.channel,
                            sec,
                            if b.ssid.is_empty() { "(hidden)" } else { b.ssid.as_str() }
                        ));
                    }
                }
            }
        }
        "connect" => {
            let Some(ssid) = args.get(1) else {
                out.push(String::from("usage: wifi connect SSID [PASSWORD]"));
                return out;
            };
            let pass = args.get(2).copied().unwrap_or("");
            if !pass.is_empty() && !(8..=63).contains(&pass.len()) {
                out.push(String::from("wifi: a WPA2 password has 8 to 63 characters"));
                return out;
            }
            let key = if pass.is_empty() { String::new() } else { psk_hex(ssid, pass) };
            match connect(ssid, &key) {
                Ok(m) => {
                    out.push(format!("wifi: {}", m));
                    let cfg = format!("ssid={}\npsk={}\n", ssid, key);
                    match crate::vfs::write(CONFIG, cfg.as_bytes()) {
                        Ok(()) => out.push(format!("saved in {} (the derived key, not the password); joins at every start", CONFIG)),
                        Err(e) => out.push(format!("not saved: {}", e)),
                    }
                }
                Err(e) => {
                    set_status(format!("join failed: {}", e));
                    out.push(format!("wifi: {}", e));
                }
            }
        }
        "disconnect" => {
            if let Some(m) = DEV.lock().as_mut() {
                m.leave();
            }
            set_status(String::from("disconnected"));
            out.push(String::from("wifi: disconnected"));
        }
        "forget" => {
            let _ = crate::vfs::remove(CONFIG);
            out.push(format!("wifi: {} removed", CONFIG));
        }
        _ => out.push(String::from("usage: wifi [status | up | scan [--raw] | connect SSID [PASSWORD] | disconnect | forget | pci]")),
    }
    out
}

/// Boot (background, after the wired network): when WIFI.CFG exists and no
/// wired network came up, start the adapter and join.
pub fn autostart() {
    let Ok(cfg) = crate::vfs::read(CONFIG) else { return };
    if !present() {
        return;
    }
    if crate::net::info().map_or(false, |i| i.attached && i.ip != [0; 4] && i.ip[0] != 127) {
        set_status(String::from("a wired network is up; Wi-Fi left off (`wifi connect` switches)"));
        return;
    }
    let text = String::from_utf8_lossy(&cfg).into_owned();
    let get = |k: &str| -> String { text.lines().find_map(|l| l.strip_prefix(k)).unwrap_or("").trim().into() };
    let (ssid, psk) = (get("ssid="), get("psk="));
    if ssid.is_empty() {
        return;
    }
    if let Err(e) = connect(&ssid, &psk) {
        set_status(format!("autostart: {}", e));
    }
}
