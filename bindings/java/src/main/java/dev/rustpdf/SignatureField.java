package dev.rustpdf;

/**
 * A signature field discovered in a PDF (pre-signing inventory; see
 * {@link Pdf#listSignatures(byte[])}).
 *
 * @param name   the (possibly hierarchical) field name
 * @param signed whether the field already carries a signature
 */
public record SignatureField(String name, boolean signed) {
}
