'use strict';

// Smoke test for the RustPdf Node binding. Exercises the whole surface,
// including licensing gating. Exits non-zero on any failed assertion.

const fs = require('fs');
const os = require('os');
const path = require('path');
const assert = require('assert');
const rp = require('../lib');

function repoRoot() {
  let dir = __dirname;
  for (let i = 0; i < 12; i++) {
    if (fs.existsSync(path.join(dir, 'Cargo.toml'))) return dir;
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  throw new Error('could not locate repo root');
}

const root = repoRoot();
const font = path.join(root, 'assets', 'fonts', 'Roboto-Regular.ttf');
const devLicense = fs.readFileSync(path.join(root, 'crates', 'license', 'fixtures', 'dev_license.txt'), 'utf8').trim();

console.log('rustpdf version:', rp.version());

// 1. Corporate features blocked without a license.
delete process.env.RUSTPDF_LICENSE;
let blocked = false;
try {
  const d = new rp.Document();
  d.pdfa().addPage();
  d.toBytes();
} catch (e) {
  blocked = e instanceof rp.PdfError;
}
assert.ok(blocked, 'PDF/A must be blocked without a license');

rp.activateLicense(devLicense);
console.log('license activated');

// 2. Tagged PDF/A-2a with a font, heading and justified paragraph.
let pdfa;
{
  const d = new rp.Document();
  d.pdfa(rp.PdfaLevel.A2a).setInfo({ title: 'Olá', author: 'rustpdf' });
  const fnt = d.addFontFile(font);
  d.addPage()
    .showText(fnt, 20, 72, 760, 'Título', 1)
    .paragraph(fnt, 12, 72, 720, 450, 'Um parágrafo. '.repeat(8), rp.Align.Justify);
  pdfa = d.toBytes();
  d.close();
}
assert.ok(pdfa.length > 0, 'pdfa bytes');
const text = rp.extractText(pdfa);
assert.ok(text.includes('Título'), `extracted text: ${text}`);
console.log(`built PDF/A-2a (${pdfa.length} bytes); extracted ok`);

// Page rendering (Pro feature; license already active).
assert.strictEqual(rp.pageCount(pdfa), 1, 'page count');
const png = rp.renderPageToPng(pdfa, 0, 72.0);
assert.ok(png.length > 8 && png[1] === 0x50 && png[2] === 0x4e && png[3] === 0x47, 'PNG header');
console.log(`rendered page 0 → ${png.length} byte PNG`);

// 3. Incremental update preserves the original prefix.
{
  const ed = rp.EditableDoc.load(pdfa);
  assert.strictEqual(ed.pageCount, 1, 'page count');
  ed.setInfo('Subject', 'via FFI');
  assert.strictEqual(ed.getInfo('Subject'), 'via FFI', 'get_info');
  const incr = ed.toBytesIncremental(pdfa);
  assert.ok(incr.subarray(0, pdfa.length).equals(pdfa), 'incremental preserves original');
  ed.close();
  console.log(`incremental update ok (${incr.length} bytes)`);
}

// 4. Merge + optimize.
{
  const a = rp.EditableDoc.load(pdfa);
  const b = rp.EditableDoc.load(pdfa);
  a.merge(b).optimize();
  const merged = rp.EditableDoc.load(a.toBytes());
  assert.strictEqual(merged.pageCount, 2, 'merged page count');
  // extractPages (size_t[] in + handle out) and reorderPages (size_t[] in).
  const one = merged.extractPages([1]);
  assert.strictEqual(one.pageCount, 1, 'extracted page count');
  merged.reorderPages([1, 0]);
  assert.strictEqual(merged.pageCount, 2, 'reordered page count');
  a.close(); b.close(); merged.close(); one.close();
  console.log('merge + optimize + extract/reorder ok');
}

// 5. AcroForm with every field type.
{
  const d = new rp.Document();
  d.addPage()
    .textField('city', 0, [120, 700, 300, 720], 'SP', 12)
    .checkbox('ok', 0, [120, 670, 138, 688], true)
    .radioGroup('plan', 0, [
      { rect: [120, 640, 138, 658], export: 'a' },
      { rect: [160, 640, 178, 658], export: 'b' },
    ], 1)
    .dropdown('country', 0, [120, 610, 300, 630], ['BR', 'PT'], 0, 12);
  const form = d.toBytes();
  assert.ok(form.includes(Buffer.from('/AcroForm')), 'AcroForm present');
  d.close();
  console.log('forms ok');
}

// 6. Encryption (AES-256) round-trips.
let plain;
{
  const d = new rp.Document();
  const fnt = d.addFontFile(font);
  d.addPage().showText(fnt, 14, 72, 700, 'segredo');
  plain = d.toBytes();
  d.close();
}
{
  const ed = rp.EditableDoc.load(plain);
  ed.encrypt({ method: rp.Encryption.Aes256, owner: 'owner' });
  const enc = ed.toBytes();
  assert.ok(enc.includes(Buffer.from('/AESV3')), 'AES-256 marker');
  assert.ok(rp.extractText(enc).includes('segredo'), 'decrypted text');
  ed.close();
  console.log('encryption ok');
}

// 7. Digital signature (PKCS#7 / PAdES) with the committed test key.
{
  const fx = path.join(root, 'crates', 'pdf', 'tests', 'fixtures');
  const key = fs.readFileSync(path.join(fx, 'signer_key.pk8'));
  const cert = fs.readFileSync(path.join(fx, 'signer_cert.der'));
  const signed = rp.sign(plain, key, cert, { reason: 'Aprovado', pades: true });
  assert.ok(signed.includes(Buffer.from('/ByteRange')), 'signature ByteRange');
  console.log(`signed ok (${signed.length} bytes)`);
}

// 8. Attachment (PDF/A-3).
{
  const d = new rp.Document();
  d.pdfa(rp.PdfaLevel.A3b);
  const fnt = d.addFontFile(font);
  d.attachFile('data.csv', 'text/csv', Buffer.from('a,b\n1,2\n'), rp.AFRelationship.Source, 'source data')
    .addPage().showText(fnt, 12, 72, 700, 'anexo');
  assert.ok(d.toBytes().includes(Buffer.from('/EmbeddedFile')), 'attachment present');
  d.close();
  console.log('attachment ok');
}

// 9. Extract raster images to a directory.
{
  // A minimal 1x1 PNG.
  const png = Buffer.from(
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
    'base64');
  const d = new rp.Document();
  const img = d.addImagePng(png);
  d.addPage().drawImage(img, 72, 600, 100, 100);
  const withImg = d.toBytes();
  d.close();

  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'rustpdf-imgs-'));
  const count = rp.extractImagesToDir(withImg, dir);
  assert.ok(count >= 1, `expected at least one image, got ${count}`);
  const files = fs.readdirSync(dir);
  assert.strictEqual(files.length, count, 'file count matches returned count');
  console.log(`extracted ${count} image(s) to ${dir}`);
}

// 10. Hyperlinks + bookmarks + Factur-X on a Document (Tier 1/2).
{
  const xml = Buffer.from('<?xml version="1.0"?><rsm:CrossIndustryInvoice/>', 'utf8');
  const d = new rp.Document();
  const fnt = d.addFontFile(font);
  d.addPage().showText(fnt, 14, 72, 700, 'page one');
  d.addPage().showText(fnt, 14, 72, 700, 'page two');
  d.linkUri([72, 690, 200, 710], 'https://example.com')
    .linkToPage([72, 660, 200, 680], 1, 720);

  const root = new rp.Bookmark('Chapter 1', 0);
  root.child(new rp.Bookmark('Section 1.1', 0, 720));
  const ch2 = new rp.Bookmark('Chapter 2', 1, 700);
  d.addBookmark(root).addBookmark(ch2);

  d.facturx(xml, rp.FacturxProfile.EN16931);
  const out = d.toBytes();
  assert.ok(out.includes(Buffer.from('/URI')), 'URI link present');
  assert.ok(out.includes(Buffer.from('/Outlines')), 'outline present');
  assert.ok(out.includes(Buffer.from('factur-x.xml')), 'facturx attachment present');
  d.close();
  console.log(`links + bookmarks + facturx ok (${out.length} bytes)`);
}

// 11. Form fill / set_checkbox / set_radio / set_choice / field_names /
//     flatten_forms on an EditableDoc (Tier 1).
{
  const d = new rp.Document();
  d.addPage()
    .textField('city', 0, [120, 700, 300, 720], '', 12)
    .checkbox('ok', 0, [120, 670, 138, 688], false)
    .radioGroup('plan', 0, [
      { rect: [120, 640, 138, 658], export: 'a' },
      { rect: [160, 640, 178, 658], export: 'b' },
    ], 0)
    .dropdown('country', 0, [120, 610, 300, 630], ['BR', 'PT'], 0, 12);
  const form = d.toBytes();
  d.close();

  const ed = rp.EditableDoc.load(form);
  const names = ed.fieldNames();
  assert.ok(names.includes('city'), `field_names: ${names}`);
  assert.ok(ed.fillTextField('city', 'Lisboa'), 'fill text field');
  assert.ok(ed.setCheckbox('ok', true), 'set checkbox');
  assert.ok(ed.setRadio('plan', 'b'), 'set radio');
  assert.ok(ed.setChoice('country', 'PT'), 'set choice');
  assert.strictEqual(ed.setCheckbox('missing', true), false, 'missing field not found');
  ed.flattenForms();
  const flat = ed.toBytes();
  assert.ok(flat.length > 0, 'flattened bytes');
  ed.close();
  console.log(`form fill + set_* + field_names + flatten ok (${flat.length} bytes)`);
}

// 12. Watermark (text) + redaction + convert_to_pdfa on an EditableDoc (Tier 1/2).
{
  const d = new rp.Document();
  const fnt = d.addFontFile(font);
  d.addPage().showText(fnt, 14, 72, 700, 'confidential body text');
  const base = d.toBytes();
  d.close();

  const ed = rp.EditableDoc.load(base);
  ed.watermarkText('DRAFT', { size: 60, color: [0.6, 0.6, 0.6], opacity: 0.25, rotationDeg: 45 });
  assert.strictEqual(ed.redact(0, [[70, 695, 260, 715]]), true, 'redact page 0');
  assert.strictEqual(ed.redact(99, [[0, 0, 10, 10]]), false, 'redact missing page');
  const wm = ed.toBytes();
  assert.ok(wm.length > 0, 'watermarked bytes');
  ed.close();
  console.log(`watermark + redact ok (${wm.length} bytes)`);

  // convert_to_pdfa needs embedded fonts only (no standard-14 watermark font).
  const ced = rp.EditableDoc.load(base);
  ced.convertToPdfa(rp.PdfaLevel.A2b);
  const out = ced.toBytes();
  assert.ok(out.includes(Buffer.from('pdfaid')), 'PDF/A identifier present');
  ced.close();
  console.log(`convert_to_pdfa ok (${out.length} bytes)`);
}

// 13. Sign a doc, then verify the signature with verifySignatures (Tier 2).
{
  const fx = path.join(root, 'crates', 'pdf', 'tests', 'fixtures');
  const key = fs.readFileSync(path.join(fx, 'signer_key.pk8'));
  const cert = fs.readFileSync(path.join(fx, 'signer_cert.der'));

  const d = new rp.Document();
  const fnt = d.addFontFile(font);
  d.addPage().showText(fnt, 14, 72, 700, 'to be signed');
  const doc = d.toBytes();
  d.close();

  const signed = rp.sign(doc, key, cert, { reason: 'verify' });
  assert.strictEqual(rp.verifySignatures(doc).length, 0, 'unsigned → empty');
  const sigs = rp.verifySignatures(signed);
  assert.strictEqual(sigs.length, 1, `one signature, got ${sigs.length}`);
  const s = sigs[0];
  assert.ok('field_name' in s && 'sub_filter' in s && 'is_valid' in s, 'signature record shape');
  assert.ok(Array.isArray(s.byte_range) && s.byte_range.length === 4, 'byte_range is int[4]');
  console.log(`verify_signatures ok (valid=${s.is_valid}, covers=${s.covers_whole_document})`);
}

// 14. Deferred / external (HSM) signing — issue #41 P0.
{
  const crypto = require('crypto');
  const fx = path.join(root, 'crates', 'pdf', 'tests', 'fixtures');
  // PKCS#8 DER private key + DER certificate (the key stays in *our* process,
  // standing in for an HSM; the library never receives it).
  const keyDer = fs.readFileSync(path.join(fx, 'signer_key.pk8'));
  const cert = fs.readFileSync(path.join(fx, 'signer_cert.der'));
  const privateKey = crypto.createPrivateKey({ key: keyDer, format: 'der', type: 'pkcs8' });

  const d = new rp.Document();
  const fnt = d.addFontFile(font);
  d.addPage().showText(fnt, 14, 72, 700, 'deferred signing');
  const doc = d.toBytes();
  d.close();

  // Pre-signing inventory: no signature fields yet.
  assert.strictEqual(rp.listSignatures(doc).length, 0, 'unsigned → no signature fields');

  // Model A: the lib calls back for the raw RSA-PKCS#1-v1.5-SHA256 signature.
  let signerCalls = 0;
  const signHash = (data) => {
    signerCalls += 1;
    // `data` are the CMS signed attributes; sign their SHA-256 (RSA PKCS#1 v1.5).
    return crypto.sign('sha256', data, { key: privateKey, padding: crypto.constants.RSA_PKCS1_PADDING });
  };
  const signed = rp.signWith(doc, cert, signHash, [], { reason: 'HSM', pades: true });
  assert.ok(signerCalls >= 1, 'remote signer callback was invoked');
  assert.ok(signed.includes(Buffer.from('/ByteRange')), 'Model A produced a /ByteRange');

  // Prove the resulting signature is cryptographically valid end-to-end.
  const sigs = rp.verifySignatures(signed);
  assert.strictEqual(sigs.length, 1, `Model A: one signature, got ${sigs.length}`);
  assert.ok(sigs[0].is_valid, `Model A signature must be valid: ${JSON.stringify(sigs[0])}`);
  console.log(`Model A (signWith) ok — valid=${sigs[0].is_valid}, callbacks=${signerCalls}`);

  // listSignatures now reports exactly one (signed) field.
  const fields = rp.listSignatures(signed);
  assert.strictEqual(fields.length, 1, `listSignatures: one field, got ${fields.length}`);
  assert.strictEqual(fields[0].signed, true, 'the field reports as signed');
  console.log(`listSignatures ok — name=${fields[0].name}, signed=${fields[0].signed}`);

  // Model B: two-phase begin/complete. Check the session shape.
  const session = rp.beginSigning(doc, { reason: 'two-phase' });
  assert.ok(session.document.length > 0, 'session document non-empty');
  assert.ok(session.bytes.length > 0, 'session bytes non-empty');
  assert.strictEqual(session.hash.length, 32, 'session hash is SHA-256 (32 bytes)');
  console.log(`Model B (beginSigning) ok — document=${session.document.length}B, tbs=${session.bytes.length}B, hash=${session.hash.length}B`);
}

// 15. Positional text search (issue #41 P1).
{
  const d = new rp.Document();
  const fnt = d.addFontFile(font);
  d.addPage().showText(fnt, 14, 72, 700, 'findme needle here');
  const doc = d.toBytes();
  d.close();

  const hits = rp.findText(doc, 'needle');
  assert.ok(Array.isArray(hits) && hits.length >= 1, `findText: expected >=1 hit, got ${hits.length}`);
  const h = hits[0];
  assert.ok(typeof h.page === 'number', 'hit.page');
  assert.ok(h.width > 0 && h.height > 0, `hit has a box: ${JSON.stringify(h)}`);
  // case-sensitive search for a different case should miss.
  assert.strictEqual(rp.findText(doc, 'NEEDLE', true).length, 0, 'case-sensitive miss');
  console.log(`findText ok — ${hits.length} hit(s), box=${h.width.toFixed(1)}x${h.height.toFixed(1)}`);
}

// 16. Normalization: set_version + normalize on a loaded doc (issue #41 P1).
{
  const ed = rp.EditableDoc.load(pdfa);
  ed.setVersion(rp.PdfVersion.V1_7);
  const v17 = ed.toBytes();
  assert.ok(v17.length > 0, 'set_version bytes');
  ed.close();

  const ed2 = rp.EditableDoc.load(pdfa);
  ed2.normalize(rp.PdfVersion.V1_7);
  const norm = ed2.toBytes();
  assert.ok(!norm.includes(Buffer.from('pdfaid')), 'normalize strips PDF/A identifier');
  ed2.close();
  console.log(`set_version + normalize ok (${norm.length} bytes)`);
}

// 17. Rich signature inspection fields (issue #41 P1).
{
  const fx = path.join(root, 'crates', 'pdf', 'tests', 'fixtures');
  const key = fs.readFileSync(path.join(fx, 'signer_key.pk8'));
  const cert = fs.readFileSync(path.join(fx, 'signer_cert.der'));
  const d = new rp.Document();
  const fnt = d.addFontFile(font);
  d.addPage().showText(fnt, 14, 72, 700, 'rich verify');
  const doc = d.toBytes();
  d.close();

  const signed = rp.sign(doc, key, cert, { reason: 'rich' });
  const s = rp.verifySignatures(signed)[0];
  for (const k of ['issuer', 'serial_number', 'valid_from', 'valid_to', 'algorithm', 'signing_time', 'cert_count', 'has_timestamp']) {
    assert.ok(k in s, `rich verify field ${k} present`);
  }
  assert.ok(typeof s.cert_count === 'number', 'cert_count is a number');
  assert.ok(typeof s.has_timestamp === 'boolean', 'has_timestamp is a boolean');
  console.log(`rich verify fields ok — issuer=${s.issuer}, algorithm=${s.algorithm}, certs=${s.cert_count}`);
}

// 18. Page geometry inspection: measurePages / measurePage (issue #45 P1).
{
  const d = new rp.Document();
  d.addPage({ width: 200, height: 400 }); // portrait
  d.addPage({ width: 400, height: 200 }); // landscape
  const doc = d.toBytes();
  d.close();

  const pages = rp.measurePages(doc);
  assert.strictEqual(pages.length, 2, 'measurePages count');
  assert.ok(Math.abs(pages[0].width - 200) < 0.01 && Math.abs(pages[0].height - 400) < 0.01, 'page 0 size');
  assert.strictEqual(pages[0].rotation, 0, 'page 0 unrotated');
  assert.ok(Math.abs(pages[0].mediaBox.width - 200) < 0.01, 'mediaBox is a PdfRect with width');
  assert.ok(Math.abs(pages[0].cropBox.height - 400) < 0.01, 'cropBox is a PdfRect with height');

  // Rotate page 0 by 90° → rotatedWidth/Height swap relative to width/height.
  const ed = rp.EditableDoc.load(doc);
  ed.rotatePage(0, 90);
  const rotated = ed.toBytes();
  ed.close();

  const g = rp.measurePage(rotated, 0);
  assert.strictEqual(g.rotation, 90, 'rotation recorded');
  assert.ok(Math.abs(g.rotatedWidth - g.height) < 0.01, 'rotatedWidth == unrotated height');
  assert.ok(Math.abs(g.rotatedHeight - g.width) < 0.01, 'rotatedHeight == unrotated width');

  // Out-of-range index throws RangeError.
  assert.throws(() => rp.measurePage(rotated, 5), RangeError, 'measurePage out of range');
  console.log(`measurePages ok — ${pages.length} pages, rotated 90° swaps ${g.rotatedWidth.toFixed(0)}x${g.rotatedHeight.toFixed(0)}`);
}

// 19. Non-mutating overview: inspect (issue #45 P1).
{
  const ov = rp.inspect(pdfa);
  assert.ok(typeof ov.version === 'string' && ov.version.length > 0, 'overview version');
  assert.ok(ov.pdfaLevel !== null, `overview pdfaLevel for a PDF/A doc: ${ov.pdfaLevel}`);
  assert.strictEqual(ov.encrypted, false, 'pdfa not encrypted');
  assert.strictEqual(ov.requiresPassword, false, 'pdfa needs no password');
  assert.ok(ov.pageCount >= 1, `overview pageCount: ${ov.pageCount}`);

  const plain = (() => { const d = new rp.Document(); d.addPage(); const b = d.toBytes(); d.close(); return b; })();
  const ovPlain = rp.inspect(plain);
  assert.strictEqual(ovPlain.pdfaLevel, null, 'plain doc has null pdfaLevel');
  assert.strictEqual(ovPlain.pageCount, 1, 'plain doc page count');
  console.log(`inspect ok — version=${ov.version}, pdfaLevel=${ov.pdfaLevel}, pages=${ov.pageCount}`);
}

// 20. Stamping: fillRect + placeText, then verify placed text extracts (issue #45 P1).
{
  const ed = rp.EditableDoc.load(pdfa);
  assert.strictEqual(ed.fillRect(0, 60, 60, 200, 40, [1, 1, 1], 1.0), true, 'fillRect page 0');
  assert.strictEqual(ed.placeText(0, 70, 72, 'STAMPED-XYZ', 18, [0, 0, 0], 0.0), true, 'placeText page 0');
  // aligned placeText (right-aligned about the anchor) — issue #50 follow-up.
  assert.strictEqual(
    ed.placeText(0, 500, 100, 'RIGHT-ALIGNED', 14, [0, 0, 0], 0.0, rp.Align.Right),
    true, 'placeText aligned page 0');
  // maskedText: paint a box and write centered text over it.
  assert.strictEqual(
    ed.maskedText(0, 60, 120, 200, 30, 'MASKED-ABC', 14, [0, 0, 0], [1, 1, 0], rp.Align.Center),
    true, 'maskedText page 0');
  // missing page returns false.
  assert.strictEqual(ed.fillRect(99, 0, 0, 10, 10), false, 'fillRect missing page');
  assert.strictEqual(ed.placeText(99, 0, 0, 'nope'), false, 'placeText missing page');
  assert.strictEqual(ed.placeText(99, 0, 0, 'nope', 12, [0, 0, 0], 0.0, rp.Align.Center), false, 'placeText aligned missing page');
  assert.strictEqual(ed.maskedText(99, 0, 0, 10, 10, 'nope'), false, 'maskedText missing page');
  const out = ed.toBytes();
  ed.close();
  assert.ok(rp.extractText(out).includes('STAMPED-XYZ'), 'placed text extracts');
  assert.ok(rp.extractText(out).includes('RIGHT-ALIGNED'), 'aligned text extracts');
  assert.ok(rp.extractText(out).includes('MASKED-ABC'), 'masked text extracts');
  // extractPageText: single-page extraction returns the same stamped text.
  assert.ok(rp.extractPageText(out, 0).includes('STAMPED-XYZ'), 'extractPageText page 0');
  console.log(`fillRect + placeText(align) + maskedText + extractPageText ok (${out.length} bytes)`);
}

// 21. Stamp an image onto an existing page (issue #50).
{
  // A minimal 1x1 PNG.
  const png = Buffer.from(
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
    'base64');
  const ed = rp.EditableDoc.load(pdfa);
  assert.strictEqual(ed.drawImage(0, png, 72, 600, 100, 100, 0.0), true, 'drawImage page 0');
  // missing page returns false.
  assert.strictEqual(ed.drawImage(99, png, 0, 0, 10, 10), false, 'drawImage missing page');
  const out = ed.toBytes();
  ed.close();
  assert.ok(out.length > 0, 'drawImage serializes');
  console.log(`drawImage ok (${out.length} bytes)`);
}

console.log('OK: full Node binding surface exercised');
