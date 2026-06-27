<?php

declare(strict_types=1);

namespace RustPdf;

/** Document encryption cipher. */
enum Encryption: int
{
    case Rc4 = 0;
    case Aes128 = 1;
    case Aes256 = 2;
}
