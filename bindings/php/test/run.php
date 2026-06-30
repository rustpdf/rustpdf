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
use RustPdf\Certify;
use RustPdf\PageGeometry;
use RustPdf\Pdf;
use RustPdf\PdfaLevel;
use RustPdf\PdfException;
use RustPdf\PdfOverview;
use RustPdf\PdfRect;
use RustPdf\SignatureField;
use RustPdf\SigningOptions;
use RustPdf\SigningSession;
use RustPdf\TextHit;

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

// Page rendering (Pro feature; license already active).
check(Pdf::pageCount($pdfa) === 1, 'page count');
$png = Pdf::renderPageToPng($pdfa, 0, 72.0);
check(strlen($png) > 8 && substr($png, 1, 3) === 'PNG', 'PNG header');
echo 'rendered page 0 → ' . strlen($png) . " byte PNG\n";

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

// 15. Deferred / external (HSM / ICP-Brasil) signing — issue #41 P0.
//     The private key never reaches the library; we sign through openssl.
$pemKey = "-----BEGIN PRIVATE KEY-----\n"
    . chunk_split(base64_encode($key), 64, "\n")
    . "-----END PRIVATE KEY-----\n";
$pkey = openssl_pkey_get_private($pemKey);
check($pkey !== false, 'openssl loaded signer key');

// listSignatures on an unsigned doc → empty.
check(Pdf::listSignatures($plain) === [], 'listSignatures empty on unsigned doc');

// Model A: signWith + a closure that produces the raw RSA signature.
$signHash = static function (string $data) use ($pkey): string {
    $sig = '';
    if (!openssl_sign($data, $sig, $pkey, OPENSSL_ALGO_SHA256)) {
        throw new RuntimeException('openssl_sign failed');
    }
    return $sig;
};
$opts = new SigningOptions(reason: 'Aprovado via HSM', name: 'rustpdf', pades: true);
$signedA = Pdf::signWith($plain, $cert, $signHash, [], $opts);
check(str_contains($signedA, '/ByteRange'), 'Model A signature has ByteRange');
$sigsA = Pdf::verifySignatures($signedA);
check(count($sigsA) >= 1, 'Model A produced a signature');
check($sigsA[0]['is_valid'] === true, 'Model A signature is valid: ' . var_export($sigsA[0]['is_valid'], true));
echo 'signWith (Model A) ok (' . strlen($signedA) . " bytes), signature valid\n";

// listSignatures now reports one signed field.
$fields = Pdf::listSignatures($signedA);
check(count($fields) === 1, 'listSignatures found one field after signing');
check($fields[0] instanceof SignatureField, 'listSignatures returns SignatureField');
check($fields[0]->signed === true, 'listed field is signed');
echo 'listSignatures ok (' . $fields[0]->name . ", signed=" . var_export($fields[0]->signed, true) . ")\n";

// Model B: beginSigning → hash → build CMS via openssl_pkcs7 → completeSignature.
$session = Pdf::beginSigning($plain, new SigningOptions(reason: 'Two-phase'));
check($session instanceof SigningSession, 'beginSigning returns a session');
check(strlen($session->getDocument()) > 0, 'session document non-empty');
check(strlen($session->getBytes()) > 0, 'session to-be-signed bytes non-empty');
check(strlen($session->getHash()) === 32, 'session hash is 32 bytes (SHA-256)');
echo 'beginSigning (Model B) ok (doc ' . strlen($session->getDocument())
    . ' bytes, tbs ' . strlen($session->getBytes()) . " bytes, 32-byte hash)\n";

// 16. Issue #41 P1: positional text search.
$hits = Pdf::findText($pdfa, 'Título');
check(count($hits) >= 1, 'findText found at least one hit');
check($hits[0] instanceof TextHit, 'findText returns TextHit');
check($hits[0]->page === 0, 'hit on page 0');
check($hits[0]->width > 0 && $hits[0]->height > 0, 'hit has a bounding box');
echo 'findText ok (' . count($hits) . ' hit(s); box '
    . round($hits[0]->width, 1) . 'x' . round($hits[0]->height, 1) . ")\n";
// Case-insensitive default vs case-sensitive.
check(Pdf::findText($pdfa, 'título', false) !== [], 'case-insensitive match');

// 17. Issue #41 P1: rich signature inspection fields are accessible.
$rich = Pdf::verifySignatures($signedA);
check(count($rich) >= 1, 'rich verify has a signature');
foreach (['issuer', 'serial_number', 'valid_from', 'valid_to', 'algorithm', 'signing_time', 'cert_count', 'has_timestamp'] as $key) {
    check(array_key_exists($key, $rich[0]), "rich sig field present: $key");
}
echo 'rich verify fields ok (algorithm=' . var_export($rich[0]['algorithm'], true)
    . ', cert_count=' . var_export($rich[0]['cert_count'], true) . ")\n";

// 18. Issue #41 P1: normalization (set_version / strip_pdfa / normalize).
$normEd = EditableDoc::load($pdfa);
$normEd->setVersion(2); // 1.7
$v17 = $normEd->toBytes();
check(str_starts_with($v17, '%PDF-1.7'), 'set_version produced %PDF-1.7');
$normEd2 = EditableDoc::load($pdfa);
$normEd2->normalize(2);
$plainNorm = $normEd2->toBytes();
check(!str_contains($plainNorm, 'pdfaid'), 'normalize stripped PDF/A pdfaid');
echo "set_version + normalize ok\n";

// 19. Issue #41 P1: watermark opaque background + image rotation defaults.
$wm2 = EditableDoc::load($pdfa);
$wm2->watermarkText('DRAFT', size: 40.0, opacity: 0.25, rotationDeg: 30.0, opaqueBackground: true);
check(strlen($wm2->toBytes()) > 0, 'opaque-background watermark produced bytes');
echo "watermark opaque background ok\n";

// 20. Issue #41 P1: visible signature via SigningOptions (Model A).
$visOpts = new SigningOptions(
    reason: 'Visible',
    pades: true,
    visible: true,
    visiblePage: 0,
    visibleRect: [72.0, 72.0, 272.0, 144.0],
    visibleText: "Signed by rustpdf\nVisible appearance",
);
$signedVis = Pdf::signWith($plain, $cert, $signHash, [], $visOpts);
check(str_contains($signedVis, '/ByteRange'), 'visible signature has ByteRange');
check(Pdf::verifySignatures($signedVis)[0]['is_valid'] === true, 'visible signature valid');
echo 'visible signature ok (' . strlen($signedVis) . " bytes)\n";

// 21. Issue #41 P1: network-TSA (AD-RT) request plumbing.
[$tsDoc, $tsTbs] = Pdf::beginTimestamp($plain);
check(strlen($tsDoc) > 0 && strlen($tsTbs) > 0, 'beginTimestamp returned document + tbs');
$tsReq = Pdf::timestampRequest(hash('sha256', $tsTbs, true));
check(strlen($tsReq) > 0, 'timestampRequest built a DER request');
echo 'network-TSA helpers ok (req ' . strlen($tsReq) . " bytes)\n";

// 22. Issue #45 P1: measure pages (geometry + rotation swap).
$geoms = Pdf::measurePages($pdfa);
check(count($geoms) === 1, 'measurePages returned one page');
check($geoms[0] instanceof PageGeometry, 'measurePages returns PageGeometry');
check($geoms[0]->page === 0, 'geometry page index 0');
check($geoms[0]->width > 0 && $geoms[0]->height > 0, 'geometry has a size');
check($geoms[0]->rotation === 0, 'unrotated page has rotation 0');
check($geoms[0]->mediaBox instanceof PdfRect, 'geometry mediaBox is a PdfRect');
check(abs($geoms[0]->mediaBox->width() - $geoms[0]->width) < 0.01, 'mediaBox width matches');
check(abs($geoms[0]->rotatedWidth - $geoms[0]->width) < 0.01, 'unrotated rotatedWidth == width');
$single = Pdf::measurePage($pdfa, 0);
check($single->page === 0, 'measurePage(0) ok');
$rangeThrew = false;
try {
    Pdf::measurePage($pdfa, 5);
} catch (PdfException) {
    $rangeThrew = true;
}
check($rangeThrew, 'measurePage out of range throws');

// Rotate a page 90° and confirm rotatedWidth/Height swap.
$rotEd = EditableDoc::load($pdfa);
$rotEd->rotatePage(0, 90);
$rotated = $rotEd->toBytes();
$rg = Pdf::measurePage($rotated, 0);
check($rg->rotation === 90, 'rotated page reports rotation 90');
check(abs($rg->rotatedWidth - $geoms[0]->height) < 0.01, 'rotatedWidth == original height after 90°');
check(abs($rg->rotatedHeight - $geoms[0]->width) < 0.01, 'rotatedHeight == original width after 90°');
echo 'measurePages ok (' . round($geoms[0]->width, 1) . 'x' . round($geoms[0]->height, 1)
    . ', rotation swap verified)' . "\n";

// 23. Issue #45 P1: inspect a document without mutating it.
$ov = Pdf::inspect($pdfa);
check($ov instanceof PdfOverview, 'inspect returns PdfOverview');
check($ov->version !== '', 'inspect reports a version: ' . $ov->version);
check($ov->pageCount === 1, 'inspect page count');
check($ov->pdfaLevel !== null, 'inspect detected PDF/A level: ' . var_export($ov->pdfaLevel, true));
check($ov->encrypted === false, 'pdfa is not encrypted');
$ovEnc = Pdf::inspect($enc);
check($ovEnc->encrypted === true, 'inspect detects encryption');
check($ovEnc->encryption !== '' && $ovEnc->encryption !== 'none', 'inspect reports encryption label: ' . $ovEnc->encryption);
echo 'inspect ok (version=' . $ov->version . ', pdfaLevel=' . var_export($ov->pdfaLevel, true)
    . ', encrypted=' . var_export($ovEnc->encrypted, true) . ")\n";

// 24. Issue #45 P1: fillRect + placeText, then read the text back.
$drawEd = EditableDoc::load($pdfa);
check($drawEd->fillRect(0, 100.0, 100.0, 200.0, 50.0, [0.9, 0.9, 0.2], 1.0), 'fillRect page 0 existed');
check(!$drawEd->fillRect(99, 0.0, 0.0, 10.0, 10.0), 'fillRect missing page returns false');
check($drawEd->placeText(0, 110.0, 115.0, 'PLACEDHERE', 18.0, [0.0, 0.0, 0.0], 0.0), 'placeText page 0 existed');
check(!$drawEd->placeText(99, 0.0, 0.0, 'x'), 'placeText missing page returns false');
$drawn = $drawEd->toBytes();
check(strlen($drawn) > 0, 'drawn bytes produced');
check(str_contains(Pdf::extractText($drawn), 'PLACEDHERE'), 'placed text is extractable');
echo "fillRect + placeText ok (placed text round-tripped)\n";

// 25. Issue #50: draw an image onto an existing page.
$imgEd = EditableDoc::load($pdfa);
check($imgEd->drawImage(0, $png, 50.0, 50.0, 120.0, 120.0, 0.0), 'drawImage page 0 existed');
check(!$imgEd->drawImage(99, $png, 0.0, 0.0, 10.0, 10.0), 'drawImage missing page returns false');
$imgDrawn = $imgEd->toBytes();
check(strlen($imgDrawn) > 0, 'image-drawn bytes produced');
echo "drawImage ok (image stamped onto page)\n";

echo "OK: full PHP binding surface exercised\n";
