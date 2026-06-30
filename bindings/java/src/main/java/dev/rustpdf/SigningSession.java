package dev.rustpdf;

import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;

/**
 * An in-progress two-phase (deferred) signature. {@link #document()} holds the
 * prepared PDF with a zero-filled {@code /Contents} placeholder and
 * {@link #bytes()} the exact bytes the signature covers. Hash {@link #bytes()}
 * (or call {@link #hash()}), hand it to a remote HSM, build the DER CMS / PKCS#7
 * container, then call {@link #complete(byte[])}. The private key never reaches
 * this library.
 */
public final class SigningSession {
    private final byte[] document;
    private final byte[] bytes;

    SigningSession(byte[] document, byte[] bytes) {
        this.document = document;
        this.bytes = bytes;
    }

    /** The prepared PDF (with a zero-filled {@code /Contents} placeholder). */
    public byte[] document() {
        return document;
    }

    /** The exact bytes covered by the signature (the two ByteRange segments). */
    public byte[] bytes() {
        return bytes;
    }

    /** SHA-256 of {@link #bytes()} — the value an HSM signs. */
    public byte[] hash() {
        try {
            return MessageDigest.getInstance("SHA-256").digest(bytes);
        } catch (NoSuchAlgorithmException e) {
            throw new IllegalStateException("SHA-256 unavailable", e);
        }
    }

    /**
     * Phase 2: complete the signature by embedding a finished DER CMS / PKCS#7
     * {@code container}, returning the final signed PDF.
     */
    public byte[] complete(byte[] container) {
        return Pdf.completeSignature(document, container);
    }
}
