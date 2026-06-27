<?php

declare(strict_types=1);

namespace RustPdf;

/** Embedded-file relationship (PDF/A-3 /AFRelationship). */
enum AFRelationship: int
{
    case Source = 0;
    case Data = 1;
    case Alternative = 2;
    case Supplement = 3;
    case Unspecified = 4;
}
