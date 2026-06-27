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
}
