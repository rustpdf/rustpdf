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

Console.WriteLine("OK: full C# binding surface exercised");
