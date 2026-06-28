<?php

declare(strict_types=1);

namespace RustPdf;

/** Top-level helpers: version, licensing, text extraction and signing. */
final class Pdf
{
    /** Native library version string. */
    public static function version(): string
    {
        // PHP FFI auto-converts a `const char *` return into a PHP string.
        return (string) Ffi::get()->pdf_version();
    }

    /**
     * Activate a license token (unlocks PDF/A, signing, encryption,
     * accessibility). Tokens may also be supplied via the RUSTPDF_LICENSE /
     * RUSTPDF_LICENSE_FILE environment variables (auto-activated).
     *
     * @throws PdfException if the token is forged, expired or malformed.
     */
    public static function activateLicense(string $token): void
    {
        Ffi::check(Ffi::get()->pdf_activate_license($token));
    }

    /** Extract a document's text (Unicode via ToUnicode). */
    public static function extractText(string $pdf): string
    {
        return Ffi::takeBytes(function ($ffi, $o, $n) use ($pdf) {
            [$b, $l] = Ffi::bytes($pdf);
            return $ffi->pdf_extract_text($b, $l, $o, $n);
        });
    }

    /**
     * Extract every raster image from `$pdf` into directory `$dir` (JPEGs are
     * written verbatim as .jpg, everything else as .png; files are named
     * page{N}_{name}.{ext}). Returns the number of images written.
     */
    public static function extractImagesToDir(string $pdf, string $dir): int
    {
        $ffi = Ffi::get();
        [$b, $l] = Ffi::bytes($pdf);
        $count = $ffi->new('uintptr_t');
        Ffi::check($ffi->pdf_extract_images_to_dir($b, $l, $dir, \FFI::addr($count)));
        return (int) $count->cdata;
    }

    /**
     * Render page `$page` (0-based) of `$pdf` to a PNG image at `$dpi`
     * dots-per-inch. Page rendering is a licensed Pro feature: throws unless a
     * license granting it is active.
     */
    public static function renderPageToPng(string $pdf, int $page = 0, float $dpi = 150.0): string
    {
        return Ffi::takeBytes(function ($ffi, $o, $n) use ($pdf, $page, $dpi) {
            [$b, $l] = Ffi::bytes($pdf);
            return $ffi->pdf_render_page_to_png($b, $l, $page, $dpi, $o, $n);
        });
    }

    /** Number of pages in `$pdf` (free — no license required). */
    public static function pageCount(string $pdf): int
    {
        $ffi = Ffi::get();
        [$b, $l] = Ffi::bytes($pdf);
        $count = $ffi->new('uintptr_t');
        Ffi::check($ffi->pdf_page_count($b, $l, \FFI::addr($count)));
        return (int) $count->cdata;
    }

    /**
     * Sign a PDF (PKCS#7 detached, incremental update). `$pades` selects
     * PAdES-B-B. Requires a license.
     */
    public static function sign(
        string $pdf,
        string $keyDer,
        string $certDer,
        ?string $reason = null,
        ?string $location = null,
        ?string $name = null,
        bool $pades = false,
    ): string {
        return Ffi::takeBytes(function ($ffi, $o, $n) use ($pdf, $keyDer, $certDer, $reason, $location, $name, $pades) {
            [$pb, $pl] = Ffi::bytes($pdf);
            [$kb, $kl] = Ffi::bytes($keyDer);
            [$cb, $cl] = Ffi::bytes($certDer);
            return $ffi->pdf_sign($pb, $pl, $kb, $kl, $cb, $cl, $reason, $location, $name, $pades ? 1 : 0, $o, $n);
        });
    }

    /** Append a document timestamp (/DocTimeStamp, PAdES-B-LTA). */
    public static function timestamp(string $pdf, string $tsaKeyDer, string $tsaCertDer, ?string $date = null): string
    {
        return Ffi::takeBytes(function ($ffi, $o, $n) use ($pdf, $tsaKeyDer, $tsaCertDer, $date) {
            [$pb, $pl] = Ffi::bytes($pdf);
            [$kb, $kl] = Ffi::bytes($tsaKeyDer);
            [$cb, $cl] = Ffi::bytes($tsaCertDer);
            return $ffi->pdf_timestamp($pb, $pl, $kb, $kl, $cb, $cl, $date, $o, $n);
        });
    }

    /**
     * Append a Document Security Store (/DSS, PAdES-B-LT).
     *
     * @param list<string> $certs
     * @param list<string> $crls
     */
    public static function addDss(string $pdf, array $certs = [], array $crls = []): string
    {
        return Ffi::takeBytes(function ($ffi, $o, $n) use ($pdf, $certs, $crls) {
            [$pb, $pl] = Ffi::bytes($pdf);
            [$cp, $cl, $k1] = Ffi::bytesArray($certs);
            [$rp, $rl, $k2] = Ffi::bytesArray($crls);
            $st = $ffi->pdf_add_dss($pb, $pl, $cp, $cl, \count($certs), $rp, $rl, \count($crls), $o, $n);
            // keep buffers alive until the call returns
            unset($k1, $k2);
            return $st;
        });
    }

    /**
     * Validate every signature in `$data`. Returns one associative array per
     * signature with keys `field_name`, `sub_filter`, `signer`,
     * `covers_whole_document`, `digest_valid`, `signature_valid`, `is_valid`
     * and `byte_range`. An empty array means the document is unsigned.
     *
     * @return list<array<string, mixed>>
     */
    public static function verifySignatures(string $data): array
    {
        $json = Ffi::takeBytes(function ($ffi, $o, $n) use ($data) {
            [$b, $l] = Ffi::bytes($data);
            return $ffi->pdf_verify_signatures_json($b, $l, $o, $n);
        });
        if ($json === '') {
            return [];
        }
        $decoded = json_decode($json, true);
        return \is_array($decoded) ? $decoded : [];
    }
}
