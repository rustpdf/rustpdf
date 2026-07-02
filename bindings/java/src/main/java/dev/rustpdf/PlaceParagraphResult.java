package dev.rustpdf;

/**
 * Measured result of {@link EditableDoc#placeParagraphMeasured}: the number of
 * {@code lines} actually drawn (0 when the page/font was invalid or nothing
 * fit — useful to detect {@code maxHeight} truncation) and the consumed block
 * {@code height} in points (top of the first drawn line's box to the bottom of
 * the last one's) — stack blocks without re-measuring.
 */
public record PlaceParagraphResult(int lines, double height) {}
