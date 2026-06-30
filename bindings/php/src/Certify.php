<?php

declare(strict_types=1);

namespace RustPdf;

/**
 * DocMDP certification level applied by the first (certifying) signature.
 * Use only on the first signature of a document.
 */
enum Certify: int
{
    /** Not a certifying signature. */
    case None = 0;
    /** /P 1 — no changes permitted after signing. */
    case Locked = 1;
    /** /P 2 — form-filling and signing permitted. */
    case Forms = 2;
    /** /P 3 — form-filling, signing and annotations permitted. */
    case FormsAndAnnotations = 3;
}
