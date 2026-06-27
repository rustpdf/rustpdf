<?php

declare(strict_types=1);

namespace RustPdf;

/** Paragraph horizontal alignment. */
enum Align: int
{
    case Left = 0;
    case Right = 1;
    case Center = 2;
    case Justify = 3;
}
