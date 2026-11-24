//! The cryptographic primitives behind `cryptoapi.rs`, free of the runtime so
//! a host test can include them: SHA-2, HMAC, AES-GCM, ECDSA P-256, RSA
//! PKCS#1 v1.5 verification, the trusted root CAs.

use alloc::string::String;
use alloc::vec::Vec;

pub fn digest(alg: i32, data: &[u8]) -> Option<Vec<u8>> {
    use sha2::Digest;
    Some(match alg {
        0 => sha2::Sha256::digest(data).to_vec(),
        1 => sha2::Sha384::digest(data).to_vec(),
        2 => sha2::Sha512::digest(data).to_vec(),
        _ => return None,
    })
}

pub fn hmac(alg: i32, key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    use hmac::{Mac, SimpleHmac};
    macro_rules! run {
        ($h:ty) => {{
            let mut m = <SimpleHmac<$h> as Mac>::new_from_slice(key).ok()?;
            m.update(data);
            m.finalize().into_bytes().to_vec()
        }};
    }
    Some(match alg {
        0 => run!(sha2::Sha256),
        1 => run!(sha2::Sha384),
        2 => run!(sha2::Sha512),
        _ => return None,
    })
}

pub fn aes_gcm(encrypt: bool, key: &[u8], nonce: &[u8], aad: &[u8], input: &[u8]) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    if nonce.len() != 12 {
        return None;
    }
    let n = aes_gcm::Nonce::from_slice(nonce);
    let p = Payload { msg: input, aad };
    match key.len() {
        16 => {
            let c = aes_gcm::Aes128Gcm::new_from_slice(key).ok()?;
            if encrypt { c.encrypt(n, p).ok() } else { c.decrypt(n, p).ok() }
        }
        32 => {
            let c = aes_gcm::Aes256Gcm::new_from_slice(key).ok()?;
            if encrypt { c.encrypt(n, p).ok() } else { c.decrypt(n, p).ok() }
        }
        _ => None,
    }
}

pub fn random(out: &mut [u8]) -> bool {
    getrandom::getrandom(out).is_ok()
}

pub fn ecdsa_generate() -> Option<Vec<u8>> {
    for _ in 0..8 {
        let mut k = [0u8; 32];
        if !random(&mut k) {
            return None;
        }
        if let Ok(sk) = p256::ecdsa::SigningKey::from_slice(&k) {
            let pk = sk.verifying_key().to_encoded_point(false);
            let mut v = k.to_vec();
            v.extend_from_slice(pk.as_bytes());
            return Some(v);
        }
    }
    None
}

pub fn ecdsa_sign(private: &[u8], msg: &[u8]) -> Option<Vec<u8>> {
    use p256::ecdsa::signature::Signer;
    let sk = p256::ecdsa::SigningKey::from_slice(private).ok()?;
    let sig: p256::ecdsa::Signature = sk.sign(msg); // RFC 6979: deterministic
    Some(sig.to_bytes().to_vec())
}

pub fn ecdsa_verify(public: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    use p256::ecdsa::signature::Verifier;
    let (Ok(vk), Ok(s)) = (p256::ecdsa::VerifyingKey::from_sec1_bytes(public), p256::ecdsa::Signature::from_slice(sig)) else {
        return false;
    };
    vk.verify(msg, &s).is_ok()
}

pub fn rsa_verify(modulus: &[u8], exponent: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    use sha2::Digest;
    let n = rsa::BigUint::from_bytes_be(modulus);
    let e = rsa::BigUint::from_bytes_be(exponent);
    let Ok(key) = rsa::RsaPublicKey::new(n, e) else { return false };
    let hash = sha2::Sha256::digest(msg);
    key.verify(rsa::Pkcs1v15Sign::new::<sha2::Sha256>(), &hash, sig).is_ok()
}

/// The common name (OID 2.5.4.3) in a DER-encoded X.501 name, else the
/// organization (2.5.4.10).
fn common_name(der: &[u8]) -> String {
    for oid in [[0x55u8, 0x04, 0x03], [0x55, 0x04, 0x0A]] {
        let mut i = 0;
        while i + 5 < der.len() {
            if der[i] == 0x06 && der[i + 1] == 3 && der[i + 2..i + 5] == oid {
                let tag_at = i + 5;
                if let (Some(_), Some(&len)) = (der.get(tag_at), der.get(tag_at + 1)) {
                    let len = len as usize;
                    if len < 0x80 {
                        if let Some(s) = der.get(tag_at + 2..tag_at + 2 + len) {
                            return String::from_utf8_lossy(s).into_owned();
                        }
                    }
                }
            }
            i += 1;
        }
    }
    String::from("?")
}

pub fn root_count() -> usize {
    webpki_roots::TLS_SERVER_ROOTS.len()
}

pub fn root_name(i: usize) -> String {
    webpki_roots::TLS_SERVER_ROOTS.get(i).map(|t| common_name(t.subject.as_ref())).unwrap_or_default()
}
