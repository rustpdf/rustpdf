package dev.rustpdf;

/**
 * What the {@code y} coordinate of a positioned text stamp means
 * ({@link EditableDoc#placeText}). {@link #BASELINE} is the historical default;
 * {@link #TOP} hangs the text from {@code y} (baseline at
 * {@code y - ascent * size}, legacy fixed-position layout semantics);
 * {@link #BOTTOM} rests the descender line on {@code y}. Ascent/descent come
 * from the selected font (embedded font metrics, or Helvetica AFM).
 */
public enum VerticalAnchor {
    /** {@code y} is the text baseline (historical default). */
    BASELINE(0),
    /** Text hangs from {@code y}: baseline at {@code y - ascent * size}. */
    TOP(1),
    /** The descender line rests on {@code y}. */
    BOTTOM(2),
    /**
     * Top of the <b>layout line box</b> (OS/2 win metrics — or typo × 1.2 —
     * plus a fixed half-leading of 0.21 em): matches legacy layout engines
     * {@code fixed-position layout} line placement exactly.
     */
    LINE_TOP(3),
    /** Bottom of the layout line box (same model as {@link #LINE_TOP}). */
    LINE_BOTTOM(4);

    final int code;

    VerticalAnchor(int code) {
        this.code = code;
    }
}
