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

Corporate features (PDF/A, signing, encryption, accessibility) require a license;
without one they throw `PdfException`. See [`docs/LICENSING.md`](../../docs/LICENSING.md).

## Test

```sh
cargo build -p pdf-ffi
php bindings/php/test/run.php      # or: make php-test
```
