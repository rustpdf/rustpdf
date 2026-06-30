//! Sign a PDF with an ICP-Brasil certificate held in the macOS Keychain, using
//! the deferred `sign_with` ("bring your own signer") path — the private key
//! never leaves the Keychain. The raw RSA signature is produced by an external
//! `sign_helper` (Swift, `SecKeyCreateSignature`); this program only assembles
//! and embeds the CMS.
//!
//! Config via env vars:
//!   RUSTPDF_ICP_CERT    DER of the signer certificate (required)
//!   RUSTPDF_ICP_CHAIN   `:`-separated DER chain certs (intermediates + root)
//!   RUSTPDF_ICP_HELPER  path to the compiled sign_helper (required)
//!   RUSTPDF_ICP_SHA1    certificate SHA-1 hex the helper matches (required)
//!   RUSTPDF_ICP_OUT     output PDF path (required)
//!   RUSTPDF_ICP_SCRATCH a writable dir for the tbs/sig temp files (required)

use std::process::Command;

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);
const DEV_LICENSE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../license/fixtures/dev_license.txt"
));

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("missing env var {key}"))
}

fn hex_decode(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

/// Optional ICP-Brasil signature policy (e.g. AD-RB) from env vars:
///   RUSTPDF_ICP_POLICY_OID / RUSTPDF_ICP_POLICY_HASH (hex) / RUSTPDF_ICP_POLICY_URI
fn policy_from_env() -> Option<pdf::SignaturePolicy> {
    let oid = std::env::var("RUSTPDF_ICP_POLICY_OID").ok()?;
    Some(pdf::SignaturePolicy {
        oid,
        hash: std::env::var("RUSTPDF_ICP_POLICY_HASH")
            .ok()
            .map(|h| hex_decode(&h))
            .unwrap_or_default(),
        hash_algorithm_oid: None, // SHA-256
        uri: std::env::var("RUSTPDF_ICP_POLICY_URI").ok(),
    })
}

fn main() {
    pdf::activate_license(DEV_LICENSE.trim()).expect("activate dev license");

    let cert = std::fs::read(env("RUSTPDF_ICP_CERT")).expect("read cert");
    let chain: Vec<Vec<u8>> = std::env::var("RUSTPDF_ICP_CHAIN")
        .unwrap_or_default()
        .split(':')
        .filter(|p| !p.is_empty())
        .map(|p| std::fs::read(p).expect("read chain cert"))
        .collect();
    let helper = env("RUSTPDF_ICP_HELPER");
    let sha1 = env("RUSTPDF_ICP_SHA1");
    let out = env("RUSTPDF_ICP_OUT");
    let scratch = env("RUSTPDF_ICP_SCRATCH");

    // Build a small document to sign.
    let mut doc = pdf::Document::new();
    let f = doc.add_font_file(FONT).expect("font");
    {
        let page = doc.add_page();
        page.text(f, 16.0)
            .at(72.0, 740.0)
            .show("Assinatura digital ICP-Brasil");
        page.text(f, 11.0)
            .at(72.0, 710.0)
            .show("Documento assinado com e-CNPJ (RFB e-CNPJ A1) via rust-pdf.");
        page.text(f, 11.0)
            .at(72.0, 690.0)
            .show("A chave privada permaneceu no Keychain do macOS (sign_with / Model A).");
        page.text(f, 11.0)
            .at(72.0, 670.0)
            .show("PAdES-B-B (ETSI.CAdES.detached + signing-certificate-v2).");
    }
    let pdf_bytes = doc.to_bytes().expect("build pdf");

    let opts = pdf::SignOptions {
        reason: Some("Teste de conformidade ICP-Brasil - rust-pdf".into()),
        location: Some("Brasil".into()),
        // The signer name (optional `/Name`) can be supplied via env; the binding
        // would typically read it from the certificate subject.
        name: std::env::var("RUSTPDF_ICP_NAME").ok(),
        pades: true,
        policy: policy_from_env(),
        // ICP-Brasil VALIDAR requires DocMDP on the first signature.
        certification: match std::env::var("RUSTPDF_ICP_CERTIFY").ok().as_deref() {
            Some("1") => Some(pdf::Certify::Locked),
            Some("2") => Some(pdf::Certify::Forms),
            Some("3") => Some(pdf::Certify::FormsAndAnnotations),
            _ => None,
        },
        ..Default::default()
    };
    if opts.policy.is_some() {
        println!("embedding signature policy (PAdES-EPES)");
    }

    let tbs_path = format!("{scratch}/tbs.bin");
    let sig_path = format!("{scratch}/sig.bin");

    // The callback: hand the to-be-signed bytes to the Keychain helper and read
    // back the raw RSA signature. rust-pdf builds the CMS around it.
    let signed = pdf::sign_with(&pdf_bytes, &cert, &chain, &opts, |bytes| {
        std::fs::write(&tbs_path, bytes)
            .map_err(|e| pdf::SignError::Key(format!("write tbs: {e}")))?;
        let status = Command::new(&helper)
            .args([&sha1, &tbs_path, &sig_path])
            .status()
            .map_err(|e| pdf::SignError::Key(format!("spawn helper: {e}")))?;
        if !status.success() {
            return Err(pdf::SignError::Key("keychain helper failed".into()));
        }
        std::fs::read(&sig_path).map_err(|e| pdf::SignError::Key(format!("read sig: {e}")))
    })
    .expect("sign_with");

    std::fs::write(&out, &signed).expect("write output");
    println!("signed PDF written to {out} ({} bytes)", signed.len());

    // Self-check with our own verifier (integrity + CMS signature math).
    match pdf::verify_signatures(&signed) {
        Ok(reports) => {
            for r in &reports {
                println!(
                    "  field={:?} subfilter={} digest_valid={} signature_valid={} covers_whole_doc={}",
                    r.field_name, r.sub_filter, r.digest_valid, r.signature_valid, r.covers_whole_document
                );
            }
        }
        Err(e) => println!("  verify error: {e}"),
    }
}
