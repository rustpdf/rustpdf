# rustpdf — Java binding

Idiomatic Java wrapper over the rust-pdf C ABI (`libpdf_ffi`) using
[JNA](https://github.com/java-native-access/jna) — pure Java, **no native build
step**. Covers the whole product surface: vector graphics, embedded/subset fonts
and Unicode text, paragraphs, images, PDF/A (1b–3a), tagged/accessible output,
attachments, AcroForm fields, manipulation (merge/split/rotate/optimize/
incremental update), text extraction, page rendering (page to PNG image),
encryption and digital signatures.

Requires Java 17+. The published artifact is a **fat JAR** — it bundles the
native library for every supported platform, so there is nothing else to install.

## Install (Maven Central)

```xml
<dependency>
  <groupId>dev.rustpdf</groupId>
  <artifactId>rustpdf</artifactId>
  <version>0.4.0</version>
</dependency>
```

```gradle
implementation("dev.rustpdf:rustpdf:0.4.0")
```

A single artifact runs on every supported platform: each prebuilt
`libpdf_ffi` is bundled as a JNA classpath resource under its
`Platform.RESOURCE_PREFIX` directory (`darwin-aarch64/`, `linux-x86-64/`,
`linux-aarch64/`, `win32-x86-64/`), and JNA extracts the matching one at
runtime. No per-OS classifier, no native build step on the consumer side.

## Layout

| File | Role |
|------|------|
| `FFI.java` | Raw JNA mapping of all 78 C exports + library locator |
| `Pdf.java` | `version` / `activateLicense` / `extractText` / `sign` / `timestamp` / `addDss` + helpers |
| `Document.java` | Authoring (graphics, fonts, text, images, PDF/A, tagging, forms) |
| `EditableDoc.java` | Manipulation (merge, split, encrypt, incremental update) |
| `PdfaLevel` / `Align` / `AFRelationship` / `Encryption` | Enums |
| `PdfException` | Thrown on a non-zero `PdfStatus` |

## Locating the native library

The loader searches, in order (`FFI.java`):

1. `RUSTPDF_LIB` (an absolute path to the shared library) — explicit override;
2. `target/debug/` then `target/release/` walking up from the working directory —
   the dev tree (run `cargo build -p pdf-ffi` from the repo root first);
3. the bundled JNA classpath resource — how the published fat JAR loads, with no
   build step.

## Distribution

Published to Maven Central via the Sonatype **Central Portal**. The release is
automated in `.github/workflows/release-java.yml`: push a tag `java-v<version>`
(matching the `<version>` in `pom.xml`). The workflow builds `libpdf_ffi` for
each target (Linux inside `manylinux_2_28` for old-glibc compat, mac/win
natively, all with the **production** license pubkey), lays them out under
`src/main/resources/<prefix>/`, runs a free-surface smoke against the bundled
native, then `mvn -Prelease deploy` packages the fat JAR + `-sources` + `-javadoc`,
GPG-signs everything, and uploads.

Required repository secrets: `RUSTPDF_LICENSE_PUBKEY` (prod Ed25519 pubkey),
`MAVEN_CENTRAL_USERNAME` / `MAVEN_CENTRAL_PASSWORD` (Central Portal token),
`MAVEN_GPG_PRIVATE_KEY` / `MAVEN_GPG_PASSPHRASE` (signing key). The `release`
Maven profile is off by default, so local `make java-test` needs no GPG key.

## Usage

```java
import dev.rustpdf.*;

// Authoring — try-with-resources frees the native handle.
try (Document doc = new Document()) {
    int font = doc.addFontFile("assets/fonts/Roboto-Regular.ttf");
    doc.addPage()
       .setFillRgb(0.86, 0.20, 0.18)
       .rect(72, 640, 200, 120).fill()
       .showText(font, 24, 72, 760, "Olá, açúcar — café");
    byte[] bytes = doc.toBytes();
    System.out.println(Pdf.extractText(bytes));
}

// Corporate features need a license (env var RUSTPDF_LICENSE auto-activates,
// or call activateLicense once):
Pdf.activateLicense(token);
try (Document doc = new Document()) {
    doc.pdfa(PdfaLevel.A2A).tagged().setTitle("Report");
    int f = doc.addFontFile("Roboto-Regular.ttf");
    doc.addPage().showText(f, 20, 72, 760, "Title", 1);
    doc.save("report.pdf");
}

// Manipulation + encryption.
try (EditableDoc ed = EditableDoc.load(bytes)) {
    ed.setInfo("Subject", "via FFI");
    ed.encrypt("", "owner", Encryption.AES256, false);
    ed.save("secured.pdf");
}

// Digital signature (PKCS#7 detached / PAdES-B-B).
byte[] signed = Pdf.sign(bytes, keyDer, certDer, "Approved", null, null, true);
```

## Test

```sh
make java-test          # from the repo root (builds the cdylib + runs the smoke test)
# or directly:
cd bindings/java && mvn -q test-compile exec:java
```

The smoke test (`src/test/java/dev/rustpdf/SmokeTest.java`) exercises the whole
surface, including the license gate, and exits non-zero on any failure.

## Notes

- String marshalling is UTF-8 (set via JNA's `OPTION_STRING_ENCODING`), matching
  the core's UTF-8 expectation.
- Pointer-sized integers (`uintptr_t`) map to Java `long`; the binding targets
  64-bit platforms, like the other language bindings.
- Licensing, gating and behavior are identical across every binding because the
  checks live in the Rust core.
