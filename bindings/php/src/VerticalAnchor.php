<?php

declare(strict_types=1);

namespace RustPdf;

/**
 * What `y` means for positioned text/paragraph stamping
 * (EditableDoc::placeText / placeParagraph).
 */
enum VerticalAnchor: int
{
    /** `y` is the (first line's) baseline — the historical behavior. */
    case Baseline = 0;
    /** Text hangs from `y`: baseline lands `ascent x size` below it (legacy fixed-position layout). */
    case Top = 1;
    /** The descender line rests on `y` (paragraphs: bottom-pinned, grows upward). */
    case Bottom = 2;
    /**
     * Top of the layout line box (OS/2 win metrics — or typo x 1.2 — plus
     * a fixed half-leading of 0.21 em): matches legacy layout engines
     * `fixed-position layout` line placement exactly.
     */
    case LineTop = 3;
    /** Bottom of the layout line box (same model as LineTop). */
    case LineBottom = 4;
}
