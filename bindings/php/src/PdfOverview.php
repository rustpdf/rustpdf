<?php

declare(strict_types=1);

namespace RustPdf;

/**
 * A non-mutating summary of a PDF from {@see Pdf::inspect()}. `$pdfaLevel` is
 * null when the document is not PDF/A. `$encryption` is a human label (e.g.
 * "none", "AES-256"); `$requiresPassword` is true when the file needs a user
 * password to open.
 */
final class PdfOverview
{
    public function __construct(
        public string $version,
        public ?string $pdfaLevel,
        public bool $encrypted,
        public string $encryption,
        public bool $requiresPassword,
        public int $pageCount,
    ) {
    }
}
