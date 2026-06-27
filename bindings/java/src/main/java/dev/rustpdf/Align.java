package dev.rustpdf;

/** Paragraph horizontal alignment. */
public enum Align {
    LEFT(0), RIGHT(1), CENTER(2), JUSTIFY(3);

    final int code;

    Align(int code) {
        this.code = code;
    }
}
