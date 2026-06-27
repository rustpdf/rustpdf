# rustpdf (Node.js binding)

Idiomatic Node.js/TypeScript binding for the `rust-pdf` core over its C ABI
(`libpdf_ffi`), using **[Koffi](https://koffi.dev)** (pure FFI — no native
compilation, no node-gyp). It covers the whole product surface: vector graphics,
embedded/subsetted fonts and text, wrapping paragraphs, images, **PDF/A**
(levels 1b–3a), **tagged/accessible** output, embedded-file attachments,
**AcroForm** fields, manipulation (merge/split/rotate/optimize/incremental
update), **text extraction**, **encryption** (RC4 / AES-128 / AES-256) and
**digital signatures** (PKCS#7 / PAdES) — plus **feature licensing**. Ships with
TypeScript types (`lib/index.d.ts`).

## Install / load the native library

```sh
npm install            # installs koffi
cargo build -p pdf-ffi # builds libpdf_ffi (from the repo root)
```

The library is found via `RUSTPDF_LIB`, then by walking up from `lib/` to
`target/{debug,release}`. For a published package, ship the platform `libpdf_ffi.*`
inside the package (or point `RUSTPDF_LIB` at it).

## Quick start

```js
const rp = require('rustpdf');           // or: import * as rp from 'rustpdf'

rp.activateLicense(token);               // or set RUSTPDF_LICENSE (auto-activated)

const doc = new rp.Document();
doc.pdfa(rp.PdfaLevel.A2a).setInfo({ title: 'Report' });
const f = doc.addFontFile('assets/fonts/Roboto-Regular.ttf');
doc.addPage()
   .showText(f, 20, 72, 760, 'Title', 1)          // heading level 1 = H1
   .paragraph(f, 12, 72, 720, 450, 'A wrapping body…', rp.Align.Justify);
const data = doc.toBytes();                        // Buffer
doc.close();

console.log(rp.extractText(data));

const ed = rp.EditableDoc.load(data);
ed.encrypt({ method: rp.Encryption.Aes256, owner: 'owner' }).save('secured.pdf');
ed.close();

const signed = rp.sign(data, keyDer, certDer, { pades: true });
```

Corporate features (PDF/A, signing, encryption, accessibility) require a license;
without one they throw `PdfError`. See [`docs/LICENSING.md`](../../docs/LICENSING.md).

## Test

```sh
cargo build -p pdf-ffi
node bindings/node/test/run.js     # or: make node-test
```
