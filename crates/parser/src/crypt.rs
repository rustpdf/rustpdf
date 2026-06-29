//! Standard security handler decryption (Fase 5.9): RC4 (V1/V2, R2–R4),
//! AESV2 (128-bit CBC) and AESV3 (256-bit CBC, V5/R6).
//!
//! On opening we derive the file encryption key from the (empty by default)
//! user password and decrypt strings/streams. R2–R4 use a per-object key
//! (Algorithm 1); R6 uses the file key directly (Algorithm 2.A/2.B).

use cos::{Dict, Object};
use md5::{Digest, Md5};
use sha2::{Sha256, Sha384, Sha512};

use crate::error::{PdfError, Result};

/// The 32-byte password padding string (PDF spec 7.6.3.3, Algorithm 2).
const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// Which cipher the standard handler uses for strings and streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cipher {
    Rc4,
    AesV2,
    AesV3,
    Identity,
}

/// A configured decryptor for one document.
#[derive(Debug, Clone)]
pub struct Decryptor {
    key: Vec<u8>,
    cipher: Cipher,
}

impl Decryptor {
    /// Build a decryptor from the `/Encrypt` dictionary and the first `/ID`
    /// element, authenticating `password` (empty string for unprotected-on-open
    /// files).
    pub fn new(encrypt: &Dict, id0: &[u8], password: &[u8]) -> Result<Decryptor> {
        let filter = encrypt.get("Filter").and_then(name_str);
        if filter.as_deref() != Some("Standard") {
            return Err(PdfError::Encryption(format!(
                "unsupported security handler {filter:?}"
            )));
        }
        let v = encrypt.get("V").and_then(int).unwrap_or(0);
        let r = encrypt.get("R").and_then(int).unwrap_or(0);

        // V5/R6 (AES-256): a single 32-byte file key, derived via Algorithm 2.A.
        if r >= 5 || v >= 5 {
            let u = string_bytes(encrypt.get("U")).unwrap_or_default();
            let ue = string_bytes(encrypt.get("UE")).unwrap_or_default();
            let o = string_bytes(encrypt.get("O")).unwrap_or_default();
            let oe = string_bytes(encrypt.get("OE")).unwrap_or_default();
            let key = compute_key_r6(password, &u, &ue, &o, &oe).ok_or_else(|| {
                PdfError::Encryption("AES-256: wrong password or malformed /U/UE".into())
            })?;
            let cipher = detect_cipher(encrypt, v);
            return Ok(Decryptor { key, cipher });
        }

        let length_bits = encrypt.get("Length").and_then(int).unwrap_or(40);
        let o = string_bytes(encrypt.get("O")).unwrap_or_default();
        let u = string_bytes(encrypt.get("U")).unwrap_or_default();
        let p = encrypt.get("P").and_then(int).unwrap_or(0) as i32 as u32;
        let encrypt_metadata = encrypt
            .get("EncryptMetadata")
            .map(|o| !matches!(o, Object::Bool(false)))
            .unwrap_or(true);
        let n = if r == 2 {
            5
        } else {
            (length_bits as usize / 8).clamp(5, 16)
        };
        let cipher = detect_cipher(encrypt, v);

        // Algorithm 2: derive the file key treating `password` as the user password.
        let key = compute_key(
            password,
            &o,
            p,
            id0,
            length_bits as usize,
            r,
            encrypt_metadata,
        );
        // Algorithm 6: authenticate by recomputing /U and comparing to the stored value.
        if authenticate_user(&key, id0, &u, r) {
            return Ok(Decryptor { key, cipher });
        }

        // Algorithm 7: maybe it's the owner password — recover the user password
        // from /O, re-derive the file key, and authenticate again.
        if let Some(user_pw) = recover_user_password(password, &o, n, r) {
            let key = compute_key(
                &user_pw,
                &o,
                p,
                id0,
                length_bits as usize,
                r,
                encrypt_metadata,
            );
            if authenticate_user(&key, id0, &u, r) {
                return Ok(Decryptor { key, cipher });
            }
        }

        // Some legacy files carry no usable /U (e.g. empty); only then fall back to
        // the unauthenticated key so we don't regress on malformed-but-openable docs.
        if u.is_empty() {
            return Ok(Decryptor { key, cipher });
        }

        Err(PdfError::Encryption(
            "wrong password (user or owner) for RC4/AES-128 document".into(),
        ))
    }

    /// Decrypt the bytes of object `(num, gen)` in place.
    pub fn decrypt(&self, num: u32, gen: u16, data: &[u8]) -> Vec<u8> {
        match self.cipher {
            Cipher::Identity => data.to_vec(),
            Cipher::Rc4 => rc4(&self.object_key(num, gen, false), data),
            Cipher::AesV2 => {
                let key = self.object_key(num, gen, true);
                aes_cbc_decrypt(&key, data).unwrap_or_else(|| data.to_vec())
            }
            // R6: the file key is used directly, with no per-object derivation.
            Cipher::AesV3 => aes256_cbc_decrypt(&self.key, data).unwrap_or_else(|| data.to_vec()),
        }
    }

    /// Per-object key (Algorithm 1): MD5(file_key || num_le[3] || gen_le[2]
    /// [|| "sAlT" for AES]), truncated to min(file_key_len + 5, 16).
    fn object_key(&self, num: u32, gen: u16, aes: bool) -> Vec<u8> {
        let mut h = Md5::new();
        h.update(&self.key);
        h.update(&num.to_le_bytes()[..3]);
        h.update(&gen.to_le_bytes()[..2]);
        if aes {
            h.update(b"sAlT");
        }
        let digest = h.finalize();
        let n = (self.key.len() + 5).min(16);
        digest[..n].to_vec()
    }
}

fn detect_cipher(encrypt: &Dict, v: i64) -> Cipher {
    if v >= 4 {
        // Look at the crypt filter named by /StmF (default StdCF).
        if let Some(Object::Dict(cf)) = encrypt.get("CF") {
            let stmf = encrypt.get("StmF").and_then(name_str).unwrap_or_default();
            if let Some(Object::Dict(filter)) = cf.get(&stmf) {
                return match filter.get("CFM").and_then(name_str).as_deref() {
                    Some("AESV2") => Cipher::AesV2,
                    Some("AESV3") => Cipher::AesV3,
                    Some("V2") => Cipher::Rc4,
                    Some("Identity") => Cipher::Identity,
                    _ => Cipher::Rc4,
                };
            }
        }
        Cipher::Rc4
    } else {
        Cipher::Rc4
    }
}

/// Algorithm 2: derive the file encryption key.
fn compute_key(
    password: &[u8],
    o: &[u8],
    p: u32,
    id0: &[u8],
    length_bits: usize,
    revision: i64,
    encrypt_metadata: bool,
) -> Vec<u8> {
    let mut padded = [0u8; 32];
    let take = password.len().min(32);
    padded[..take].copy_from_slice(&password[..take]);
    padded[take..].copy_from_slice(&PAD[..32 - take]);

    let mut h = Md5::new();
    h.update(padded);
    let mut o32 = [0u8; 32];
    let on = o.len().min(32);
    o32[..on].copy_from_slice(&o[..on]);
    h.update(o32);
    h.update(p.to_le_bytes());
    h.update(id0);
    if revision >= 4 && !encrypt_metadata {
        h.update([0xFF, 0xFF, 0xFF, 0xFF]);
    }
    let mut key = h.finalize().to_vec();

    let n = if revision == 2 {
        5
    } else {
        (length_bits / 8).clamp(5, 16)
    };
    if revision >= 3 {
        for _ in 0..50 {
            let mut h = Md5::new();
            h.update(&key[..n]);
            key = h.finalize().to_vec();
        }
    }
    key.truncate(n);
    key
}

/// Algorithm 4 (R2) / 5 (R3+) + 6: recompute `/U` from the file key and compare
/// it to the stored value to authenticate the user password.
fn authenticate_user(file_key: &[u8], id0: &[u8], stored_u: &[u8], revision: i64) -> bool {
    if revision == 2 {
        // Algorithm 4: /U = RC4(file_key, PAD); compare all 32 bytes.
        let computed = rc4(file_key, &PAD);
        stored_u.len() >= 32 && computed[..32] == stored_u[..32]
    } else {
        // Algorithm 5: RC4 chain over MD5(PAD || id0). Only the first 16 bytes are
        // meaningful (Algorithm 6 compares the first 16).
        let mut h = Md5::new();
        h.update(PAD);
        h.update(id0);
        let mut data = h.finalize().to_vec();
        data = rc4(file_key, &data);
        for i in 1..=19u8 {
            let key: Vec<u8> = file_key.iter().map(|b| b ^ i).collect();
            data = rc4(&key, &data);
        }
        stored_u.len() >= 16 && data[..16] == stored_u[..16]
    }
}

/// Algorithm 7: recover the padded user password from `/O` using the owner
/// password, mirroring how `/O` is built (Algorithm 3). Returns the 32-byte
/// padded user password, suitable as input to [`compute_key`].
fn recover_user_password(owner_pw: &[u8], o: &[u8], n: usize, revision: i64) -> Option<Vec<u8>> {
    if o.len() < 32 {
        return None;
    }
    // Owner key (Algorithm 3, steps a–d): MD5(pad(owner)), then 50 full-key
    // re-hashes for R3+, truncated to n.
    let mut padded = [0u8; 32];
    let take = owner_pw.len().min(32);
    padded[..take].copy_from_slice(&owner_pw[..take]);
    padded[take..].copy_from_slice(&PAD[..32 - take]);
    let mut key = {
        let mut h = Md5::new();
        h.update(padded);
        h.finalize().to_vec()
    };
    if revision >= 3 {
        for _ in 0..50 {
            let mut h = Md5::new();
            h.update(&key);
            key = h.finalize().to_vec();
        }
    }
    key.truncate(n);

    // Reverse the RC4 chain that produced /O.
    let mut data = o[..32].to_vec();
    if revision == 2 {
        data = rc4(&key, &data);
    } else {
        for i in (1..=19u8).rev() {
            let k: Vec<u8> = key.iter().map(|b| b ^ i).collect();
            data = rc4(&k, &data);
        }
        data = rc4(&key, &data);
    }
    Some(data)
}

/// RC4 stream cipher (used both for decryption and key setup).
fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut s: [u8; 256] = std::array::from_fn(|i| i as u8);
    let mut j = 0u8;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
        s.swap(i, j as usize);
    }
    let mut out = Vec::with_capacity(data.len());
    let (mut i, mut j) = (0u8, 0u8);
    for &byte in data {
        i = i.wrapping_add(1);
        j = j.wrapping_add(s[i as usize]);
        s.swap(i as usize, j as usize);
        let k = s[(s[i as usize].wrapping_add(s[j as usize])) as usize];
        out.push(byte ^ k);
    }
    out
}

/// AES-128-CBC decrypt; the 16-byte IV is prepended to the ciphertext.
fn aes_cbc_decrypt(key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    type Dec = cbc::Decryptor<aes::Aes128>;
    if data.len() < 16 {
        return None;
    }
    let (iv, ct) = data.split_at(16);
    let mut buf = ct.to_vec();
    let dec = Dec::new_from_slices(key, iv).ok()?;
    let pt = dec.decrypt_padded_mut::<Pkcs7>(&mut buf).ok()?;
    Some(pt.to_vec())
}

/// AES-256-CBC decrypt; the 16-byte IV is prepended to the ciphertext (V5/R6).
fn aes256_cbc_decrypt(key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    type Dec = cbc::Decryptor<aes::Aes256>;
    if data.len() < 16 {
        return None;
    }
    let (iv, ct) = data.split_at(16);
    let mut buf = ct.to_vec();
    let dec = Dec::new_from_slices(key, iv).ok()?;
    let pt = dec.decrypt_padded_mut::<Pkcs7>(&mut buf).ok()?;
    Some(pt.to_vec())
}

/// AES-CBC with an explicit IV and no padding (used in R6 key derivation and to
/// unwrap `/UE`/`/OE`). `key` selects AES-128 (16 bytes) or AES-256 (32 bytes).
fn aes_cbc_nopad(key: &[u8], iv: &[u8], data: &[u8], encrypt: bool) -> Option<Vec<u8>> {
    use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
    if data.len() % 16 != 0 {
        return None;
    }
    let mut buf = data.to_vec();
    match (key.len(), encrypt) {
        (16, true) => cbc::Encryptor::<aes::Aes128>::new_from_slices(key, iv)
            .ok()?
            .encrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(&mut buf, data.len())
            .ok()?,
        (32, false) => cbc::Decryptor::<aes::Aes256>::new_from_slices(key, iv)
            .ok()?
            .decrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(&mut buf)
            .ok()?,
        _ => return None,
    };
    Some(buf)
}

/// Algorithm 2.A (R6): derive the 32-byte file key from the user (then owner)
/// password. Returns `None` if neither password validates.
fn compute_key_r6(password: &[u8], u: &[u8], ue: &[u8], o: &[u8], oe: &[u8]) -> Option<Vec<u8>> {
    // User password: U = hash(32) || validation salt(8) || key salt(8).
    if u.len() >= 48 {
        let (hash, vsalt, ksalt) = (&u[..32], &u[32..40], &u[40..48]);
        if hash_2b(password, vsalt, &[]) == hash {
            let ik = hash_2b(password, ksalt, &[]);
            return aes_cbc_nopad(&ik, &[0u8; 16], ue, false);
        }
    }
    // Owner password: salts as above, but U (48 bytes) is mixed into the hash.
    if o.len() >= 48 && u.len() >= 48 {
        let (hash, vsalt, ksalt) = (&o[..32], &o[32..40], &o[40..48]);
        if hash_2b(password, vsalt, &u[..48]) == hash {
            let ik = hash_2b(password, ksalt, &u[..48]);
            return aes_cbc_nopad(&ik, &[0u8; 16], oe, false);
        }
    }
    None
}

/// Algorithm 2.B (R6): the iterated SHA-2 / AES-128 hash. Returns 32 bytes.
fn hash_2b(password: &[u8], salt: &[u8], udata: &[u8]) -> Vec<u8> {
    let mut k = {
        let mut h = Sha256::new();
        h.update(password);
        h.update(salt);
        h.update(udata);
        h.finalize().to_vec()
    };
    let mut round = 0;
    loop {
        // K1 = (password || K || udata) repeated 64 times.
        let mut block = Vec::with_capacity(password.len() + k.len() + udata.len());
        block.extend_from_slice(password);
        block.extend_from_slice(&k);
        block.extend_from_slice(udata);
        let mut k1 = Vec::with_capacity(block.len() * 64);
        for _ in 0..64 {
            k1.extend_from_slice(&block);
        }
        // E = AES-128-CBC(key=K[0..16], iv=K[16..32], K1), no padding.
        let e = aes_cbc_nopad(&k[..16], &k[16..32], &k1, true).unwrap_or_default();
        // First 16 bytes of E as a big-endian integer mod 3 (== sum mod 3).
        let m = e[..16].iter().map(|&b| b as u32).sum::<u32>() % 3;
        k = match m {
            0 => Sha256::digest(&e).to_vec(),
            1 => Sha384::digest(&e).to_vec(),
            _ => Sha512::digest(&e).to_vec(),
        };
        if round >= 63 && (*e.last().unwrap_or(&0) as i32) <= round - 32 {
            break;
        }
        round += 1;
    }
    k.truncate(32);
    k
}

fn name_str(o: &Object) -> Option<String> {
    match o {
        Object::Name(n) => Some(n.as_str().to_string()),
        _ => None,
    }
}

fn int(o: &Object) -> Option<i64> {
    match o {
        Object::Integer(n) => Some(*n),
        _ => None,
    }
}

fn string_bytes(o: Option<&Object>) -> Option<Vec<u8>> {
    match o {
        Some(Object::String(s)) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rc4_is_involutive() {
        // RC4 is symmetric: encrypting twice with the same key restores input.
        let key = b"Key";
        let pt = b"Plaintext";
        let ct = rc4(key, pt);
        assert_ne!(&ct, pt);
        assert_eq!(rc4(key, &ct), pt);
    }

    #[test]
    fn rc4_known_vector() {
        // RFC 6229-style: key "Key", "Plaintext" -> BBF316E8D940AF0AD3.
        let ct = rc4(b"Key", b"Plaintext");
        assert_eq!(
            ct,
            vec![0xBB, 0xF3, 0x16, 0xE8, 0xD9, 0x40, 0xAF, 0x0A, 0xD3]
        );
    }
}
