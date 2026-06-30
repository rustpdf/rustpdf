package dev.rustpdf;

/**
 * DocMDP certification level applied by the first (certifying) signature.
 * Use only on the first signature of a document.
 */
public enum Certify {
    /** Not a certifying signature. */
    NONE(0),
    /** {@code /P 1} — no changes permitted after signing. */
    LOCKED(1),
    /** {@code /P 2} — form-filling and signing permitted. */
    FORMS(2),
    /** {@code /P 3} — form-filling, signing and annotations permitted. */
    FORMS_AND_ANNOTATIONS(3);

    final int code;

    Certify(int code) {
        this.code = code;
    }
}
