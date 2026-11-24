//! Cryptography for managed programs (`Buitenzorg.Security.Cryptography` in
//! BZAPP.DLL): RustBCL has no System.Security.Cryptography, so the host lends
//! the RustCrypto primitives its TLS stack already links (rustls-rustcrypto).
//! Every native writes into a buffer the caller sized and returns the bytes
//! written, or -1 / 0 on failure:
//!
//! - `CryptoHash(alg, data, out)` -- SHA-256 / SHA-384 / SHA-512 (alg 0 / 1 / 2)
//! - `CryptoHmac(alg, key, data, out)` -- HMAC over the same hashes
//! - `CryptoAesGcm(encrypt, key, nonce, aad, input, out)` -- AES-128/256-GCM,
//!   12-byte nonce; encrypting writes ciphertext + 16-byte tag, decrypting
//!   checks the tag (-1 when it does not match)
//! - `CryptoEcdsa(op, a, b, c, out)` -- P-256: op 0 generate (out = 32-byte
//!   private key + 65-byte uncompressed public key), op 1 sign the SHA-256 of
//!   `b` with private key `a` (out = r || s, 64 bytes), op 2 verify signature
//!   `c` of `b` under public key `a` (1 = valid)
//! - `CryptoRsaVerify(modulus, exponent, data, signature)` -- PKCS#1 v1.5 with
//!   SHA-256 (1 = valid)
//! - `CryptoRandom(out)` -- bytes from the kernel's RANDOM syscall
//! - `CryptoRootCount()` / `CryptoRootName(i)` -- Mozilla's root CAs the TLS
//!   client trusts (webpki-roots), by common name

use alloc::vec::Vec;

use crate::crypto::*;

use rustclr_core::{ArrayStorage, ClrArray, ExecResult, Interpreter, Value};

fn bytes(i: &Interpreter, a: &[Value], k: usize) -> Vec<u8> {
    let Some(Value::Obj(h)) = a.get(k) else { return Vec::new() };
    if h.is_null() {
        return Vec::new();
    }
    i.heap
        .with::<ClrArray, _>(*h, |arr| match &arr.storage {
            ArrayStorage::U8(v) => v.clone(),
            ArrayStorage::I8(v) => v.iter().map(|b| *b as u8).collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

fn int(a: &[Value], k: usize) -> i32 {
    match a.get(k) {
        Some(Value::I32(v)) => *v,
        Some(Value::I64(v)) => *v as i32,
        _ => 0,
    }
}

fn put(i: &mut Interpreter, a: &[Value], k: usize, data: &[u8]) -> ExecResult<Option<Value>> {
    crate::bzapi::store_bytes(i, a, k, data);
    Ok(Some(Value::I32(data.len() as i32)))
}

fn n_hash(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    match digest(int(a, 0), &bytes(i, a, 1)) {
        Some(d) => put(i, a, 2, &d),
        None => Ok(Some(Value::I32(-1))),
    }
}

fn n_hmac(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    match hmac(int(a, 0), &bytes(i, a, 1), &bytes(i, a, 2)) {
        Some(d) => put(i, a, 3, &d),
        None => Ok(Some(Value::I32(-1))),
    }
}

fn n_aes_gcm(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = aes_gcm(int(a, 0) != 0, &bytes(i, a, 1), &bytes(i, a, 2), &bytes(i, a, 3), &bytes(i, a, 4));
    match r {
        Some(d) => put(i, a, 5, &d),
        None => Ok(Some(Value::I32(-1))),
    }
}

fn n_ecdsa(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let r = match int(a, 0) {
        0 => ecdsa_generate(),
        1 => ecdsa_sign(&bytes(i, a, 1), &bytes(i, a, 2)),
        2 => return Ok(Some(Value::I32(ecdsa_verify(&bytes(i, a, 1), &bytes(i, a, 2), &bytes(i, a, 3)) as i32))),
        _ => None,
    };
    match r {
        Some(d) => put(i, a, 4, &d),
        None => Ok(Some(Value::I32(-1))),
    }
}

fn n_rsa_verify(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let ok = rsa_verify(&bytes(i, a, 0), &bytes(i, a, 1), &bytes(i, a, 2), &bytes(i, a, 3));
    Ok(Some(Value::I32(ok as i32)))
}

fn n_random(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let n = bytes(i, a, 0).len();
    let mut v = alloc::vec![0u8; n];
    if !random(&mut v) {
        return Ok(Some(Value::I32(-1)));
    }
    put(i, a, 0, &v)
}

fn n_root_count(_i: &mut Interpreter, _a: &[Value]) -> ExecResult<Option<Value>> {
    Ok(Some(Value::I32(root_count() as i32)))
}

fn n_root_name(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let s = root_name(int(a, 0).max(0) as usize);
    Ok(Some(Value::Obj(i.alloc_string(&s))))
}

pub fn register(interp: &mut Interpreter) {
    let natives: [(&str, rustclr_core::interp::NativeFn); 8] = [
        ("CryptoHash", n_hash),
        ("CryptoHmac", n_hmac),
        ("CryptoAesGcm", n_aes_gcm),
        ("CryptoEcdsa", n_ecdsa),
        ("CryptoRsaVerify", n_rsa_verify),
        ("CryptoRandom", n_random),
        ("CryptoRootCount", n_root_count),
        ("CryptoRootName", n_root_name),
    ];
    for (name, f) in natives {
        interp.register_native(alloc::format!("pinvoke:buitenzorg!{}", name), f);
    }
}
