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

    public SigningOptions() {}
}
