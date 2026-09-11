# RustPdf for Node.js

Generate, edit, sign and process PDFs from Node.js: vector graphics, embedded fonts and Unicode text, wrapping paragraphs, images, **PDF/A** (1b-4f), **tagged/accessible** output, attachments, **AcroForm** fields, page manipulation (merge/split/stamp), watermarks, true **redaction**, **AES-256** encryption, **digital signatures (PAdES)** with HSM/deferred signing, timestamps/LTV, text extraction and search, and page **rendering to PNG**. No native compilation, no node-gyp.

## Documentation

- **Full API reference:** https://rustpdf.dev/docs/node
- **Interactive positioning guide** (coordinates, anchors, rotation): https://rustpdf.dev/positioning
- All product guides (PDF/A, signatures, encryption, redaction, rendering): https://rustpdf.dev/docs/

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

Every feature is free — PDF/A, digital signatures/PAdES, encryption,
accessibility/tagging, redaction and page rendering are all included.

## Deferred / HSM signing

Sign without ever handing this library a private key — the key stays in an HSM,
cloud KMS, smartcard or PKI token. It works with any PKI (eIDAS, AATL and other
trust lists): the binding only builds and embeds the CMS. Two models.

**Model A — bring your own signer.** `signWith` builds the CMS and calls your
callback for the raw RSA signature over the document hash:

```js
const { signWith } = require('rustpdf');

// signHash receives the bytes to sign and returns the raw RSA signature from
// your HSM / KMS / smartcard. The private key never enters the library.
const signed = signWith(pdf, certDer, (data) => hsm.signRsaSha256(data), [], {
  reason: 'Approved', name: 'Jane Doe', pades: true,
});
```

**Model B — two-phase.** `beginSigning` prepares the PDF and exposes the bytes
(and their `hash`) to sign; send the hash to a remote/asynchronous signer, wrap
the result in a DER CMS / PKCS#7 container, then `complete`:

```js
const { beginSigning, listSignatures } = require('rustpdf');

listSignatures(pdf);                       // inventory existing fields ([] = none)

const session = beginSigning(pdf, { pades: true });
const container = await remoteSigner.buildCms(session.hash);   // DER CMS / PKCS#7
const signed = session.complete(container);
```

`SigningOptions` also carries `certify` (DocMDP), `containerSize` (reserved
`/Contents` bytes) and a `policy` (PAdES-EPES) identifier; see the TypeScript
types in [`lib/index.d.ts`](lib/index.d.ts).

## Test

```sh
cargo build -p pdf-ffi
node bindings/node/test/run.js     # or: make node-test
```
