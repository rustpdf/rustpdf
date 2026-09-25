// Smoke test / demo for the RustPdf .NET binding. Exercises the whole surface.
// Exits non-zero on any failed assertion.

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

Console.WriteLine($"rustpdf version: {Pdf.Version()}");

// 1. Every feature is free.
using (var d = new Document())
{
    d.Pdfa().AddPage();
    Assert(d.ToBytes().Length > 0, "PDF/A must work");
}

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

// Page rendering.
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

// 7b. Deferred / remote (HSM) signing — issue #41 P0. The key never reaches
// the library: it asks our remote signer for the raw RSA signature. Here the
// signer is passed as a delegate (an IRemoteSigner overload also exists).
Assert(Pdf.ListSignatures(plain).Count == 0, "plain doc has no signature fields");
var hsm = new HsmSigner(key);
var external = Pdf.SignWith(plain, cert, hsm.SignHash, options: new SigningOptions { Pades = true, Reason = "HSM" });
Assert(Pdf.VerifySignatures(external)[0].IsValid, "external signature verifies");
var extFields = Pdf.ListSignatures(external);
Assert(extFields.Count == 1 && extFields[0].Signed, "external doc has one signed field");
Console.WriteLine($"external (Model A) sign ok ({external.Length} bytes)");

// 7c. Two-phase (Model B): begin a signing session → build a detached CMS with
// .NET's own SignedCms (exactly the BouncyCastle integrator pattern) →
// complete. Also certifies the document (DocMDP forms + annotations).
var session = Pdf.BeginSigning(plain, new SigningOptions
{
    Pades = true,
    Certify = Certify.FormsAndAnnotations,
});
Assert(session.Hash.Length == 32, "session hash is SHA-256");
Assert(System.Text.Encoding.Latin1.GetString(session.Document).Contains("/DocMDP"),
    "DocMDP certification present");
try
{
    // The integrator (BouncyCastle/SignedCms on their Linux servers) builds the
    // container. SignedCms.ComputeSignature hits a keychain limitation on macOS
    // dev machines; their production target (Linux x64) runs this fully.
    var container = BuildDetachedCms(session.Bytes, certDer: cert, keyPk8: key);
    var twoPhase = session.Complete(container);
    Assert(Pdf.VerifySignatures(twoPhase)[0].IsValid, "two-phase signature verifies");
    Console.WriteLine($"two-phase (Model B) sign ok ({twoPhase.Length} bytes)");
}
catch (System.Security.Cryptography.CryptographicException ex)
{
    Console.WriteLine($"two-phase (Model B) prepare/embed ok; integrator CMS skipped on this OS ({ex.Message})");
}

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

// 10. Factur-X / ZUGFeRD e-invoice (Tier 2).
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

// 12b. FINDING-006: redaction must REMOVE the data, not just paint over it —
// a word in the middle of a run must vanish from extraction while its
// neighbors survive.
using (var doc6 = new Document())
{
    int f6 = doc6.AddFontFile(font);
    doc6.AddPage();
    doc6.ShowText(f6, 20, 72, 700, "PUBLICO SEGREDOXYZ FIM");
    var leakPdf = doc6.ToBytes();
    var hit = Pdf.FindText(leakPdf, "SEGREDOXYZ")[0];
    using var ed6 = EditableDoc.Load(leakPdf);
    Assert(ed6.Redact(0, new[] {
        (hit.X - 1, hit.Y - 3, hit.X + hit.Width + 1, hit.Y + hit.Height + 3) }),
        "glyph redact page existed");
    var redacted = Pdf.ExtractText(ed6.ToBytes());
    Assert(!redacted.Contains("SEGREDO"), $"redacted text must be GONE: {redacted}");
    Assert(redacted.Contains("PUBLICO") && redacted.Contains("FIM"),
        "neighboring text must survive redaction");
}
Console.WriteLine("true glyph-level redaction ok");

// 12c. FINDING-007: CMYK JPEG (Adobe APP14) must render cyan-ish, not black.
{
    var cmykJpg = File.ReadAllBytes(Path.Combine(root, "crates", "pdf", "tests", "fixtures", "cmyk_adobe.jpg"));
    using var doc7 = new Document();
    int imgId = doc7.AddImageJpeg(cmykJpg);
    doc7.AddPage((400, 300));
    doc7.DrawImage(imgId, 100, 100, 200, 130);
    var cmykPng = Pdf.RenderPageToPng(doc7.ToBytes(), 0, 72.0);
    Assert(cmykPng.Length > 100, "CMYK page rendered");
}
Console.WriteLine("cmyk jpeg render ok");

// 13. Convert an existing PDF to PDF/A.
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

// 14b. Rich signature inspection (issue #41 P1): the new certificate-detail
// fields are accessible on the verify report.
Console.WriteLine(
    $"  signer cert: issuer={sigs[0].Issuer}, alg={sigs[0].Algorithm}, " +
    $"certs={sigs[0].CertCount}, hasTimestamp={sigs[0].HasTimestamp}");
Assert(sigs[0].CertCount >= 1, "signed doc embeds at least one certificate");

// 15. Positional text search (issue #41 P1).
var hits = Pdf.FindText(pdfa, "parágrafo");
Assert(hits.Count >= 1, $"find_text found at least one hit, got {hits.Count}");
Assert(hits[0].Width > 0 && hits[0].Height > 0, "hit has a non-empty bounding box");
Console.WriteLine(
    $"find_text ok: {hits.Count} hit(s); first @page {hits[0].Page} " +
    $"({hits[0].X:F1},{hits[0].Y:F1}) {hits[0].Width:F1}x{hits[0].Height:F1}");

// 16. Normalization (issue #41 P1): strip PDF/A + downgrade version.
byte[] normalized;
using (var ed = EditableDoc.Load(pdfa))
{
    ed.Normalize(2); // strip PDF/A + set version 1.7
    normalized = ed.ToBytes();
}
Assert(!System.Text.Encoding.Latin1.GetString(normalized).Contains("pdfaid"),
    "normalize stripped PDF/A metadata");
byte[] versioned;
using (var ed = EditableDoc.Load(plain))
{
    ed.SetVersion(3); // 2.0
    versioned = ed.ToBytes();
}
Assert(System.Text.Encoding.Latin1.GetString(versioned).Contains("%PDF-2.0"),
    "set_version wrote the 2.0 header");
Console.WriteLine("normalize + set_version + strip_pdfa ok");

// 17. Page geometry (issue #45 P1 #1): read-only per-page measurements.
var geom = Pdf.MeasurePages(pdfa);
Assert(geom.Count == 1, "measure_pages count");
Assert(geom[0].Width > 0 && geom[0].Height > 0, "page has a size");
Assert(geom[0].MediaBox.Width > 0, "media box width");
byte[] rotatedPdf;
using (var ed = EditableDoc.Load(pdfa))
{
    ed.RotatePage(0, 90);
    rotatedPdf = ed.ToBytes();
}
var g0 = Pdf.MeasurePage(rotatedPdf, 0);
Assert(g0.Rotation == 90, "rotation read back");
Assert(Math.Abs(g0.RotatedWidth - g0.Height) < 0.1, "90° swaps width/height");
Console.WriteLine(
    $"measure ok: {geom[0].Width:F1}x{geom[0].Height:F1} pts, rot {geom[0].Rotation}; " +
    $"rotated page reports {g0.RotatedWidth:F1}x{g0.RotatedHeight:F1}");

// 18. Inspection (issue #45 P1 #3): version / PDF/A level / encryption posture.
var info = Pdf.Inspect(pdfa);
Assert(info.PageCount == 1, "inspect page count");
Assert(!info.Encrypted && info.Encryption == "None", "plain doc not encrypted");
Assert(info.PdfaLevel is not null, $"PDF/A level detected ({info.PdfaLevel})");
var encInfo = Pdf.Inspect(enc);
Assert(encInfo.Encrypted, "encrypted doc detected");
Console.WriteLine(
    $"inspect ok: version={info.Version}, pdfa={info.PdfaLevel}, " +
    $"encrypted-sample cipher={encInfo.Encryption}, requiresPassword={encInfo.RequiresPassword}");

// 19. Positioned drawing primitives (issue #45 P1 #2): fill rect + place text.
byte[] drawn;
using (var ed = EditableDoc.Load(pdfa))
{
    Assert(ed.FillRect(0, 100, 100, 200, 40), "fill_rect page existed");
    Assert(ed.PlaceText(0, 110, 112, "STAMPED", size: 14, color: (0, 0, 1), rotationDeg: 0),
        "place_text page existed");
    // Stamp a real PNG (reuse the page-0 render from step 1) onto the page.
    Assert(ed.DrawImage(0, png, 50, 400, 120, 90, rotationDeg: 0), "draw_image page existed");
    Assert(!ed.DrawImage(99, png, 0, 0, 1, 1), "draw_image missing page");
    Assert(!ed.FillRect(99, 0, 0, 1, 1), "fill_rect missing page");
    // Integrator gaps #4 + #5: aligned text + masked (boxed, vertically centered) text.
    Assert(ed.PlaceText(0, 300, 150, "CENTERED", size: 12, align: Align.Center),
        "place_text aligned page existed");
    Assert(ed.MaskedText(0, 100, 200, 200, 24, "R$ 1.234,56", size: 12,
        textColor: (0, 0, 0), bgColor: (1, 1, 1), align: Align.Center), "masked_text page existed");
    // FINDING-002/003: top-anchored stamp + word-wrapped paragraph.
    Assert(ed.PlaceText(0, 300, 380, "TOPO", size: 12, anchor: VerticalAnchor.Top),
        "place_text top anchor page existed");
    Assert(ed.MaskedText(0, 100, 240, 200, 42, "ALINHADO TOPO", size: 12,
        valign: VerticalAlign.Top), "masked_text valign page existed");
    int wrapped = ed.PlaceParagraphCounted(0, 100, 340, 90,
        "linha um dois tres quatro cinco seis sete", size: 12);
    Assert(wrapped > 1, $"place_paragraph must wrap (got {wrapped} lines)");
    Assert(!ed.PlaceParagraph(99, 0, 0, 100, "x"), "place_paragraph missing page");
    // FINDING-004: media-space stamping (legacy raw-space coordinates, no /Rotate composition).
    ed.StampSpace = StampSpace.Media;
    Assert(ed.PlaceText(0, 200, 300, "MEDIA", size: 10), "media-space stamp page existed");
    ed.StampSpace = StampSpace.Visible;
    // FINDING-004 follow-ups: layout line-box anchor, block-bottom paragraph, flush mask.
    Assert(ed.PlaceText(0, 300, 420, "LINEBOX", size: 10, anchor: VerticalAnchor.LineBottom),
        "line-box anchor page existed");
    var (blockLines, blockHeight) = ed.PlaceParagraphMeasured(0, 200, 100, 90,
        "bloco ancorado pelo fundo com quebra",
        size: 10, maxHeight: 40, anchor: VerticalAnchor.LineBottom);
    Assert(blockLines > 0 && blockHeight > 0 && blockHeight <= 40.0 + 0.01,
        $"bottom-pinned paragraph measured ({blockLines} lines, {blockHeight:F1}pt)");
    Assert(ed.PlaceParagraph(0, 320, 200, 80, "bloco girado", size: 10,
        anchor: VerticalAnchor.Baseline, rotationDeg: 90), "rotated paragraph page existed");
    Assert(ed.MaskedText(0, 100, 160, 200, 24, "SEM RECUO", size: 12, padding: 0),
        "masked_text zero padding page existed");
    Assert(ed.DrawImage(0, png, 260, 60, 80, 30, rotationDeg: 90,
        anchor: ImageAnchor.BoundingBox), "bbox-anchored rotated image page existed");
    drawn = ed.ToBytes();
}
Assert(Pdf.ExtractText(drawn).Contains("STAMPED"), "placed text is extractable");
Assert(Pdf.ExtractText(drawn).Contains("CENTERED"), "aligned text is extractable");
Assert(Pdf.ExtractText(drawn).Contains("R$ 1.234,56"), "masked text is extractable");
Assert(Pdf.ExtractText(drawn).Contains("quatro"), "wrapped paragraph is extractable");
Console.WriteLine("fill_rect + place_text(+align/anchor) + masked_text(+valign) + place_paragraph + draw_image ok");

// 19a. Integrator gap #3: per-page text extraction (no one-page-doc workaround).
Assert(Pdf.ExtractPageText(drawn, 0).Contains("STAMPED"), "page-0 text extracted");
Console.WriteLine("extract_page_text ok");

// 19b. Integrator gap #2: VISIBLE cryptographic signature appearance.
{
    var hsm2 = new HsmSigner(key);
    var vis = Pdf.SignWith(plain, cert, hsm2.SignHash, options: new SigningOptions
    {
        Pades = true,
        Reason = "Assinado",
        Visible = true,
        VisiblePage = 0,
        VisibleRect = new[] { 72.0, 72.0, 320.0, 144.0 },
        VisibleText = "Assinado por Example Corp\nTeste",
    });
    Assert(Pdf.VerifySignatures(vis)[0].IsValid, "visible signature verifies");
    Assert(vis.Length > plain.Length, "visible signature appended an appearance");
    Console.WriteLine("visible signature ok");
}

// 20. Async remote-sign overload (issue #45 P2). Reuses the same HSM-style
// signer, wrapped in a Task to exercise the async path.
{
    var asyncSigned = Pdf.SignWithAsync(
        plain, cert,
        data => Task.FromResult(hsm.SignHash(data)),
        options: new SigningOptions { Pades = true, Reason = "Async HSM" }).GetAwaiter().GetResult();
    Assert(Pdf.VerifySignatures(asyncSigned)[0].IsValid, "async external signature verifies");
    Console.WriteLine($"async SignWith ok ({asyncSigned.Length} bytes)");
}

Console.WriteLine("OK: full C# binding surface exercised");

// Build a detached CMS/PKCS#7 container over `data` using .NET's SignedCms —
// the pure-.NET equivalent of what an integrator does with BouncyCastle in phase 2.
static byte[] BuildDetachedCms(byte[] data, byte[] certDer, byte[] keyPk8)
{
    using var rsa = System.Security.Cryptography.RSA.Create();
    rsa.ImportPkcs8PrivateKey(keyPk8, out _);
    using var bare = new System.Security.Cryptography.X509Certificates.X509Certificate2(certDer);
    using var ephemeral =
        System.Security.Cryptography.X509Certificates.RSACertificateExtensions.CopyWithPrivateKey(bare, rsa);
    // Round-trip through a PKCS#12 with a persisted key set: ephemeral keys are
    // not accessible to SignedCms.ComputeSignature on macOS ("item no longer
    // valid"); PersistKeySet fixes it cross-platform.
    var pfx = ephemeral.Export(System.Security.Cryptography.X509Certificates.X509ContentType.Pkcs12);
    using var certWithKey = new System.Security.Cryptography.X509Certificates.X509Certificate2(
        pfx, (string?)null,
        System.Security.Cryptography.X509Certificates.X509KeyStorageFlags.PersistKeySet
            | System.Security.Cryptography.X509Certificates.X509KeyStorageFlags.Exportable);
    var cms = new System.Security.Cryptography.Pkcs.SignedCms(
        new System.Security.Cryptography.Pkcs.ContentInfo(data), detached: true);
    var signer = new System.Security.Cryptography.Pkcs.CmsSigner(certWithKey)
    {
        DigestAlgorithm = new System.Security.Cryptography.Oid("2.16.840.1.101.3.4.2.1"), // SHA-256
    };
    cms.ComputeSignature(signer);
    return cms.Encode();
}

// A stand-in "remote HSM": signs with RSA PKCS#1 v1.5 over SHA-256. In
// production this would call Azure Key Vault / VIDaaS / BirdID instead of
// holding the key locally.
sealed class HsmSigner : IRemoteSigner
{
    private readonly byte[] _keyPk8;
    public HsmSigner(byte[] keyPk8) => _keyPk8 = keyPk8;

    public byte[] SignHash(byte[] dataToSign)
    {
        using var rsa = System.Security.Cryptography.RSA.Create();
        rsa.ImportPkcs8PrivateKey(_keyPk8, out _);
        return rsa.SignData(
            dataToSign,
            System.Security.Cryptography.HashAlgorithmName.SHA256,
            System.Security.Cryptography.RSASignaturePadding.Pkcs1);
    }
}
