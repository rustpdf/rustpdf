package dev.rustpdf;

/** Encryption cipher. */
public enum Encryption {
    RC4(0), AES128(1), AES256(2);

    final int code;

    Encryption(int code) {
        this.code = code;
    }
}
