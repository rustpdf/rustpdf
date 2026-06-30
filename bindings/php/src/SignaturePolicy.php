<?php

declare(strict_types=1);

namespace RustPdf;

/**
 * A signature-policy identifier (PAdES-EPES / ICP-Brasil AD-RB). Attached to
 * {@see SigningOptions::$policy}.
 */
final class SignaturePolicy
{
    /**
     * @param string      $oid              the policy OID (dotted-decimal), e.g. the ICP-Brasil AD-RB OID
     * @param string      $hash             the policy document hash (raw bytes, under $hashAlgorithmOid)
     * @param string|null $hashAlgorithmOid hash algorithm OID; null = SHA-256
     * @param string|null $uri              optional SPURI qualifier — where the policy can be retrieved
     */
    public function __construct(
        public string $oid,
        public string $hash,
        public ?string $hashAlgorithmOid = null,
        public ?string $uri = null,
    ) {
    }
}
