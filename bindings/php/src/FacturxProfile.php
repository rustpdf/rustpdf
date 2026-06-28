<?php

declare(strict_types=1);

namespace RustPdf;

/** ZUGFeRD / Factur-X conformance profile (electronic-invoice level). */
enum FacturxProfile: int
{
    case Minimum = 0;
    case BasicWL = 1;
    case Basic = 2;
    case EN16931 = 3;
    case Extended = 4;
}
