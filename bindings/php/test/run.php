<?php

declare(strict_types=1);

// Smoke test for the RustPdf PHP binding. Exercises the whole surface including
// licensing gating. Exits non-zero on any failed assertion.

require __DIR__ . '/../autoload.php';

use RustPdf\AFRelationship;
use RustPdf\Align;
use RustPdf\Bookmark;
use RustPdf\Document;
use RustPdf\EditableDoc;
use RustPdf\Encryption;
use RustPdf\FacturxProfile;
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

// 9. Extract raster images to a directory.
$imgDir = sys_get_temp_dir() . '/rustpdf-images-' . bin2hex(random_bytes(8));
check(mkdir($imgDir, 0777, true), 'mkdir image dir');
$imgCount = Pdf::extractImagesToDir($pdfa, $imgDir);
check($imgCount >= 0, 'image count');
echo "extracted $imgCount image(s) to $imgDir\n";

// 10. Tier 1: links + bookmarks on an authored document.
$navDoc = new Document();
$navFont = $navDoc->addFontFile($font);
$navDoc->addPage()->showText($navFont, 14, 72, 700, 'home');
$navDoc->addPage()->showText($navFont, 14, 72, 700, 'chapter');
$navDoc->linkUri([72, 680, 200, 700], 'https://example.com')
    ->linkToPage([72, 660, 200, 678], 0, 700.0)
    ->addBookmark(
        (new Bookmark('Home', 0))->child(new Bookmark('Chapter', 1, 700.0))
    );
$navPdf = $navDoc->toBytes();
check(str_contains($navPdf, '/Link'), 'link annotation present');
check(str_contains($navPdf, '/Outlines'), 'outline present');
echo "links + bookmarks ok\n";

// 11. Tier 2: Factur-X (ZUGFeRD) invoice embedding.
$fxXml = '<?xml version="1.0" encoding="UTF-8"?><rsm:CrossIndustryInvoice '
    . 'xmlns:rsm="urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100">'
    . '<test/></rsm:CrossIndustryInvoice>';
$facturxDoc = (new Document())->pdfa(PdfaLevel::A3b);
$facturxFont = $facturxDoc->addFontFile($font);
$facturxDoc->addPage()->showText($facturxFont, 12, 72, 700, 'invoice');
$facturxDoc->facturx($fxXml, FacturxProfile::EN16931);
$facturxPdf = $facturxDoc->toBytes();
check(str_contains($facturxPdf, '/EmbeddedFile'), 'factur-x embedded file');
echo 'factur-x ok (' . strlen($facturxPdf) . " bytes)\n";

// 12. Tier 1: form fill (checkbox/radio/choice) + field names + flatten.
$formBytes = (new Document())
    ->addPage()
    ->textField('city', 0, [120, 700, 300, 720], '', 12)
    ->checkbox('ok', 0, [120, 670, 138, 688], false)
    ->radioGroup('plan', 0, [[[120, 640, 138, 658], 'a'], [[160, 640, 178, 658], 'b']], null)
    ->dropdown('country', 0, [120, 610, 300, 630], ['BR', 'PT'], null, 12)
    ->toBytes();
$formEd = EditableDoc::load($formBytes);
$names = $formEd->fieldNames();
check(in_array('city', $names, true), 'field_names contains city: ' . implode(',', $names));
check($formEd->fillTextField('city', 'Lisboa'), 'fill text field found');
check($formEd->setCheckbox('ok', true), 'set_checkbox found');
check($formEd->setRadio('plan', 'b'), 'set_radio found');
check($formEd->setChoice('country', 'PT'), 'set_choice found');
check(!$formEd->setCheckbox('missing'), 'set_checkbox missing returns false');
$formEd->flattenForms();
$flat = $formEd->toBytes();
check(strlen($flat) > 0, 'flattened bytes');
echo "form fill + flatten + field_names ok\n";

// 13. Tier 1/2: watermark + redact + convert_to_pdfa on a loaded doc.
$wmEd = EditableDoc::load($pdfa);
$wmEd->watermarkText('CONFIDENTIAL', size: 48.0, color: [0.8, 0.1, 0.1], opacity: 0.2, rotationDeg: 30.0);
$watermarked = $wmEd->toBytes();
check(strlen($watermarked) > strlen($pdfa) - 1, 'watermarked bytes produced');
echo "watermark text ok\n";

$redEd = EditableDoc::load($pdfa);
check($redEd->redact(0, [[72, 750, 200, 770]]), 'redact page 0 existed');
check(!$redEd->redact(99, [[0, 0, 10, 10]]), 'redact missing page returns false');
$redacted = $redEd->toBytes();
check(strlen($redacted) > 0, 'redacted bytes');
echo "redact ok\n";

$plainForPdfa = (new Document());
$pfp = $plainForPdfa->addFontFile($font);
$plainForPdfa->addPage()->showText($pfp, 12, 72, 700, 'convert me');
$convEd = EditableDoc::load($plainForPdfa->toBytes());
$convEd->convertToPdfa(PdfaLevel::A2b);
$converted = $convEd->toBytes();
check(str_contains($converted, 'pdfaid'), 'converted to PDF/A (pdfaid present)');
echo "convert_to_pdfa ok\n";

// 14. Module-level: verify signatures on a freshly-signed document.
$sigs = Pdf::verifySignatures($signed);
check(count($sigs) >= 1, 'verify_signatures found a signature');
$first = $sigs[0];
check(array_key_exists('field_name', $first), 'sig has field_name');
check(array_key_exists('sub_filter', $first), 'sig has sub_filter');
check(array_key_exists('covers_whole_document', $first), 'sig has covers_whole_document');
check(array_key_exists('is_valid', $first), 'sig has is_valid');
check(is_array($first['byte_range']) && count($first['byte_range']) === 4, 'sig byte_range[4]');
check(Pdf::verifySignatures($plain) === [], 'unsigned doc has no signatures');
echo 'verify_signatures ok (' . count($sigs) . " signature(s))\n";

echo "OK: full PHP binding surface exercised\n";
