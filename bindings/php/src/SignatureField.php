<?php

declare(strict_types=1);

namespace RustPdf;

/** A signature field discovered in a PDF (pre-signing inventory). */
final class SignatureField
{
    public function __construct(
        public string $name,
        public bool $signed,
    ) {
    }
}
