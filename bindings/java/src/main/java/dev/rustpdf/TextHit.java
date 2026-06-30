package dev.rustpdf;

/**
 * One positional match from {@link Pdf#findText(byte[], String)}. Coordinates are
 * in PDF user space (points, origin lower-left).
 *
 * @param page   the 0-based page index
 * @param text   the matched text
 * @param x      lower-left x of the bounding box
 * @param y      lower-left y of the bounding box
 * @param width  the bounding-box width
 * @param height the bounding-box height
 */
public record TextHit(int page, String text, double x, double y, double width, double height) {
}
