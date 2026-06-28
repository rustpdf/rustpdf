package dev.rustpdf;

/** PDF/A conformance level (archival profile). */
public enum PdfaLevel {
    A1B(0), A2B(1), A2A(2), A3B(3), A3A(4),
    // PDF/A-4 (ISO 19005-4), based on PDF 2.0.
    A4(5), A4E(6), A4F(7);

    final int code;

    PdfaLevel(int code) {
        this.code = code;
    }
}
