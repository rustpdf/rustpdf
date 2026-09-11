package rustpdf

import (
	"crypto"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/x509"
	"path/filepath"
	"testing"
)

// TestDeferredSigning proves the Model A "bring your own signer" flow end to
// end: rust-pdf builds the CMS signed attributes and calls our callback for the
// raw RSA signature (the private key never enters the library), then assembles
// and embeds the CMS — and the resulting signature validates. It also exercises
// the Model B two-phase BeginSigning flow and ListSignatures.
func TestDeferredSigning(t *testing.T) {
	root := repoRoot(t)
	font := filepath.Join(root, "assets", "fonts", "Roboto-Regular.ttf")
	fx := filepath.Join(root, "crates", "pdf", "tests", "fixtures")
	keyDER := mustRead(t, filepath.Join(fx, "signer_key.pk8"))
	certDER := mustRead(t, filepath.Join(fx, "signer_cert.der"))

	parsed, err := x509.ParsePKCS8PrivateKey(keyDER)
	if err != nil {
		t.Fatalf("parse PKCS#8 key: %v", err)
	}
	rsaKey, ok := parsed.(*rsa.PrivateKey)
	if !ok {
		t.Fatalf("expected an RSA private key, got %T", parsed)
	}

	// The "remote HSM": RSA PKCS#1 v1.5 over SHA-256 of the bytes rust-pdf hands us.
	signHash := func(data []byte) ([]byte, error) {
		digest := sha256.Sum256(data)
		return rsa.SignPKCS1v15(nil, rsaKey, crypto.SHA256, digest[:])
	}

	// Build a one-page PDF to sign.
	d, err := New()
	if err != nil {
		t.Fatal(err)
	}
	defer d.Close()
	f, err := d.AddFontFile(font)
	if err != nil {
		t.Fatal(err)
	}
	if err := d.AddPage(); err != nil {
		t.Fatal(err)
	}
	if err := d.ShowText(f, 14, 72, 700, "assinatura diferida", 0); err != nil {
		t.Fatal(err)
	}
	pdf, err := d.ToBytes()
	if err != nil {
		t.Fatal(err)
	}

	// No signature fields before signing.
	if before, err := ListSignatures(pdf); err != nil {
		t.Fatalf("list signatures (before): %v", err)
	} else if len(before) != 0 {
		t.Fatalf("expected 0 signature fields before signing, got %d", len(before))
	}

	// Model A: SignWith calls our callback for the raw RSA signature.
	opts := &SigningOptions{Reason: "Aprovado via HSM", Location: "BR", PAdES: true}
	signed, err := SignWith(pdf, certDER, signHash, nil, opts)
	if err != nil {
		t.Fatalf("SignWith: %v", err)
	}
	if len(signed) == 0 {
		t.Fatal("SignWith returned an empty document")
	}

	// The signature must validate.
	reports, err := VerifySignatures(signed)
	if err != nil {
		t.Fatalf("verify: %v", err)
	}
	if len(reports) != 1 {
		t.Fatalf("expected 1 signature, got %d", len(reports))
	}
	if !reports[0].IsValid {
		t.Fatalf("deferred signature is not valid: %+v", reports[0])
	}
	// Issue #41 P1: the rich certificate fields are populated for a real signer.
	if reports[0].Issuer == nil || *reports[0].Issuer == "" {
		t.Fatalf("expected an issuer in the report: %+v", reports[0])
	}
	if reports[0].CertCount < 1 {
		t.Fatalf("expected cert_count >= 1, got %d", reports[0].CertCount)
	}

	// ListSignatures sees exactly one (signed) field afterwards.
	after, err := ListSignatures(signed)
	if err != nil {
		t.Fatalf("list signatures (after): %v", err)
	}
	if len(after) != 1 {
		t.Fatalf("expected 1 signature field after signing, got %d", len(after))
	}
	if !after[0].Signed {
		t.Fatalf("expected the field to report as signed: %+v", after[0])
	}

	// Model B: BeginSigning yields a non-empty prepared doc + bytes + a 32-byte hash.
	session, err := BeginSigning(pdf, opts)
	if err != nil {
		t.Fatalf("BeginSigning: %v", err)
	}
	if len(session.Document()) == 0 {
		t.Fatal("BeginSigning produced an empty prepared document")
	}
	if len(session.Bytes()) == 0 {
		t.Fatal("BeginSigning produced no bytes-to-sign")
	}
	if h := session.Hash(); len(h) != 32 {
		t.Fatalf("expected a 32-byte hash, got %d", len(h))
	}
}
