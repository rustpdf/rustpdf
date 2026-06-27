# RustPdf (.NET binding)

Idiomatic C#/.NET binding for the `rust-pdf` core over its C ABI
(`libpdf_ffi`), using source-generated P/Invoke (`LibraryImport`). It mirrors the
full product surface: vector graphics, embedded/subsetted fonts and text,
wrapping paragraphs, images, **PDF/A** (levels 1b–3a), **tagged/accessible**
output, embedded-file attachments, **AcroForm** fields, manipulation
(merge/split/rotate/optimize/incremental update), **text extraction**,
**encryption** (RC4 / AES-128 / AES-256) and **digital signatures** (PKCS#7 /
PAdES) — plus **feature licensing**.

Layout:

* `RustPdf/Native.cs` — raw P/Invoke (1:1 with `include/pdf.h`) + a native-library
  resolver;
* `RustPdf/Document.cs`, `EditableDoc.cs` — `IDisposable` wrappers with fluent,
  exception-based APIs;
* `RustPdf/Pdf.cs` — static helpers (`Version`, `ActivateLicense`, `ExtractText`,
  `Sign`, `Timestamp`, `AddDss`) and the `PdfaLevel` / `Align` / `AFRelationship`
  / `Encryption` enums.

## Loading the native library

`Native.cs` resolves `libpdf_ffi` in this order:

1. `RUSTPDF_LIB` (explicit path);
2. by walking up from the assembly directory looking for
   `target/{debug,release}/libpdf_ffi.*` (local build tree);
3. the platform default search path (e.g. a library shipped next to the app).

Build it from the repo root with `cargo build -p pdf-ffi`.

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

Corporate features (PDF/A, signing, encryption, accessibility) require a license;
without one they throw `PdfException`. See [`docs/LICENSING.md`](../../docs/LICENSING.md).

## Build & run the sample

```sh
cargo build -p pdf-ffi
dotnet run --project bindings/csharp/Sample          # exercises the whole surface
```

Targets `net8.0` (works on .NET 8+); the source-generated P/Invoke needs C# 11+.
