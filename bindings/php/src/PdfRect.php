<?php

declare(strict_types=1);

namespace RustPdf;

/**
 * A rectangle in PDF user space (points), given by its lower-left
 * `($x0, $y0)` and upper-right `($x1, $y1)` corners.
 */
final class PdfRect
{
    public function __construct(
        public float $x0,
        public float $y0,
        public float $x1,
        public float $y1,
    ) {
    }

    public function width(): float
    {
        return $this->x1 - $this->x0;
    }

    public function height(): float
    {
        return $this->y1 - $this->y0;
    }

    /** @param array<int, mixed> $a `[x0, y0, x1, y1]` (e.g. from json_decode). */
    public static function fromArray(array $a): self
    {
        return new self(
            (float) ($a[0] ?? 0.0),
            (float) ($a[1] ?? 0.0),
            (float) ($a[2] ?? 0.0),
            (float) ($a[3] ?? 0.0),
        );
    }
}
