package dev.rustpdf;

import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
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
