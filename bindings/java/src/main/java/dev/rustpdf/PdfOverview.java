package dev.rustpdf;

/**
 * A non-mutating summary of a PDF (from {@link Pdf#inspect(byte[])}): the PDF
 * version, PDF/A level (if any), encryption posture and page count. Works even
 * on password-protected files (the encryption fields are still reported).
 *
 * @param version          the PDF version string (e.g. {@code "1.7"})
 * @param pdfaLevel        the PDF/A level (e.g. {@code "2b"}) or {@code null} if not PDF/A
 * @param encrypted        whether the document is encrypted
 * @param encryption       a description of the encryption method ({@code "none"} if plain)
 * @param requiresPassword whether opening requires a password
 * @param pageCount        the number of pages
 */
public record PdfOverview(
        String version, String pdfaLevel, boolean encrypted, String encryption,
        boolean requiresPassword, int pageCount) {
}
