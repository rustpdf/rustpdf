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

        System.out.println("OK: full Java binding surface exercised");
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
