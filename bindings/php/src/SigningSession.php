<?php

declare(strict_types=1);

namespace RustPdf;

/**
 * An in-progress two-phase (Model B) signature. {@see getDocument()} holds the
 * prepared PDF (with a zero-filled /Contents placeholder) and {@see getBytes()}
 * the exact bytes the signature covers. Hand {@see getHash()} to a remote
 * signer / HSM, build the DER CMS container, then call {@see complete()}.
 */
final class SigningSession
{
    public function __construct(
        private string $document,
        private string $bytes,
    ) {
    }

    /** The prepared PDF (with a zero-filled /Contents placeholder). */
    public function getDocument(): string
    {
        return $this->document;
    }

    /** The exact bytes covered by the signature (the two ByteRange segments). */
    public function getBytes(): string
    {
        return $this->bytes;
    }

    /** SHA-256 (raw 32 bytes) of {@see getBytes()} — the value an HSM signs. */
    public function getHash(): string
    {
        return hash('sha256', $this->bytes, true);
    }

    /**
     * Phase 2: complete the signature by embedding a finished DER CMS / PKCS#7
     * $container, returning the final signed PDF.
     */
    public function complete(string $container): string
    {
        return Pdf::completeSignature($this->document, $container);
    }
}
