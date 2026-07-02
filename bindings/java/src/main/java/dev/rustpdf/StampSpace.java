package dev.rustpdf;

/**
 * Coordinate space of the positioned stamping primitives
 * ({@link EditableDoc#fillRect}, {@link EditableDoc#placeText},
 * {@link EditableDoc#maskedText}, {@link EditableDoc#placeParagraph},
 * {@link EditableDoc#drawImage}) — set via
 * {@link EditableDoc#setStampSpace(StampSpace)}. {@link #VISIBLE} (historical
 * default): coordinates in the page's displayed space, compensating
 * {@code /Rotate} so a {@code rotationDeg = 0} stamp reads upright on screen.
 * {@link #MEDIA}: raw PDF user space (legacy layout engines
 * {@code fixed-position layout}/{@code rotation} semantics) — no composition
 * with the page's {@code /Rotate} or crop offset; {@code rotationDeg} is the
 * baseline angle in media space. Use {@code MEDIA} to reproduce coordinates
 * computed for legacy PDF libraries on rotated (scanned) pages. Watermarks and redaction are
 * unaffected.
 */
public enum StampSpace {
    VISIBLE(0), MEDIA(1);

    final int code;

    StampSpace(int code) {
        this.code = code;
    }
}
