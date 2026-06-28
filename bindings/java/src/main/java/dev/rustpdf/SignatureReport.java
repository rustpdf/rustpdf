package dev.rustpdf;

/**
 * The result of validating one signature in a PDF (see
 * {@link Pdf#verifySignatures(byte[])}).
 *
 * @param fieldName           the signature field name, or {@code null}
 * @param subFilter           the signature sub-filter (e.g. {@code adbe.pkcs7.detached})
 * @param signer              the signer's common name, or {@code null}
 * @param coversWholeDocument whether the signature's ByteRange covers the whole file
 * @param digestValid         whether the embedded message digest matches the bytes
 * @param signatureValid      whether the CMS signature verifies against the digest
 * @param isValid             whether the signature is valid overall
 * @param byteRange           the four-element {@code /ByteRange} array
 */
public record SignatureReport(
        String fieldName,
        String subFilter,
        String signer,
        boolean coversWholeDocument,
        boolean digestValid,
        boolean signatureValid,
        boolean isValid,
        long[] byteRange) {
}
