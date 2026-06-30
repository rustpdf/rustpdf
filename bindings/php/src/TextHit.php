<?php

declare(strict_types=1);

namespace RustPdf;

/**
 * A positioned text match from {@see Pdf::findText()}. Coordinates are in PDF
 * user space (points, origin lower-left).
 */
final class TextHit
{
    public function __construct(
        public int $page,
        public string $text,
        public float $x,
        public float $y,
        public float $width,
        public float $height,
    ) {
    }
}
