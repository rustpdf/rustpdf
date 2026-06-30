<?php

declare(strict_types=1);

namespace RustPdf;

/**
 * Per-page geometry from {@see Pdf::measurePages()} / {@see Pdf::measurePage()}.
 * Sizes are in PDF points. `$width`/`$height` are the unrotated media size;
 * `$rotatedWidth`/`$rotatedHeight` account for the page `/Rotate` (swapped for
 * 90/270). `$mediaBox`/`$cropBox` are the raw boxes.
 */
final class PageGeometry
{
    public function __construct(
        public int $page,
        public float $width,
        public float $height,
        public int $rotation,
        public float $rotatedWidth,
        public float $rotatedHeight,
        public PdfRect $mediaBox,
        public PdfRect $cropBox,
    ) {
    }
}
