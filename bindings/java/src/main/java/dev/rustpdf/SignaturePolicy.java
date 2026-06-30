package dev.rustpdf;

/**
 * A signature-policy identifier (PAdES-EPES / ICP-Brasil AD-RB). Attach one to a
 * {@link SigningOptions} to embed a {@code signature-policy-identifier} signed
 * attribute.
 */
public final class SignaturePolicy {
    /** The policy OID (dotted-decimal), e.g. the ICP-Brasil AD-RB OID. */
    public String oid;
    /** The policy document hash (under {@link #hashAlgorithmOid}). */
    public byte[] hash;
    /** Hash algorithm OID; {@code null} = SHA-256. */
    public String hashAlgorithmOid;
    /** Optional SPURI qualifier — where the policy can be retrieved. */
    public String uri;

    public SignaturePolicy() {}

    public SignaturePolicy(String oid, byte[] hash, String hashAlgorithmOid, String uri) {
        this.oid = oid;
        this.hash = hash;
        this.hashAlgorithmOid = hashAlgorithmOid;
        this.uri = uri;
    }
}
