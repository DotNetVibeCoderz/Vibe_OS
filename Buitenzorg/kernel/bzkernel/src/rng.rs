//! Kernel random numbers for keys (SSH host key, key exchange, salts): a
//! ChaCha20 generator whose key is SHA-256 over an entropy pool. Sources:
//! RDRAND where the x86 CPU has it, the jitter of the cycle counter around
//! short busy loops, the clocks, and a seed kept in `/disk/RANDOM.SEE` that
//! is replaced every boot (so two boots never start alike even on a board
//! without a hardware generator). Every request also mixes in the cycle
//! counter, and the key is ratcheted after each request (forward secrecy).

use chacha20::cipher::{KeyIvInit, StreamCipher};
use sha2::{Digest, Sha256};
use spin::Mutex;

const SEED_FILE: &str = "/disk/RANDOM.SEE";

struct Drbg {
    key: [u8; 32],
    counter: u64,
    seeded: bool,
}

static RNG: Mutex<Drbg> = Mutex::new(Drbg { key: [0; 32], counter: 0, seeded: false });

fn hardware(h: &mut Sha256) -> bool {
    let mut got = false;
    for _ in 0..16 {
        if let Some(v) = crate::arch::cpu::hardware_random() {
            h.update(v.to_le_bytes());
            got = true;
        }
    }
    got
}

/// Timing jitter: how long tiny busy loops take, in cycle-counter units.
fn jitter(h: &mut Sha256) {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    for i in 0..256u64 {
        let t0 = crate::arch::cpu::timestamp();
        for _ in 0..(i % 13 + 3) {
            x = x.rotate_left(7) ^ x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        }
        let t1 = crate::arch::cpu::timestamp();
        h.update((t1.wrapping_sub(t0) ^ x).to_le_bytes());
    }
}

/// Seed the generator (once `/disk` is mounted) and replace the stored seed.
pub fn init() {
    let mut h = Sha256::new();
    h.update(b"buitenzorg-rng-v1");
    let hw = hardware(&mut h);
    jitter(&mut h);
    h.update(crate::arch::cpu::nanos().to_le_bytes());
    h.update(crate::clock::utc_unix().to_le_bytes());
    let stored = crate::vfs::read(SEED_FILE).ok();
    if let Some(s) = &stored {
        h.update(s);
    }
    {
        let mut r = RNG.lock();
        r.key.copy_from_slice(&h.finalize());
        r.seeded = true;
    }
    let mut next = [0u8; 32];
    fill(&mut next);
    let saved = crate::vfs::write(SEED_FILE, &next).is_ok();
    crate::println!(
        "[rng] seeded: {}cycle jitter, clocks, {}; new seed {}",
        if hw { "RDRAND, " } else { "" },
        if stored.is_some() { "stored seed" } else { "no stored seed (first boot)" },
        if saved { "saved" } else { "not saved (read-only)" }
    );
}

/// Fill `out` with random bytes.
pub fn fill(out: &mut [u8]) {
    let mut r = RNG.lock();
    if !r.seeded {
        // before init: still better than nothing, never all-zero keys
        let mut h = Sha256::new();
        jitter(&mut h);
        r.key.copy_from_slice(&h.finalize());
        r.seeded = true;
    }
    // per-request freshness
    let mut h = Sha256::new();
    h.update(r.key);
    h.update(crate::arch::cpu::timestamp().to_le_bytes());
    let key: [u8; 32] = h.finalize().into();
    r.counter = r.counter.wrapping_add(1);
    let nonce = r.counter.to_le_bytes();
    let mut c = chacha20::ChaCha20Legacy::new((&key).into(), (&nonce).into());
    out.fill(0);
    c.apply_keystream(out);
    // ratchet: the next key comes from the stream, past what was handed out
    let mut next = [0u8; 32];
    c.apply_keystream(&mut next);
    r.key = next;
}

pub fn bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    fill(&mut b);
    b
}
