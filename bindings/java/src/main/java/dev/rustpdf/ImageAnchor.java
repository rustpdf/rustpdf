package dev.rustpdf;

/**
 * How a rotated image is anchored at {@code (x, y)}
 * ({@link EditableDoc#drawImage}). {@link #CORNER} (default): the image's own
 * lower-left corner — the image sweeps around it when rotated.
 * {@link #BOUNDING_BOX}: the rotated image's bounding box lands with its
 * lower-left at {@code (x, y)} (bounding-box layout semantics — pixels always
 * at/above/right of the anchor).
 */
public enum ImageAnchor {
    CORNER(0), BOUNDING_BOX(1);

    final int code;

    ImageAnchor(int code) {
        this.code = code;
    }
}
