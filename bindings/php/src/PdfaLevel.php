<?php

declare(strict_types=1);

namespace RustPdf;

/** PDF/A conformance level (archival profile). */
enum PdfaLevel: int
{
    case A1b = 0;
    case A2b = 1;
    case A2a = 2;
    case A3b = 3;
    case A3a = 4;
    // PDF/A-4 (ISO 19005-4), based on PDF 2.0.
    case A4 = 5;
    case A4e = 6;
    case A4f = 7;
}
