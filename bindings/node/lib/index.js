'use strict';

// Node.js binding for the rust-pdf core over its C ABI (libpdf_ffi), via Koffi
// (pure FFI, no native compilation). Covers the whole product surface.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const koffi = require('koffi');

class PdfError extends Error {
  constructor(status, message) {
    super(status ? `PdfStatus=${status}: ${message}` : message);
    this.name = 'PdfError';
    this.status = status;
  }
}

// ---- enums -----------------------------------------------------------------

const PdfaLevel = Object.freeze({ A1b: 0, A2b: 1, A2a: 2, A3b: 3, A3a: 4, A4: 5, A4e: 6, A4f: 7 });
const Align = Object.freeze({ Left: 0, Right: 1, Center: 2, Justify: 3 });
const AFRelationship = Object.freeze({ Source: 0, Data: 1, Alternative: 2, Supplement: 3, Unspecified: 4 });
const Encryption = Object.freeze({ Rc4: 0, Aes128: 1, Aes256: 2 });
const FacturxProfile = Object.freeze({ Minimum: 0, BasicWL: 1, Basic: 2, EN16931: 3, Extended: 4 });
// PDF version codes (Document.setVersion / EditableDoc.setVersion / normalize).
const PdfVersion = Object.freeze({ V1_4: 0, V1_5: 1, V1_7: 2, V2_0: 3 });
// DocMDP certification level applied by the first (certifying) signature.
const Certify = Object.freeze({ None: 0, Locked: 1, Forms: 2, FormsAndAnnotations: 3 });

// ---- Bookmark (document outline tree) --------------------------------------

class Bookmark {
  // top is optional; children is an array of Bookmark.
  constructor(title, page, top = null, children = []) {
    this.title = title;
    this.page = page;
    this.top = top;
    this.children = Array.from(children);
  }

  // Append a child bookmark; returns the child (for chaining nested builds).
  child(bookmark) {
    this.children.push(bookmark);
    return bookmark;
  }

  // Pre-order flatten into the parallel-array shape the C API expects.
  _flatten(level, out) {
    out.push({ level, title: this.title, page: this.page, top: this.top });
    for (const c of this.children) c._flatten(level + 1, out);
  }
}

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

// Deferred / external (HSM) signing — issue #41 P0.
// The C-ABI PdfSigningOptions struct (field order must match include/pdf.h).
const SigningOptionsStruct = koffi.struct('PdfSigningOptions', {
  reason: 'const char *',
  location: 'const char *',
  name: 'const char *',
  pades: 'int',
  certification: 'int',
  estimated_size: 'size_t',
  policy_oid: 'const char *',
  policy_hash: 'const uint8_t *',
  policy_hash_len: 'size_t',
  policy_hash_alg_oid: 'const char *',
  policy_uri: 'const char *',
  // Visible signature + embedded image — issue #41 P1 (appended at the end).
  visible: 'int',
  vis_page: 'size_t',
  vis_rect: koffi.array('double', 4),
  vis_text: 'const char *',
  vis_image: 'const uint8_t *',
  vis_image_len: 'size_t',
});
void SigningOptionsStruct; // registered by name; referenced in func strings below

// Model A callback: produce the raw RSA PKCS#1 v1.5 signature over SHA-256 of
// `data`, writing it into `sig_buf` (capacity `sig_cap`), set `*sig_len`, return 0.
const SignHashFn = koffi.proto(
  'int PdfSignHashFn(void *ctx, uint8_t *data, size_t data_len, uint8_t *sig_buf, size_t sig_cap, size_t *sig_len)',
);

const f = {
  version: lib.func('const char *pdf_version()'),
  lastError: lib.func('const char *pdf_last_error_message()'),
  activateLicense: lib.func('int pdf_activate_license(const char *token)'),
  bufferFree: lib.func('void pdf_buffer_free(uint8_t *ptr, size_t len)'),

  newDoc: lib.func('void *pdf_document_new()'),
  freeDoc: lib.func('void pdf_document_free(void *doc)'),
  addPage: lib.func('int pdf_document_add_page(void *doc)'),
  addPageSized: lib.func('int pdf_document_add_page_sized(void *doc, double w, double h)'),
  docPageCount: lib.func('int pdf_document_page_count(void *doc)'),
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

  // Tier 1: hyperlinks + bookmarks + Factur-X (Document)
  linkUri: lib.func('int pdf_page_link_uri(void *doc, double x0, double y0, double x1, double y1, const char *uri)'),
  linkToPage: lib.func('int pdf_page_link_to_page(void *doc, double x0, double y0, double x1, double y1, size_t target_page, double top, int has_top)'),
  addBookmarks: lib.func('int pdf_document_add_bookmarks(void *doc, size_t count, const int *levels, const char **titles, const size_t *pages, const double *tops, const int *has_tops)'),
  facturx: lib.func('int pdf_document_facturx(void *doc, const uint8_t *xml, size_t len, int profile)'),

  // Tier 1: form fill + flatten + field names + watermark (EditableDoc)
  edSetCheckbox: lib.func('int pdf_editable_set_checkbox(void *ed, const char *name, int checked, _Out_ int *found)'),
  edSetRadio: lib.func('int pdf_editable_set_radio(void *ed, const char *name, const char *export_value, _Out_ int *found)'),
  edSetChoice: lib.func('int pdf_editable_set_choice(void *ed, const char *name, const char *value, _Out_ int *found)'),
  edFlatten: lib.func('int pdf_editable_flatten_forms(void *ed)'),
  edFieldNames: lib.func('int pdf_editable_field_names(void *ed, _Out_ uint8_t **out, _Out_ size_t *len)'),
  edWatermarkText: lib.func('int pdf_editable_watermark_text(void *ed, const char *text, double size, double r, double g, double b, double opacity, double rotation_deg, int opaque_background)'),
  edWatermarkImage: lib.func('int pdf_editable_watermark_image_file(void *ed, const char *path, double width, double height, double opacity, double rotation_deg)'),

  // Normalization (issue #41 P1) — version downgrade / strip PDF/A (EditableDoc).
  edSetVersion: lib.func('int pdf_editable_set_version(void *ed, int version)'),
  edStripPdfa: lib.func('int pdf_editable_strip_pdfa(void *ed)'),
  edNormalize: lib.func('int pdf_editable_normalize(void *ed, int version)'),

  // Tier 2: redaction + PDF/A conversion (EditableDoc)
  edRedact: lib.func('int pdf_editable_redact(void *ed, size_t index, const double *rects, size_t count, _Out_ int *found)'),
  edConvertPdfa: lib.func('int pdf_editable_convert_to_pdfa(void *ed, int level)'),

  // Stamping (issue #45 P1): fill a rectangle / place a line of text on a page.
  edFillRect: lib.func('int pdf_editable_fill_rect(void *ed, int index, double x, double y, double width, double height, double r, double g, double b, double opacity, _Out_ int *found)'),
  edPlaceText: lib.func('int pdf_editable_place_text(void *ed, int index, double x, double y, const char *text, double size, double r, double g, double b, double rotation_deg, _Out_ int *found)'),

  // Tier 2: signature verification (module-level)
  verifySignatures: lib.func('int pdf_verify_signatures_json(const uint8_t *data, size_t len, _Out_ uint8_t **out, _Out_ size_t *len2)'),

  // Positional text search (issue #41 P1) — JSON array of bounding boxes.
  findText: lib.func('int pdf_find_text_json(const uint8_t *data, size_t len, const char *query, int case_sensitive, _Out_ uint8_t **out, _Out_ size_t *len2)'),

  // Inspection (issue #45 P1): per-page geometry + non-mutating overview, both JSON.
  measurePages: lib.func('int pdf_measure_pages_json(const uint8_t *data, size_t len, _Out_ uint8_t **out, _Out_ size_t *len2)'),
  inspect: lib.func('int pdf_inspect_json(const uint8_t *data, size_t len, _Out_ uint8_t **out, _Out_ size_t *len2)'),

  extractText: lib.func('int pdf_extract_text(const uint8_t *data, size_t len, _Out_ uint8_t **out, _Out_ size_t *len2)'),
  extractImagesToDir: lib.func('int pdf_extract_images_to_dir(const uint8_t *data, size_t len, const char *dir, _Out_ size_t *out_count)'),
  renderPageToPng: lib.func('int pdf_render_page_to_png(const uint8_t *data, size_t len, size_t page_index, double dpi, _Out_ uint8_t **out, _Out_ size_t *len2)'),
  pageCount: lib.func('int pdf_page_count(const uint8_t *data, size_t len, _Out_ size_t *out_count)'),
  sign: lib.func('int pdf_sign(const uint8_t *pdf, size_t pl, const uint8_t *key, size_t kl, const uint8_t *cert, size_t cl, const char *reason, const char *location, const char *name, int pades, _Out_ uint8_t **out, _Out_ size_t *len)'),
  timestamp: lib.func('int pdf_timestamp(const uint8_t *pdf, size_t pl, const uint8_t *key, size_t kl, const uint8_t *cert, size_t cl, const char *date, _Out_ uint8_t **out, _Out_ size_t *len)'),
  addDss: lib.func('int pdf_add_dss(const uint8_t *pdf, size_t pl, const uint8_t **cp, const size_t *cl, size_t cc, const uint8_t **rp, const size_t *rl, size_t rc, _Out_ uint8_t **out, _Out_ size_t *len)'),

  // Deferred / external (HSM) signing — issue #41 P0.
  signBegin: lib.func('int pdf_sign_begin(const uint8_t *pdf, size_t pl, const PdfSigningOptions *params, _Out_ uint8_t **out_doc, _Out_ size_t *out_doc_len, _Out_ uint8_t **out_tbs, _Out_ size_t *out_tbs_len)'),
  signComplete: lib.func('int pdf_sign_complete(const uint8_t *document, size_t dl, const uint8_t *container, size_t cl, _Out_ uint8_t **out, _Out_ size_t *len)'),
  signWith: lib.func('int pdf_sign_with(const uint8_t *pdf, size_t pl, const uint8_t *cert, size_t cl, const uint8_t **chain_ptrs, const size_t *chain_lens, size_t chain_count, const PdfSigningOptions *params, PdfSignHashFn *callback, void *ctx, _Out_ uint8_t **out, _Out_ size_t *len)'),
  listSignatures: lib.func('int pdf_list_signatures(const uint8_t *pdf, size_t pl, _Out_ uint8_t **out, _Out_ size_t *len)'),

  // Network TSA (AD-RT) — issue #41 P1.
  timestampBegin: lib.func('int pdf_timestamp_begin(const uint8_t *pdf, size_t pl, _Out_ uint8_t **out_doc, _Out_ size_t *out_doc_len, _Out_ uint8_t **out_tbs, _Out_ size_t *out_tbs_len)'),
  timestampRequest: lib.func('int pdf_timestamp_request(const uint8_t *imprint, size_t il, const uint8_t *nonce, size_t nl, int cert_req, _Out_ uint8_t **out, _Out_ size_t *len)'),
  timestampTokenFromResponse: lib.func('int pdf_timestamp_token_from_response(const uint8_t *response, size_t rl, _Out_ uint8_t **out, _Out_ size_t *len)'),
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

// Copy a single native out-buffer into a Buffer and free it (used where one
// call returns two out-buffers and takeBytes' single-buffer shape doesn't fit).
function copyAndFree(ptr, n) {
  if (!ptr || n === 0) return Buffer.alloc(0);
  const buf = Buffer.from(koffi.decode(ptr, 'uint8_t', n));
  f.bufferFree(ptr, n);
  return buf;
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

function extractImagesToDir(pdf, dir) {
  const b = asBuf(pdf);
  const count = [0n];
  check(f.extractImagesToDir(b, b.length, dir, count));
  return Number(count[0]);
}

function renderPageToPng(pdf, page = 0, dpi = 150.0) {
  const b = asBuf(pdf);
  return takeBytes((o, n) => f.renderPageToPng(b, b.length, page, dpi, o, n));
}

function pageCount(pdf) {
  const b = asBuf(pdf);
  const count = [0n];
  check(f.pageCount(b, b.length, count));
  return Number(count[0]);
}

// Validate every signature; returns one record object per signature (empty when
// the document is unsigned). Fields: field_name, sub_filter, signer,
// covers_whole_document, digest_valid, signature_valid, is_valid, byte_range.
function verifySignatures(pdf) {
  const b = asBuf(pdf);
  const js = takeBytes((o, n) => f.verifySignatures(b, b.length, o, n)).toString('utf8');
  return js ? JSON.parse(js) : [];
}

// Find every occurrence of `query` in `pdf`; returns an array of bounding boxes
// { page, text, x, y, width, height } (coords in PDF points, origin lower-left).
// `caseSensitive` defaults to false (case-insensitive). Empty array = no match.
function findText(pdf, query, caseSensitive = false) {
  const b = asBuf(pdf);
  const js = takeBytes((o, n) => f.findText(b, b.length, query, caseSensitive ? 1 : 0, o, n)).toString('utf8');
  return js ? JSON.parse(js) : [];
}

// Build a PdfRect ({ x0, y0, x1, y1, width, height }) from a JSON [x0,y0,x1,y1].
function toRect(a) {
  const [x0, y0, x1, y1] = Array.isArray(a) ? a : [0, 0, 0, 0];
  return { x0, y0, x1, y1, width: Math.abs(x1 - x0), height: Math.abs(y1 - y0) };
}

// Read per-page geometry: an array of PageGeometry objects. Sizes are in PDF
// points; `width`/`height` are unrotated, `rotatedWidth`/`rotatedHeight` account
// for `/Rotate` (swapped for 90/270). `mediaBox`/`cropBox` are PdfRect objects.
function measurePages(pdf) {
  const b = asBuf(pdf);
  const js = takeBytes((o, n) => f.measurePages(b, b.length, o, n)).toString('utf8');
  const arr = js ? JSON.parse(js) : [];
  return arr.map((p) => ({
    page: p.page,
    width: p.width,
    height: p.height,
    rotation: p.rotation,
    rotatedWidth: p.rotatedWidth,
    rotatedHeight: p.rotatedHeight,
    mediaBox: toRect(p.mediaBox),
    cropBox: toRect(p.cropBox),
  }));
}

// Geometry of a single page (0-based). Throws RangeError if out of range.
function measurePage(pdf, index) {
  const pages = measurePages(pdf);
  if (index < 0 || index >= pages.length) {
    throw new RangeError(`page index ${index} out of range (0..${pages.length})`);
  }
  return pages[index];
}

// Non-mutating overview: { version, pdfaLevel, encrypted, encryption,
// requiresPassword, pageCount }. Never fails on a password-locked file.
function inspect(pdf) {
  const b = asBuf(pdf);
  const js = takeBytes((o, n) => f.inspect(b, b.length, o, n)).toString('utf8');
  return JSON.parse(js);
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

// ---- deferred / external (HSM) signing — issue #41 P0 ----------------------

// Marshal a JS SigningOptions object into the native PdfSigningOptions struct.
// SigningOptions: { reason?, location?, name?, pades?, certify?, containerSize?,
//   policy?, visible?, visiblePage?, visibleRect?, visibleText?, visibleImage? },
// where policy = { oid, hash, hashAlgorithmOid?, uri? }.
function buildSigningOptions(options) {
  const o = options || {};
  const pol = o.policy || null;
  const hash = pol && pol.hash ? asBuf(pol.hash) : null;
  const visImg = o.visibleImage ? asBuf(o.visibleImage) : null;
  const visRect = Array.isArray(o.visibleRect) ? o.visibleRect : [0, 0, 0, 0];
  return {
    reason: o.reason ?? null,
    location: o.location ?? null,
    name: o.name ?? null,
    pades: o.pades ? 1 : 0,
    certification: o.certify ?? Certify.None,
    estimated_size: o.containerSize && o.containerSize > 0 ? o.containerSize : 0,
    policy_oid: pol ? pol.oid ?? null : null,
    policy_hash: hash && hash.length > 0 ? hash : null,
    policy_hash_len: hash ? hash.length : 0,
    policy_hash_alg_oid: pol ? pol.hashAlgorithmOid ?? null : null,
    policy_uri: pol ? pol.uri ?? null : null,
    // Visible signature + embedded image (issue #41 P1).
    visible: o.visible ? 1 : 0,
    vis_page: o.visiblePage && o.visiblePage > 0 ? o.visiblePage : 0,
    vis_rect: [visRect[0] || 0, visRect[1] || 0, visRect[2] || 0, visRect[3] || 0],
    vis_text: o.visibleText ?? null,
    vis_image: visImg && visImg.length > 0 ? visImg : null,
    vis_image_len: visImg ? visImg.length : 0,
  };
}

// An in-progress two-phase signature (Model B). `document` holds the prepared
// PDF (zero-filled /Contents placeholder); `bytes` the exact bytes the
// signature covers. Hand `hash` to a remote HSM, build the CMS container, then
// call `complete`.
class SigningSession {
  constructor(document, bytes) {
    this.document = document; // Uint8Array (Buffer)
    this.bytes = bytes; // Uint8Array (Buffer)
  }

  // SHA-256 of `bytes` — the value an HSM signs.
  get hash() {
    return crypto.createHash('sha256').update(this.bytes).digest();
  }

  // Phase 2: embed a finished DER CMS / PKCS#7 container, returning the final PDF.
  complete(container) {
    return completeSignature(this.document, container);
  }
}

// Model A — remote signer. Sign `pdf` without handing this library a key: it
// builds the CMS signed attributes and calls `signHash(data: Buffer) => Buffer`
// for the raw RSA PKCS#1 v1.5 signature over SHA-256 of `data`, then assembles
// and embeds the CMS. `certDer` is the signer certificate; `chain` are
// intermediates (DER), supplied independently of the key.
function signWith(pdf, certDer, signHash, chain = [], options) {
  const p = asBuf(pdf), c = asBuf(certDer);
  const chainBufs = chain.map(asBuf);
  const params = buildSigningOptions(options);
  const jsCb = (ctx, data, dataLen, sigBuf, sigCap, sigLenPtr) => {
    try {
      const input = Buffer.from(koffi.decode(data, 'uint8_t', Number(dataLen)));
      const sig = asBuf(signHash(input));
      if (sig.length > Number(sigCap)) return 2; // buffer too small
      koffi.encode(sigBuf, 'uint8_t', sig, sig.length);
      koffi.encode(sigLenPtr, 'size_t', sig.length);
      return 0;
    } catch {
      return 1; // signer threw
    }
  };
  const cb = koffi.register(jsCb, koffi.pointer(SignHashFn));
  try {
    return takeBytes((o, n) => f.signWith(
      p, p.length, c, c.length,
      chainBufs, chainBufs.map((x) => x.length), chainBufs.length,
      params, cb, null, o, n));
  } finally {
    koffi.unregister(cb);
  }
}

// Model B — two-phase signing, phase 1. Prepare `pdf` for deferred signing and
// return a SigningSession. The key never reaches this library.
function beginSigning(pdf, options) {
  const p = asBuf(pdf);
  const params = buildSigningOptions(options);
  const outDoc = [null], outDocLen = [0n], outTbs = [null], outTbsLen = [0n];
  check(f.signBegin(p, p.length, params, outDoc, outDocLen, outTbs, outTbsLen));
  const document = copyAndFree(outDoc[0], Number(outDocLen[0]));
  const bytes = copyAndFree(outTbs[0], Number(outTbsLen[0]));
  return new SigningSession(document, bytes);
}

// Model B — two-phase signing, phase 2. Embed a complete DER CMS / PKCS#7
// `container` into a prepared `document` (from beginSigning), returning the
// final signed PDF.
function completeSignature(document, container) {
  const d = asBuf(document), c = asBuf(container);
  return takeBytes((o, n) => f.signComplete(d, d.length, c, c.length, o, n));
}

// List the signature fields in `pdf` (detect existing signatures before
// signing). Returns [{ name, signed }, ...]; an empty array means none.
function listSignatures(pdf) {
  const b = asBuf(pdf);
  const text = takeBytes((o, n) => f.listSignatures(b, b.length, o, n)).toString('utf8');
  const out = [];
  for (const line of text.split('\n')) {
    if (!line) continue;
    const tab = line.indexOf('\t');
    if (tab < 0) continue;
    out.push({ name: line.slice(tab + 1), signed: line.slice(0, tab) === '1' });
  }
  return out;
}

// ---- network TSA (AD-RT) — issue #41 P1 ------------------------------------

// Phase 1 of a network (RFC 3161) timestamp. Prepare `pdf` for a /DocTimeStamp
// and return { document, bytes }: SHA-256 `bytes`, build a request with
// timestampRequest, POST it to the TSA, extract the token with
// timestampTokenFromResponse, then embed it via completeSignature(document, token).
function beginTimestamp(pdf) {
  const p = asBuf(pdf);
  const outDoc = [null], outDocLen = [0n], outTbs = [null], outTbsLen = [0n];
  check(f.timestampBegin(p, p.length, outDoc, outDocLen, outTbs, outTbsLen));
  const document = copyAndFree(outDoc[0], Number(outDocLen[0]));
  const bytes = copyAndFree(outTbs[0], Number(outTbsLen[0]));
  return { document, bytes };
}

// Build an RFC 3161 TimeStampReq (DER) for `imprint` (the SHA-256 of the bytes
// to timestamp). `nonce` is optional (null = none); `certReq` asks the TSA to
// embed its certificate. POST the returned bytes to the TSA.
function timestampRequest(imprint, nonce = null, certReq = true) {
  const im = asBuf(imprint);
  const nb = nonce ? asBuf(nonce) : null;
  return takeBytes((o, n) => f.timestampRequest(
    im, im.length, nb, nb ? nb.length : 0, certReq ? 1 : 0, o, n));
}

// Extract the TimeStampToken (CMS ContentInfo) from a TSA's RFC 3161
// TimeStampResp. Embed the result via completeSignature(document, token).
function timestampTokenFromResponse(response) {
  const r = asBuf(response);
  return takeBytes((o, n) => f.timestampTokenFromResponse(r, r.length, o, n));
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

  // hyperlinks (Tier 1)
  linkUri(rect, uri) {
    check(f.linkUri(this._ptr, rect[0], rect[1], rect[2], rect[3], uri));
    return this;
  }
  linkToPage(rect, pageIndex, top = null) {
    check(f.linkToPage(this._ptr, rect[0], rect[1], rect[2], rect[3], pageIndex,
      top == null ? 0.0 : top, top == null ? 0 : 1));
    return this;
  }

  // bookmarks / outline (Tier 1) — one root tree per call, pre-order flattened.
  addBookmark(bookmark) {
    const entries = [];
    bookmark._flatten(0, entries);
    const n = entries.length;
    const levels = new Int32Array(n);
    const pages = new Array(n);
    const titles = new Array(n);
    const tops = new Float64Array(n);
    const hasTops = new Int32Array(n);
    entries.forEach((e, i) => {
      levels[i] = e.level;
      pages[i] = e.page;
      titles[i] = e.title;
      if (e.top == null) { hasTops[i] = 0; tops[i] = 0.0; } else { hasTops[i] = 1; tops[i] = e.top; }
    });
    check(f.addBookmarks(this._ptr, n, levels, titles, pages, tops, hasTops));
    return this;
  }

  // ZUGFeRD / Factur-X (Tier 2)
  facturx(xml, profile = FacturxProfile.EN16931) {
    const b = asBuf(xml);
    check(f.facturx(this._ptr, b, b.length, profile));
    return this;
  }

  get pageCount() { return f.docPageCount(this._ptr); }
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

  // form fill + flatten + field names (Tier 1)
  setCheckbox(name, checked = true) {
    const found = [0];
    check(f.edSetCheckbox(this._ptr, name, checked ? 1 : 0, found));
    return found[0] !== 0;
  }
  setRadio(name, exportValue) {
    const found = [0];
    check(f.edSetRadio(this._ptr, name, exportValue, found));
    return found[0] !== 0;
  }
  setChoice(name, value) {
    const found = [0];
    check(f.edSetChoice(this._ptr, name, value, found));
    return found[0] !== 0;
  }
  flattenForms() { check(f.edFlatten(this._ptr)); return this; }
  fieldNames() {
    const h = this._ptr;
    const text = takeBytes((o, n) => f.edFieldNames(h, o, n)).toString('utf8');
    return text.split('\n').filter((s) => s.length > 0);
  }

  // watermarks (Tier 1; opaqueBackground / rotationDeg added in issue #41 P1)
  watermarkText(text, { size = 64.0, color = [0.5, 0.5, 0.5], opacity = 0.30, rotationDeg = 45.0, opaqueBackground = false } = {}) {
    const [r, g, b] = color;
    check(f.edWatermarkText(this._ptr, text, size, r, g, b, opacity, rotationDeg, opaqueBackground ? 1 : 0));
    return this;
  }
  watermarkImageFile(path, width, height, opacity = 0.30, rotationDeg = 0.0) {
    check(f.edWatermarkImage(this._ptr, path, width, height, opacity, rotationDeg));
    return this;
  }

  // normalization (issue #41 P1): downgrade version / strip PDF/A.
  setVersion(version) { check(f.edSetVersion(this._ptr, version)); return this; }
  stripPdfa() { check(f.edStripPdfa(this._ptr)); return this; }
  normalize(version = 2) { check(f.edNormalize(this._ptr, version)); return this; }

  // redaction + PDF/A conversion (Tier 2)
  redact(pageIndex, rects) {
    const n = rects.length;
    const flat = new Float64Array(n * 4);
    rects.forEach((r, i) => flat.set(r, i * 4));
    const found = [0];
    check(f.edRedact(this._ptr, pageIndex, flat, n, found));
    return found[0] !== 0;
  }
  convertToPdfa(level = PdfaLevel.A2b) { check(f.edConvertPdfa(this._ptr, level)); return this; }

  // stamping (issue #45 P1). Coordinates are in the page VISIBLE space (origin
  // lower-left, y up); content lands where viewed regardless of /Rotate.
  // Returns false if the page index does not exist.
  fillRect(pageIndex, x, y, width, height, color = [1, 1, 1], opacity = 1.0) {
    const [r, g, b] = color;
    const found = [0];
    check(f.edFillRect(this._ptr, pageIndex, x, y, width, height, r, g, b, opacity, found));
    return found[0] !== 0;
  }
  // `rotationDeg` rotates the text counter-clockwise about its anchor (x, y).
  placeText(pageIndex, x, y, text, size = 12.0, color = [0, 0, 0], rotationDeg = 0.0) {
    const [r, g, b] = color;
    const found = [0];
    check(f.edPlaceText(this._ptr, pageIndex, x, y, text, size, r, g, b, rotationDeg, found));
    return found[0] !== 0;
  }

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
  FacturxProfile,
  PdfVersion,
  Certify,
  Bookmark,
  SigningSession,
  Document,
  EditableDoc,
  version,
  activateLicense,
  extractText,
  extractImagesToDir,
  renderPageToPng,
  pageCount,
  verifySignatures,
  findText,
  measurePages,
  measurePage,
  inspect,
  sign,
  timestamp,
  addDss,
  signWith,
  beginSigning,
  completeSignature,
  listSignatures,
  beginTimestamp,
  timestampRequest,
  timestampTokenFromResponse,
};
