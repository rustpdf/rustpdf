# rustpdf (Node.js binding)

Idiomatic Node.js/TypeScript binding for the `rust-pdf` core over its C ABI
(`libpdf_ffi`), using **[Koffi](https://koffi.dev)** (pure FFI — no native
compilation, no node-gyp). It covers the whole product surface: vector graphics,
embedded/subsetted fonts and text, wrapping paragraphs, images, **PDF/A**
(levels 1b–3a), **tagged/accessible** output, embedded-file attachments,
**AcroForm** fields, manipulation (merge/split/rotate/optimize/incremental
update), **text extraction**, **page rendering** (page to PNG image),
**encryption** (RC4 / AES-128 / AES-256) and **digital signatures** (PKCS#7 /
PAdES) — plus **feature licensing**. Ships with TypeScript types
(`lib/index.d.ts`).

## Install

```sh
npm install rustpdf
```

The native library ships as **per-platform optional dependencies**
(`@rustpdf/darwin-arm64`, `@rustpdf/linux-x64-gnu`, `@rustpdf/linux-arm64-gnu`,
`@rustpdf/win32-x64-msvc`) — npm installs only the one matching your `os`/`cpu`,
so there's no native compilation and no node-gyp. This mirrors how the Python
binding ships one platform wheel per target.

The cdylib is located at load time in this order:

1. `RUSTPDF_LIB` (explicit path to a `libpdf_ffi.*`);
2. the matching `@rustpdf/<platform>` package (the normal install path);
3. the workspace `target/{debug,release}` (monorepo dev — see below).

### Developing in the monorepo

```sh
npm install            # installs koffi
cargo build -p pdf-ffi # builds libpdf_ffi (from the repo root)
```

No platform package is installed, so the loader falls back to `target/`.

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

Corporate features (PDF/A, signing, encryption, accessibility, page rendering — a **Pro** feature) require a license;
without one they throw `PdfError`. See [`docs/LICENSING.md`](../../docs/LICENSING.md).

## Test

```sh
cargo build -p pdf-ffi
node bindings/node/test/run.js     # or: make node-test
```
