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

console.log('OK: full Node binding surface exercised');
