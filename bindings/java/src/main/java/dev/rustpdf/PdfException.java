package dev.rustpdf;

/** Thrown when a native call returns a non-zero {@code PdfStatus}. */
public final class PdfException extends RuntimeException {
    private final int status;

    public PdfException(int status, String message) {
        super("PdfStatus=" + status + ": " + message);
        this.status = status;
    }

    /** The numeric {@code PdfStatus} code. */
    public int status() {
        return status;
    }
}
