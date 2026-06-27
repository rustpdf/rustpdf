'use strict';

// Node.js binding for the rust-pdf core over its C ABI (libpdf_ffi), via Koffi
// (pure FFI, no native compilation). Covers the whole product surface.

const fs = require('fs');
const path = require('path');
const koffi = require('koffi');

class PdfError extends Error {
  constructor(status, message) {
    super(status ? `PdfStatus=${status}: ${message}` : message);
    this.name = 'PdfError';
    this.status = status;
  }
}

// ---- enums -----------------------------------------------------------------

const PdfaLevel = Object.freeze({ A1b: 0, A2b: 1, A2a: 2, A3b: 3, A3a: 4 });
const Align = Object.freeze({ Left: 0, Right: 1, Center: 2, Justify: 3 });
const AFRelationship = Object.freeze({ Source: 0, Data: 1, Alternative: 2, Supplement: 3, Unspecified: 4 });
const Encryption = Object.freeze({ Rc4: 0, Aes128: 1, Aes256: 2 });

// ---- locate + load the native library --------------------------------------

function libFileName() {
  if (process.platform === 'win32') return 'pdf_ffi.dll';
  if (process.platform === 'darwin') return 'libpdf_ffi.dylib';
  return 'libpdf_ffi.so';
}

// On Linux the cdylib links glibc or musl — pick the matching prebuilt package.
function isMusl() {
  if (process.platform !== 'linux') return false;
  try {
    // glibcVersionRuntime is present on glibc, absent on musl.
    return !process.report.getReport().header.glibcVersionRuntime;
  } catch {
    return false;
  }
}

// Name of the published @rustpdf/<platform> package that ships this host's
// cdylib (mirrors the per-platform wheels the Python binding publishes to PyPI).
function platformPackage() {
  const { platform, arch } = process;
  if (platform === 'linux') return `@rustpdf/linux-${arch}-${isMusl() ? 'musl' : 'gnu'}`;
  if (platform === 'win32') return `@rustpdf/win32-${arch}-msvc`;
  return `@rustpdf/${platform}-${arch}`; // darwin-arm64, darwin-x64
}

function libPath() {
  // 1) explicit override.
  const env = process.env.RUSTPDF_LIB;
  if (env && fs.existsSync(env)) return env;

  const file = libFileName();

  // 2) published per-platform package (the production install path: npm pulls in
  //    only the @rustpdf/<platform> optionalDependency matching os/cpu).
  try {
    return require.resolve(`${platformPackage()}/${file}`);
  } catch {
    /* not installed — monorepo dev, or an unsupported platform; fall through. */
  }

  // 3) workspace build tree (monorepo dev): walk up from lib/ to target/.
  let dir = __dirname;
  for (let i = 0; i < 10; i++) {
    for (const profile of ['debug', 'release']) {
      const candidate = path.join(dir, 'target', profile, file);
      if (fs.existsSync(candidate)) return candidate;
    }
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }

  throw new PdfError(
    0,
    `could not locate ${file}: no matching prebuilt package installed ` +
      `(expected ${platformPackage()}), no RUSTPDF_LIB set, and no ` +
      `target/{debug,release} build found. Install rustpdf from npm, run ` +
      '`cargo build -p pdf-ffi`, or set RUSTPDF_LIB to a libpdf_ffi path.',
  );
}

const lib = koffi.load(libPath());

const f = {
  version: lib.func('const char *pdf_version()'),
  lastError: lib.func('const char *pdf_last_error_message()'),
  activateLicense: lib.func('int pdf_activate_license(const char *token)'),
  bufferFree: lib.func('void pdf_buffer_free(uint8_t *ptr, size_t len)'),

  newDoc: lib.func('void *pdf_document_new()'),
  freeDoc: lib.func('void pdf_document_free(void *doc)'),
  addPage: lib.func('int pdf_document_add_page(void *doc)'),
  addPageSized: lib.func('int pdf_document_add_page_sized(void *doc, double w, double h)'),
  pageCount: lib.func('int pdf_document_page_count(void *doc)'),
  setFillRgb: lib.func('int pdf_page_set_fill_rgb(void *doc, double r, double g, double b)'),
  setStrokeRgb: lib.func('int pdf_page_set_stroke_rgb(void *doc, double r, double g, double b)'),
  setLineWidth: lib.func('int pdf_page_set_line_width(void *doc, double w)'),
  rect: lib.func('int pdf_page_rect(void *doc, double x, double y, double w, double h)'),
  fill: lib.func('int pdf_page_fill(void *doc)'),
  stroke: lib.func('int pdf_page_stroke(void *doc)'),
  save: lib.func('int pdf_document_save(void *doc, const char *path)'),
  write: lib.func('int pdf_document_write(void *doc, _Out_ uint8_t **out, _Out_ size_t *len)'),
  pdfa: lib.func('int pdf_document_pdfa(void *doc)'),
  pdfaLevel: lib.func('int pdf_document_pdfa_level(void *doc, int level)'),
  tagged: lib.func('int pdf_document_tagged(void *doc)'),
  setVersion: lib.func('int pdf_document_set_version(void *doc, int v)'),
  setDefaultSize: lib.func('int pdf_document_set_default_size(void *doc, double w, double h)'),
  setInfo: lib.func('int pdf_document_set_info(void *doc, const char *title, const char *author, const char *subject, const char *keywords, const char *creator)'),
  addFontFile: lib.func('int pdf_document_add_font_file(void *doc, const char *path, _Out_ int *id)'),
  addFont: lib.func('int pdf_document_add_font(void *doc, const uint8_t *data, size_t len, _Out_ int *id)'),
  showText: lib.func('int pdf_page_show_text(void *doc, int font, double size, double x, double y, const char *text, int hl)'),
  paragraph: lib.func('int pdf_page_paragraph(void *doc, int font, double size, double x, double y, double width, int align, const char *text)'),
  addImageFile: lib.func('int pdf_document_add_image_file(void *doc, const char *path, _Out_ int *id)'),
  addImagePng: lib.func('int pdf_document_add_image_png(void *doc, const uint8_t *data, size_t len, _Out_ int *id)'),
  addImageJpeg: lib.func('int pdf_document_add_image_jpeg(void *doc, const uint8_t *data, size_t len, _Out_ int *id)'),
  drawImage: lib.func('int pdf_page_draw_image(void *doc, int image, double x, double y, double w, double h)'),
  figure: lib.func('int pdf_page_figure(void *doc, int image, double x, double y, double w, double h, const char *alt)'),
  attachFile: lib.func('int pdf_document_attach_file(void *doc, const char *name, const char *mime, const uint8_t *data, size_t len, int rel, const char *desc)'),
  textField: lib.func('int pdf_document_text_field(void *doc, const char *name, size_t page, double x0, double y0, double x1, double y1, const char *value, double size)'),
  checkbox: lib.func('int pdf_document_checkbox(void *doc, const char *name, size_t page, double x0, double y0, double x1, double y1, int checked)'),
  dropdown: lib.func('int pdf_document_dropdown(void *doc, const char *name, size_t page, double x0, double y0, double x1, double y1, const char *options, int selected, double size)'),
  radioGroup: lib.func('int pdf_document_radio_group(void *doc, const char *name, size_t page, size_t count, const double *rects, const char **exports, int selected)'),

  edLoad: lib.func('void *pdf_editable_load(const uint8_t *data, size_t len)'),
  edLoadPw: lib.func('void *pdf_editable_load_password(const uint8_t *data, size_t len, const char *password)'),
  edFree: lib.func('void pdf_editable_free(void *ed)'),
  edPageCount: lib.func('int pdf_editable_page_count(void *ed)'),
  edMerge: lib.func('int pdf_editable_merge(void *ed, void *other)'),
  edRotate: lib.func('int pdf_editable_rotate_page(void *ed, size_t index, int degrees)'),
  edDelete: lib.func('int pdf_editable_delete_page(void *ed, size_t index)'),
  edReorder: lib.func('int pdf_editable_reorder_pages(void *ed, const size_t *order, size_t count)'),
  edExtract: lib.func('int pdf_editable_extract_pages(void *ed, const size_t *indices, size_t count, _Out_ void **out)'),
  edSetInfo: lib.func('int pdf_editable_set_info(void *ed, const char *key, const char *value)'),
  edGetInfo: lib.func('int pdf_editable_get_info(void *ed, const char *key, _Out_ uint8_t **out, _Out_ size_t *len)'),
  edSetXmp: lib.func('int pdf_editable_set_xmp(void *ed, const uint8_t *xml, size_t len)'),
  edOverlay: lib.func('int pdf_editable_overlay_page(void *ed, size_t index, const uint8_t *content, size_t len)'),
  edFill: lib.func('int pdf_editable_fill_text_field(void *ed, const char *name, const char *value, _Out_ int *found)'),
  edOptimize: lib.func('int pdf_editable_optimize(void *ed)'),
  edCompact: lib.func('int pdf_editable_compact(void *ed, int on)'),
  edEncrypt: lib.func('int pdf_editable_encrypt(void *ed, int method, const char *user, const char *owner, int readOnly)'),
  edToBytes: lib.func('int pdf_editable_to_bytes(void *ed, _Out_ uint8_t **out, _Out_ size_t *len)'),
  edIncremental: lib.func('int pdf_editable_to_bytes_incremental(void *ed, const uint8_t *original, size_t olen, _Out_ uint8_t **out, _Out_ size_t *len)'),
  edSave: lib.func('int pdf_editable_save(void *ed, const char *path)'),

  extractText: lib.func('int pdf_extract_text(const uint8_t *data, size_t len, _Out_ uint8_t **out, _Out_ size_t *len2)'),
  sign: lib.func('int pdf_sign(const uint8_t *pdf, size_t pl, const uint8_t *key, size_t kl, const uint8_t *cert, size_t cl, const char *reason, const char *location, const char *name, int pades, _Out_ uint8_t **out, _Out_ size_t *len)'),
  timestamp: lib.func('int pdf_timestamp(const uint8_t *pdf, size_t pl, const uint8_t *key, size_t kl, const uint8_t *cert, size_t cl, const char *date, _Out_ uint8_t **out, _Out_ size_t *len)'),
  addDss: lib.func('int pdf_add_dss(const uint8_t *pdf, size_t pl, const uint8_t **cp, const size_t *cl, size_t cc, const uint8_t **rp, const size_t *rl, size_t rc, _Out_ uint8_t **out, _Out_ size_t *len)'),
};

// ---- helpers ---------------------------------------------------------------

function lastError() {
  return f.lastError() || 'unknown error';
}

function check(status) {
  if (status !== 0) throw new PdfError(status, lastError());
}

// call: (outArr, lenArr) => status
function takeBytes(call) {
  const out = [null];
  const len = [0n];
  check(call(out, len));
  const n = Number(len[0]);
  if (!out[0] || n === 0) return Buffer.alloc(0);
  const arr = koffi.decode(out[0], 'uint8_t', n); // Uint8Array
  const buf = Buffer.from(arr);
  f.bufferFree(out[0], n);
  return buf;
}

function asBuf(data) {
  return Buffer.isBuffer(data) ? data : Buffer.from(data);
}

// ---- top-level API ---------------------------------------------------------

function version() {
  return f.version();
}

function activateLicense(token) {
  check(f.activateLicense(token));
}

function extractText(pdf) {
  const b = asBuf(pdf);
  return takeBytes((o, n) => f.extractText(b, b.length, o, n)).toString('utf8');
}

function sign(pdf, keyDer, certDer, opts = {}) {
  const p = asBuf(pdf), k = asBuf(keyDer), c = asBuf(certDer);
  return takeBytes((o, n) => f.sign(
    p, p.length, k, k.length, c, c.length,
    opts.reason ?? null, opts.location ?? null, opts.name ?? null, opts.pades ? 1 : 0, o, n));
}

function timestamp(pdf, tsaKeyDer, tsaCertDer, date = null) {
  const p = asBuf(pdf), k = asBuf(tsaKeyDer), c = asBuf(tsaCertDer);
  return takeBytes((o, n) => f.timestamp(p, p.length, k, k.length, c, c.length, date, o, n));
}

function addDss(pdf, certs = [], crls = []) {
  const p = asBuf(pdf);
  const cb = certs.map(asBuf), rb = crls.map(asBuf);
  return takeBytes((o, n) => f.addDss(
    p, p.length,
    cb, cb.map((x) => x.length), cb.length,
    rb, rb.map((x) => x.length), rb.length, o, n));
}

// ---- Document --------------------------------------------------------------

class Document {
  constructor() {
    this._h = f.newDoc();
    if (!this._h) throw new PdfError(0, 'pdf_document_new returned NULL');
  }

  close() {
    if (this._h) {
      f.freeDoc(this._h);
      this._h = null;
    }
  }

  get _ptr() {
    if (!this._h) throw new PdfError(0, 'operation on a closed Document');
    return this._h;
  }

  pdfa(level) {
    check(level === undefined ? f.pdfa(this._ptr) : f.pdfaLevel(this._ptr, level));
    return this;
  }
  tagged() { check(f.tagged(this._ptr)); return this; }
  setVersion(v) { check(f.setVersion(this._ptr, v)); return this; }
  setDefaultSize(w, h) { check(f.setDefaultSize(this._ptr, w, h)); return this; }
  setInfo({ title = null, author = null, subject = null, keywords = null, creator = null } = {}) {
    check(f.setInfo(this._ptr, title, author, subject, keywords, creator));
    return this;
  }

  addPage(size) {
    check(size ? f.addPageSized(this._ptr, size.width, size.height) : f.addPage(this._ptr));
    return this;
  }
  setFillRgb(r, g, b) { check(f.setFillRgb(this._ptr, r, g, b)); return this; }
  setStrokeRgb(r, g, b) { check(f.setStrokeRgb(this._ptr, r, g, b)); return this; }
  setLineWidth(w) { check(f.setLineWidth(this._ptr, w)); return this; }
  rect(x, y, w, h) { check(f.rect(this._ptr, x, y, w, h)); return this; }
  fill() { check(f.fill(this._ptr)); return this; }
  stroke() { check(f.stroke(this._ptr)); return this; }

  addFontFile(path) { const id = [0]; check(f.addFontFile(this._ptr, path, id)); return id[0]; }
  addFont(data) { const b = asBuf(data); const id = [0]; check(f.addFont(this._ptr, b, b.length, id)); return id[0]; }
  showText(font, size, x, y, text, headingLevel = 0) {
    check(f.showText(this._ptr, font, size, x, y, text, headingLevel));
    return this;
  }
  paragraph(font, size, x, y, width, text, align = Align.Left) {
    check(f.paragraph(this._ptr, font, size, x, y, width, align, text));
    return this;
  }

  addImageFile(path) { const id = [0]; check(f.addImageFile(this._ptr, path, id)); return id[0]; }
  addImagePng(data) { const b = asBuf(data); const id = [0]; check(f.addImagePng(this._ptr, b, b.length, id)); return id[0]; }
  addImageJpeg(data) { const b = asBuf(data); const id = [0]; check(f.addImageJpeg(this._ptr, b, b.length, id)); return id[0]; }
  drawImage(image, x, y, w, h) { check(f.drawImage(this._ptr, image, x, y, w, h)); return this; }
  figure(image, x, y, w, h, alt) { check(f.figure(this._ptr, image, x, y, w, h, alt)); return this; }

  attachFile(name, mime, data, relationship = AFRelationship.Source, description = '') {
    const b = asBuf(data);
    check(f.attachFile(this._ptr, name, mime, b, b.length, relationship, description));
    return this;
  }

  textField(name, page, rect, value = '', size = 0) {
    check(f.textField(this._ptr, name, page, rect[0], rect[1], rect[2], rect[3], value, size));
    return this;
  }
  checkbox(name, page, rect, checked) {
    check(f.checkbox(this._ptr, name, page, rect[0], rect[1], rect[2], rect[3], checked ? 1 : 0));
    return this;
  }
  dropdown(name, page, rect, options, selected = null, size = 0) {
    check(f.dropdown(this._ptr, name, page, rect[0], rect[1], rect[2], rect[3], options.join('\n'), selected ?? -1, size));
    return this;
  }
  // buttons: [{ rect: [x0,y0,x1,y1], export: 'value' }, ...]
  radioGroup(name, page, buttons, selected = null) {
    const rects = new Float64Array(buttons.length * 4);
    const exports = [];
    buttons.forEach((b, i) => {
      rects.set(b.rect, i * 4);
      exports.push(b.export);
    });
    check(f.radioGroup(this._ptr, name, page, buttons.length, rects, exports, selected ?? -1));
    return this;
  }

  get pageCount() { return f.pageCount(this._ptr); }
  toBytes() { const h = this._ptr; return takeBytes((o, n) => f.write(h, o, n)); }
  save(path) { check(f.save(this._ptr, path)); }
}

// ---- EditableDoc -----------------------------------------------------------

class EditableDoc {
  constructor(handle) {
    if (!handle) throw new PdfError(6, lastError());
    this._h = handle;
  }

  static load(data, password) {
    const b = asBuf(data);
    const h = password === undefined || password === null
      ? f.edLoad(b, b.length)
      : f.edLoadPw(b, b.length, password);
    return new EditableDoc(h);
  }

  static loadFile(path, password) {
    return EditableDoc.load(fs.readFileSync(path), password);
  }

  close() { if (this._h) { f.edFree(this._h); this._h = null; } }
  get _ptr() { if (!this._h) throw new PdfError(0, 'operation on a closed EditableDoc'); return this._h; }

  get pageCount() { return f.edPageCount(this._ptr); }
  merge(other) { check(f.edMerge(this._ptr, other._ptr)); return this; }
  rotatePage(index, degrees) { check(f.edRotate(this._ptr, index, degrees)); return this; }
  deletePage(index) { check(f.edDelete(this._ptr, index)); return this; }
  reorderPages(order) { check(f.edReorder(this._ptr, order, order.length)); return this; }
  extractPages(indices) {
    const out = [null];
    check(f.edExtract(this._ptr, indices, indices.length, out));
    return new EditableDoc(out[0]);
  }
  setInfo(key, value) { check(f.edSetInfo(this._ptr, key, value)); return this; }
  getInfo(key) { const h = this._ptr; return takeBytes((o, n) => f.edGetInfo(h, key, o, n)).toString('utf8'); }
  setXmp(xml) { const b = asBuf(xml); check(f.edSetXmp(this._ptr, b, b.length)); return this; }
  overlayPage(index, content) { const b = asBuf(content); check(f.edOverlay(this._ptr, index, b, b.length)); return this; }
  fillTextField(name, value) { const found = [0]; check(f.edFill(this._ptr, name, value, found)); return found[0] !== 0; }
  optimize() { check(f.edOptimize(this._ptr)); return this; }
  compact(on = true) { check(f.edCompact(this._ptr, on ? 1 : 0)); return this; }
  encrypt({ method = Encryption.Aes256, user = '', owner = '', readOnly = false } = {}) {
    check(f.edEncrypt(this._ptr, method, user, owner, readOnly ? 1 : 0));
    return this;
  }
  toBytes() { const h = this._ptr; return takeBytes((o, n) => f.edToBytes(h, o, n)); }
  toBytesIncremental(original) {
    const h = this._ptr;
    const b = asBuf(original);
    return takeBytes((o, n) => f.edIncremental(h, b, b.length, o, n));
  }
  save(path) { check(f.edSave(this._ptr, path)); }
}

module.exports = {
  PdfError,
  PdfaLevel,
  Align,
  AFRelationship,
  Encryption,
  Document,
  EditableDoc,
  version,
  activateLicense,
  extractText,
  sign,
  timestamp,
  addDss,
};
