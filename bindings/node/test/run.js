'use strict';

// Smoke test for the RustPdf Node binding. Exercises the whole surface,
// including licensing gating. Exits non-zero on any failed assertion.

const fs = require('fs');
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

console.log('OK: full Node binding surface exercised');
