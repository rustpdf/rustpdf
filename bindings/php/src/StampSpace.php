<?php

declare(strict_types=1);

namespace RustPdf;

/**
 * Coordinate space of the positioned stamping primitives (fillRect, placeText,
 * maskedText, placeParagraph, drawImage) — set via EditableDoc::setStampSpace.
 */
enum StampSpace: int
{
    /**
     * Historical default: coordinates in the page's displayed space,
     * compensating `/Rotate` so a `rotationDeg = 0` stamp reads upright.
     */
    case Visible = 0;
    /**
     * Raw PDF user space (legacy fixed-position layout/rotation
     * semantics) — never composes with the page's `/Rotate` or crop offset.
     */
    case Media = 1;
}
