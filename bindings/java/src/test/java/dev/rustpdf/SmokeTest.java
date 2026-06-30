package dev.rustpdf;

import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.KeyFactory;
import java.security.PrivateKey;
import java.security.Signature;
import java.security.spec.PKCS8EncodedKeySpec;
import java.util.Arrays;
import java.util.List;

/**
 * Smoke test / demo for the Java binding. Exercises the whole product surface,
 * including license gating. Exits non-zero on any failed assertion.
 *
 * <p>Run via {@code make java-test} (or
 * {@code mvn -q test-compile exec:java} from this directory).
 */
public final class SmokeTest {

    private static final String INVOICE_XML =
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
            + "<rsm:CrossIndustryInvoice"
            + " xmlns:rsm=\"urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100\">\n"
            + "  <rsm:ExchangedDocument><ram:ID>INV-2026-001</ram:ID></rsm:ExchangedDocument>\n"
            + "</rsm:CrossIndustryInvoice>";

    public static void main(String[] args) throws Exception {
        Path root = repoRoot();
        String font = root.resolve("assets/fonts/Roboto-Regular.ttf").toString();
        String devLicense = Files.readString(
                root.resolve("crates/license/fixtures/dev_license.txt")).trim();

        System.out.println("rustpdf version: " + Pdf.version());

        // 1. Corporate features are blocked until a license is activated.
        boolean blocked = false;
        try (Document d = new Document()) {
            d.pdfa().addPage();
            d.toBytes();
        } catch (PdfException e) {
            blocked = true;
        }
        assertThat(blocked, "PDF/A must be blocked without a license");

        Pdf.activateLicense(devLicense);
        System.out.println("license activated");

        // 2. Build a tagged PDF/A-2a doc with a font, heading and justified paragraph.
        byte[] pdfa;
        try (Document doc = new Document()) {
            doc.pdfa(PdfaLevel.A2A).setInfo("Olá", "rustpdf", null, null, null);
            int f = doc.addFontFile(font);
            doc.addPage();
            doc.showText(f, 20, 72, 760, "Título", 1);
            doc.paragraph(f, 12, 72, 720, 450, "Um parágrafo. ".repeat(8), Align.JUSTIFY);
            pdfa = doc.toBytes();
        }
        assertThat(pdfa.length > 0, "pdfa bytes");
        String text = Pdf.extractText(pdfa);
        assertThat(text.contains("Título"), "extracted text: " + text);
        System.out.println("built PDF/A-2a (" + pdfa.length + " bytes); extracted ok");

        // 2b. Page rendering (Pro feature; license already active).
        assertThat(Pdf.pageCount(pdfa) == 1, "page count");
        byte[] png = Pdf.renderPageToPng(pdfa, 0, 72.0);
        assertThat(png.length > 8 && png[1] == 'P' && png[2] == 'N' && png[3] == 'G', "PNG header");
        System.out.println("rendered page 0 -> " + png.length + " byte PNG");

        // 3. Manipulation: incremental update preserves the original prefix.
        byte[] incr;
        try (EditableDoc ed = EditableDoc.load(pdfa)) {
            assertThat(ed.pageCount() == 1, "page count");
            ed.setInfo("Subject", "via FFI");
            assertThat(ed.getInfo("Subject").equals("via FFI"), "get_info");
            incr = ed.toBytesIncremental(pdfa);
        }
        assertThat(Arrays.equals(Arrays.copyOf(incr, pdfa.length), pdfa),
                "incremental preserves original");
        System.out.println("incremental update ok (" + incr.length + " bytes)");

        // 4. Merge + optimize.
        try (EditableDoc a = EditableDoc.load(pdfa);
             EditableDoc b = EditableDoc.load(pdfa)) {
            a.merge(b).optimize();
            try (EditableDoc merged = EditableDoc.load(a.toBytes())) {
                assertThat(merged.pageCount() == 2, "merged page count");
            }
        }
        System.out.println("merge + optimize ok");

        // 5. AcroForm with every field type.
        byte[] form;
        try (Document doc = new Document()) {
            doc.addPage();
            doc.textField("city", 0, new double[] {120, 700, 300, 720}, "SP", 12);
            doc.checkbox("ok", 0, new double[] {120, 670, 138, 688}, true);
            doc.radioGroup("plan", 0,
                    new double[][] {{120, 640, 138, 658}, {160, 640, 178, 658}},
                    new String[] {"a", "b"}, 1);
            doc.dropdown("country", 0, new double[] {120, 610, 300, 630}, List.of("BR", "PT"), 0, 12);
            form = doc.toBytes();
        }
        assertThat(latin1(form).contains("/AcroForm"), "AcroForm present");
        System.out.println("forms ok");

        // 6. Encryption (AES-256) round-trips.
        byte[] plain;
        try (Document doc = new Document()) {
            int f = doc.addFontFile(font);
            doc.addPage();
            doc.showText(f, 14, 72, 700, "segredo");
            plain = doc.toBytes();
        }
        byte[] enc;
        try (EditableDoc ed = EditableDoc.load(plain)) {
            ed.encrypt("", "owner", Encryption.AES256, false);
            enc = ed.toBytes();
        }
        assertThat(latin1(enc).contains("/AESV3"), "AES-256 marker");
        assertThat(Pdf.extractText(enc).contains("segredo"), "decrypted text");
        System.out.println("encryption ok");

        // 7. Digital signature (PKCS#7 / PAdES) with the committed test key.
        Path fx = root.resolve("crates/pdf/tests/fixtures");
        byte[] key = Files.readAllBytes(fx.resolve("signer_key.pk8"));
        byte[] cert = Files.readAllBytes(fx.resolve("signer_cert.der"));
        byte[] signed = Pdf.sign(plain, key, cert, "Aprovado", null, null, true);
        assertThat(latin1(signed).contains("/ByteRange"), "signature ByteRange");
        System.out.println("signed ok (" + signed.length + " bytes)");

        // 8. Extract images to a directory.
        byte[] withImage;
        try (Document doc = new Document()) {
            doc.addPage();
            int img = doc.addImagePng(tinyPng());
            doc.drawImage(img, 72, 600, 64, 64);
            withImage = doc.toBytes();
        }
        Path imgDir = Files.createTempDirectory("rustpdf-images");
        long count = Pdf.extractImagesToDir(withImage, imgDir.toString());
        assertThat(count >= 1, "extracted image count: " + count);
        System.out.println("extracted " + count + " image(s) to " + imgDir);

        // 9. Tier 1: hyperlinks + bookmarks (Document authoring).
        byte[] navDoc;
        try (Document doc = new Document()) {
            int f = doc.addFontFile(font);
            doc.addPage();
            doc.showText(f, 18, 72, 740, "Cover");
            doc.linkUri(new double[] {72, 700, 300, 720}, "https://rustpdf.dev");
            doc.linkToPage(new double[] {72, 670, 300, 690}, 1, 760.0);
            doc.addPage();
            doc.showText(f, 18, 72, 740, "Chapter");
            Bookmark outline = new Bookmark("Cover", 0)
                    .child(new Bookmark("Chapter", 1, 760.0));
            doc.addBookmark(outline);
            navDoc = doc.toBytes();
        }
        assertThat(latin1(navDoc).contains("/URI"), "URI link present");
        assertThat(latin1(navDoc).contains("/Outlines"), "outline present");
        System.out.println("links + bookmarks ok (" + navDoc.length + " bytes)");

        // 10. Tier 2: ZUGFeRD / Factur-X e-invoice embedding (license-gated).
        byte[] invoice;
        try (Document doc = new Document()) {
            int f = doc.addFontFile(font);
            doc.addPage();
            doc.showText(f, 18, 72, 760, "Invoice INV-2026-001");
            doc.facturx(INVOICE_XML.getBytes(StandardCharsets.UTF_8), FacturxProfile.EN16931);
            invoice = doc.toBytes();
        }
        assertThat(latin1(invoice).contains("factur-x.xml"), "factur-x embedded file");
        assertThat(latin1(invoice).contains("<pdfaid:part>3</pdfaid:part>"), "Factur-X is PDF/A-3");
        System.out.println("factur-x ok (" + invoice.length + " bytes)");

        // 11. Tier 1/2: form set/flatten/field-names + watermark + redact + convert.
        try (EditableDoc ed = EditableDoc.load(form)) {
            List<String> names = ed.fieldNames();
            assertThat(names.contains("city") && names.contains("ok"), "field names: " + names);
            assertThat(ed.setCheckbox("ok", false), "set checkbox");
            assertThat(ed.setRadio("plan", "a"), "set radio");
            assertThat(ed.setChoice("country", "PT"), "set choice");
            assertThat(!ed.setCheckbox("nope", true), "missing field returns false");
            ed.flattenForms();
            assertThat(ed.fieldNames().isEmpty(), "fields gone after flatten");
        }
        System.out.println("form set/flatten/field-names ok");

        byte[] stamped;
        try (EditableDoc ed = EditableDoc.load(plain)) {
            ed.watermarkText("CONFIDENTIAL");
            assertThat(ed.redact(0, new double[][] {{72, 695, 200, 712}}), "redact page 0");
            stamped = ed.toBytes();
        }
        assertThat(stamped.length > 0, "watermark + redact bytes");
        System.out.println("watermark + redact ok (" + stamped.length + " bytes)");

        // convert_to_pdfa needs all fonts embedded (plain uses an embedded subset font).
        byte[] converted;
        try (EditableDoc ed = EditableDoc.load(plain)) {
            ed.convertToPdfa(PdfaLevel.A2B);
            converted = ed.toBytes();
        }
        assertThat(latin1(converted).contains("pdfaid"), "converted to PDF/A");
        System.out.println("convert_to_pdfa ok (" + converted.length + " bytes)");

        // 12. Tier 2: signature validation on the freshly-signed doc.
        List<SignatureReport> reports = Pdf.verifySignatures(signed);
        assertThat(!reports.isEmpty(), "at least one signature reported");
        SignatureReport r = reports.get(0);
        assertThat(r.byteRange().length == 4, "byte range has 4 entries");
        System.out.println("verify_signatures: " + reports.size() + " sig(s), first isValid="
                + r.isValid() + " subFilter=" + r.subFilter()
                + " coversWhole=" + r.coversWholeDocument());

        // 13. Deferred / external (HSM) signing — issue #41 P0.
        //     The private key stays in the JVM (java.security); the library only
        //     receives the raw RSA-PKCS#1-v1.5-over-SHA-256 signature via a callback.
        KeyFactory kf = KeyFactory.getInstance("RSA");
        PrivateKey privateKey = kf.generatePrivate(new PKCS8EncodedKeySpec(key));

        // listSignatures on an unsigned doc is empty.
        assertThat(Pdf.listSignatures(plain).isEmpty(), "no signatures before signing");

        // Model A: the library calls back for the signature over `data`.
        SigningOptions opts = new SigningOptions();
        opts.reason = "Assinado via HSM";
        opts.pades = true;
        boolean[] called = {false};
        byte[] signedA = Pdf.signWith(plain, cert, data -> {
            called[0] = true;
            try {
                Signature sg = Signature.getInstance("SHA256withRSA");
                sg.initSign(privateKey);
                sg.update(data);
                return sg.sign();
            } catch (Exception e) {
                throw new RuntimeException(e);
            }
        }, List.of(), opts);
        assertThat(called[0], "signHash callback was invoked");
        assertThat(latin1(signedA).contains("/ByteRange"), "Model A signature ByteRange");
        List<SignatureReport> reportsA = Pdf.verifySignatures(signedA);
        assertThat(!reportsA.isEmpty(), "Model A produced a signature");
        assertThat(reportsA.get(0).isValid(), "Model A signature is valid: " + reportsA.get(0).isValid());
        System.out.println("Model A (signWith) ok (" + signedA.length + " bytes); isValid="
                + reportsA.get(0).isValid());

        // listSignatures now reports exactly one (signed) field.
        List<SignatureField> fields = Pdf.listSignatures(signedA);
        assertThat(fields.size() == 1, "one signature field after signing: " + fields.size());
        assertThat(fields.get(0).signed(), "field is signed: " + fields.get(0));
        System.out.println("listSignatures: " + fields.size() + " field(s), first="
                + fields.get(0).name() + " signed=" + fields.get(0).signed());

        // Model B: two-phase (begin → hash → complete). Here we just prove phase 1
        // yields a prepared document and a non-empty 32-byte digest.
        SigningSession session = Pdf.beginSigning(plain, opts);
        assertThat(session.document().length > 0, "begin produced a prepared document");
        assertThat(session.bytes().length > 0, "begin produced bytes-to-sign");
        assertThat(session.hash().length == 32, "SHA-256 hash is 32 bytes: " + session.hash().length);
        System.out.println("Model B (beginSigning) ok: document=" + session.document().length
                + " bytes, tbs=" + session.bytes().length + " bytes, hash=" + session.hash().length);

        // 14. Issue #41 P1: positional text search.
        List<TextHit> hits = Pdf.findText(pdfa, "Título");
        assertThat(!hits.isEmpty(), "findText found at least one hit");
        TextHit hit = hits.get(0);
        assertThat(hit.width() > 0 && hit.height() > 0, "hit has a bounding box: " + hit);
        System.out.println("findText: " + hits.size() + " hit(s), first page=" + hit.page()
                + " x=" + hit.x() + " y=" + hit.y() + " w=" + hit.width() + " h=" + hit.height());

        // 15. Issue #41 P1: normalization (set_version / strip_pdfa / normalize).
        byte[] normalized;
        try (EditableDoc ed = EditableDoc.load(pdfa)) {
            ed.setVersion(2);            // 1.7
            ed.normalize(2);             // strip PDF/A + set 1.7
            normalized = ed.toBytes();
        }
        assertThat(normalized.length > 0, "normalized bytes");
        assertThat(!latin1(normalized).contains("pdfaid"), "PDF/A identifier stripped");
        System.out.println("normalize ok (" + normalized.length + " bytes)");

        // 16. Issue #41 P1: watermark opaque background + image rotation API.
        try (EditableDoc ed = EditableDoc.load(plain)) {
            ed.watermarkText("DRAFT", 48.0, 0.8, 0.1, 0.1, 0.4, 30.0, true);
            assertThat(ed.toBytes().length > 0, "opaque watermark bytes");
        }
        System.out.println("watermark opaque-background ok");

        // 17. Issue #41 P1: rich verify fields are accessible on the signed doc.
        SignatureReport rich = Pdf.verifySignatures(signed).get(0);
        System.out.println("rich verify: algorithm=" + rich.algorithm()
                + " issuer=" + rich.issuer() + " serial=" + rich.serialNumber()
                + " certCount=" + rich.certCount() + " hasTimestamp=" + rich.hasTimestamp());

        // 18. Issue #41 P1: network-TSA (AD-RT) request plumbing.
        SigningSession tsSession = Pdf.beginTimestamp(plain);
        assertThat(tsSession.document().length > 0, "timestamp begin produced a document");
        assertThat(tsSession.hash().length == 32, "timestamp tbs hashes to 32 bytes");
        byte[] tsReq = Pdf.timestampRequest(tsSession.hash());
        assertThat(tsReq.length > 0, "TimeStampReq built");
        System.out.println("beginTimestamp ok: doc=" + tsSession.document().length
                + " bytes, request=" + tsReq.length + " bytes");

        // 19. Issue #45 P1: page geometry (measurePages / measurePage + rotation swap).
        List<PageGeometry> geom = Pdf.measurePages(pdfa);
        assertThat(geom.size() == 1, "measurePages returned one page");
        PageGeometry g0 = Pdf.measurePage(pdfa, 0);
        assertThat(g0.width() > 0 && g0.height() > 0, "page has size: " + g0);
        assertThat(g0.rotation() == 0, "unrotated page rotation 0");
        assertThat(g0.rotatedWidth() == g0.width() && g0.rotatedHeight() == g0.height(),
                "no rotation: rotated size equals size");
        assertThat(g0.mediaBox().width() > 0, "mediaBox width: " + g0.mediaBox());
        boolean oob = false;
        try {
            Pdf.measurePage(pdfa, 5);
        } catch (IndexOutOfBoundsException e) {
            oob = true;
        }
        assertThat(oob, "measurePage out-of-range throws IndexOutOfBoundsException");
        byte[] rotated;
        try (EditableDoc ed = EditableDoc.load(pdfa)) {
            ed.rotatePage(0, 90);
            rotated = ed.toBytes();
        }
        PageGeometry gr = Pdf.measurePage(rotated, 0);
        assertThat(gr.rotation() == 90, "rotated page reports 90: " + gr.rotation());
        assertThat(Math.abs(gr.rotatedWidth() - g0.height()) < 1e-6
                        && Math.abs(gr.rotatedHeight() - g0.width()) < 1e-6,
                "90deg swaps rotated width/height: " + gr);
        System.out.println("measurePages ok: " + g0.width() + "x" + g0.height()
                + ", rotated " + gr.rotatedWidth() + "x" + gr.rotatedHeight());

        // 20. Issue #45 P1: non-mutating inspection.
        PdfOverview ov = Pdf.inspect(pdfa);
        assertThat(ov.pageCount() == 1, "inspect page count: " + ov.pageCount());
        assertThat(!ov.encrypted(), "pdfa is not encrypted");
        assertThat(ov.pdfaLevel() != null, "pdfa level reported: " + ov.pdfaLevel());
        PdfOverview ovEnc = Pdf.inspect(enc);
        assertThat(ovEnc.encrypted(), "encrypted doc reported as encrypted");
        System.out.println("inspect ok: version=" + ov.version() + " pdfaLevel=" + ov.pdfaLevel()
                + " encrypted=" + ov.encrypted() + " enc.encryption=" + ovEnc.encryption());

        // 21. Issue #45 P1: fillRect + placeText, then extractText sees the placed text.
        byte[] drawn;
        try (EditableDoc ed = EditableDoc.load(plain)) {
            assertThat(ed.fillRect(0, 100, 100, 200, 50, 1.0, 1.0, 1.0, 1.0), "fillRect page 0");
            assertThat(ed.placeText(0, 110, 120, "PlacedHere", 14.0, 0.0, 0.0, 0.0, 0.0),
                    "placeText page 0");
            assertThat(ed.drawImage(0, tinyPng(), 150, 150, 64, 64), "drawImage page 0");
            assertThat(!ed.fillRect(9, 0, 0, 10, 10, 0, 0, 0, 1.0), "fillRect missing page false");
            assertThat(!ed.placeText(9, 0, 0, "x", 12.0, 0, 0, 0, 0.0), "placeText missing page false");
            assertThat(!ed.drawImage(9, tinyPng(), 0, 0, 10, 10), "drawImage missing page false");
            drawn = ed.toBytes();
        }
        assertThat(drawn.length > 0, "drawn bytes");
        assertThat(Pdf.extractText(drawn).contains("PlacedHere"), "placed text extracted");
        System.out.println("fillRect + placeText + drawImage ok (" + drawn.length + " bytes)");

        // 22. ForSign follow-ups: extractPageText + aligned placeText + maskedText.
        String page0Text = Pdf.extractPageText(pdfa, 0);
        assertThat(page0Text.contains("Título"), "extractPageText page 0: " + page0Text);
        boolean pageOob = false;
        try {
            Pdf.extractPageText(pdfa, 9);
        } catch (PdfException e) {
            pageOob = true;
        }
        assertThat(pageOob, "extractPageText out-of-range throws PdfException");

        byte[] aligned;
        try (EditableDoc ed = EditableDoc.load(plain)) {
            assertThat(ed.placeText(0, 300, 200, "RightAligned", 14.0, 0.0, 0.0, 0.0, 0.0,
                    Align.RIGHT), "aligned placeText page 0");
            assertThat(ed.maskedText(0, 100, 250, 200, 24, "Masked", 12.0,
                    new double[] {0, 0, 0}, new double[] {1, 1, 1}, Align.CENTER),
                    "maskedText page 0");
            assertThat(ed.maskedText(0, 100, 300, 200, 24, "MaskedDefault", 12.0),
                    "maskedText default overload");
            assertThat(!ed.placeText(9, 0, 0, "x", 12.0, 0, 0, 0, 0.0, Align.LEFT),
                    "aligned placeText missing page false");
            assertThat(!ed.maskedText(9, 0, 0, 10, 10, "x", 12.0),
                    "maskedText missing page false");
            aligned = ed.toBytes();
        }
        assertThat(aligned.length > 0, "aligned/masked bytes");
        assertThat(Pdf.extractText(aligned).contains("RightAligned"), "aligned text extracted");
        assertThat(Pdf.extractText(aligned).contains("Masked"), "masked text extracted");
        System.out.println("extractPageText + aligned placeText + maskedText ok ("
                + aligned.length + " bytes)");

        System.out.println("OK: full Java binding surface exercised");
    }

    /** A minimal valid 1x1 red RGB PNG, built with the JDK only. */
    private static byte[] tinyPng() throws Exception {
        java.io.ByteArrayOutputStream out = new java.io.ByteArrayOutputStream();
        out.write(new byte[] {(byte) 0x89, 'P', 'N', 'G', '\r', '\n', 0x1a, '\n'});
        java.io.ByteArrayOutputStream ihdr = new java.io.ByteArrayOutputStream();
        java.io.DataOutputStream ih = new java.io.DataOutputStream(ihdr);
        ih.writeInt(1);        // width
        ih.writeInt(1);        // height
        ih.writeByte(8);       // bit depth
        ih.writeByte(2);       // color type: RGB
        ih.writeByte(0);       // compression
        ih.writeByte(0);       // filter
        ih.writeByte(0);       // interlace
        writeChunk(out, "IHDR", ihdr.toByteArray());
        java.util.zip.Deflater def = new java.util.zip.Deflater();
        def.setInput(new byte[] {0x00, (byte) 0xff, 0x00, 0x00}); // filter 0 + one red pixel
        def.finish();
        byte[] buf = new byte[64];
        int n = def.deflate(buf);
        writeChunk(out, "IDAT", java.util.Arrays.copyOf(buf, n));
        writeChunk(out, "IEND", new byte[0]);
        return out.toByteArray();
    }

    private static void writeChunk(java.io.ByteArrayOutputStream out, String tag, byte[] data)
            throws Exception {
        java.io.DataOutputStream d = new java.io.DataOutputStream(out);
        d.writeInt(data.length);
        byte[] tagBytes = tag.getBytes(StandardCharsets.US_ASCII);
        d.write(tagBytes);
        d.write(data);
        java.util.zip.CRC32 crc = new java.util.zip.CRC32();
        crc.update(tagBytes);
        crc.update(data);
        d.writeInt((int) crc.getValue());
    }

    private static String latin1(byte[] b) {
        return new String(b, StandardCharsets.ISO_8859_1);
    }

    private static void assertThat(boolean cond, String msg) {
        if (!cond) {
            throw new AssertionError("ASSERT FAILED: " + msg);
        }
    }

    private static Path repoRoot() {
        File dir = new File(System.getProperty("user.dir")).getAbsoluteFile();
        for (int i = 0; i < 12 && dir != null; i++) {
            if (new File(dir, "Cargo.toml").isFile()) {
                return dir.toPath();
            }
            dir = dir.getParentFile();
        }
        throw new IllegalStateException("could not locate repo root (Cargo.toml)");
    }
}
