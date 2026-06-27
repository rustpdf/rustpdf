package rustpdf

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func repoRoot(t *testing.T) string {
	t.Helper()
	dir, _ := os.Getwd()
	for i := 0; i < 12; i++ {
		if _, err := os.Stat(filepath.Join(dir, "Cargo.toml")); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}
	t.Fatal("could not locate repo root (Cargo.toml)")
	return ""
}

func mustRead(t *testing.T, path string) []byte {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return b
}

// One test exercises the whole surface serially (the license is process-global).
func TestFullSurface(t *testing.T) {
	root := repoRoot(t)
	font := filepath.Join(root, "assets", "fonts", "Roboto-Regular.ttf")
	devLicense := strings.TrimSpace(string(mustRead(t,
		filepath.Join(root, "crates", "license", "fixtures", "dev_license.txt"))))

	if Version() == "" {
		t.Fatal("empty version")
	}

	// 1. Corporate features blocked without a license.
	os.Unsetenv("RUSTPDF_LICENSE")
	os.Unsetenv("RUSTPDF_LICENSE_FILE")
	{
		d, err := New()
		if err != nil {
			t.Fatal(err)
		}
		defer d.Close()
		if err := d.Pdfa(); err != nil {
			t.Fatal(err)
		}
		if err := d.AddPage(); err != nil {
			t.Fatal(err)
		}
		if _, err := d.ToBytes(); err == nil {
			t.Fatal("PDF/A must be blocked without a license")
		}
	}

	// 2. Activate and build a tagged PDF/A-2a doc.
	if err := ActivateLicense(devLicense); err != nil {
		t.Fatalf("activate: %v", err)
	}
	var pdfa []byte
	{
		d, err := New()
		if err != nil {
			t.Fatal(err)
		}
		defer d.Close()
		if err := d.PdfaLevel(A2a); err != nil {
			t.Fatal(err)
		}
		if err := d.SetInfo(Info{Title: "Olá", Author: "rustpdf"}); err != nil {
			t.Fatal(err)
		}
		f, err := d.AddFontFile(font)
		if err != nil {
			t.Fatal(err)
		}
		if err := d.AddPage(); err != nil {
			t.Fatal(err)
		}
		if err := d.ShowText(f, 20, 72, 760, "Título", 1); err != nil {
			t.Fatal(err)
		}
		if err := d.Paragraph(f, 12, 72, 720, 450, strings.Repeat("Um parágrafo. ", 8), AlignJustify); err != nil {
			t.Fatal(err)
		}
		pdfa, err = d.ToBytes()
		if err != nil {
			t.Fatal(err)
		}
	}

	// 3. Text extraction.
	text, err := ExtractText(pdfa)
	if err != nil || !strings.Contains(text, "Título") {
		t.Fatalf("extract: %q err=%v", text, err)
	}

	// 4. Incremental update preserves the original prefix.
	{
		ed, err := Load(pdfa)
		if err != nil {
			t.Fatal(err)
		}
		defer ed.Close()
		if ed.PageCount() != 1 {
			t.Fatalf("pages=%d", ed.PageCount())
		}
		if err := ed.SetInfo("Subject", "via FFI"); err != nil {
			t.Fatal(err)
		}
		incr, err := ed.ToBytesIncremental(pdfa)
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.HasPrefix(incr, pdfa) {
			t.Fatal("incremental must preserve the original")
		}
	}

	// 5. Merge + optimize.
	{
		a, _ := Load(pdfa)
		b, _ := Load(pdfa)
		defer a.Close()
		defer b.Close()
		if err := a.Merge(b); err != nil {
			t.Fatal(err)
		}
		if err := a.Optimize(); err != nil {
			t.Fatal(err)
		}
		out, err := a.ToBytes()
		if err != nil {
			t.Fatal(err)
		}
		m, _ := Load(out)
		defer m.Close()
		if m.PageCount() != 2 {
			t.Fatalf("merged pages=%d", m.PageCount())
		}
	}

	// 6. AcroForm with every field type.
	{
		d, _ := New()
		defer d.Close()
		d.AddPage()
		d.TextField("city", 0, [4]float64{120, 700, 300, 720}, "SP", 12)
		d.Checkbox("ok", 0, [4]float64{120, 670, 138, 688}, true)
		d.RadioGroup("plan", 0, []RadioButton{
			{[4]float64{120, 640, 138, 658}, "a"},
			{[4]float64{160, 640, 178, 658}, "b"},
		}, 1)
		d.Dropdown("country", 0, [4]float64{120, 610, 300, 630}, []string{"BR", "PT"}, 0, 12)
		form, err := d.ToBytes()
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Contains(form, []byte("/AcroForm")) {
			t.Fatal("AcroForm missing")
		}
	}

	// 7. Encryption (AES-256) round-trips.
	var plain []byte
	{
		d, _ := New()
		defer d.Close()
		f, _ := d.AddFontFile(font)
		d.AddPage()
		d.ShowText(f, 14, 72, 700, "segredo", 0)
		plain, _ = d.ToBytes()
	}
	{
		ed, _ := Load(plain)
		defer ed.Close()
		if err := ed.Encrypt(AES256, "", "owner", false); err != nil {
			t.Fatal(err)
		}
		enc, err := ed.ToBytes()
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Contains(enc, []byte("/AESV3")) {
			t.Fatal("AES-256 marker missing")
		}
		dec, _ := ExtractText(enc)
		if !strings.Contains(dec, "segredo") {
			t.Fatalf("decrypted text: %q", dec)
		}
	}

	// 8. Digital signature (PKCS#7 / PAdES) with the committed test key.
	fx := filepath.Join(root, "crates", "pdf", "tests", "fixtures")
	key := mustRead(t, filepath.Join(fx, "signer_key.pk8"))
	cert := mustRead(t, filepath.Join(fx, "signer_cert.der"))
	signed, err := Sign(plain, key, cert, SignOptions{Reason: "Aprovado", PAdES: true})
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(signed, []byte("/ByteRange")) {
		t.Fatal("signature ByteRange missing")
	}
}
