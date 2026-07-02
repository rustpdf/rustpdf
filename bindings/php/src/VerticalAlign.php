<?php

declare(strict_types=1);

namespace RustPdf;

/** Vertical alignment of the line inside a maskedText box. */
enum VerticalAlign: int
{
    /** Line hangs from the top edge (baseline at `y + height - ascent x size`, top line-alignment in rectangle-based text APIs). */
    case Top = 0;
    /** Historical cap-height centering (the default). */
    case Middle = 1;
    /** Descender line rests on the bottom edge. */
    case Bottom = 2;
}
