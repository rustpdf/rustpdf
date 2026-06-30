<?php

declare(strict_types=1);

namespace RustPdf;

/** Options for deferred / external signing (issue #41 P0). */
final class SigningOptions
{
    /**
     * @param string|null          $reason        the signature reason
     * @param string|null          $location      the signing location
     * @param string|null          $name          the signer name
     * @param bool                 $pades         produce a PAdES-B-B signature (ETSI.CAdES.detached)
     * @param Certify              $certify       certify the document (DocMDP) — first signature only
     * @param int                  $containerSize reserved /Contents bytes; 0 = default (8192). Raise for
     *                                            large cloud-HSM CMS containers
     * @param SignaturePolicy|null $policy        signature-policy identifier (PAdES-EPES); null = none
     */
    public function __construct(
        public ?string $reason = null,
        public ?string $location = null,
        public ?string $name = null,
        public bool $pades = false,
        public Certify $certify = Certify::None,
        public int $containerSize = 0,
        public ?SignaturePolicy $policy = null,
    ) {
    }
}
