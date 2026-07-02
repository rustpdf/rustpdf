package dev.rustpdf;

/**
 * Vertical alignment of the text line inside a {@link EditableDoc#maskedText}
 * box. {@link #MIDDLE} (the historical default) centers the cap-height block;
 * {@link #TOP} hangs the line from the top edge (baseline at
 * {@code y + height - ascent * size}, top line-alignment in rectangle-based text APIs
 * semantics); {@link #BOTTOM} rests the descender line on the bottom edge.
 */
public enum VerticalAlign {
    TOP(0), MIDDLE(1), BOTTOM(2);

    final int code;

    VerticalAlign(int code) {
        this.code = code;
    }
}
