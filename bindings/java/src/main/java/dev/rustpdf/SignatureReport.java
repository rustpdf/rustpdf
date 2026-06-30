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
 * @param issuer              the signer certificate issuer (RFC 4514 DN), or {@code null}
 * @param serialNumber        the signer certificate serial number (uppercase hex), or {@code null}
 * @param validFrom           certificate validity start (ISO-8601), or {@code null}
 * @param validTo             certificate validity end (ISO-8601), or {@code null}
 * @param algorithm           the signature algorithm (e.g. {@code SHA256withRSA}), or {@code null}
 * @param signingTime         the claimed signing time (ISO-8601), or {@code null}
 * @param certCount           the number of certificates embedded in the CMS
 * @param hasTimestamp        whether the signature carries an embedded timestamp
 */
public record SignatureReport(
        String fieldName,
        String subFilter,
        String signer,
        boolean coversWholeDocument,
        boolean digestValid,
        boolean signatureValid,
        boolean isValid,
        long[] byteRange,
        String issuer,
        String serialNumber,
        String validFrom,
        String validTo,
        String algorithm,
        String signingTime,
        long certCount,
        boolean hasTimestamp) {
}
