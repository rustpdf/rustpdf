package dev.rustpdf;

/** Embedded-file relationship (PDF/A-3 {@code /AFRelationship}). */
public enum AFRelationship {
    SOURCE(0), DATA(1), ALTERNATIVE(2), SUPPLEMENT(3), UNSPECIFIED(4);

    final int code;

    AFRelationship(int code) {
        this.code = code;
    }
}
