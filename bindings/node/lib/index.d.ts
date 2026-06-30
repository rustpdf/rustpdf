// Type declarations for the rustpdf Node binding.

export class PdfError extends Error {
  status: number;
}

export const PdfaLevel: {
  readonly A1b: 0; readonly A2b: 1; readonly A2a: 2; readonly A3b: 3; readonly A3a: 4;
  // PDF/A-4 (ISO 19005-4), based on PDF 2.0.
  readonly A4: 5; readonly A4e: 6; readonly A4f: 7;
};
export const Align: {
  readonly Left: 0; readonly Right: 1; readonly Center: 2; readonly Justify: 3;
};
export const AFRelationship: {
  readonly Source: 0; readonly Data: 1; readonly Alternative: 2; readonly Supplement: 3; readonly Unspecified: 4;
};
export const Encryption: { readonly Rc4: 0; readonly Aes128: 1; readonly Aes256: 2 };
export const FacturxProfile: {
  readonly Minimum: 0; readonly BasicWL: 1; readonly Basic: 2; readonly EN16931: 3; readonly Extended: 4;
};
/** PDF version codes (setVersion / normalize): 0=1.4, 1=1.5, 2=1.7, 3=2.0. */
export const PdfVersion: { readonly V1_4: 0; readonly V1_5: 1; readonly V1_7: 2; readonly V2_0: 3 };
/** DocMDP certification level applied by the first (certifying) signature. */
export const Certify: {
  readonly None: 0; readonly Locked: 1; readonly Forms: 2; readonly FormsAndAnnotations: 3;
};

export type Rect = [number, number, number, number];
export type Bytes = Buffer | Uint8Array;

export class Bookmark {
  title: string;
  page: number;
  top: number | null;
  children: Bookmark[];
  constructor(title: string, page: number, top?: number | null, children?: Bookmark[]);
  child(bookmark: Bookmark): Bookmark;
}

export interface WatermarkTextOptions {
  size?: number;
  color?: [number, number, number];
  opacity?: number;
  rotationDeg?: number;
  /** Draw an opaque (solid) background behind the watermark text. */
  opaqueBackground?: boolean;
}

/** A positional text-search hit (coords in PDF points, origin lower-left). */
export interface TextHit {
  page: number;
  text: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

/** A rectangle in PDF user space (points, origin lower-left). */
export interface PdfRect {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  /** Absolute width (|x1 - x0|). */
  width: number;
  /** Absolute height (|y1 - y0|). */
  height: number;
}

/** Read-only geometry of one page (from {@link measurePage}/{@link measurePages}).
 * Sizes are in PDF points; `width`/`height` ignore rotation while
 * `rotatedWidth`/`rotatedHeight` account for `/Rotate` (swapped for 90/270). */
export interface PageGeometry {
  page: number;
  width: number;
  height: number;
  rotation: number;
  rotatedWidth: number;
  rotatedHeight: number;
  mediaBox: PdfRect;
  cropBox: PdfRect;
}

/** A non-mutating summary of a PDF (from {@link inspect}). */
export interface PdfOverview {
  version: string;
  pdfaLevel: string | null;
  encrypted: boolean;
  encryption: string;
  requiresPassword: boolean;
  pageCount: number;
}

export interface SignatureInfo {
  field_name: string | null;
  sub_filter: string;
  signer: string | null;
  covers_whole_document: boolean;
  digest_valid: boolean;
  signature_valid: boolean;
  is_valid: boolean;
  byte_range: [number, number, number, number];
  // Rich certificate / signature metadata (issue #41 P1; any may be null).
  issuer: string | null;
  serial_number: string | null;
  valid_from: string | null;
  valid_to: string | null;
  algorithm: string | null;
  signing_time: string | null;
  cert_count: number;
  has_timestamp: boolean;
}

export interface Info {
  title?: string | null;
  author?: string | null;
  subject?: string | null;
  keywords?: string | null;
  creator?: string | null;
}

export interface RadioButton {
  rect: Rect;
  export: string;
}

export interface SignOptions {
  reason?: string | null;
  location?: string | null;
  name?: string | null;
  pades?: boolean;
}

export interface EncryptOptions {
  method?: number;
  user?: string;
  owner?: string;
  readOnly?: boolean;
}

export class Document {
  constructor();
  close(): void;
  pdfa(level?: number): this;
  tagged(): this;
  setVersion(v: number): this;
  setDefaultSize(w: number, h: number): this;
  setInfo(info?: Info): this;
  addPage(size?: { width: number; height: number }): this;
  setFillRgb(r: number, g: number, b: number): this;
  setStrokeRgb(r: number, g: number, b: number): this;
  setLineWidth(w: number): this;
  rect(x: number, y: number, w: number, h: number): this;
  fill(): this;
  stroke(): this;
  addFontFile(path: string): number;
  addFont(data: Bytes): number;
  showText(font: number, size: number, x: number, y: number, text: string, headingLevel?: number): this;
  paragraph(font: number, size: number, x: number, y: number, width: number, text: string, align?: number): this;
  addImageFile(path: string): number;
  addImagePng(data: Bytes): number;
  addImageJpeg(data: Bytes): number;
  drawImage(image: number, x: number, y: number, w: number, h: number): this;
  figure(image: number, x: number, y: number, w: number, h: number, alt: string): this;
  attachFile(name: string, mime: string, data: Bytes, relationship?: number, description?: string): this;
  textField(name: string, page: number, rect: Rect, value?: string, size?: number): this;
  checkbox(name: string, page: number, rect: Rect, checked: boolean): this;
  dropdown(name: string, page: number, rect: Rect, options: string[], selected?: number | null, size?: number): this;
  radioGroup(name: string, page: number, buttons: RadioButton[], selected?: number | null): this;
  linkUri(rect: Rect, uri: string): this;
  linkToPage(rect: Rect, pageIndex: number, top?: number | null): this;
  addBookmark(bookmark: Bookmark): this;
  facturx(xml: Bytes, profile?: number): this;
  readonly pageCount: number;
  toBytes(): Buffer;
  save(path: string): void;
}

export class EditableDoc {
  static load(data: Bytes, password?: string): EditableDoc;
  static loadFile(path: string, password?: string): EditableDoc;
  close(): void;
  readonly pageCount: number;
  merge(other: EditableDoc): this;
  rotatePage(index: number, degrees: number): this;
  deletePage(index: number): this;
  reorderPages(order: number[]): this;
  extractPages(indices: number[]): EditableDoc;
  setInfo(key: string, value: string): this;
  getInfo(key: string): string;
  setXmp(xml: Bytes): this;
  overlayPage(index: number, content: Bytes): this;
  fillTextField(name: string, value: string): boolean;
  setCheckbox(name: string, checked?: boolean): boolean;
  setRadio(name: string, exportValue: string): boolean;
  setChoice(name: string, value: string): boolean;
  flattenForms(): this;
  fieldNames(): string[];
  watermarkText(text: string, opts?: WatermarkTextOptions): this;
  watermarkImageFile(path: string, width: number, height: number, opacity?: number, rotationDeg?: number): this;
  /** Set the output PDF version: 0=1.4, 1=1.5, 2=1.7, 3=2.0. */
  setVersion(version: number): this;
  /** Strip PDF/A conformance (OutputIntents, XMP pdfaid, /Version). */
  stripPdfa(): this;
  /** Normalize to a plain PDF at `version` (strip PDF/A + set version). */
  normalize(version?: number): this;
  redact(pageIndex: number, rects: Rect[]): boolean;
  convertToPdfa(level?: number): this;
  /**
   * Fill a rectangle on page `pageIndex` (0-based) with `color` (RGB, each 0..1)
   * at `opacity`. Coordinates are in the page VISIBLE space (origin lower-left,
   * y up); content lands where viewed regardless of `/Rotate`. Returns `false`
   * if the page does not exist.
   */
  fillRect(pageIndex: number, x: number, y: number, width: number, height: number, color?: [number, number, number], opacity?: number): boolean;
  /**
   * Draw a line of Helvetica `text` with baseline at `(x, y)` on page
   * `pageIndex` (0-based), `size` points, `color` (RGB, each 0..1). `rotationDeg`
   * rotates the text counter-clockwise about its anchor `(x, y)`. Coordinates are
   * in the page VISIBLE space (origin lower-left, y up); content lands where
   * viewed regardless of `/Rotate`. `align` shifts the start point along the
   * baseline so the text is left/right/center aligned about `(x, y)` (Justify
   * behaves like Left). Returns `false` if the page does not exist.
   */
  placeText(pageIndex: number, x: number, y: number, text: string, size?: number, color?: [number, number, number], rotationDeg?: number, align?: number): boolean;
  /**
   * Draw `text` over an opaque background box `[x, y, x+width, y+height]`: fills
   * the box in `bgColor` (default white), then writes the text (standard
   * Helvetica, `size` points, `textColor`, default black) horizontally aligned
   * per `align` and vertically centered within the box — the "mask a placeholder
   * and stamp the real value over it" convenience. Coordinates are in the page
   * VISIBLE space (origin lower-left, y up). Returns `false` if the page does
   * not exist.
   */
  maskedText(pageIndex: number, x: number, y: number, width: number, height: number, text: string, size?: number, textColor?: [number, number, number], bgColor?: [number, number, number], align?: number): boolean;
  /**
   * Stamp an image (`image` is PNG or JPEG bytes — the core dispatches on the
   * signature) onto page `pageIndex` (0-based). The image's lower-left corner
   * lands at `(x, y)` and is scaled to `width` x `height` points; `rotationDeg`
   * rotates it counter-clockwise about that corner. Coordinates are in the page
   * VISIBLE space (origin lower-left, y up); content lands where viewed
   * regardless of `/Rotate`. Returns `false` if the page does not exist.
   */
  drawImage(pageIndex: number, image: Bytes, x: number, y: number, width: number, height: number, rotationDeg?: number): boolean;
  optimize(): this;
  compact(on?: boolean): this;
  encrypt(opts?: EncryptOptions): this;
  toBytes(): Buffer;
  toBytesIncremental(original: Bytes): Buffer;
  save(path: string): void;
}

/** A signature-policy identifier (PAdES-EPES / ICP-Brasil AD-RB). */
export interface SignaturePolicy {
  /** The policy OID (dotted-decimal), e.g. the ICP-Brasil AD-RB OID. */
  oid: string;
  /** The policy document hash (under `hashAlgorithmOid`). */
  hash: Bytes;
  /** Hash algorithm OID; omit for SHA-256. */
  hashAlgorithmOid?: string | null;
  /** Optional SPURI qualifier — where the policy can be retrieved. */
  uri?: string | null;
}

/** Options for deferred / external signing (issue #41 P0). */
export interface SigningOptions {
  reason?: string | null;
  location?: string | null;
  name?: string | null;
  /** Produce a PAdES-B-B signature (`ETSI.CAdES.detached`). */
  pades?: boolean;
  /** Certify the document (DocMDP) — use only on the first signature. */
  certify?: number;
  /** Reserved `/Contents` bytes; 0/omitted = default (8192). */
  containerSize?: number;
  /** Signature-policy identifier (PAdES-EPES); omit for none. */
  policy?: SignaturePolicy | null;
  /** Draw a visible signature using the fields below (issue #41 P1). */
  visible?: boolean;
  /** 0-based page index for the visible appearance. */
  visiblePage?: number;
  /** Appearance rectangle [x0, y0, x1, y1] in page points. */
  visibleRect?: Rect;
  /** Appearance text lines, separated by '\n'; omit for none. */
  visibleText?: string | null;
  /** PNG/JPEG bytes of a handwritten-signature image; omit for none. */
  visibleImage?: Bytes | null;
}

/** A signature field discovered in a PDF (pre-signing inventory). */
export interface SignatureField {
  name: string;
  signed: boolean;
}

/**
 * The "bring your own signer" callback: returns the raw RSA PKCS#1 v1.5
 * signature over SHA-256 of `data`, typically by calling a remote HSM. The
 * private key never reaches this library.
 */
export type SignHash = (data: Buffer) => Bytes;

/**
 * An in-progress two-phase signature (Model B): `document` holds the prepared
 * PDF and `bytes` the exact bytes the signature covers. Hand `hash` to a remote
 * signer, build the CMS container, then call `complete`.
 */
export class SigningSession {
  /** The prepared PDF (with a zero-filled `/Contents` placeholder). */
  document: Uint8Array;
  /** The exact bytes covered by the signature (the two ByteRange segments). */
  bytes: Uint8Array;
  /** SHA-256 of `bytes` — the value an HSM signs. */
  readonly hash: Buffer;
  /** Phase 2: embed a finished DER CMS / PKCS#7 container, returning the PDF. */
  complete(container: Bytes): Buffer;
}

/**
 * Model A — remote signer. Sign `pdf` without handing this library a key: it
 * builds the CMS signed attributes and calls `signHash` for the raw RSA
 * signature, then assembles and embeds the CMS. `certDer` is the signer
 * certificate; `chain` are intermediates (DER), supplied independently of the key.
 */
export function signWith(
  pdf: Bytes, certDer: Bytes, signHash: SignHash, chain?: Bytes[], options?: SigningOptions,
): Buffer;
/** Model B — two-phase signing, phase 1. Prepare `pdf` for deferred signing. */
export function beginSigning(pdf: Bytes, options?: SigningOptions): SigningSession;
/** Model B — two-phase signing, phase 2. Embed a CMS container into a prepared PDF. */
export function completeSignature(document: Bytes, container: Bytes): Buffer;
/** List the signature fields in `pdf` (empty array means none). */
export function listSignatures(pdf: Bytes): SignatureField[];

export function version(): string;
export function activateLicense(token: string): void;
export function extractText(pdf: Bytes): string;
/** Extract the text of a single page (0-based) — the fast per-page path. */
export function extractPageText(pdf: Bytes, pageIndex: number): string;
export function extractImagesToDir(pdf: Bytes, dir: string): number;
/**
 * Render page `page` (0-based) of `pdf` to a PNG image at `dpi` dots-per-inch.
 * Page rendering is a licensed Pro feature: throws unless a license granting it
 * is active.
 */
export function renderPageToPng(pdf: Bytes, page?: number, dpi?: number): Buffer;
/** Number of pages in `pdf` (free — no license required). */
export function pageCount(pdf: Bytes): number;
export function verifySignatures(pdf: Bytes): SignatureInfo[];
/** Find every occurrence of `query` in `pdf` (case-insensitive by default). */
export function findText(pdf: Bytes, query: string, caseSensitive?: boolean): TextHit[];
/** Read the geometry (size, rotation, MediaBox, CropBox) of every page. */
export function measurePages(pdf: Bytes): PageGeometry[];
/** Geometry of a single page (0-based). Throws `RangeError` if out of range. */
export function measurePage(pdf: Bytes, index: number): PageGeometry;
/** Inspect a PDF without mutating it: version, PDF/A level, encryption, page count. */
export function inspect(pdf: Bytes): PdfOverview;
export function sign(pdf: Bytes, keyDer: Bytes, certDer: Bytes, opts?: SignOptions): Buffer;
export function timestamp(pdf: Bytes, tsaKeyDer: Bytes, tsaCertDer: Bytes, date?: string | null): Buffer;
export function addDss(pdf: Bytes, certs?: Bytes[], crls?: Bytes[]): Buffer;

/** A prepared network-timestamp job: SHA-256 `bytes`, fetch a token, then completeSignature(document, token). */
export interface TimestampSession {
  /** The prepared PDF (with a zero-filled /Contents placeholder). */
  document: Buffer;
  /** The exact bytes covered by the timestamp. */
  bytes: Buffer;
}
/** Network TSA (AD-RT) phase 1: prepare `pdf` for a /DocTimeStamp. */
export function beginTimestamp(pdf: Bytes): TimestampSession;
/** Build an RFC 3161 TimeStampReq (DER) for `imprint` (SHA-256 of the bytes to timestamp). */
export function timestampRequest(imprint: Bytes, nonce?: Bytes | null, certReq?: boolean): Buffer;
/** Extract the TimeStampToken (CMS) from a TSA's RFC 3161 TimeStampResp. */
export function timestampTokenFromResponse(response: Bytes): Buffer;
