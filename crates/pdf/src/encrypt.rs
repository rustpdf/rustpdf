//! Standard-handler encryption on write (Fase 7.3): RC4-128 (V2/R3), AESV2
//! (V4/R4, AES-128-CBC) and AESV3 (V5/R6, AES-256-CBC), with permission flags.
//! The user password is empty by default so viewers open the file directly
//! while still honoring permissions.
//!
//! Per-object AES IVs and, for R6, the file encryption key and password salts
//! come from the operating-system CSPRNG ([`getrandom`]), as the PDF spec
//! requires — so each encryption is unique. (Encrypted output is therefore not
//! byte-reproducible, but the determinism invariant only applies to plain,
//! unencrypted documents, which never reach this module.)

use cos::{Dict, Object, PdfString, Stream};
use md5::{Digest, Md5};
use sha2::{Sha256, Sha384, Sha512};

/// `n` cryptographically-secure random bytes from the OS CSPRNG. Falls back to
/// a hash-derived value only if the OS source is somehow unavailable, so we
/// never emit an all-zero IV/key.
fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    if getrandom::getrandom(&mut buf).is_err() {
        // Extremely rare; derive a non-constant fallback from the address space.
        let seed = (&buf as *const _ as usize as u64).to_le_bytes();
        for (i, b) in buf.iter_mut().enumerate() {
            *b = seed[i % 8] ^ (i as u8).wrapping_mul(31);
        }
    }
    buf
}

const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// Which cipher to use for the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encryption {
    /// RC4, 128-bit (V2 / R3).
    Rc4,
    /// AES-128-CBC (V4 / R4, `AESV2`).
    Aes128,
    /// AES-256-CBC (V5 / R6, `AESV3`).
    Aes256,
}

/// Document permission flags (PDF spec table 22). `true` = allowed.
#[derive(Debug, Clone, Copy)]
pub struct Permissions {
    pub print: bool,
    pub modify: bool,
    pub copy: bool,
    pub annotate: bool,
    pub fill_forms: bool,
    pub extract_accessibility: bool,
    pub assemble: bool,
    pub print_high_res: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        Permissions {
            print: true,
            modify: true,
            copy: true,
            annotate: true,
            fill_forms: true,
            extract_accessibility: true,
            assemble: true,
            print_high_res: true,
        }
    }
}

impl Permissions {
    /// Block everything except opening the document.
    pub fn read_only() -> Self {
        Permissions {
            print: false,
            modify: false,
            copy: false,
            annotate: false,
            fill_forms: false,
            extract_accessibility: true,
            assemble: false,
            print_high_res: false,
        }
    }

    /// The `/P` permission integer (reserved high bits set to 1).
    fn to_p(self) -> i32 {
        let mut p: u32 = 0xFFFF_FFFF;
        p &= !0b11; // bits 1–2 are reserved and must be 0
        let mut clear = |allowed: bool, bit: u32| {
            if !allowed {
                p &= !(1 << (bit - 1));
            }
        };
        clear(self.print, 3);
        clear(self.modify, 4);
        clear(self.copy, 5);
        clear(self.annotate, 6);
        clear(self.fill_forms, 9);
        clear(self.extract_accessibility, 10);
        clear(self.assemble, 11);
        clear(self.print_high_res, 12);
        p as i32
    }
}

/// Encryption request stored on an [`EditableDoc`](crate::EditableDoc).
#[derive(Debug, Clone)]
pub(crate) struct EncryptConfig {
    pub user: Vec<u8>,
    pub owner: Vec<u8>,
    pub perms: Permissions,
    pub method: Encryption,
}

/// Everything needed to encrypt object data and emit the `/Encrypt` dict.
pub(crate) struct Prepared {
    file_key: Vec<u8>,
    method: Encryption,
    pub dict: Dict,
}

impl Prepared {
    /// Encrypt one object's strings and stream body (generation 0).
    pub fn encrypt_object(&self, num: u32, obj: &Object) -> Object {
        match obj {
            Object::String(s) => Object::String(PdfString::literal(self.crypt(num, s.as_bytes()))),
            Object::Array(a) => {
                Object::Array(a.iter().map(|o| self.encrypt_object(num, o)).collect())
            }
            Object::Dict(d) => Object::Dict(self.encrypt_dict(num, d)),
            Object::Stream(s) => Object::Stream(Stream {
                dict: self.encrypt_dict(num, &s.dict),
                data: self.crypt(num, &s.data),
            }),
            other => other.clone(),
        }
    }

    fn encrypt_dict(&self, num: u32, dict: &Dict) -> Dict {
        // The `/Contents` of a signature / document-timestamp dict (identified
        // by `/ByteRange`) must be left in the clear (ISO 32000 §7.6.2), so it
        // round-trips through our own reader and validates in any viewer.
        let is_signature = dict.get("ByteRange").is_some();
        let mut out = Dict::new();
        for (k, v) in dict.iter() {
            if is_signature && k.as_str() == "Contents" {
                out.set(k.clone(), v.clone());
            } else {
                out.set(k.clone(), self.encrypt_object(num, v));
            }
        }
        out
    }

    /// Encrypt bytes for object `num` (gen 0).
    fn crypt(&self, num: u32, data: &[u8]) -> Vec<u8> {
        match self.method {
            Encryption::Rc4 => rc4(&object_key(&self.file_key, num, 0, false), data),
            Encryption::Aes128 => {
                aes128_cbc_encrypt(&object_key(&self.file_key, num, 0, true), data)
            }
            // R6: the file key is used directly, with no per-object derivation.
            Encryption::Aes256 => aes256_cbc_encrypt(&self.file_key, data),
        }
    }
}

/// Derive keys and build the `/Encrypt` dictionary for `config`.
pub(crate) fn prepare(config: &EncryptConfig, id0: Vec<u8>) -> Prepared {
    if config.method == Encryption::Aes256 {
        return prepare_r6(config, &id0);
    }
    let n = 16; // 128-bit
    let p = config.perms.to_p();

    let owner_key = owner_key(&config.owner, &config.user, n);
    let o = compute_o(&owner_key, &config.user);
    let file_key = file_key(&config.user, &o, p, &id0, n);
    let u = compute_u(&file_key, &id0);

    let mut dict = Dict::new()
        .with("Filter", Object::name("Standard"))
        .with("Length", 128)
        .with("P", p as i64)
        .with("O", PdfString::literal(o))
        .with("U", PdfString::literal(u));

    match config.method {
        Encryption::Rc4 => {
            dict.set("V", 2);
            dict.set("R", 3);
        }
        Encryption::Aes128 => {
            dict.set("V", 4);
            dict.set("R", 4);
            let std_cf = Dict::new()
                .with("CFM", Object::name("AESV2"))
                .with("AuthEvent", Object::name("DocOpen"))
                .with("Length", 16);
            let cf = Dict::new().with("StdCF", Object::Dict(std_cf));
            dict.set("CF", Object::Dict(cf));
            dict.set("StmF", Object::name("StdCF"));
            dict.set("StrF", Object::name("StdCF"));
            dict.set("EncryptMetadata", Object::Bool(true));
        }
        Encryption::Aes256 => unreachable!("handled by prepare_r6"),
    }

    Prepared {
        file_key,
        method: config.method,
        dict,
    }
}

/// Build the R6 (V5/AES-256) `/Encrypt` dictionary: file key, `/U`,`/UE`,
/// `/O`,`/OE`,`/Perms` (Algorithms 8–10). Salts and file key are derived
/// deterministically (see module note) to keep output byte-stable.
fn prepare_r6(config: &EncryptConfig, id0: &[u8]) -> Prepared {
    let p = config.perms.to_p();
    let _ = id0;
    let file_key = random_bytes(32);

    // Algorithm 8 — /U and /UE from the user password.
    let u_vsalt = random_bytes(8);
    let u_ksalt = random_bytes(8);
    let mut u = hash_2b(&config.user, &u_vsalt, &[]);
    u.extend_from_slice(&u_vsalt);
    u.extend_from_slice(&u_ksalt);
    let u_ik = hash_2b(&config.user, &u_ksalt, &[]);
    let ue = aes256_cbc_nopad(&u_ik, &[0u8; 16], &file_key);

    // Algorithm 9 — /O and /OE from the owner password (mixes in the 48-byte U).
    let owner = if config.owner.is_empty() {
        &config.user
    } else {
        &config.owner
    };
    let o_vsalt = random_bytes(8);
    let o_ksalt = random_bytes(8);
    let mut o = hash_2b(owner, &o_vsalt, &u);
    o.extend_from_slice(&o_vsalt);
    o.extend_from_slice(&o_ksalt);
    let o_ik = hash_2b(owner, &o_ksalt, &u);
    let oe = aes256_cbc_nopad(&o_ik, &[0u8; 16], &file_key);

    // Algorithm 10 — /Perms (AES-256-ECB of the permission block, file key).
    let perms = compute_perms(p, &file_key);

    let std_cf = Dict::new()
        .with("CFM", Object::name("AESV3"))
        .with("AuthEvent", Object::name("DocOpen"))
        .with("Length", 32);
    let cf = Dict::new().with("StdCF", Object::Dict(std_cf));
    let dict = Dict::new()
        .with("Filter", Object::name("Standard"))
        .with("V", 5)
        .with("R", 6)
        .with("Length", 256)
        .with("P", p as i64)
        .with("O", PdfString::literal(o))
        .with("U", PdfString::literal(u))
        .with("OE", PdfString::literal(oe))
        .with("UE", PdfString::literal(ue))
        .with("Perms", PdfString::literal(perms))
        .with("CF", Object::Dict(cf))
        .with("StmF", Object::name("StdCF"))
        .with("StrF", Object::name("StdCF"))
        .with("EncryptMetadata", Object::Bool(true));

    Prepared {
        file_key,
        method: config.method,
        dict,
    }
}

/// Algorithm 10: the 16-byte permission block, AES-256-ECB encrypted.
fn compute_perms(p: i32, file_key: &[u8]) -> Vec<u8> {
    use aes::cipher::{BlockEncrypt, KeyInit};
    let mut block = [0u8; 16];
    block[..4].copy_from_slice(&(p as u32).to_le_bytes());
    block[4..8].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    block[8] = b'T'; // EncryptMetadata = true
    block[9..12].copy_from_slice(b"adb");
    block[12..16].copy_from_slice(&random_bytes(4)); // bytes 12-15 are arbitrary
    let cipher = aes::Aes256::new_from_slice(file_key).expect("32-byte key");
    let mut b = block.into();
    cipher.encrypt_block(&mut b);
    b.to_vec()
}

/// A deterministic 16-byte document ID derived from object count.
pub(crate) fn derive_id(object_count: usize) -> Vec<u8> {
    let mut h = Md5::new();
    h.update(b"rust-pdf-id");
    h.update((object_count as u64).to_le_bytes());
    h.finalize().to_vec()
}

// ---- algorithms ------------------------------------------------------------

fn pad_password(pw: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let take = pw.len().min(32);
    out[..take].copy_from_slice(&pw[..take]);
    out[take..].copy_from_slice(&PAD[..32 - take]);
    out
}

/// The RC4 key derived from the owner password (Algorithm 3, steps a–d).
fn owner_key(owner: &[u8], user: &[u8], n: usize) -> Vec<u8> {
    let base = if owner.is_empty() { user } else { owner };
    let mut key = md5(&[&pad_password(base)]);
    for _ in 0..50 {
        key = md5(&[&key]);
    }
    key.truncate(n);
    key
}

/// The `/O` entry (Algorithm 3, steps e–f) for R3.
fn compute_o(owner_key: &[u8], user: &[u8]) -> Vec<u8> {
    let mut data = pad_password(user).to_vec();
    data = rc4(owner_key, &data);
    for i in 1..=19u8 {
        let key: Vec<u8> = owner_key.iter().map(|b| b ^ i).collect();
        data = rc4(&key, &data);
    }
    data
}

/// The file encryption key (Algorithm 2) for R3+.
fn file_key(user: &[u8], o: &[u8], p: i32, id0: &[u8], n: usize) -> Vec<u8> {
    let padded = pad_password(user);
    let p_bytes = (p as u32).to_le_bytes();
    let mut key = md5(&[&padded, o, &p_bytes, id0]);
    for _ in 0..50 {
        key.truncate(n);
        key = md5(&[&key]);
    }
    key.truncate(n);
    key
}

/// The `/U` entry (Algorithm 5) for R3+.
fn compute_u(file_key: &[u8], id0: &[u8]) -> Vec<u8> {
    let mut data = md5(&[&PAD, id0]);
    data = rc4(file_key, &data);
    for i in 1..=19u8 {
        let key: Vec<u8> = file_key.iter().map(|b| b ^ i).collect();
        data = rc4(&key, &data);
    }
    data.resize(32, 0); // pad to 32 bytes
    data
}

/// Per-object key (Algorithm 1).
fn object_key(file_key: &[u8], num: u32, gen: u16, aes: bool) -> Vec<u8> {
    let mut h = Md5::new();
    h.update(file_key);
    h.update(&num.to_le_bytes()[..3]);
    h.update(&gen.to_le_bytes()[..2]);
    if aes {
        h.update(b"sAlT");
    }
    let digest = h.finalize();
    let n = (file_key.len() + 5).min(16);
    digest[..n].to_vec()
}

fn md5(parts: &[&[u8]]) -> Vec<u8> {
    let mut h = Md5::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().to_vec()
}

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
        out.push(byte ^ s[(s[i as usize].wrapping_add(s[j as usize])) as usize]);
    }
    out
}

fn sha256(parts: &[&[u8]]) -> Vec<u8> {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().to_vec()
}

/// Algorithm 2.B (R6): iterated SHA-2 / AES-128 hash. Returns 32 bytes.
fn hash_2b(password: &[u8], salt: &[u8], udata: &[u8]) -> Vec<u8> {
    let mut k = sha256(&[password, salt, udata]);
    let mut round = 0;
    loop {
        let mut block = Vec::with_capacity(password.len() + k.len() + udata.len());
        block.extend_from_slice(password);
        block.extend_from_slice(&k);
        block.extend_from_slice(udata);
        let mut k1 = Vec::with_capacity(block.len() * 64);
        for _ in 0..64 {
            k1.extend_from_slice(&block);
        }
        let e = aes128_cbc_nopad_encrypt(&k[..16], &k[16..32], &k1);
        let m = e[..16].iter().map(|&b| b as u32).sum::<u32>() % 3;
        k = match m {
            0 => Sha256::digest(&e).to_vec(),
            1 => Sha384::digest(&e).to_vec(),
            _ => Sha512::digest(&e).to_vec(),
        };
        // ISO 32000-2 Algorithm 2.B numbers rounds 1-based: after at least 64
        // rounds, stop once the last byte of E is <= (round number) - 32.
        // Increment before the test so `round` is 1-based; testing with a
        // 0-based counter made the threshold one too low, so a few percent of
        // keys diverged from qpdf/pdfium and yielded files no other reader could
        // open (the same buggy hash on both sides only stayed self-consistent).
        round += 1;
        if round >= 64 && (*e.last().unwrap_or(&0) as i32) <= round - 32 {
            break;
        }
    }
    k.truncate(32);
    k
}

/// AES-128-CBC encrypt, explicit IV, no padding (R6 key derivation).
fn aes128_cbc_nopad_encrypt(key: &[u8], iv: &[u8], data: &[u8]) -> Vec<u8> {
    use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyIvInit};
    let mut buf = data.to_vec();
    let len = data.len();
    cbc::Encryptor::<aes::Aes128>::new_from_slices(key, iv)
        .expect("valid AES-128 key/iv")
        .encrypt_padded_mut::<NoPadding>(&mut buf, len)
        .expect("block-aligned");
    buf
}

/// AES-256-CBC encrypt, explicit IV, no padding (wraps `/UE` and `/OE`).
fn aes256_cbc_nopad(key: &[u8], iv: &[u8], data: &[u8]) -> Vec<u8> {
    use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyIvInit};
    let mut buf = data.to_vec();
    let len = data.len();
    cbc::Encryptor::<aes::Aes256>::new_from_slices(key, iv)
        .expect("valid AES-256 key/iv")
        .encrypt_padded_mut::<NoPadding>(&mut buf, len)
        .expect("block-aligned");
    buf
}

/// AES-256-CBC encrypt with a random per-object IV, prepended (R6 data).
fn aes256_cbc_encrypt(file_key: &[u8], data: &[u8]) -> Vec<u8> {
    use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    type Enc = cbc::Encryptor<aes::Aes256>;
    let iv = random_bytes(16);
    let enc = match Enc::new_from_slices(file_key, &iv[..16]) {
        Ok(e) => e,
        Err(_) => return data.to_vec(),
    };
    let mut buf = vec![0u8; (data.len() / 16 + 1) * 16];
    buf[..data.len()].copy_from_slice(data);
    match enc.encrypt_padded_mut::<Pkcs7>(&mut buf, data.len()) {
        Ok(ct) => {
            let mut out = iv[..16].to_vec();
            out.extend_from_slice(ct);
            out
        }
        Err(_) => data.to_vec(),
    }
}

/// AES-128-CBC encrypt with a random IV, prepended.
fn aes128_cbc_encrypt(key: &[u8], data: &[u8]) -> Vec<u8> {
    use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    type Enc = cbc::Encryptor<aes::Aes128>;

    let iv = random_bytes(16);
    let enc = match Enc::new_from_slices(key, &iv[..16]) {
        Ok(e) => e,
        Err(_) => return data.to_vec(),
    };
    let mut buf = vec![0u8; (data.len() / 16 + 1) * 16];
    buf[..data.len()].copy_from_slice(data);
    let ct = enc.encrypt_padded_mut::<Pkcs7>(&mut buf, data.len());
    match ct {
        Ok(ct) => {
            let mut out = iv[..16].to_vec();
            out.extend_from_slice(ct);
            out
        }
        Err(_) => data.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_all_allowed_is_minus_four() {
        assert_eq!(Permissions::default().to_p(), -4);
    }

    #[test]
    fn read_only_denies_print_and_modify() {
        let p = Permissions::read_only().to_p() as u32;
        assert_eq!(p & (1 << 2), 0, "print bit should be cleared");
        assert_eq!(p & (1 << 3), 0, "modify bit should be cleared");
    }

    #[test]
    fn rc4_roundtrips() {
        let k = b"0123456789abcdef";
        let ct = rc4(k, b"secret");
        assert_eq!(rc4(k, &ct), b"secret");
    }

    /// Known-answer test for ISO 32000-2 Algorithm 2.B (the R6/AES-256 key
    /// derivation). The expected value was computed by an independent
    /// reference implementation of the spec (1-based round counting, matching
    /// qpdf/pdfium). These inputs deliberately drive the loop past 64 rounds so
    /// the stopping condition is exercised: the previous 0-based counter stopped
    /// one round too early here and produced `0883e398…`, a key no other PDF
    /// reader could reproduce — the off-by-one that made ~a few percent of
    /// AES-256 files unreadable outside rust-pdf.
    #[test]
    fn hash_2b_matches_spec_at_round_boundary() {
        let salt = [0u8, 0, 0, 0, 0, 0, 0, 2];
        let got = hash_2b(b"pw", &salt, &[]);
        let expected =
            hex_to_bytes("edb92bcc700f47c957b9c76684a73c4dd6f6df4a33e474467d16977516f637ec");
        assert_eq!(got, expected, "Algorithm 2.B diverged from the spec");
    }

    fn hex_to_bytes(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
