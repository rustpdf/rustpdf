// Type declarations for the rustpdf Node binding.

export class PdfError extends Error {
  status: number;
}

export const PdfaLevel: {
  readonly A1b: 0; readonly A2b: 1; readonly A2a: 2; readonly A3b: 3; readonly A3a: 4;
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
  watermarkImageFile(path: string, width: number, height: number, opacity?: number): this;
  redact(pageIndex: number, rects: Rect[]): boolean;
  convertToPdfa(level?: number): this;
  optimize(): this;
  compact(on?: boolean): this;
  encrypt(opts?: EncryptOptions): this;
  toBytes(): Buffer;
  toBytesIncremental(original: Bytes): Buffer;
  save(path: string): void;
}

export function version(): string;
export function activateLicense(token: string): void;
export function extractText(pdf: Bytes): string;
export function extractImagesToDir(pdf: Bytes, dir: string): number;
export function verifySignatures(pdf: Bytes): SignatureInfo[];
export function sign(pdf: Bytes, keyDer: Bytes, certDer: Bytes, opts?: SignOptions): Buffer;
export function timestamp(pdf: Bytes, tsaKeyDer: Bytes, tsaCertDer: Bytes, date?: string | null): Buffer;
export function addDss(pdf: Bytes, certs?: Bytes[], crls?: Bytes[]): Buffer;
