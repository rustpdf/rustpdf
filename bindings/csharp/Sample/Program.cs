// Smoke test / demo for the RustPdf .NET binding. Exercises the whole surface,
// including licensing gating. Exits non-zero on any failed assertion.

using RustPdf;

static string RepoRoot()
{
    var dir = AppContext.BaseDirectory;
    for (int i = 0; i < 12 && dir is not null; i++)
    {
        if (File.Exists(Path.Combine(dir, "Cargo.toml")))
            return dir;
        dir = Path.GetDirectoryName(dir.TrimEnd(Path.DirectorySeparatorChar));
    }
    throw new Exception("could not locate repo root (Cargo.toml)");
}

static void Assert(bool cond, string msg)
{
    if (!cond)
        throw new Exception("ASSERT FAILED: " + msg);
}

var root = RepoRoot();
var font = Path.Combine(root, "assets", "fonts", "Roboto-Regular.ttf");
var devLicense = File.ReadAllText(Path.Combine(root, "crates", "license", "fixtures", "dev_license.txt")).Trim();

Console.WriteLine($"rustpdf version: {Pdf.Version()}");

// 1. Corporate features are blocked until a license is activated.
bool blocked = false;
try
{
    using var d = new Document();
    d.Pdfa().AddPage();
    d.ToBytes();
}
catch (PdfException)
{
    blocked = true;
}
Assert(blocked, "PDF/A must be blocked without a license");

Pdf.ActivateLicense(devLicense);
Console.WriteLine("license activated");

// 2. Build a tagged PDF/A-2a doc with a font, heading and justified paragraph.
byte[] pdfa;
using (var doc = new Document())
{
    doc.Pdfa(PdfaLevel.A2a).SetInfo(title: "Olá", author: "rustpdf");
    int f = doc.AddFontFile(font);
    doc.AddPage();
    doc.ShowText(f, 20, 72, 760, "Título", headingLevel: 1);
    doc.Paragraph(f, 12, 72, 720, 450, string.Concat(Enumerable.Repeat("Um parágrafo. ", 8)), Align.Justify);
    pdfa = doc.ToBytes();
}
Assert(pdfa.Length > 0, "pdfa bytes");
var text = Pdf.ExtractText(pdfa);
Assert(text.Contains("Título"), $"extracted text: {text}");
Console.WriteLine($"built PDF/A-2a ({pdfa.Length} bytes); extracted ok");

// Page rendering (Pro feature; license already active).
Assert(Pdf.PageCount(pdfa) == 1, "page count");
var png = Pdf.RenderPageToPng(pdfa, 0, 72.0);
Assert(png.Length > 8 && png[1] == 0x50 && png[2] == 0x4E && png[3] == 0x47, "PNG header");
Console.WriteLine($"rendered page 0 → {png.Length} byte PNG");

// 3. Manipulation: incremental update preserves the original prefix.
byte[] incr;
using (var ed = EditableDoc.Load(pdfa))
{
    Assert(ed.PageCount == 1, "page count");
    ed.SetInfo("Subject", "via FFI");
    Assert(ed.GetInfo("Subject") == "via FFI", "get_info");
    incr = ed.ToBytesIncremental(pdfa);
}
Assert(incr.AsSpan(0, pdfa.Length).SequenceEqual(pdfa), "incremental preserves original");
Console.WriteLine($"incremental update ok ({incr.Length} bytes)");

// 4. Merge + optimize.
using (var a = EditableDoc.Load(pdfa))
using (var b = EditableDoc.Load(pdfa))
{
    a.Merge(b).Optimize();
    using var merged = EditableDoc.Load(a.ToBytes());
    Assert(merged.PageCount == 2, "merged page count");
}
Console.WriteLine("merge + optimize ok");

// 5. AcroForm with every field type.
byte[] form;
using (var doc = new Document())
{
    doc.AddPage();
    doc.TextField("city", 0, (120, 700, 300, 720), "SP", 12);
    doc.Checkbox("ok", 0, (120, 670, 138, 688), true);
    doc.RadioGroup("plan", 0, new[] { ((120.0, 640.0, 138.0, 658.0), "a"), ((160.0, 640.0, 178.0, 658.0), "b") }, selected: 1);
    doc.Dropdown("country", 0, (120, 610, 300, 630), new[] { "BR", "PT" }, selected: 0, size: 12);
    form = doc.ToBytes();
}
Assert(System.Text.Encoding.Latin1.GetString(form).Contains("/AcroForm"), "AcroForm present");
Console.WriteLine("forms ok");

// 6. Encryption (AES-256) round-trips.
byte[] plain;
using (var doc = new Document())
{
    int f = doc.AddFontFile(font);
    doc.AddPage();
    doc.ShowText(f, 14, 72, 700, "segredo");
    plain = doc.ToBytes();
}
byte[] enc;
using (var ed = EditableDoc.Load(plain))
{
    ed.Encrypt(owner: "owner", method: Encryption.Aes256);
    enc = ed.ToBytes();
}
Assert(System.Text.Encoding.Latin1.GetString(enc).Contains("/AESV3"), "AES-256 marker");
Assert(Pdf.ExtractText(enc).Contains("segredo"), "decrypted text");
Console.WriteLine("encryption ok");

// 7. Digital signature (PKCS#7 / PAdES) with the committed test key.
var fx = Path.Combine(root, "crates", "pdf", "tests", "fixtures");
var key = File.ReadAllBytes(Path.Combine(fx, "signer_key.pk8"));
var cert = File.ReadAllBytes(Path.Combine(fx, "signer_cert.der"));
var signed = Pdf.Sign(plain, key, cert, reason: "Aprovado", pades: true);
Assert(System.Text.Encoding.Latin1.GetString(signed).Contains("/ByteRange"), "signature ByteRange");
Console.WriteLine($"signed ok ({signed.Length} bytes)");

// 8. Extract raster images to a directory.
var imgDir = Path.Combine(Path.GetTempPath(), "rustpdf-images-" + Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(imgDir);
int imgCount = Pdf.ExtractImagesToDir(pdfa, imgDir);
Console.WriteLine($"extracted {imgCount} image(s) to {imgDir}");

// 9. Hyperlinks + bookmarks (Tier 1, Document authoring).
byte[] nav;
using (var doc = new Document())
{
    int f = doc.AddFontFile(font);
    doc.AddPage();
    doc.ShowText(f, 14, 72, 700, "Page 1 — see Anthropic");
    doc.LinkUri((72, 695, 300, 715), "https://www.anthropic.com");
    doc.LinkToPage((72, 670, 200, 690), 1);
    doc.AddPage();
    doc.ShowText(f, 14, 72, 700, "Page 2");
    doc.AddBookmark(new Bookmark("Cover", 0)
        .Child(new Bookmark("Intro", 0, top: 700))
        .Child(new Bookmark("Details", 1, top: 700)));
    nav = doc.ToBytes();
}
var navStr = System.Text.Encoding.Latin1.GetString(nav);
Assert(navStr.Contains("/Annots"), "link annotations present");
Assert(navStr.Contains("/URI"), "URI action present");
Assert(navStr.Contains("/Outlines"), "outline dictionary present");
Console.WriteLine($"links + bookmarks ok ({nav.Length} bytes)");

// 10. Factur-X / ZUGFeRD e-invoice (Tier 2, license-gated).
const string invoiceXml =
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n" +
    "<rsm:CrossIndustryInvoice xmlns:rsm=\"urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100\">" +
    "<rsm:ExchangedDocument><ram:ID>INV-2026-001</ram:ID></rsm:ExchangedDocument>" +
    "</rsm:CrossIndustryInvoice>";
byte[] facturx;
using (var doc = new Document())
{
    int f = doc.AddFontFile(font);
    doc.AddPage();
    doc.ShowText(f, 18, 72, 760, "Invoice INV-2026-001");
    doc.Facturx(System.Text.Encoding.UTF8.GetBytes(invoiceXml), FacturxProfile.En16931);
    facturx = doc.ToBytes();
}
var fxStr = System.Text.Encoding.Latin1.GetString(facturx);
Assert(fxStr.Contains("factur-x.xml"), "factur-x.xml embedded");
Assert(fxStr.Contains("<pdfaid:part>3</pdfaid:part>"), "factur-x marks PDF/A-3");
Console.WriteLine($"factur-x ok ({facturx.Length} bytes)");

// 11. Form filling: text/checkbox/radio/choice + field_names + flatten.
byte[] richForm;
using (var doc = new Document())
{
    doc.AddPage();
    doc.TextField("city", 0, (120, 700, 300, 720), "", 12);
    doc.Checkbox("ok", 0, (120, 670, 138, 688), false);
    doc.RadioGroup("plan", 0, new[] { ((120.0, 640.0, 138.0, 658.0), "a"), ((160.0, 640.0, 178.0, 658.0), "b") });
    doc.Dropdown("country", 0, (120, 610, 300, 630), new[] { "BR", "PT" }, size: 12);
    richForm = doc.ToBytes();
}
byte[] filled;
using (var ed = EditableDoc.Load(richForm))
{
    var names = ed.FieldNames();
    Assert(names.Contains("city"), $"field_names contains city: [{string.Join(", ", names)}]");
    Assert(ed.FillTextField("city", "São Paulo"), "fill text field found");
    Assert(ed.SetCheckbox("ok", true), "set checkbox found");
    Assert(ed.SetRadio("plan", "b"), "set radio found");
    Assert(ed.SetChoice("country", "PT"), "set choice found");
    Assert(!ed.SetCheckbox("missing"), "missing checkbox not found");
    ed.FlattenForms();
    filled = ed.ToBytes();
}
Assert(filled.Length > 0, "flattened form bytes");
Console.WriteLine("form fill + flatten + field_names ok");

// 12. Watermark + redaction (Tier 1 + Tier 2).
byte[] stamped;
using (var ed = EditableDoc.Load(pdfa))
{
    ed.WatermarkText("CONFIDENTIAL", size: 60, color: (0.7, 0.1, 0.1), opacity: 0.25, rotationDeg: 45);
    Assert(ed.Redact(0, new[] { (70.0, 750.0, 200.0, 775.0) }), "redact page existed");
    Assert(!ed.Redact(99, new[] { (0.0, 0.0, 10.0, 10.0) }), "redact missing page");
    stamped = ed.ToBytes();
}
Assert(stamped.Length > 0, "watermark + redact bytes");
Console.WriteLine("watermark + redaction ok");

// 13. Convert an existing PDF to PDF/A (license-gated).
byte[] converted;
using (var ed = EditableDoc.Load(plain))
{
    ed.ConvertToPdfa(PdfaLevel.A2b);
    converted = ed.ToBytes();
}
Assert(System.Text.Encoding.Latin1.GetString(converted).Contains("pdfaid"), "converted has PDF/A metadata");
Console.WriteLine($"convert_to_pdfa ok ({converted.Length} bytes)");

// 14. Verify signatures on the freshly-signed document.
var sigs = Pdf.VerifySignatures(signed);
Assert(sigs.Count >= 1, $"expected at least one signature, got {sigs.Count}");
Assert(sigs[0].ByteRange.Length == 4, "signature byte range has 4 ints");
Console.WriteLine($"verify_signatures ok: {sigs.Count} signature(s), first sub_filter={sigs[0].SubFilter}, valid={sigs[0].IsValid}");
Assert(Pdf.VerifySignatures(plain).Count == 0, "unsigned doc has no signatures");

Console.WriteLine("OK: full C# binding surface exercised");
