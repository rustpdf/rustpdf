//! Fase 7.3 tests: encrypt with our writer, then decrypt + extract with our own
//! parser (full round-trip). Confirmed against qpdf manually in the shell.

use pdf::{Document, EditableDoc, Encryption, Permissions};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

fn sample() -> Vec<u8> {
    let mut doc = Document::new();
    let font = doc.add_font_file(FONT).unwrap();
    doc.add_page()
        .text(font, 20.0)
        .at(72.0, 700.0)
        .show("Segredo 42 — café");
    doc.to_bytes().unwrap()
}

#[test]
fn rc4_encrypt_then_decrypt_roundtrip() {
    let mut doc = EditableDoc::load(sample()).unwrap();
    doc.encrypt_with(Encryption::Rc4, "", "owner", Permissions::read_only());
    let bytes = doc.to_bytes().unwrap();

    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Filter /Standard"));
    assert!(text.contains("/V 2") && text.contains("/R 3"));

    // Our parser opens it (empty user password) and decrypts the content.
    let extracted = pdf::extract_text(&bytes).unwrap();
    assert!(
        extracted.contains("Segredo 42 — café"),
        "got: {extracted:?}"
    );
}

#[test]
fn encryption_uses_random_iv_each_run() {
    // Two encryptions of the same document must differ (random IVs/salts/key),
    // yet both must still decrypt to the same content.
    let enc = || {
        let mut d = EditableDoc::load(sample()).unwrap();
        d.encrypt_with(Encryption::Aes256, "", "owner", Permissions::default());
        d.to_bytes().unwrap()
    };
    let a = enc();
    let b = enc();
    assert_ne!(
        a, b,
        "encrypted output should be randomized, not deterministic"
    );
    assert!(pdf::extract_text(&a).unwrap().contains("Segredo 42"));
    assert!(pdf::extract_text(&b).unwrap().contains("Segredo 42"));
}

#[test]
fn aes128_encrypt_then_decrypt_roundtrip() {
    let mut doc = EditableDoc::load(sample()).unwrap();
    doc.encrypt("", "owner", Permissions::default()); // AES-128
    let bytes = doc.to_bytes().unwrap();

    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/V 4") && text.contains("/R 4"));
    assert!(text.contains("/AESV2"));

    let extracted = pdf::extract_text(&bytes).unwrap();
    assert!(
        extracted.contains("Segredo 42 — café"),
        "got: {extracted:?}"
    );
}

#[test]
fn aes256_r6_encrypt_then_decrypt_roundtrip() {
    let mut doc = EditableDoc::load(sample()).unwrap();
    doc.encrypt_with(Encryption::Aes256, "", "owner", Permissions::default());
    let bytes = doc.to_bytes().unwrap();

    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/V 5") && text.contains("/R 6"));
    assert!(text.contains("/AESV3"));
    assert!(text.contains("/UE") && text.contains("/OE") && text.contains("/Perms"));

    let extracted = pdf::extract_text(&bytes).unwrap();
    assert!(
        extracted.contains("Segredo 42 — café"),
        "got: {extracted:?}"
    );
}

#[test]
fn permissions_are_encoded_in_p() {
    let mut doc = EditableDoc::load(sample()).unwrap();
    doc.encrypt("", "owner", Permissions::read_only());
    let bytes = doc.to_bytes().unwrap();
    // read_only clears the print (bit 3) and copy (bit 5) bits, so /P is a
    // specific negative value, not the all-allowed -4.
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/P "));
    assert!(!text.contains("/P -4"), "read_only must not be all-allowed");

    // Still decryptable + re-loadable.
    let reloaded = EditableDoc::load(&bytes).unwrap();
    assert_eq!(reloaded.page_count(), 1);
}
