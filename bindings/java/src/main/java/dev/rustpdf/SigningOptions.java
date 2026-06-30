package dev.rustpdf;

/** Options for deferred / external signing (issue #41 P0). */
public final class SigningOptions {
    public String reason;
    public String location;
    public String name;
    /** Produce a PAdES-B-B signature ({@code ETSI.CAdES.detached}). */
    public boolean pades;
    /** Certify the document (DocMDP) — use only on the first signature. */
    public Certify certify = Certify.NONE;
    /**
     * Reserved {@code /Contents} bytes; 0 = library default (8192). Raise for
     * large cloud-HSM CMS containers.
     */
    public int containerSize;
    /** Signature-policy identifier (PAdES-EPES); {@code null} = none. */
    public SignaturePolicy policy;

    // ---- visible signature appearance (issue #41 P1) ------------------------
    /** Draw a visible signature appearance using the fields below. */
    public boolean visible;
    /** 0-based page index for the visible appearance. */
    public long visiblePage;
    /** Appearance rectangle {@code [x0, y0, x1, y1]} in page points. */
    public double[] visibleRect;
    /** Appearance text lines, separated by {@code '\n'}; {@code null} = none. */
    public String visibleText;
    /** PNG/JPEG bytes of a handwritten-signature image; {@code null} = none. */
    public byte[] visibleImage;

    public SigningOptions() {}
}
