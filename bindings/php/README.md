# rustpdf (PHP binding)

Idiomatic PHP binding for the `rust-pdf` core over its C ABI (`libpdf_ffi`),
using the built-in **FFI** extension (`ext-ffi`, PHP 8.1+) — no compilation
needed. It covers the whole product surface: vector graphics, embedded/subsetted
fonts and text, wrapping paragraphs, images, **PDF/A** (levels 1b–3a),
**tagged/accessible** output, embedded-file attachments, **AcroForm** fields,
manipulation (merge/split/rotate/optimize/incremental update), **text
extraction**, **encryption** (RC4 / AES-128 / AES-256) and **digital signatures**
(PKCS#7 / PAdES) — plus **feature licensing**.

Classes (namespace `RustPdf`, PSR-4 under `src/`):

* `Pdf` — static helpers (`version`, `activateLicense`, `extractText`, `sign`,
  `timestamp`, `addDss`);
* `Document` — fluent authoring API;
* `EditableDoc` — manipulation API;
* `PdfaLevel`, `Align`, `AFRelationship`, `Encryption` — enums;
* `PdfException`, `Ffi` (internal FFI loader).

## Loading the native library

`Ffi` finds `libpdf_ffi` via `RUSTPDF_LIB`, then by walking up from `src/` to
`target/{debug,release}`. Build it from the repo root with
`cargo build -p pdf-ffi`. In CLI, FFI is enabled by default; for FPM/web you
typically preload (`ffi.preload`) or set `ffi.enable=1`.

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

Corporate features (PDF/A, signing, encryption, accessibility) require a license;
without one they throw `PdfException`. See [`docs/LICENSING.md`](../../docs/LICENSING.md).

## Test

```sh
cargo build -p pdf-ffi
php bindings/php/test/run.php      # or: make php-test
```
