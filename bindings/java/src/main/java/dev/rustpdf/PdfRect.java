package dev.rustpdf;

/**
 * A rectangle in PDF user space (points, origin lower-left).
 *
 * @param x0 lower-left x
 * @param y0 lower-left y
 * @param x1 upper-right x
 * @param y1 upper-right y
 */
public record PdfRect(double x0, double y0, double x1, double y1) {
    /** Width of the rectangle (non-negative). */
    public double width() {
        return Math.abs(x1 - x0);
    }

    /** Height of the rectangle (non-negative). */
    public double height() {
        return Math.abs(y1 - y0);
    }
}
