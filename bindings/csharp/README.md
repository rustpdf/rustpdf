# RustPdf for .NET

Generate, edit, sign and process PDFs from .NET: vector graphics, embedded fonts and Unicode text, wrapping paragraphs, images, **PDF/A** (1b-4f), **tagged/accessible** output, attachments, **AcroForm** fields, page manipulation (merge/split/stamp), watermarks, true **redaction**, **AES-256** encryption, **digital signatures (PAdES)** with HSM/deferred signing, timestamps/LTV, text extraction and search, and page **rendering to PNG**. Native libraries for macOS, Linux and Windows are bundled, so `dotnet add package RustPdf` is all it takes.

## Documentation

- **Full API reference:** https://rustpdf.dev/docs/csharp
- **Interactive positioning guide** (coordinates, anchors, rotation): https://rustpdf.dev/positioning
- All product guides (PDF/A, signatures, encryption, redaction, rendering): https://rustpdf.dev/docs/

The public API is three types: `Document` (create PDFs), `EditableDoc` (load and
edit existing PDFs) and the static `Pdf` class (extract, sign, render, verify),
all `IDisposable` with exception-based error handling.

## Install

```sh
dotnet add package RustPdf
```

The NuGet package bundles the native `libpdf_ffi` for every supported runtime
(`osx-arm64`, `linux-x64`, `linux-arm64`, `win-x64`) under
`runtimes/<rid>/native/`. .NET's runtime resolves the matching one
automatically — no native build, no extra setup.

## Loading the native library

For a published package the native lib is resolved from the package's
`runtimes/<rid>/native/` (the standard NuGet RID-asset convention). For local
development against the repo build tree, `Native.cs` falls back to resolving
`libpdf_ffi` in this order:

1. `RUSTPDF_LIB` (explicit path);
2. by walking up from the assembly directory looking for
   `target/{debug,release}/libpdf_ffi.*` (local build tree);
3. the platform default search path (e.g. a library shipped next to the app).

Build it from the repo root with `cargo build -p pdf-ffi`.

## Distribution

Published to NuGet.org as the single package **`RustPdf`** that carries all four
platforms' cdylibs. CI (`.github/workflows/release-csharp.yml`, trigger
`csharp-v*`) fans out one build job per RID (Linux in `manylinux_2_28`, macOS
arm64, Windows x64 — all with the production license pubkey), then a `pack` job
stages each lib into `runtimes/<rid>/native/`, runs `dotnet pack`, and pushes via
NuGet **Trusted Publishing** (OIDC — no long-lived API key). A free-surface
smoke (`bindings/csharp/Smoke`) verifies each platform's lib loads.

## Quick start

```csharp
using RustPdf;

// A token via the RUSTPDF_LICENSE env var is auto-activated; or:
Pdf.ActivateLicense(token);

using (var doc = new Document())
{
    doc.Pdfa(PdfaLevel.A2a).SetInfo(title: "Report", author: "me");
    int f = doc.AddFontFile("assets/fonts/Roboto-Regular.ttf");
    doc.AddPage();
    doc.ShowText(f, 20, 72, 760, "Title", headingLevel: 1);
    doc.Paragraph(f, 12, 72, 720, 450, "A wrapping, justified body…", Align.Justify);
    byte[] data = doc.ToBytes();

    Console.WriteLine(Pdf.ExtractText(data));

    using var ed = EditableDoc.Load(data);     // manipulate + encrypt
    ed.SetInfo("Subject", "Edited");
    ed.Encrypt(owner: "owner", method: Encryption.Aes256);
    ed.Save("secured.pdf");

    byte[] signed = Pdf.Sign(data, keyDer, certDer, reason: "Approved", pades: true);
}
```

Corporate features (PDF/A, signing, encryption, accessibility, page rendering)
require a license; without one they throw `PdfException`. Page rendering is a
**Pro** feature. See [`docs/LICENSING.md`](../../docs/LICENSING.md).

## Deferred / HSM signing (key never enters the library)

When the private key lives in an HSM, a cloud KMS, a smartcard or a PKI token
(any PKI — eIDAS, AATL, a national CA), the library never sees it: you provide
the signature, the library builds the CMS/PKCS#7 container and embeds it. Use
this for hardware-backed or remote signing of any kind.

**Model A — remote signer callback.** The library prepares the signed
attributes and calls you back for the raw RSA PKCS#1 v1.5 signature over their
SHA-256 digest:

```csharp
using RustPdf;

byte[] pdf     = File.ReadAllBytes("contract.pdf");
byte[] certDer = File.ReadAllBytes("signer-cert.der");   // X.509 (DER), key stays remote

byte[] signed = Pdf.SignWith(pdf, certDer,
    signHash: dataToSign => hsm.SignRsaPkcs1Sha256(dataToSign),   // call your HSM / KMS / token
    chain: new[] { intermediateDer },
    options: new SigningOptions { Reason = "Approved", Pades = true });

File.WriteAllBytes("contract.signed.pdf", signed);
```

Prefer an interface? Implement `IRemoteSigner.SignHash` and pass the instance to
the same `SignWith` overload.

**Model B — two-phase (async / detached) signing.** Phase 1 prepares the PDF
and hands you the bytes to sign; do the remote signing out of band; phase 2
embeds the finished container:

```csharp
SigningSession session = Pdf.BeginSigning(pdf,
    new SigningOptions { Name = "Jane Doe", ContainerSize = 16384 });

byte[] cmsDer = await BuildCmsWithRemoteSignatureAsync(session.Hash);  // SHA-256 of the covered bytes

byte[] signed = session.Complete(cmsDer);            // or Pdf.CompleteSignature(session.Document, cmsDer)
```

Inspect existing signature fields before signing with
`Pdf.ListSignatures(pdf)` (an empty list means the document is unsigned).
`SigningOptions` also carries `Certify` (DocMDP certification) and `Policy`
(`SignaturePolicy` for PAdES-EPES). Raise `ContainerSize` when a cloud-HSM CMS
container is larger than the default 8 KB reservation.

## Build & run the sample

```sh
cargo build -p pdf-ffi
dotnet run --project bindings/csharp/Sample          # exercises the whole surface
```

Targets `net8.0` (works on .NET 8+); the source-generated P/Invoke needs C# 11+.
