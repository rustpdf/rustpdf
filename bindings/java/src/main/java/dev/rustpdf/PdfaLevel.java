package dev.rustpdf;

/** PDF/A conformance level (archival profile). */
public enum PdfaLevel {
    A1B(0), A2B(1), A2A(2), A3B(3), A3A(4);

    final int code;

    PdfaLevel(int code) {
        this.code = code;
    }
}
