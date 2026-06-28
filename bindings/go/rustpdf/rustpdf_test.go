package rustpdf

import (
	"bytes"
	"image"
	"image/color"
	"image/png"
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

	// 3b. Page rendering (Pro feature; license already active).
	if n, err := PageCount(pdfa); err != nil || n != 1 {
		t.Fatalf("page count: %d err=%v", n, err)
	}
	png, err := RenderPageToPng(pdfa, 0, 72.0)
	if err != nil || len(png) < 8 || string(png[1:4]) != "PNG" {
		t.Fatalf("render: %d bytes err=%v", len(png), err)
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

	// 9. PAdES LTV: DSS (B-LT) then document timestamp (B-LTA). Regression test
	// for the cgo pointer-pinning bug — AddDss passes a [][]byte across cgo and
	// must not panic under the default cgo pointer checks.
	ca := mustRead(t, filepath.Join(fx, "signer_ca.der"))
	crl := mustRead(t, filepath.Join(fx, "test.crl"))
	lt, err := AddDss(signed, [][]byte{cert, ca}, [][]byte{crl})
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(lt, []byte("/DSS")) {
		t.Fatal("DSS dictionary missing")
	}
	tsaKey := mustRead(t, filepath.Join(fx, "tsa_key.pk8"))
	tsaCert := mustRead(t, filepath.Join(fx, "tsa_cert.der"))
	lta, err := Timestamp(lt, tsaKey, tsaCert, "")
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(lta, []byte("/DocTimeStamp")) {
		t.Fatal("DocTimeStamp missing")
	}

	// 10. verify_signatures on the freshly-signed doc.
	{
		reports, err := VerifySignatures(signed)
		if err != nil {
			t.Fatalf("verify signatures: %v", err)
		}
		if len(reports) != 1 {
			t.Fatalf("expected 1 signature, got %d", len(reports))
		}
		if reports[0].SubFilter == "" {
			t.Fatal("signature sub_filter missing")
		}
		if len(reports[0].ByteRange) != 4 {
			t.Fatalf("byte_range len=%d", len(reports[0].ByteRange))
		}
		// An unsigned doc reports no signatures.
		if empty, err := VerifySignatures(plain); err != nil {
			t.Fatal(err)
		} else if len(empty) != 0 {
			t.Fatalf("unsigned doc reported %d signatures", len(empty))
		}
	}

	// 11. Tier 1: hyperlinks + bookmarks on an authored Document.
	{
		d, _ := New()
		defer d.Close()
		f, _ := d.AddFontFile(font)
		d.AddPage()
		d.AddPage()
		d.ShowText(f, 14, 72, 700, "linked", 0)
		if err := d.LinkURI([4]float64{72, 690, 200, 710}, "https://example.com"); err != nil {
			t.Fatal(err)
		}
		top := 800.0
		if err := d.LinkToPage([4]float64{72, 660, 200, 680}, 1, &top); err != nil {
			t.Fatal(err)
		}
		if err := d.AddBookmark(Bookmark{
			Title: "Chapter 1", Page: 0,
			Children: []Bookmark{{Title: "Section 1.1", Page: 1, Top: &top}},
		}); err != nil {
			t.Fatal(err)
		}
		out, err := d.ToBytes()
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Contains(out, []byte("/Outlines")) {
			t.Fatal("outline missing")
		}
		if !bytes.Contains(out, []byte("/Link")) {
			t.Fatal("link annotation missing")
		}
	}

	// 12. Tier 2: Factur-X / ZUGFeRD invoice (license-gated).
	{
		d, _ := New()
		defer d.Close()
		f, _ := d.AddFontFile(font)
		d.AddPage()
		d.ShowText(f, 12, 72, 700, "Invoice", 0)
		invoiceXML := []byte(`<?xml version="1.0" encoding="UTF-8"?><Invoice/>`)
		if err := d.Facturx(invoiceXML, FacturxEN16931); err != nil {
			t.Fatal(err)
		}
		out, err := d.ToBytes()
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Contains(out, []byte("factur-x.xml")) {
			t.Fatal("Factur-X attachment missing")
		}
	}

	// 13. EditableDoc manipulation: form fields, flatten, watermark, redact, PDF/A.
	{
		// Build a form doc with every field type.
		d, _ := New()
		defer d.Close()
		f, _ := d.AddFontFile(font)
		d.AddPage()
		d.ShowText(f, 12, 72, 760, "Form", 0)
		d.TextField("city", 0, [4]float64{120, 700, 300, 720}, "", 12)
		d.Checkbox("ok", 0, [4]float64{120, 670, 138, 688}, false)
		d.RadioGroup("plan", 0, []RadioButton{
			{[4]float64{120, 640, 138, 658}, "a"},
			{[4]float64{160, 640, 178, 658}, "b"},
		}, -1)
		d.Dropdown("country", 0, [4]float64{120, 610, 300, 630}, []string{"BR", "PT"}, -1, 12)
		formPDF, err := d.ToBytes()
		if err != nil {
			t.Fatal(err)
		}

		ed, err := Load(formPDF)
		if err != nil {
			t.Fatal(err)
		}
		defer ed.Close()

		names, err := ed.FieldNames()
		if err != nil {
			t.Fatal(err)
		}
		if len(names) == 0 {
			t.Fatal("expected form field names")
		}

		if ok, err := ed.FillTextField("city", "SP"); err != nil || !ok {
			t.Fatalf("fill city: ok=%v err=%v", ok, err)
		}
		if ok, err := ed.SetCheckbox("ok", true); err != nil || !ok {
			t.Fatalf("set checkbox: ok=%v err=%v", ok, err)
		}
		if ok, err := ed.SetRadio("plan", "b"); err != nil || !ok {
			t.Fatalf("set radio: ok=%v err=%v", ok, err)
		}
		if ok, err := ed.SetChoice("country", "PT"); err != nil || !ok {
			t.Fatalf("set choice: ok=%v err=%v", ok, err)
		}
		// Non-existent field returns found=false (not an error).
		if ok, err := ed.SetCheckbox("nope", true); err != nil || ok {
			t.Fatalf("missing checkbox: ok=%v err=%v", ok, err)
		}

		if err := ed.WatermarkText("DRAFT", 64, 0.5, 0.5, 0.5, 0.3, 45); err != nil {
			t.Fatal(err)
		}
		if ok, err := ed.Redact(0, [][4]float64{{72, 755, 140, 775}}); err != nil || !ok {
			t.Fatalf("redact: ok=%v err=%v", ok, err)
		}
		if err := ed.FlattenForms(); err != nil {
			t.Fatal(err)
		}
		flat, err := ed.ToBytes()
		if err != nil {
			t.Fatal(err)
		}
		if bytes.Contains(flat, []byte("/AcroForm")) {
			t.Fatal("AcroForm should be gone after flatten")
		}
	}

	// 14. EditableDoc PDF/A conversion (license-gated; needs embedded fonts).
	{
		d, _ := New()
		defer d.Close()
		f, _ := d.AddFontFile(font)
		d.AddPage()
		d.ShowText(f, 12, 72, 700, "convertível", 0)
		base, err := d.ToBytes()
		if err != nil {
			t.Fatal(err)
		}
		ed, err := Load(base)
		if err != nil {
			t.Fatal(err)
		}
		defer ed.Close()
		if err := ed.ConvertToPdfa(A2b); err != nil {
			t.Fatal(err)
		}
		out, err := ed.ToBytes()
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Contains(out, []byte("pdfaid")) {
			t.Fatal("PDF/A identifier missing after conversion")
		}
	}

	// 15. Watermark with an image file.
	{
		ed, err := Load(plain)
		if err != nil {
			t.Fatal(err)
		}
		defer ed.Close()
		pngPath := filepath.Join(t.TempDir(), "wm.png")
		if err := os.WriteFile(pngPath, makePNG(t), 0o644); err != nil {
			t.Fatal(err)
		}
		if err := ed.WatermarkImageFile(pngPath, 64, 64, 0.3); err != nil {
			t.Fatal(err)
		}
		if _, err := ed.ToBytes(); err != nil {
			t.Fatal(err)
		}
	}
}

// makePNG encodes a tiny solid-color PNG in memory.
func makePNG(t *testing.T) []byte {
	t.Helper()
	img := image.NewRGBA(image.Rect(0, 0, 8, 8))
	for y := 0; y < 8; y++ {
		for x := 0; x < 8; x++ {
			img.Set(x, y, color.RGBA{R: 200, G: 40, B: 40, A: 255})
		}
	}
	var buf bytes.Buffer
	if err := png.Encode(&buf, img); err != nil {
		t.Fatalf("encode png: %v", err)
	}
	return buf.Bytes()
}

// TestExtractImagesToDir builds a PDF with one embedded image and extracts it.
func TestExtractImagesToDir(t *testing.T) {
	d, err := New()
	if err != nil {
		t.Fatal(err)
	}
	defer d.Close()
	if err := d.AddPage(); err != nil {
		t.Fatal(err)
	}
	img, err := d.AddImagePNG(makePNG(t))
	if err != nil {
		t.Fatal(err)
	}
	if err := d.DrawImage(img, 72, 600, 144, 144); err != nil {
		t.Fatal(err)
	}
	pdf, err := d.ToBytes()
	if err != nil {
		t.Fatal(err)
	}

	dir := t.TempDir()
	n, err := ExtractImagesToDir(pdf, dir)
	if err != nil {
		t.Fatalf("extract images: %v", err)
	}
	if n < 1 {
		t.Fatalf("expected at least one image, got %d", n)
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != n {
		t.Fatalf("count=%d but %d files written", n, len(entries))
	}
}
