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
     * Find every occurrence of `$query` in `$pdf`, returning a positioned hit
     * (page + bounding box in PDF points, origin lower-left) for each match.
     * `$caseSensitive` defaults to a case-insensitive search.
     *
     * @return list<TextHit>
     */
    public static function findText(string $pdf, string $query, bool $caseSensitive = false): array
    {
        $json = Ffi::takeBytes(function ($ffi, $o, $n) use ($pdf, $query, $caseSensitive) {
            [$b, $l] = Ffi::bytes($pdf);
            return $ffi->pdf_find_text_json($b, $l, $query, $caseSensitive ? 1 : 0, $o, $n);
        });
        if ($json === '') {
            return [];
        }
        $decoded = json_decode($json, true);
        if (!\is_array($decoded)) {
            return [];
        }
        $out = [];
        foreach ($decoded as $h) {
            $out[] = new TextHit(
                (int) ($h['page'] ?? 0),
                (string) ($h['text'] ?? ''),
                (float) ($h['x'] ?? 0.0),
                (float) ($h['y'] ?? 0.0),
                (float) ($h['width'] ?? 0.0),
                (float) ($h['height'] ?? 0.0),
            );
        }
        return $out;
    }

    /**
     * Measure every page of `$pdf`, returning its geometry (size, `/Rotate`,
     * media/crop boxes — all in PDF points, origin lower-left).
     *
     * @return list<PageGeometry>
     */
    public static function measurePages(string $pdf): array
    {
        $json = Ffi::takeBytes(function ($ffi, $o, $n) use ($pdf) {
            [$b, $l] = Ffi::bytes($pdf);
            return $ffi->pdf_measure_pages_json($b, $l, $o, $n);
        });
        if ($json === '') {
            return [];
        }
        $decoded = json_decode($json, true);
        if (!\is_array($decoded)) {
            return [];
        }
        $out = [];
        foreach ($decoded as $g) {
            $out[] = new PageGeometry(
                (int) ($g['page'] ?? 0),
                (float) ($g['width'] ?? 0.0),
                (float) ($g['height'] ?? 0.0),
                (int) ($g['rotation'] ?? 0),
                (float) ($g['rotatedWidth'] ?? 0.0),
                (float) ($g['rotatedHeight'] ?? 0.0),
                PdfRect::fromArray(\is_array($g['mediaBox'] ?? null) ? $g['mediaBox'] : []),
                PdfRect::fromArray(\is_array($g['cropBox'] ?? null) ? $g['cropBox'] : []),
            );
        }
        return $out;
    }

    /**
     * Measure a single page (0-based `$index`) of `$pdf`.
     *
     * @throws PdfException if `$index` is out of range.
     */
    public static function measurePage(string $pdf, int $index): PageGeometry
    {
        $pages = self::measurePages($pdf);
        if ($index < 0 || $index >= \count($pages)) {
            throw new PdfException("page index $index out of range (" . \count($pages) . ' pages)');
        }
        return $pages[$index];
    }

    /**
     * Inspect `$pdf` without mutating it: version, PDF/A level, encryption and
     * page count. Never fails on a password-locked file.
     */
    public static function inspect(string $pdf): PdfOverview
    {
        $json = Ffi::takeBytes(function ($ffi, $o, $n) use ($pdf) {
            [$b, $l] = Ffi::bytes($pdf);
            return $ffi->pdf_inspect_json($b, $l, $o, $n);
        });
        $d = $json === '' ? [] : json_decode($json, true);
        if (!\is_array($d)) {
            $d = [];
        }
        return new PdfOverview(
            (string) ($d['version'] ?? ''),
            isset($d['pdfaLevel']) && $d['pdfaLevel'] !== null ? (string) $d['pdfaLevel'] : null,
            (bool) ($d['encrypted'] ?? false),
            (string) ($d['encryption'] ?? ''),
            (bool) ($d['requiresPassword'] ?? false),
            (int) ($d['pageCount'] ?? 0),
        );
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
     * and `byte_range`, plus certificate detail (any may be null): `issuer`,
     * `serial_number`, `valid_from`, `valid_to`, `algorithm`, `signing_time`,
     * `cert_count` and `has_timestamp`. An empty array means the document is
     * unsigned.
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

    // ---- Deferred / external (HSM / ICP-Brasil) signing — issue #41 P0 ------

    /**
     * **Model A — remote signer.** Sign `$pdf` without handing this library a
     * private key: it builds the CMS signed attributes and calls `$signHash`
     * (which receives the raw bytes to sign and must return the raw RSA PKCS#1
     * v1.5 signature over their SHA-256), then assembles and embeds the CMS.
     * `$certDer` is the signer certificate; `$chain` are intermediates (DER),
     * supplied independently of the key. The key never reaches this library —
     * `$signHash` typically forwards to a cloud HSM (Azure Key Vault, VIDaaS,
     * BirdID, …).
     *
     * @param callable(string): string $signHash receives the to-be-signed bytes,
     *                                            returns the raw RSA signature
     * @param list<string>             $chain    intermediate certificates (DER)
     *
     * @throws PdfException on any native error (including a signer failure).
     */
    public static function signWith(
        string $pdf,
        string $certDer,
        callable $signHash,
        array $chain = [],
        ?SigningOptions $options = null,
    ): string {
        return Ffi::takeBytes(function ($ffi, $o, $n) use ($pdf, $certDer, $signHash, $chain, $options) {
            [$pb, $pl] = Ffi::bytes($pdf);
            [$cb, $cl] = Ffi::bytes($certDer);
            [$chp, $chl, $chk] = Ffi::bytesArray($chain);
            [$params, $keep] = self::buildSigningOptions($options);
            // PHP FFI accepts a PHP closure for a C function-pointer parameter.
            $callback = static function ($ctx, $data, $dataLen, $sigBuf, $sigCap, $sigLen) use ($signHash): int {
                try {
                    $input = \FFI::string($data, $dataLen);
                    $sig = $signHash($input);
                    if (!\is_string($sig)) {
                        return 1; // signer returned a non-string
                    }
                    if (\strlen($sig) > $sigCap) {
                        return 2; // buffer too small
                    }
                    if ($sig !== '') {
                        \FFI::memcpy($sigBuf, $sig, \strlen($sig));
                    }
                    $sigLen[0] = \strlen($sig);
                    return 0;
                } catch (\Throwable) {
                    return 1; // signer threw
                }
            };
            $st = $ffi->pdf_sign_with(
                $pb,
                $pl,
                $cb,
                $cl,
                $chp,
                $chl,
                \count($chain),
                \FFI::addr($params),
                $callback,
                null,
                $o,
                $n,
            );
            // keep buffers/options alive until the native call returns
            unset($chk, $keep);
            return $st;
        });
    }

    /**
     * **Model B — two-phase signing, phase 1.** Prepare `$pdf` for deferred
     * signing: returns a {@see SigningSession} whose {@see SigningSession::getHash()}
     * you send to a remote HSM. Build the DER CMS container, then call
     * {@see SigningSession::complete()} (or {@see completeSignature()}). The key
     * never reaches this library.
     */
    public static function beginSigning(string $pdf, ?SigningOptions $options = null): SigningSession
    {
        $ffi = Ffi::get();
        [$pb, $pl] = Ffi::bytes($pdf);
        [$params, $keep] = self::buildSigningOptions($options);
        $docPtr = $ffi->new('uint8_t*');
        $docLen = $ffi->new('uintptr_t');
        $tbsPtr = $ffi->new('uint8_t*');
        $tbsLen = $ffi->new('uintptr_t');
        Ffi::check($ffi->pdf_sign_begin(
            $pb,
            $pl,
            \FFI::addr($params),
            \FFI::addr($docPtr),
            \FFI::addr($docLen),
            \FFI::addr($tbsPtr),
            \FFI::addr($tbsLen),
        ));
        $document = self::takeOut($docPtr, $docLen);
        $tbs = self::takeOut($tbsPtr, $tbsLen);
        unset($keep);
        return new SigningSession($document, $tbs);
    }

    /**
     * **Model B — two-phase signing, phase 2.** Embed a complete DER CMS /
     * PKCS#7 `$container` into a prepared `$document` (from {@see beginSigning()}),
     * producing the final signed PDF.
     */
    public static function completeSignature(string $document, string $container): string
    {
        return Ffi::takeBytes(function ($ffi, $o, $n) use ($document, $container) {
            [$db, $dl] = Ffi::bytes($document);
            [$cb, $cl] = Ffi::bytes($container);
            return $ffi->pdf_sign_complete($db, $dl, $cb, $cl, $o, $n);
        });
    }

    // ---- Network timestamp (AD-RT / PAdES-B-LTA over a remote TSA) ----------

    /**
     * **Network timestamp, phase 1.** Prepare `$pdf` for a `/DocTimeStamp` from
     * a network RFC 3161 TSA. Returns `[document, tbs]`: SHA-256 `$tbs`, build a
     * request with {@see timestampRequest()}, POST it to the TSA, extract the
     * token with {@see timestampTokenFromResponse()}, then embed it via
     * {@see completeSignature()}.
     *
     * @return array{0: string, 1: string} `[document, to-be-signed bytes]`
     */
    public static function beginTimestamp(string $pdf): array
    {
        $ffi = Ffi::get();
        [$pb, $pl] = Ffi::bytes($pdf);
        $docPtr = $ffi->new('uint8_t*');
        $docLen = $ffi->new('uintptr_t');
        $tbsPtr = $ffi->new('uint8_t*');
        $tbsLen = $ffi->new('uintptr_t');
        Ffi::check($ffi->pdf_timestamp_begin(
            $pb,
            $pl,
            \FFI::addr($docPtr),
            \FFI::addr($docLen),
            \FFI::addr($tbsPtr),
            \FFI::addr($tbsLen),
        ));
        return [self::takeOut($docPtr, $docLen), self::takeOut($tbsPtr, $tbsLen)];
    }

    /**
     * Build an RFC 3161 `TimeStampReq` (DER) for `$imprint` (the SHA-256 of the
     * to-be-signed bytes from {@see beginTimestamp()}). POST the result to the
     * TSA. `$nonce` is optional; `$certReq` asks the TSA to embed its certificate.
     */
    public static function timestampRequest(string $imprint, ?string $nonce = null, bool $certReq = true): string
    {
        return Ffi::takeBytes(function ($ffi, $o, $n) use ($imprint, $nonce, $certReq) {
            [$ib, $il] = Ffi::bytes($imprint);
            [$nb, $nl] = Ffi::bytes($nonce ?? '');
            return $ffi->pdf_timestamp_request($ib, $il, $nb, $nl, $certReq ? 1 : 0, $o, $n);
        });
    }

    /**
     * Extract the `TimeStampToken` (CMS) from a TSA's RFC 3161 `TimeStampResp`.
     * Embed the returned token via {@see completeSignature()}.
     */
    public static function timestampTokenFromResponse(string $response): string
    {
        return Ffi::takeBytes(function ($ffi, $o, $n) use ($response) {
            [$rb, $rl] = Ffi::bytes($response);
            return $ffi->pdf_timestamp_token_from_response($rb, $rl, $o, $n);
        });
    }

    /**
     * List the signature fields in `$pdf` (detect existing signatures before
     * signing — the iText `SignatureUtil.getSignatureNames` equivalent). An
     * empty list means there are no signature fields.
     *
     * @return list<SignatureField>
     */
    public static function listSignatures(string $pdf): array
    {
        $text = Ffi::takeBytes(function ($ffi, $o, $n) use ($pdf) {
            [$b, $l] = Ffi::bytes($pdf);
            return $ffi->pdf_list_signatures($b, $l, $o, $n);
        });
        $out = [];
        foreach (explode("\n", $text) as $line) {
            if ($line === '') {
                continue;
            }
            $tab = strpos($line, "\t");
            if ($tab === false) {
                continue;
            }
            $out[] = new SignatureField(substr($line, $tab + 1), substr($line, 0, $tab) === '1');
        }
        return $out;
    }

    /**
     * Build a native `PdfSigningOptions` from `$options`. Returns
     * `[params, keep]`: pass `\FFI::addr($params)` to the native call and keep
     * `$keep` (the backing string/byte buffers) alive until it returns.
     *
     * @return array{0: \FFI\CData, 1: list<\FFI\CData>}
     */
    private static function buildSigningOptions(?SigningOptions $options): array
    {
        $ffi = Ffi::get();
        $params = $ffi->new('PdfSigningOptions'); // zero-initialised: all pointers NULL
        $keep = [];
        if ($options === null) {
            return [$params, $keep];
        }
        // Allocate an owned, NUL-terminated C string and return a `char*` to it.
        $cstr = static function (?string $s) use ($ffi, &$keep): ?\FFI\CData {
            if ($s === null) {
                return null;
            }
            $len = \strlen($s) + 1;
            $buf = $ffi->new("char[$len]");
            if ($s !== '') {
                \FFI::memcpy($buf, $s, \strlen($s));
            }
            $keep[] = $buf;
            return $ffi->cast('char*', $buf);
        };
        if (($r = $cstr($options->reason)) !== null) {
            $params->reason = $r;
        }
        if (($loc = $cstr($options->location)) !== null) {
            $params->location = $loc;
        }
        if (($nm = $cstr($options->name)) !== null) {
            $params->name = $nm;
        }
        $params->pades = $options->pades ? 1 : 0;
        $params->certification = $options->certify->value;
        $params->estimated_size = $options->containerSize > 0 ? $options->containerSize : 0;
        if ($options->policy !== null) {
            $pol = $options->policy;
            if (($oid = $cstr($pol->oid)) !== null) {
                $params->policy_oid = $oid;
            }
            if ($pol->hash !== '') {
                [$hb, $hl] = Ffi::bytes($pol->hash);
                if ($hb !== null) {
                    $params->policy_hash = $ffi->cast('uint8_t*', $hb);
                    $params->policy_hash_len = $hl;
                    $keep[] = $hb;
                }
            }
            if (($ha = $cstr($pol->hashAlgorithmOid)) !== null) {
                $params->policy_hash_alg_oid = $ha;
            }
            if (($uri = $cstr($pol->uri)) !== null) {
                $params->policy_uri = $uri;
            }
        }
        // Visible signature appearance (issue #41 P1).
        if ($options->visible) {
            $params->visible = 1;
            $params->vis_page = $options->visiblePage;
            for ($i = 0; $i < 4; $i++) {
                $params->vis_rect[$i] = (float) ($options->visibleRect[$i] ?? 0.0);
            }
            if (($vt = $cstr($options->visibleText)) !== null) {
                $params->vis_text = $vt;
            }
            if ($options->visibleImage !== null && $options->visibleImage !== '') {
                [$ib, $il] = Ffi::bytes($options->visibleImage);
                if ($ib !== null) {
                    $params->vis_image = $ffi->cast('uint8_t*', $ib);
                    $params->vis_image_len = $il;
                    $keep[] = $ib;
                }
            }
        }
        return [$params, $keep];
    }

    /**
     * Copy a native out-buffer (`uint8_t*` + length CData) into a PHP string and
     * free it with `pdf_buffer_free`.
     */
    private static function takeOut(\FFI\CData $ptr, \FFI\CData $len): string
    {
        $n = (int) $len->cdata;
        if ($n === 0 || \FFI::isNull($ptr)) {
            return '';
        }
        $bytes = \FFI::string($ptr, $n);
        Ffi::get()->pdf_buffer_free($ptr, $n);
        return $bytes;
    }
}
