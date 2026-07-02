<?php

declare(strict_types=1);

namespace RustPdf;

/** How a rotated image is anchored at `(x, y)` (EditableDoc::drawImage). */
enum ImageAnchor: int
{
    /** `(x, y)` is the image's own lower-left corner — the image sweeps around it when rotated. */
    case Corner = 0;
    /**
     * The rotated image's bounding box lands with its lower-left at `(x, y)`
     * (bounding-box layout semantics — pixels always at/above/right of the anchor).
     */
    case BoundingBox = 1;
}
