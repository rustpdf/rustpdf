package dev.rustpdf;

import java.nio.charset.StandardCharsets;

/**
 * Release-time smoke test for the <em>published</em> fat JAR. Unlike
 * {@link SmokeTest}, it exercises ONLY the free surface (no PDF/A, tagging,
 * encryption or signing), because the released {@code libpdf_ffi} is built with
 * the PRODUCTION license pubkey and therefore rejects the committed dev token —
 * same constraint the Go release smoke hit. Its job is narrow but important:
 * prove that the native library loads from the JAR's JNA classpath resource
 * (with {@code RUSTPDF_LIB} unset) and that the prod-key gate is live.
 *
 * <p>Run by {@code release-java.yml} after assembling the resources, via
 * {@code mvn -q test-compile exec:java -Dexec.mainClass=dev.rustpdf.ReleaseSmoke}.
 */
public final class ReleaseSmoke {

    public static void main(String[] args) {
        // Loads libpdf_ffi from the bundled classpath resource (target/classes/<prefix>/).
        String version = Pdf.version();
        assertThat(version != null && !version.isEmpty(), "pdf_version returned empty");
        System.out.println("rustpdf version: " + version + " (loaded from bundled resource)");

        // Free surface: a plain vector + text PDF, then extract it back.
        byte[] pdf;
        try (Document doc = new Document()) {
            doc.addPage()
               .setFillRgb(0.86, 0.20, 0.18)
               .rect(72, 640, 200, 120).fill();
            pdf = doc.toBytes();
        }
        assertThat(pdf.length > 0 && latin1(pdf).startsWith("%PDF-"), "PDF header");
        System.out.println("built a plain PDF (" + pdf.length + " bytes)");

        // The prod-key gate must block corporate features without a valid license.
        boolean blocked = false;
        try (Document doc = new Document()) {
            doc.pdfa().addPage();
            doc.toBytes();
        } catch (PdfException e) {
            blocked = true;
        }
        assertThat(blocked, "PDF/A must be blocked without a production license");
        System.out.println("license gate is live (corporate features blocked)");

        System.out.println("OK: published fat JAR loads and the free surface works");
    }

    private static String latin1(byte[] b) {
        return new String(b, StandardCharsets.ISO_8859_1);
    }

    private static void assertThat(boolean cond, String msg) {
        if (!cond) {
            throw new AssertionError("ASSERT FAILED: " + msg);
        }
    }
}
