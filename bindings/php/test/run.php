<?php

declare(strict_types=1);

// Smoke test for the RustPdf PHP binding. Exercises the whole surface including
// licensing gating. Exits non-zero on any failed assertion.

require __DIR__ . '/../autoload.php';

use RustPdf\AFRelationship;
use RustPdf\Align;
use RustPdf\Document;
use RustPdf\EditableDoc;
use RustPdf\Encryption;
use RustPdf\Pdf;
use RustPdf\PdfaLevel;
use RustPdf\PdfException;

function repoRoot(): string
{
    $dir = __DIR__;
    for ($i = 0; $i < 12; $i++) {
        if (is_file($dir . '/Cargo.toml')) {
            return $dir;
        }
        $parent = dirname($dir);
        if ($parent === $dir) {
            break;
        }
        $dir = $parent;
    }
    fwrite(STDERR, "could not locate repo root\n");
    exit(2);
}

function check(bool $cond, string $msg): void
{
    if (!$cond) {
        fwrite(STDERR, "ASSERT FAILED: $msg\n");
        exit(1);
    }
}

$root = repoRoot();
$font = "$root/assets/fonts/Roboto-Regular.ttf";
$devLicense = trim((string) file_get_contents("$root/crates/license/fixtures/dev_license.txt"));

echo 'rustpdf version: ' . Pdf::version() . "\n";

// 1. Corporate features blocked without a license.
putenv('RUSTPDF_LICENSE');
$blocked = false;
try {
    $d = new Document();
    $d->pdfa()->addPage();
    $d->toBytes();
} catch (PdfException) {
    $blocked = true;
}
check($blocked, 'PDF/A must be blocked without a license');

Pdf::activateLicense($devLicense);
echo "license activated\n";

// 2. Tagged PDF/A-2a with a font, heading and justified paragraph.
$doc = new Document();
$doc->pdfa(PdfaLevel::A2a)->setInfo(title: 'Olá', author: 'rustpdf');
$f = $doc->addFontFile($font);
$doc->addPage()
    ->showText($f, 20, 72, 760, 'Título', 1)
    ->paragraph($f, 12, 72, 720, 450, str_repeat('Um parágrafo. ', 8), Align::Justify);
$pdfa = $doc->toBytes();
check(strlen($pdfa) > 0, 'pdfa bytes');
$text = Pdf::extractText($pdfa);
check(str_contains($text, 'Título'), "extracted text: $text");
echo 'built PDF/A-2a (' . strlen($pdfa) . " bytes); extracted ok\n";

// 3. Incremental update preserves the original prefix.
$ed = EditableDoc::load($pdfa);
check($ed->pageCount() === 1, 'page count');
$ed->setInfo('Subject', 'via FFI');
check($ed->getInfo('Subject') === 'via FFI', 'get_info');
$incr = $ed->toBytesIncremental($pdfa);
check(str_starts_with($incr, $pdfa), 'incremental preserves original');
echo 'incremental update ok (' . strlen($incr) . " bytes)\n";

// 4. Merge + optimize.
$a = EditableDoc::load($pdfa);
$b = EditableDoc::load($pdfa);
$a->merge($b)->optimize();
$merged = EditableDoc::load($a->toBytes());
check($merged->pageCount() === 2, 'merged page count');
echo "merge + optimize ok\n";

// 5. AcroForm with every field type.
$form = (new Document())
    ->addPage()
    ->textField('city', 0, [120, 700, 300, 720], 'SP', 12)
    ->checkbox('ok', 0, [120, 670, 138, 688], true)
    ->radioGroup('plan', 0, [[[120, 640, 138, 658], 'a'], [[160, 640, 178, 658], 'b']], 1)
    ->dropdown('country', 0, [120, 610, 300, 630], ['BR', 'PT'], 0, 12)
    ->toBytes();
check(str_contains($form, '/AcroForm'), 'AcroForm present');
echo "forms ok\n";

// 6. Encryption (AES-256) round-trips.
$plainDoc = new Document();
$pf = $plainDoc->addFontFile($font);
$plain = $plainDoc->addPage()->showText($pf, 14, 72, 700, 'segredo')->toBytes();
$encEd = EditableDoc::load($plain);
$encEd->encrypt(Encryption::Aes256, owner: 'owner');
$enc = $encEd->toBytes();
check(str_contains($enc, '/AESV3'), 'AES-256 marker');
check(str_contains(Pdf::extractText($enc), 'segredo'), 'decrypted text');
echo "encryption ok\n";

// 7. Digital signature (PKCS#7 / PAdES) with the committed test key.
$fx = "$root/crates/pdf/tests/fixtures";
$key = (string) file_get_contents("$fx/signer_key.pk8");
$cert = (string) file_get_contents("$fx/signer_cert.der");
$signed = Pdf::sign($plain, $key, $cert, reason: 'Aprovado', pades: true);
check(str_contains($signed, '/ByteRange'), 'signature ByteRange');
echo 'signed ok (' . strlen($signed) . " bytes)\n";

// 8. Attachment (PDF/A-3).
$a3 = (new Document())
    ->pdfa(PdfaLevel::A3b);
$a3f = $a3->addFontFile($font);
$a3->attachFile('data.csv', 'text/csv', "a,b\n1,2\n", AFRelationship::Source, 'source data')
    ->addPage()->showText($a3f, 12, 72, 700, 'anexo');
check(str_contains($a3->toBytes(), '/EmbeddedFile'), 'attachment present');
echo "attachment ok\n";

echo "OK: full PHP binding surface exercised\n";
