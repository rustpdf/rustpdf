<?php

declare(strict_types=1);

namespace RustPdf;

/** Thrown when a native call returns a non-zero PdfStatus. */
final class PdfException extends \RuntimeException
{
    public function __construct(string $message, public readonly int $status = 0)
    {
        parent::__construct($status !== 0 ? "PdfStatus=$status: $message" : $message);
    }
}
