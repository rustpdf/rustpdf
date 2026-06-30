package dev.rustpdf;

/**
 * Read-only geometry of one page (from {@link Pdf#measurePage(byte[], int)} /
 * {@link Pdf#measurePages(byte[])}). Sizes are in PDF points;
 * {@link #width()}/{@link #height()} ignore page rotation while
 * {@link #rotatedWidth()}/{@link #rotatedHeight()} account for it (swapped for
 * 90/270 pages).
 *
 * @param page          the 0-based page index
 * @param width         the unrotated page width (points)
 * @param height        the unrotated page height (points)
 * @param rotation      the {@code /Rotate} value (0/90/180/270)
 * @param rotatedWidth  the width after applying rotation
 * @param rotatedHeight the height after applying rotation
 * @param mediaBox      the {@code /MediaBox}
 * @param cropBox       the {@code /CropBox}
 */
public record PageGeometry(
        int page, double width, double height, int rotation,
        double rotatedWidth, double rotatedHeight, PdfRect mediaBox, PdfRect cropBox) {
}
