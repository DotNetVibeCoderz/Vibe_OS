//! WPA2-Personal on the host side (IEEE 802.11-2016 12.7): the 4-way
//! handshake and the group-key handshake of a supplicant, for Wi-Fi chips
//! whose firmware has no supplicant of its own (Intel iwlmvm). AKM PSK
//! (00-0F-AC:2), pairwise CCMP, group CCMP; key descriptor version 2
//! (HMAC-SHA1 MIC, AES key wrap). Generic code: no hardware here.
//!
//! The driver hands every EAPOL frame it receives to `Supplicant::on_eapol`
//! and sends back what it returns; when the 4-way handshake ends it installs
//! the pairwise key (TK) and the group key it was given.

use alloc::string::String;
use alloc::vec::Vec;

use hmac::{Hmac, Mac};

type HmacSha1 = Hmac<sha1::Sha1>;

pub const OUI: [u8; 3] = [0x00, 0x0F, 0xAC];
pub const CIPHER_CCMP: u8 = 4;
#[allow(dead_code)]
pub const CIPHER_TKIP: u8 = 2;
pub const AKM_PSK: u8 = 2;

/// The PMK of a passphrase network: PBKDF2-HMAC-SHA1(passphrase, SSID, 4096, 256 bits).
pub fn psk(ssid: &str, pass: &str) -> [u8; 32] {
    let mut k = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(pass.as_bytes(), ssid.as_bytes(), 4096, &mut k);
    k
}

fn hmac_sha1(key: &[u8], parts: &[&[u8]]) -> [u8; 20] {
    let mut m = <HmacSha1 as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    for p in parts {
        m.update(p);
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&m.finalize().into_bytes());
    out
}

/// PRF-n of IEEE 802.11 (12.7.1.2): HMAC-SHA1(K, A || 0 || B || i), i = 0, 1, ...
pub fn prf(key: &[u8], label: &[u8], data: &[u8], out: &mut [u8]) {
    let mut i = 0u8;
    let mut at = 0;
    while at < out.len() {
        let h = hmac_sha1(key, &[label, &[0], data, &[i]]);
        let n = (out.len() - at).min(20);
        out[at..at + n].copy_from_slice(&h[..n]);
        at += n;
        i += 1;
    }
}

#[derive(Clone, Copy)]
pub struct Ptk {
    pub kck: [u8; 16],
    pub kek: [u8; 16],
    pub tk: [u8; 16],
}

pub fn derive_ptk(pmk: &[u8; 32], aa: &[u8; 6], spa: &[u8; 6], anonce: &[u8; 32], snonce: &[u8; 32]) -> Ptk {
    let mut d = Vec::with_capacity(76);
    let (lo, hi) = if aa < spa { (aa, spa) } else { (spa, aa) };
    d.extend_from_slice(lo);
    d.extend_from_slice(hi);
    let (lo, hi) = if anonce < snonce { (anonce, snonce) } else { (snonce, anonce) };
    d.extend_from_slice(lo);
    d.extend_from_slice(hi);
    let mut p = [0u8; 48];
    prf(pmk, b"Pairwise key expansion", &d, &mut p);
    let mut k = Ptk { kck: [0; 16], kek: [0; 16], tk: [0; 16] };
    k.kck.copy_from_slice(&p[..16]);
    k.kek.copy_from_slice(&p[16..32]);
    k.tk.copy_from_slice(&p[32..48]);
    k
}

/// RFC 3394 AES key unwrap (128-bit KEK). None = integrity check failed.
pub fn aes_unwrap(kek: &[u8; 16], c: &[u8]) -> Option<Vec<u8>> {
    use aes::cipher::{BlockDecrypt, KeyInit};
    if c.len() < 24 || c.len() % 8 != 0 {
        return None;
    }
    let cipher = aes::Aes128::new_from_slice(kek).unwrap();
    let n = c.len() / 8 - 1;
    let mut a = [0u8; 8];
    a.copy_from_slice(&c[..8]);
    let mut r: Vec<[u8; 8]> = (0..n).map(|i| c[8 + i * 8..16 + i * 8].try_into().unwrap()).collect();
    for j in (0..6).rev() {
        for i in (0..n).rev() {
            let t = (n * j + i + 1) as u64;
            let mut b = [0u8; 16];
            let x = u64::from_be_bytes(a) ^ t;
            b[..8].copy_from_slice(&x.to_be_bytes());
            b[8..].copy_from_slice(&r[i]);
            cipher.decrypt_block(aes::Block::from_mut_slice(&mut b));
            a.copy_from_slice(&b[..8]);
            r[i].copy_from_slice(&b[8..]);
        }
    }
    if a != [0xA6; 8] {
        return None;
    }
    Some(r.concat())
}

/// RFC 3394 AES key wrap (the self-test's authenticator uses it).
pub fn aes_wrap(kek: &[u8; 16], p: &[u8]) -> Vec<u8> {
    use aes::cipher::{BlockEncrypt, KeyInit};
    let cipher = aes::Aes128::new_from_slice(kek).unwrap();
    let n = p.len() / 8;
    let mut a = [0xA6u8; 8];
    let mut r: Vec<[u8; 8]> = (0..n).map(|i| p[i * 8..i * 8 + 8].try_into().unwrap()).collect();
    for j in 0..6 {
        for i in 0..n {
            let mut b = [0u8; 16];
            b[..8].copy_from_slice(&a);
            b[8..].copy_from_slice(&r[i]);
            cipher.encrypt_block(aes::Block::from_mut_slice(&mut b));
            let t = (n * j + i + 1) as u64;
            a = (u64::from_be_bytes(b[..8].try_into().unwrap()) ^ t).to_be_bytes();
            r[i].copy_from_slice(&b[8..]);
        }
    }
    let mut out = a.to_vec();
    for x in r {
        out.extend_from_slice(&x);
    }
    out
}

/// The RSN element a supplicant sends: version 1, the network's group
/// cipher, pairwise CCMP, AKM PSK, no capabilities (no MFP).
pub fn rsn_ie(group: u8) -> [u8; 22] {
    [
        0x30, 20, 1, 0, // id, length, version
        0x00, 0x0F, 0xAC, group, // group cipher
        1, 0, 0x00, 0x0F, 0xAC, CIPHER_CCMP, // one pairwise suite
        1, 0, 0x00, 0x0F, 0xAC, AKM_PSK, // one AKM suite
        0, 0, // RSN capabilities
    ]
}

/// What a network's RSN element (body, after id + length) offers.
#[derive(Clone, Copy, Debug, Default)]
pub struct Rsn {
    pub group: u8,
    pub ccmp: bool,
    pub psk: bool,
    pub sae_only: bool,
    pub mfp_required: bool,
}

pub fn parse_rsn(b: &[u8]) -> Option<Rsn> {
    if b.len() < 2 || u16::from_le_bytes([b[0], b[1]]) != 1 {
        return None;
    }
    let mut r = Rsn { group: CIPHER_CCMP, ccmp: true, psk: true, ..Rsn::default() };
    let mut p = 2;
    if b.len() >= p + 4 {
        if b[p..p + 3] == OUI {
            r.group = b[p + 3];
        }
        p += 4;
    } else {
        return Some(r);
    }
    if b.len() >= p + 2 {
        let n = u16::from_le_bytes([b[p], b[p + 1]]) as usize;
        p += 2;
        r.ccmp = (0..n).any(|i| b.get(p + 4 * i..p + 4 * i + 4) == Some(&[0x00, 0x0F, 0xAC, CIPHER_CCMP]));
        p += 4 * n;
    }
    if b.len() >= p + 2 {
        let n = u16::from_le_bytes([b[p], b[p + 1]]) as usize;
        p += 2;
        let akm = |s: u8| (0..n).any(|i| b.get(p + 4 * i..p + 4 * i + 4) == Some(&[0x00, 0x0F, 0xAC, s]));
        r.psk = akm(AKM_PSK);
        r.sae_only = !r.psk && akm(8);
        p += 4 * n;
    }
    if b.len() >= p + 2 {
        let caps = u16::from_le_bytes([b[p], b[p + 1]]);
        r.mfp_required = caps & (1 << 6) != 0;
    }
    Some(r)
}

// key information bits
const KI_VERSION_2: u16 = 2;
const KI_PAIRWISE: u16 = 1 << 3;
const KI_INSTALL: u16 = 1 << 6;
const KI_ACK: u16 = 1 << 7;
const KI_MIC: u16 = 1 << 8;
const KI_SECURE: u16 = 1 << 9;
const KI_ENCRYPTED: u16 = 1 << 12;

/// An EAPOL-Key frame (from the 802.1X version byte on).
struct Key<'a> {
    version: u8,
    info: u16,
    replay: [u8; 8],
    nonce: [u8; 32],
    data: &'a [u8],
}

fn parse_key(f: &[u8]) -> Option<Key<'_>> {
    // version, type 3 = Key, body length, descriptor type 2 = RSN
    if f.len() < 99 || f[1] != 3 || f[4] != 2 {
        return None;
    }
    let body = u16::from_be_bytes([f[2], f[3]]) as usize;
    let dlen = u16::from_be_bytes([f[97], f[98]]) as usize;
    if 4 + body > f.len() || 99 + dlen > f.len() {
        return None;
    }
    Some(Key {
        version: f[0],
        info: u16::from_be_bytes([f[5], f[6]]),
        replay: f[9..17].try_into().ok()?,
        nonce: f[17..49].try_into().ok()?,
        data: &f[99..99 + dlen],
    })
}

fn mic_ok(kck: &[u8; 16], f: &[u8]) -> bool {
    let mut c = f.to_vec();
    c[81..97].fill(0);
    let m = hmac_sha1(kck, &[&c]);
    let mut d = 0u8;
    for i in 0..16 {
        d |= m[i] ^ f[81 + i];
    }
    d == 0
}

/// Build an EAPOL-Key frame and sign it with `kck`.
fn build_key(version: u8, info: u16, replay: &[u8; 8], nonce: &[u8; 32], data: &[u8], kck: &[u8; 16]) -> Vec<u8> {
    let body = 95 + data.len();
    let mut f = Vec::with_capacity(4 + body);
    f.push(version);
    f.push(3);
    f.extend_from_slice(&(body as u16).to_be_bytes());
    f.push(2);
    f.extend_from_slice(&info.to_be_bytes());
    f.extend_from_slice(&0u16.to_be_bytes()); // key length
    f.extend_from_slice(replay);
    f.extend_from_slice(nonce);
    f.extend_from_slice(&[0; 16 + 8 + 8 + 16]); // IV, RSC, reserved, MIC
    f.extend_from_slice(&(data.len() as u16).to_be_bytes());
    f.extend_from_slice(data);
    let m = hmac_sha1(kck, &[&f]);
    f[81..97].copy_from_slice(&m[..16]);
    f
}

/// The GTK key data element (KDE dd len 00-0F-AC:1) in unwrapped key data.
fn find_gtk(data: &[u8]) -> Option<(u8, Vec<u8>)> {
    let mut p = 0;
    while p + 2 <= data.len() {
        let (id, len) = (data[p], data[p + 1] as usize);
        if id == 0xDD && len == 0 {
            break; // padding
        }
        let v = data.get(p + 2..p + 2 + len)?;
        if id == 0xDD && len >= 6 && v[..3] == OUI && v[3] == 1 {
            return Some((v[4] & 3, v[6..].to_vec()));
        }
        p += 2 + len;
    }
    None
}

/// What to do after an EAPOL frame.
#[derive(Default)]
pub struct Step {
    /// EAPOL frame to send to the authenticator.
    pub reply: Option<Vec<u8>>,
    /// Install this pairwise TK (after the reply has been sent).
    pub ptk: Option<[u8; 16]>,
    /// Install this group key (key id, key) after the reply.
    pub gtk: Option<(u8, Vec<u8>)>,
    /// The 4-way handshake just completed.
    pub done: bool,
    pub error: Option<String>,
}

pub struct Supplicant {
    pmk: [u8; 32],
    aa: [u8; 6],
    spa: [u8; 6],
    group: u8,
    snonce: [u8; 32],
    ptk: Option<Ptk>,
    replay: Option<u64>,
    pub complete: bool,
}

impl Supplicant {
    pub fn new(pmk: [u8; 32], aa: [u8; 6], spa: [u8; 6], group: u8) -> Self {
        let mut snonce = [0u8; 32];
        crate::rng::fill(&mut snonce);
        Supplicant { pmk, aa, spa, group, snonce, ptk: None, replay: None, complete: false }
    }

    /// Handle one EAPOL frame from the authenticator.
    pub fn on_eapol(&mut self, f: &[u8]) -> Step {
        let mut s = Step::default();
        let Some(k) = parse_key(f) else { return s };
        if k.info & 7 != KI_VERSION_2 {
            s.error = Some(alloc::format!("key descriptor version {} (only 2: HMAC-SHA1/AES)", k.info & 7));
            return s;
        }
        if k.info & KI_ACK == 0 {
            return s;
        }
        let rc = u64::from_be_bytes(k.replay);
        // message 1 may repeat with the same counter; the others must grow
        if let Some(last) = self.replay {
            if rc < last || (rc == last && k.info & KI_MIC != 0) {
                return s;
            }
        }
        let pairwise = k.info & KI_PAIRWISE != 0;
        if pairwise && k.info & KI_MIC == 0 {
            // message 1: ANonce -> PTK, answer with message 2 (SNonce + our RSN element)
            let ptk = derive_ptk(&self.pmk, &self.aa, &self.spa, &k.nonce, &self.snonce);
            self.ptk = Some(ptk);
            self.replay = Some(rc);
            s.reply = Some(build_key(k.version, KI_VERSION_2 | KI_PAIRWISE | KI_MIC, &k.replay, &self.snonce, &rsn_ie(self.group), &ptk.kck));
            return s;
        }
        let Some(ptk) = self.ptk else { return s };
        if !mic_ok(&ptk.kck, f) {
            s.error = Some(String::from("MIC salah (kata sandi salah?)"));
            return s;
        }
        self.replay = Some(rc);
        let data = if k.info & KI_ENCRYPTED != 0 {
            match aes_unwrap(&ptk.kek, k.data) {
                Some(d) => d,
                None => {
                    s.error = Some(String::from("key data does not unwrap"));
                    return s;
                }
            }
        } else {
            k.data.to_vec()
        };
        if pairwise {
            // message 3: install the PTK and the GTK, answer with message 4
            if k.info & KI_INSTALL == 0 {
                return s;
            }
            s.reply = Some(build_key(k.version, KI_VERSION_2 | KI_PAIRWISE | KI_MIC | KI_SECURE, &k.replay, &[0; 32], &[], &ptk.kck));
            s.ptk = Some(ptk.tk);
            s.gtk = find_gtk(&data);
            if s.gtk.is_none() {
                s.error = Some(String::from("message 3 carries no group key"));
            }
            self.complete = true;
            s.done = true;
        } else {
            // group key handshake message 1: new GTK, answer with message 2
            s.gtk = find_gtk(&data);
            s.reply = Some(build_key(k.version, KI_VERSION_2 | KI_MIC | KI_SECURE, &k.replay, &[0; 32], &[], &ptk.kck));
        }
        s
    }
}

/// The PRF against Python's hmac/hashlib, RFC 3394's key-wrap vector, and a
/// whole 4-way + group handshake against an authenticator built here.
pub fn self_test() -> bool {
    let hex = |s: &str| -> Vec<u8> { (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect() };
    let pmk: [u8; 32] = hex("0dc0d6eb90555ed6419756b9a15ec3e3209b63df707dd508d14581f8982721af").try_into().unwrap();
    let aa = [0xa0, 0xa1, 0xa1, 0xa3, 0xa4, 0xa5];
    let spa = [0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5];
    let an: [u8; 32] = core::array::from_fn(|i| 0xe0 + i as u8);
    let sn: [u8; 32] = core::array::from_fn(|i| 0xc0 + i as u8);
    let p = derive_ptk(&pmk, &aa, &spa, &an, &sn);
    let prf_ok = p.kck[..] == hex("e79498d36bd81b9b41819b48f3c97f96")[..]
        && p.kek[..] == hex("6e620972f3d263238df571d65fa2b8fa")[..]
        && p.tk[..] == hex("d503e8b5891f81c74422468fc2d4dd39")[..];
    let kek: [u8; 16] = core::array::from_fn(|i| i as u8);
    let plain = hex("00112233445566778899aabbccddeeff");
    let wrapped = hex("1fa68b0a8112b447aef34bd8fb5a7b829d3e862371d2cfe5");
    let wrap_ok = aes_wrap(&kek, &plain) == wrapped
        && aes_unwrap(&kek, &wrapped).as_deref() == Some(&plain[..])
        && aes_unwrap(&kek, &{
            let mut w = wrapped.clone();
            w[5] ^= 1;
            w
        })
        .is_none();

    // the handshake: we play the access point
    let mut sup = Supplicant::new(pmk, aa, spa, CIPHER_CCMP);
    let m1 = build_key(2, KI_VERSION_2 | KI_PAIRWISE | KI_ACK, &1u64.to_be_bytes(), &an, &[], &[0; 16]);
    let st = sup.on_eapol(&m1);
    let Some(m2) = st.reply else { return report(prf_ok, wrap_ok, "no message 2") };
    let Some(k2) = parse_key(&m2) else { return report(prf_ok, wrap_ok, "message 2 unparsable") };
    let ptk = derive_ptk(&pmk, &aa, &spa, &an, &k2.nonce);
    if !mic_ok(&ptk.kck, &m2) || k2.data != rsn_ie(CIPHER_CCMP) {
        return report(prf_ok, wrap_ok, "message 2 MIC or RSN element wrong");
    }
    let gtk: [u8; 16] = core::array::from_fn(|i| 0x50 + i as u8);
    let mut kde = alloc::vec![0xDD, 22, 0x00, 0x0F, 0xAC, 1, 1, 0];
    kde.extend_from_slice(&gtk);
    kde.extend_from_slice(&[0xDD, 0, 0, 0, 0, 0, 0, 0]); // pad to a multiple of 8
    let m3 = build_key(
        2,
        KI_VERSION_2 | KI_PAIRWISE | KI_INSTALL | KI_ACK | KI_MIC | KI_SECURE | KI_ENCRYPTED,
        &2u64.to_be_bytes(),
        &an,
        &aes_wrap(&ptk.kek, &kde),
        &ptk.kck,
    );
    // a forged message 3 (wrong key) must be refused
    let forged = build_key(2, KI_VERSION_2 | KI_PAIRWISE | KI_INSTALL | KI_ACK | KI_MIC | KI_SECURE, &2u64.to_be_bytes(), &an, &[], &[7; 16]);
    let refused = sup.on_eapol(&forged).error.is_some();
    let st = sup.on_eapol(&m3);
    let four_ok = st.done
        && st.ptk == Some(ptk.tk)
        && st.gtk.as_ref().map(|(i, k)| (*i, &k[..])) == Some((1, &gtk[..]))
        && st.reply.as_ref().is_some_and(|m4| mic_ok(&ptk.kck, m4) && parse_key(m4).is_some_and(|k| k.info & KI_SECURE != 0));
    // group rekey
    let gtk2: [u8; 16] = [0x99; 16];
    let mut kde = alloc::vec![0xDD, 22, 0x00, 0x0F, 0xAC, 1, 2, 0];
    kde.extend_from_slice(&gtk2);
    kde.extend_from_slice(&[0xDD, 0, 0, 0, 0, 0, 0, 0]);
    let g1 = build_key(2, KI_VERSION_2 | KI_ACK | KI_MIC | KI_SECURE | KI_ENCRYPTED, &3u64.to_be_bytes(), &[0; 32], &aes_wrap(&ptk.kek, &kde), &ptk.kck);
    let st = sup.on_eapol(&g1);
    let group_ok = st.gtk.as_ref().map(|(i, k)| (*i, &k[..])) == Some((2, &gtk2[..])) && st.reply.is_some();
    // a replayed message is ignored
    let replay_ok = sup.on_eapol(&g1).reply.is_none();
    let all = prf_ok && wrap_ok && refused && four_ok && group_ok && replay_ok;
    if all {
        crate::println!("[kernel] MILESTONE: WPA2 OK (PTK PRF, AES key wrap, 4-way + group handshake, forged/replayed refused)");
    } else {
        crate::println!(
            "[wpa] self-test FAILED: prf {} wrap {} forged-refused {} 4-way {} group {} replay {}",
            prf_ok, wrap_ok, refused, four_ok, group_ok, replay_ok
        );
    }
    all
}

fn report(prf: bool, wrap: bool, what: &str) -> bool {
    crate::println!("[wpa] self-test FAILED: prf {} wrap {}: {}", prf, wrap, what);
    false
}
