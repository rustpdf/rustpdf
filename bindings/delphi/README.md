# RustPdf for Delphi / Free Pascal

Generate, edit, sign and process PDFs from Delphi / Free Pascal: vector graphics, embedded fonts and Unicode text, wrapping paragraphs, images, **PDF/A** (1b-4f), **tagged/accessible** output, attachments, **AcroForm** fields, page manipulation (merge/split/stamp), watermarks, true **redaction**, **AES-256** encryption, **digital signatures (PAdES)** with HSM/deferred signing, timestamps/LTV, text extraction and search, and page **rendering to PNG**. One thin unit, nothing to compile; works with Delphi 10.x+ and FPC 3.2+.

## Documentation

- **Full API reference:** https://rustpdf.dev/docs/delphi
- **Interactive positioning guide** (coordinates, anchors, rotation): https://rustpdf.dev/positioning
- All product guides (PDF/A, signatures, encryption, redaction, rendering): https://rustpdf.dev/docs/

The unit dynamically loads the cdylib at run time, so there is no fixed link
name: it finds the library via the `RUSTPDF_LIB` environment variable or by
walking up to the workspace's `target/{debug,release}` directory.

## Layout

| File | Purpose |
|------|---------|
| `RustPdf.pas` | The binding: C declarations, the loader, and `TPdfDocument` / `TPdfEditable` / `Pdf`. |
| `test/run.dpr` | Full-surface smoke test (mirrors the other bindings). |

## Build the native library

```sh
cargo build -p pdf-ffi            # produces target/debug/libpdf_ffi.{dylib,so,dll}
```

## Use it

```pascal
program hello;
{$IFDEF FPC}{$MODE DELPHI}{$H+}{$CODEPAGE UTF8}{$ENDIF}
uses RustPdf;
var
  Doc: TPdfDocument;
  Font: Integer;
begin
  Doc := TPdfDocument.Create;
  try
    Font := Doc.AddFontFile('assets/fonts/Roboto-Regular.ttf');
    Doc.AddPage
       .ShowText(Font, 24, 72, 760, 'Hello from Delphi')
       .FillRgb(0.1, 0.4, 0.9).Rect(72, 700, 200, 40).Fill;
    Doc.SaveToFile('hello.pdf');
  finally
    Doc.Free;
  end;
end.
```

Compile (Free Pascal):

```sh
fpc -Mdelphi -Fubindings/delphi hello.dpr
```

Compile (Delphi): add `bindings/delphi` to the unit search path and build as a
console/VCL/FMX app.

## API shape

- **`TPdfDocument`** — author a new PDF. Fluent mutators return `Self`, so calls
  chain. Covers graphics, fonts/text, paragraphs, images, PDF/A (`Pdfa` /
  `Pdfa(palA2A)`), tagging (`Tagged`), attachments, and every AcroForm field
  type (`TextField` / `Checkbox` / `Dropdown` / `RadioGroup`). Free with `.Free`.
- **`TPdfEditable`** — load and manipulate an existing PDF
  (`TPdfEditable.Load(bytes)` / `LoadFromFile(path)`): merge, reorder, rotate,
  delete, extract pages, edit `/Info` and XMP, overlay content, fill form
  fields, optimize, compact, encrypt, and serialize (full or incremental).
- **`Pdf`** (record with static methods) — stateless entry points:
  `Pdf.Version`, `Pdf.ExtractText`, `Pdf.Sign`,
  `Pdf.Timestamp`, `Pdf.AddDss`.

Strings cross as UTF-8; byte payloads are `TBytes`; rectangles use `TPdfRect`
(`PdfRect(x0, y0, x1, y1)`). Any non-zero `PdfStatus` is raised as an
`ERustPdf` carrying the native thread-local error message and a `Status` code.

## Deferred / HSM signing

Sign **without** handing the library a private key. When the key lives in an HSM,
a cloud KMS, a smartcard or a PKI token (any PKI — eIDAS, AATL, a national CA),
you supply the raw RSA signature and the binding builds and embeds the
CMS / PKCS#7 container. The private key never enters the library.

`TPdfRemoteSign` is a method pointer (`of object`, so it can carry state — FPC
3.2 has no anonymous methods), so the callback is a method of one of your
classes:

```pascal
type
  TMySigner = class
    function SignHash(const DataToSign: TBytes): TBytes;   // call your HSM / KMS / token
  end;

var
  Signer: TMySigner;
  Opts: TSigningOptions;
  Signed: TBytes;
begin
  Signer := TMySigner.Create;
  try
    Opts := SigningOptions;        // zero-initialised record
    Opts.Reason := 'Approved';
    Opts.Pades  := True;           // PAdES-B-B
    // Model A: SignHash returns the raw RSA PKCS#1 v1.5 signature over SHA-256(DataToSign).
    Signed := Pdf.SignWith(PdfBytes, CertDer, Signer.SignHash, [IntermediateDer], Opts);
  finally
    Signer.Free;
  end;
end;
```

For an asynchronous or out-of-band signer, use the two-phase flow instead:
`Pdf.BeginSigning` returns a `TSigningSession` whose `Hash` you send to the
remote signer; build a DER CMS container, then `Session.Complete(Container)`
(or `Pdf.CompleteSignature`). `Pdf.ListSignatures` reports the signature fields
already present. `TSigningOptions` also carries `Location` / `Name`, DocMDP
certification (`TCertify`) and a signature policy (`TSignaturePolicy`).

## All features included

Every feature is free — PDF/A, tagging, encryption, signing/PAdES and page
rendering are all included.

## Distribution

Delphi has no central package registry (no PyPI/npm/NuGet equivalent), so the
binding ships as a **versioned archive** plus an optional Git/Boss channel.

### Build an archive

```sh
make delphi-dist            # → bindings/delphi/dist/rustpdf-delphi-<version>.zip
```

`scripts/package.sh` builds the native cdylib for **every Rust target currently
installed** (`rustup target add x86_64-pc-windows-msvc …` to add more) and lays
the archive out as:

```
rustpdf-delphi-<version>/
  RustPdf.pas                       # the binding (add to your unit search path)
  lib/windows-x64/pdf_ffi.dll          # one per platform you built
  lib/macos-universal/libpdf_ffi.dylib # official release ships a universal dylib
  lib/linux-x64/libpdf_ffi.so          # (a local single-arch build is macos-arm64 / macos-x64)
  examples/smoke_test.dpr
  boss.json  README.md  INSTALL.txt  LICENSES.md
```

A build/CI machine with the cross-compilation targets installed produces a
complete multi-platform archive; a dev box produces just its host library.

### Deploying the native library

The unit loads `libpdf_ffi` at run time. Ship the matching library **next to the
built executable** (the loader checks the exe's own directory first) — or set
`RUSTPDF_LIB` to its full path. Two things to get right:

- **Bitness must match the app.** A Delphi **Win32** target needs the 32-bit
  `pdf_ffi.dll` (`i686-pc-windows-msvc`); a **Win64** target needs the 64-bit one.
- **macOS** dylibs should be codesigned/notarized for distribution; **Linux**
  `.so` placed beside the binary resolves because the loader checks the exe dir.

### Git / Boss

For teams using [Boss](https://github.com/HashLoad/boss) (a community Delphi
dependency manager), the repo is installable directly — `boss.json` is included:

```sh
boss install github.com/<org>/rust-pdf
```

This pulls the source unit; the customer still deploys the native library (above).
A vendor-hosted private repo or tagged release is the usual delivery for a paid
product. Embarcadero's **GetIt** is also an option but requires partner approval.

### How the public download on the site stays current

**One command does the whole release** — see [`RELEASING.md`](RELEASING.md):

```sh
scripts/release-delphi.sh 0.3.0     # bump → tag → CI release → deploy → verify
```

The zip is a build artifact (never committed). The full chain it automates:

1. **Bump + tag.** Set the workspace `version` and push a tag `delphi-v<version>`.
2. **CI builds + releases.** `.github/workflows/release-delphi.yml` compiles the
   cdylib on native runners (Windows x64+x86, macOS universal, Linux x64+arm64), runs `package.sh` to assemble the
   archive, and attaches `rustpdf-delphi-<version>.zip` + `.sha256` to a GitHub
   Release. The Release is the immutable source of truth.
3. **Deploy pulls it in.** `site/scripts/deploy.sh` runs
   `sync-delphi-download.sh` first, which downloads that Release asset into
   `site/public/downloads/` (checksum-verified, `gh` for the private repo). The
   rsync + `docker build` (`COPY site/`) then bakes it into the site image, and
   k3s serves it at `https://rustpdf.dev/downloads/`.

So updating the live download = **tag a release, then deploy** (both done by
`scripts/release-delphi.sh`). The version is **not** hardcoded in the page: the
server injects it from the zip present in `public/downloads/`, so there is no
HTML to edit per release. Skip the fetch with
`SKIP_DOWNLOADS=1 ./site/scripts/deploy.sh` (e.g. before the first release exists).

## Run the smoke test

```sh
make delphi-test            # builds the cdylib, then compiles + runs test/run.dpr
```

It exercises the whole surface (graphics, PDF/A-2a, tagging, text extraction,
incremental update, merge/optimize, all form fields, AES-256 encryption,
PAdES signing, timestamp and DSS), and exits non-zero on
any failed assertion. The target skips cleanly when neither `fpc` nor `dcc64`
is on `PATH`.
