# RustPdf for PHP

Generate, edit, sign and process PDFs from PHP: vector graphics, embedded fonts and Unicode text, wrapping paragraphs, images, **PDF/A** (1b-4f), **tagged/accessible** output, attachments, **AcroForm** fields, page manipulation (merge/split/stamp), watermarks, true **redaction**, **AES-256** encryption, **digital signatures (PAdES)** with HSM/deferred signing, timestamps/LTV, text extraction and search, and page **rendering to PNG**. Uses the built-in FFI extension (PHP 8.1+), nothing to compile.

## Documentation

- **Full API reference:** https://rustpdf.dev/docs/php
- **Interactive positioning guide** (coordinates, anchors, rotation): https://rustpdf.dev/positioning
- All product guides (PDF/A, signatures, encryption, redaction, rendering): https://rustpdf.dev/docs/

Classes (namespace `RustPdf`, PSR-4 under `src/`):

* `Pdf` — static helpers (`version`, `activateLicense`, `extractText`, `sign`,
  `timestamp`, `addDss`, plus deferred/HSM signing: `signWith`, `beginSigning`,
  `completeSignature`, `listSignatures`);
* `Document` — fluent authoring API;
* `EditableDoc` — manipulation API;
* `PdfaLevel`, `Align`, `AFRelationship`, `Encryption` — enums;
* `PdfException`, `Ffi` (internal FFI loader).

## Install

```sh
composer require rust-pdf/rustpdf
```

The package is pure PHP; the native `libpdf_ffi` is fetched per platform from the
GitHub Release. `Ffi::libPath()` resolves it in this order:

1. `RUSTPDF_LIB` — an absolute path to a library you provide (always wins);
2. the bundled copy under `lib/<os>-<arch>/`, placed there by the installer;
3. the dev tree (`target/{debug,release}`, walking up from `src/`);
4. **lazy download** of the matching prebuilt library on first use.

Because Composer only runs scripts for the *root* package (never a dependency),
the download can't fire automatically on `composer require`. The step-4 lazy
fetch covers most setups with zero config, but if your production filesystem is
read-only, fetch the library **once at deploy/CI time** instead:

```sh
php vendor/rust-pdf/rustpdf/bin/rustpdf-install-lib
```

Or wire it into your **root** `composer.json`:

```json
"scripts": {
  "post-install-cmd": "@php vendor/rust-pdf/rustpdf/bin/rustpdf-install-lib",
  "post-update-cmd":  "@php vendor/rust-pdf/rustpdf/bin/rustpdf-install-lib"
}
```

Set `RUSTPDF_NO_DOWNLOAD=1` to forbid the network fetch (you must then supply the
library via `RUSTPDF_LIB` or a local `cargo build -p pdf-ffi --release`).

### Enabling `ext-ffi`

FFI ships with PHP 8.1+ but is often **disabled in production**. In CLI it's
enabled by default; for FPM/web set in `php.ini`:

```ini
extension=ffi
ffi.enable=true        ; or, hardened: ffi.enable=preload + ffi.preload=...
```

Without it, install succeeds but the first call throws.

## Quick start

```php
<?php
require 'vendor/autoload.php'; // or bindings/php/autoload.php

use RustPdf\{Pdf, Document, EditableDoc, PdfaLevel, Align, Encryption};

// A token in RUSTPDF_LICENSE is auto-activated; or:
Pdf::activateLicense($token);

$doc = new Document();
$doc->pdfa(PdfaLevel::A2a)->setInfo(title: 'Report');
$f = $doc->addFontFile('assets/fonts/Roboto-Regular.ttf');
$doc->addPage()
    ->showText($f, 20, 72, 760, 'Title', 1)               // heading level 1 = H1
    ->paragraph($f, 12, 72, 720, 450, 'A wrapping body…', Align::Justify);
$data = $doc->toBytes();

echo Pdf::extractText($data);

$ed = EditableDoc::load($data);
$ed->encrypt(Encryption::Aes256, owner: 'owner')->save('secured.pdf');

$signed = Pdf::sign($data, $keyDer, $certDer, pades: true);
```

Corporate features (PDF/A, signing, encryption, accessibility, page rendering — a **Pro** feature) require a license;
without one they throw `PdfException`. See [`docs/LICENSING.md`](../../docs/LICENSING.md).

## Deferred / HSM signing (key stays out of the library)

When the private key lives in an HSM, a cloud KMS, a smartcard or a PKI
authentication token, it must never enter this library. Two flows keep it there
and work with any PKI (eIDAS, AATL and the like):

**Model A — bring your own signer.** `Pdf::signWith` builds the CMS signed
attributes and calls your closure with the bytes to sign; the closure forwards
them to the HSM/KMS and returns the raw RSA signature. The cert and any
intermediates are passed independently of the key.

```php
use RustPdf\{Pdf, SigningOptions};

$pdf     = file_get_contents('contract.pdf');
$certDer = file_get_contents('signer-cert.der');           // X.509 (DER)

$signed = Pdf::signWith(
    $pdf,
    $certDer,
    fn (string $toSign): string => $hsm->signRsaSha256($toSign), // key never leaves the HSM
    chain: [$intermediateDer],
    options: new SigningOptions(reason: 'Approved', pades: true),
);
file_put_contents('contract.signed.pdf', $signed);
```

**Model B — two-phase.** When the signer is asynchronous (a remote service or a
user interaction), split it: prepare the document, ship the hash, then embed the
finished CMS container.

```php
$session   = Pdf::beginSigning($pdf, new SigningOptions(pades: true));
$container = $hsm->buildCmsContainer($session->getHash());   // raw 32-byte SHA-256
$signed    = $session->complete($container);                 // final signed PDF
```

`Pdf::listSignatures($pdf)` returns the existing signature fields (each a
`SignatureField` with `$name` / `$signed`) so you can detect prior signatures
before adding another. `SigningOptions` also carries `certify` (a `Certify`
DocMDP level) and `policy` (a `SignaturePolicy` for PAdES-EPES).

## Test

```sh
cargo build -p pdf-ffi
php bindings/php/test/run.php      # or: make php-test
```
