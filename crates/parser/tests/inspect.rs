//! Issue #45 P1 #3: lightweight inspection probes, cross-validated against the
//! qpdf-generated fixture PDFs (RC4 40/128, AES-128, AES-256) so the cipher and
//! version detection is checked against real third-party output, not just our
//! own writer.

use parser::{header_version, probe_encryption};

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn header_version_is_read() {
    let v = header_version(fixture("base.pdf"));
    assert!(v.is_some(), "header version parsed");
    let v = v.unwrap();
    assert!(v.starts_with('1') || v.starts_with('2'), "got {v}");
}

#[test]
fn plain_fixture_not_encrypted() {
    let p = probe_encryption(fixture("base.pdf"));
    assert!(!p.encrypted);
    assert_eq!(p.cipher, "None");
    assert!(!p.requires_password);
}

#[test]
fn rc4_fixtures_detected() {
    for f in ["enc_rc4_40.pdf", "enc_rc4_128.pdf"] {
        let p = probe_encryption(fixture(f));
        assert!(p.encrypted, "{f} encrypted");
        assert_eq!(p.cipher, "RC4", "{f} cipher");
        // The fixtures open with the empty user password.
        assert!(!p.requires_password, "{f} opens with empty password");
    }
}

#[test]
fn aes128_fixture_detected() {
    let p = probe_encryption(fixture("enc_aes128.pdf"));
    assert!(p.encrypted);
    assert_eq!(p.cipher, "AES-128");
}

#[test]
fn aes256_fixture_detected() {
    let p = probe_encryption(fixture("enc_aes256.pdf"));
    assert!(p.encrypted);
    assert_eq!(p.cipher, "AES-256");
    assert_eq!(p.revision, 6, "AES-256 fixture is R6");
}
