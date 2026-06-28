#!/usr/bin/env python3
"""Generate task x language spoke landing pages (Content Strategy waves 1-3).

Reads the verbatim, real API code blocks out of the per-language docs
(docs/<lang>.html) so the snippets always match the shipped binding, then
renders one SEO spoke page per (task, language) cell into public/<task>/<lang>.html.

Re-runnable: regenerates every page and rewrites the sitemap spoke block.
No em-dashes in body copy (a repo hook forbids them); the script asserts this.
"""
import re, html, json, os, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))  # site/
PUB = os.path.join(ROOT, "public")
DOCS = os.path.join(PUB, "docs")
TODAY = "2026-06-28"

# ---------------------------------------------------------------- languages
LANGS = {
    "go":     dict(name="Go",     long="Go",                  hub="/golang", docs="/docs/go.html",     hl="go",         install="go get github.com/rustpdf/rustpdf-go", aliases=["golang"],
                   gap="Go's PDF story is fragmented: gofpdf is archived and unipdf is commercial, so production-grade output usually means a paid dependency or a brittle wrapper."),
    "php":    dict(name="PHP",    long="PHP",                 hub="/php",    docs="/docs/php.html",    hl="php",        install="composer require rust-pdf/rustpdf", aliases=[],
                   gap="PHP's classic libraries (TCPDF, FPDF) predate modern PDF and have no real PDF/A, no PAdES signatures and no AES-256, while the commercial alternatives are costly."),
    "ruby":   dict(name="Ruby",   long="Ruby",                hub="/ruby",   docs="/docs/ruby.html",   hl="ruby",       install="gem install rustpdf", aliases=[],
                   gap="Prawn generates beautiful PDFs but has no PDF/A, no digital signatures, no AES-256 and no PDF/UA, so the regulated features have simply been missing from Ruby."),
    "node":   dict(name="Node.js", long="Node.js and TypeScript", hub="/nodejs", docs="/docs/node.html", hl="javascript", install="npm install rustpdf", aliases=["nodejs"],
                   gap="Node typically reaches for heavy wrappers or headless Chrome for anything past basic output, which is slow, fragile and never archival-grade."),
    "python": dict(name="Python", long="Python",              hub="/python", docs="/docs/python.html", hl="python",     install="pip install rustpdf", aliases=[],
                   gap="ReportLab handles layout (its strongest parts are paid) while pikepdf and pypdf cover slices, so archival PDF/A and signatures stay a recurring pain in Python."),
    "csharp": dict(name="C#",     long="C# and .NET",         hub="/dotnet", docs="/docs/csharp.html", hl="csharp",     install="dotnet add package RustPdf", aliases=["dotnet"],
                   gap=".NET's mature options (iText, Aspose) are powerful but expensive and carry AGPL or per-server licensing, which is exactly what teams want to avoid."),
}

# docs section id -> task slug (the first code block of that section is harvested)
SECTION_FOR_TASK = {
    "generate-pdf": "quickstart",
    "pdf-a":        "pdfa",
    "pdf-forms":    "forms",
    "merge-pdf":    "ed-pages",
    "compress-pdf": "ed-optimize",
    "encrypt-pdf":  "ed-encrypt",
    "sign-pdf":     "sign",
    "extract-text": "extract",
}

GATED = {"go", "php", "ruby", "node", "python", "csharp"}
FREE = ["go", "php", "ruby", "node"]
ALL6 = ["go", "php", "ruby", "node", "python", "csharp"]

# ---------------------------------------------------------------- tasks
TASKS = {
    "sign-pdf": dict(
        eyebrow="Digital signatures",
        title="Digitally Sign a PDF in %L (PAdES)",
        h1='Digitally sign a <span class="grad">PDF</span> in %L',
        kw="sign a PDF in %l",
        hub_url="/pades", hub_name="PAdES signatures",
        gated=True,
        validators=["pdfsig", "openssl", "qpdf"],
        lede="Add a cryptographic PKCS#7 or PAdES signature to a PDF from %L. rust-pdf signs through a non-destructive incremental update, so the original bytes are preserved and the signature stays verifiable in Adobe Reader, pdfsig and any PAdES validator.",
        why_p="A real digital signature gives a document legal weight: it proves who signed it and that nothing changed afterwards. rust-pdf builds the detached CMS by hand to control the ByteRange, supports PAdES B-B, B-LT and B-LTA for long-term validation, and lets you supply your own key and certificate as DER.",
        bullets=[
            "PKCS#7 detached and PAdES B-B, with B-LT and B-LTA for long-term validation.",
            "Incremental update: the original file is preserved byte for byte, so earlier signatures stay valid.",
            "Bring your own key and X.509 certificate (PKCS#8 DER), or chain to a TSA for timestamps.",
        ],
        faq=[
            ("Is the signature legally valid?", "rust-pdf produces standards-compliant PKCS#7 and PAdES signatures. Legal validity depends on the certificate you sign with (for example an eIDAS qualified certificate or an ICP-Brasil certificate). The library handles the cryptography and the PDF structure correctly, which is what validators such as pdfsig and Adobe Reader check."),
            ("Does it support long-term validation (LTV)?", "Yes. After signing you can append a Document Security Store with certificates and CRLs (PAdES B-LT) and an RFC 3161 document timestamp (PAdES B-LTA), all offline. A trusted external TSA and live OCSP fetching are the only parts that need network infrastructure."),
            ("Do I need a license to sign in %L?", "Signing is a corporate feature, so it requires an active license token. Basic PDF generation in %L is free. The same offline Ed25519 token unlocks signing across every language."),
        ],
    ),
    "pdf-a": dict(
        eyebrow="Long-term archiving",
        title="Create PDF/A in %L (Archival PDF)",
        h1='Create <span class="grad">PDF/A</span> in %L',
        kw="create PDF/A in %l",
        hub_url="/pdf-a", hub_name="PDF/A",
        gated=True,
        validators=["veraPDF", "qpdf", "mutool"],
        lede="Generate archival-grade PDF/A from %L with one method call. rust-pdf embeds the sRGB ICC profile, adds the output intent, writes the XMP metadata and document ID, and enforces the rules, so the output validates under veraPDF, the reference validator.",
        why_p="PDF/A is the version of PDF built to last: every font and color profile is sealed inside the file so it renders identically decades from now. It is mandatory for e-invoicing, public-sector archiving, legal, healthcare and finance. rust-pdf produces and validates PDF/A-1b, 2b, 2a, 3b and 3a.",
        bullets=[
            "Levels A-1b, A-2b, A-2a, A-3b and A-3a, with the accessible a-levels building a full tagged structure tree.",
            "Fonts embedded and subset automatically, ICC profile and output intent added for you.",
            "XMP metadata kept in sync with the document info, validated by veraPDF.",
        ],
        faq=[
            ("Which PDF/A levels are supported in %L?", "rust-pdf creates PDF/A-1b, 2b, 2a, 3b and 3a. Use a basic b-level for visual fidelity, an a-level for an accessible tagged structure, or a 3-level when you need to embed source files such as an e-invoice XML."),
            ("How is conformance verified?", "Output is validated with veraPDF, the open-source reference validator for PDF/A, plus qpdf and mutool for structure. The claim is backed by validators, not adjectives."),
            ("Do I need a license to create PDF/A in %L?", "PDF/A is a corporate feature and requires an active license token. Basic generation in %L is free. One offline token unlocks PDF/A in every supported language."),
        ],
    ),
    "encrypt-pdf": dict(
        eyebrow="Encryption",
        title="Encrypt a PDF in %L (AES-256)",
        h1='Encrypt a <span class="grad">PDF</span> in %L',
        kw="encrypt a PDF in %l",
        hub_url="/encrypt-pdf", hub_name="Encrypt PDF",
        gated=True,
        validators=["qpdf", "mutool"],
        lede="Password-protect a PDF from %L with strong AES-256 encryption. rust-pdf applies standard-handler encryption at output, deriving keys and IVs from the operating system CSPRNG, and supports user and owner passwords plus permission flags.",
        why_p="Encryption keeps sensitive documents (statements, records, contracts) confidential and helps meet LGPD, GDPR and HIPAA obligations. rust-pdf implements AES-256 (V5/R6) directly, validated by qpdf for both user and owner passwords, and also supports AES-128 and legacy RC4.",
        bullets=[
            "AES-256 (V5/R6) with keys and IVs from the OS CSPRNG, plus AES-128 and RC4 for legacy needs.",
            "Separate user and owner passwords, with a read-only permission mode.",
            "Encrypt new documents or an existing PDF you load and re-save.",
        ],
        faq=[
            ("How strong is the encryption?", "rust-pdf uses AES-256 with the modern V5/R6 security handler, the strongest standard PDF encryption. Keys, salts and IVs come from the operating system CSPRNG, so every encrypted file is unique. qpdf validates the output for both user and owner passwords."),
            ("What is the difference between user and owner passwords?", "A user password is required to open the document. An owner password leaves the file openable but restricts actions such as printing or copying. You can set either or both, and enable a read-only permission mode."),
            ("Do I need a license to encrypt in %L?", "Encryption is a corporate feature and needs an active license token. Basic generation in %L is free. The same offline token enables encryption in every language."),
        ],
    ),
    "merge-pdf": dict(
        eyebrow="Manipulation",
        title="Merge PDF Files in %L",
        h1='Merge <span class="grad">PDF</span> files in %L',
        kw="merge PDFs in %l",
        hub_url="/merge-pdf", hub_name="Merge & split PDF",
        gated=False,
        validators=["qpdf", "mutool"],
        lede="Combine, split, reorder and rotate PDF pages from %L. rust-pdf parses each document into an editable model, renumbers and remaps every object correctly on merge, and rebuilds a clean page tree on output.",
        why_p="Merging and splitting are list operations on a flat page order: append another document, extract a subset, reverse, rotate or delete pages, then save. Because the parser handles classic and cross-reference streams, object streams and encrypted input, it works on real-world files, not just ones it wrote itself.",
        bullets=[
            "Merge whole documents, extract page subsets, reorder with a permutation, rotate or delete pages.",
            "Objects are renumbered and references deep-remapped, so merged files stay valid.",
            "Reads classic and xref streams, object streams and RC4 / AES encrypted input.",
        ],
        faq=[
            ("Can I merge more than two PDFs in %L?", "Yes. Load each document and call merge for each one; pages are appended in order. You can then reorder, rotate or extract any subset before saving."),
            ("Does merging keep the files valid?", "Yes. On merge every object from the other document is renumbered and its references are deep-remapped, and the real page tree is rebuilt on output, so the result passes qpdf and mutool."),
            ("Is merging free?", "Yes. Loading, merging, splitting, reordering and text extraction are all part of the free tier in %L. You only need a license for the corporate features such as PDF/A, signatures and encryption."),
        ],
    ),
    "extract-text": dict(
        eyebrow="Extraction",
        title="Extract Text From a PDF in %L",
        h1='Extract <span class="grad">text</span> from a PDF in %L',
        kw="extract text from a PDF in %l",
        hub_url="/extract-text", hub_name="Extract text",
        gated=False,
        validators=["pdftotext"],
        lede="Pull the text out of any PDF from %L. rust-pdf walks the content stream, maps shown glyph codes back to Unicode through each font's ToUnicode map, and infers spaces and line breaks, including two-byte Type0 fonts and CJK.",
        why_p="Reliable extraction is harder than it looks: codes in the stream are font-specific and must be mapped to Unicode, and spacing has to be inferred from positioning. rust-pdf does both, so the text you get back is the text a human reads, ready for search, indexing or data pipelines.",
        bullets=[
            "Maps glyph codes to Unicode via each font's ToUnicode map, including two-byte Type0 and CJK.",
            "Infers spaces from large negative adjustments and line breaks from text positioning.",
            "Pull raster images out too: JPEGs verbatim as .jpg, everything else as .png.",
        ],
        faq=[
            ("Does it extract Unicode and CJK text in %L?", "Yes. Each shown code is mapped back to Unicode through the font's ToUnicode CMap, including two-byte Type0 fonts, so Japanese, Greek, Arabic and other scripts come back correctly."),
            ("Can it extract scanned PDFs?", "No. Extraction reads the text layer of a PDF. A scanned document is an image with no text layer, which needs OCR first. For PDFs that contain real text, extraction is exact."),
            ("Is text extraction free?", "Yes. Text and image extraction are part of the free tier in %L. No license token is required."),
        ],
    ),
    "compress-pdf": dict(
        eyebrow="Optimization",
        title="Compress and Optimize a PDF in %L",
        h1='Compress a <span class="grad">PDF</span> in %L',
        kw="compress a PDF in %l",
        hub_url="/compress-pdf", hub_name="Compress PDF",
        gated=False,
        validators=["qpdf", "mutool"],
        lede="Shrink PDF file size from %L. rust-pdf drops unreferenced objects, Flate-compresses uncompressed streams, dedupes byte-identical objects, and can pack everything into object streams with a cross-reference stream.",
        why_p="Optimization is lossless structural cleanup: nothing in the rendered page changes, the file just carries less weight. Combine optimize (dedupe and compress) with compact (object and xref streams) for the smallest valid output, ideal before archiving, emailing or serving documents at scale.",
        bullets=[
            "Drop unreferenced objects, Flate-compress streams and dedupe identical objects.",
            "Pack objects into object streams and emit a cross-reference stream for the smallest size.",
            "Lossless: the rendered page is unchanged, the output still passes qpdf and mutool.",
        ],
        faq=[
            ("How much smaller will my PDF be in %L?", "It depends on the input. Files with uncompressed streams, duplicated objects or no object streams shrink the most. Optimization is lossless, so the savings come from structure, not from degrading content."),
            ("Does compression reduce quality?", "No. This is structural optimization, not image down-sampling. The rendered page is identical; only redundant or uncompressed structure is removed, so the output still validates."),
            ("Is optimization free?", "Yes. Optimize and compact are part of the free tier in %L. No license is needed."),
        ],
    ),
    "generate-pdf": dict(
        eyebrow="Authoring",
        title="Generate a PDF in %L",
        h1='Generate a <span class="grad">PDF</span> in %L',
        kw="generate a PDF in %l",
        hub_url="/docs/", hub_name="Documentation",
        gated=False,
        validators=["qpdf", "mutool"],
        lede="Create PDFs programmatically from %L: pages, vector graphics, embedded and subset fonts with full Unicode shaping, justified paragraphs, tables and images. rust-pdf gives %L a fast, memory-safe core with deterministic output.",
        why_p="Generation is the free foundation. Draw shapes, place shaped and kerned Unicode text, lay out wrapping paragraphs, embed JPEG and PNG images, and add links and bookmarks. Output is deterministic (the same input yields the same bytes), which makes it auditable and testable.",
        bullets=[
            "Vector graphics, embedded and subset fonts, HarfBuzz-quality Unicode shaping and kerning.",
            "Wrapping and justified paragraphs, tables, images (JPEG, PNG, alpha, 16-bit).",
            "Deterministic output: identical bytes for identical input, ideal for audits and tests.",
        ],
        faq=[
            ("Is generating PDFs free in %L?", "Yes. All basic generation (pages, graphics, fonts, text, paragraphs, images, links, bookmarks, watermarks) is free forever. You only license corporate features such as PDF/A, signatures and encryption when you ship them."),
            ("Does it support Unicode and custom fonts?", "Yes. Fonts are embedded and subset with HarfBuzz-quality shaping and kerning, using Type0 fonts with ToUnicode, so non-Latin scripts render and extract correctly."),
            ("Why a Rust core for %L?", "One memory-safe, high-performance Rust core is exposed to %L through a thin idiomatic binding. You get the same engine, the same features and deterministic output, without a heavy runtime."),
        ],
    ),
    "pdf-forms": dict(
        eyebrow="Forms",
        title="Create and Fill PDF Forms in %L",
        h1='Create and fill <span class="grad">PDF forms</span> in %L',
        kw="create PDF forms in %l",
        hub_url="/pdf-forms", hub_name="PDF forms",
        gated=False,
        validators=["qpdf", "mutool"],
        lede="Build interactive AcroForm fields from %L (text inputs, checkboxes, radio groups and dropdowns) with generated appearance streams, then fill and flatten them later. No NeedAppearances hack required.",
        why_p="rust-pdf generates a full AcroForm with appearance streams baked in, so fields render correctly everywhere without relying on the viewer. Names support dotted hierarchy for grouped fields, and an existing form can be filled programmatically and flattened into static content.",
        bullets=[
            "Text fields, checkboxes, radio groups and dropdowns with generated appearance streams.",
            "Hierarchical dotted field names for grouped data.",
            "Fill an existing form and flatten it into non-editable content.",
        ],
        faq=[
            ("Do the form fields render without NeedAppearances in %L?", "Yes. rust-pdf generates an appearance stream for every field, so checkboxes, text and choices display correctly in all viewers without the NeedAppearances workaround."),
            ("Can I fill an existing form in %L?", "Yes. Load the PDF, set text fields, checkboxes, radios and dropdowns by name, and optionally flatten the form so the values become permanent static content."),
            ("Is form authoring free?", "Yes. Creating, filling and flattening AcroForm fields is part of the free tier in %L."),
        ],
    ),
}

MATRIX = {t: (ALL6 if d["gated"] else FREE) for t, d in TASKS.items()}

# ---------------------------------------------------------------- code harvest
def harvest():
    code = {}  # code[task][lang]
    for lang, meta in LANGS.items():
        doc_file = os.path.join(DOCS, ("node.html" if lang == "node" else f"{lang}.html"))
        src = open(doc_file, encoding="utf-8").read()
        sections = {}
        for sec in re.finditer(r'<h2 id="([^"]+)">.*?</h2>(.*?)(?=<h2 id=|</main>)', src, re.S):
            sid = sec.group(1)
            blocks = re.findall(r'<pre[^>]*><code>(.*?)</code></pre>', sec.group(2), re.S)
            if blocks:
                sections[sid] = html.unescape(re.sub(r'<[^>]+>', '', blocks[0])).strip()
        for task, secid in SECTION_FOR_TASK.items():
            if lang in MATRIX[task]:
                if secid not in sections:
                    sys.exit(f"missing section {secid} in {doc_file} for {task}/{lang}")
                code.setdefault(task, {})[lang] = sections[secid]
    return code

# ---------------------------------------------------------------- template
def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

STYLE = """
  <style>
    /* ===== task x language spoke (scoped, reduced-motion safe) ===== */
    .sp-lede { max-width: 720px; }
    .sp-scene { position: relative; height: 210px; max-width: 340px; margin: 30px auto 0; }
    .sp-paper { position: absolute; left: 50%; top: 50%; transform: translate(-50%,-50%);
      width: 150px; height: 184px; background: var(--panel); border: 1px solid var(--border);
      border-radius: 12px; padding: 18px 16px; display: flex; flex-direction: column; gap: 9px;
      box-shadow: 0 30px 60px -30px rgba(249,115,22,.5); animation: spPulse 4.5s ease-in-out infinite; }
    .sp-paper .ln { height: 8px; border-radius: 4px; background: linear-gradient(90deg,#243049,#2c3a57); }
    .sp-paper .ln.s { width: 56%; }
    .sp-badge { position: absolute; top: -12px; left: -12px; background: var(--panel-2);
      border: 1px solid var(--border); color: #cbd5e1; font-size: .72rem; font-weight: 800;
      letter-spacing: .03em; padding: 5px 11px; border-radius: 999px; box-shadow: 0 8px 20px -10px rgba(0,0,0,.6); }
    .sp-seal { position: absolute; bottom: -11px; right: -11px; background: var(--brand); color: #1a0e02;
      font-size: .62rem; font-weight: 800; letter-spacing: .04em; padding: 5px 10px; border-radius: 999px; }
    @keyframes spPulse { 0%,100% { box-shadow: 0 30px 60px -30px rgba(249,115,22,.38); }
      50% { box-shadow: 0 30px 70px -28px rgba(249,115,22,.72); } }
    .sp-why { display: grid; grid-template-columns: 1.1fr 1fr; gap: 30px; align-items: start; margin-top: 22px; }
    .sp-install { font-family: "SF Mono", ui-monospace, Menlo, monospace; font-size: .9rem; }
    @media (max-width: 820px) { .sp-why { grid-template-columns: 1fr; } }
    @media (prefers-reduced-motion: reduce) { .sp-paper { animation: none !important; } }
  </style>
"""

PAGE = """<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>%%TITLE%% | rust-pdf</title>
  <meta name="description" content="%%DESC%%" />
  <link rel="canonical" href="https://rustpdf.dev%%PATH%%" />
  <link rel="icon" href="/favicon.svg" type="image/svg+xml" />
  <meta name="theme-color" content="#0a0e16" />
  <meta property="og:type" content="article" />
  <meta property="og:site_name" content="rust-pdf" />
  <meta property="og:url" content="https://rustpdf.dev%%PATH%%" />
  <meta property="og:title" content="%%TITLE%%" />
  <meta property="og:description" content="%%DESC%%" />
  <meta property="og:image" content="https://rustpdf.dev/og.png" />
  <meta property="og:image:width" content="1200" />
  <meta property="og:image:height" content="630" />
  <meta property="og:image:alt" content="%%TITLE%%" />
  <meta name="twitter:card" content="summary_large_image" />
  <meta name="twitter:title" content="%%TITLE%%" />
  <meta name="twitter:description" content="%%DESC%%" />
  <meta name="twitter:image" content="https://rustpdf.dev/og.png" />
  <meta name="twitter:image:alt" content="%%TITLE%%" />
  <script type="application/ld+json">
%%JSONLD%%
  </script>
  <link rel="stylesheet" href="/styles.css?v=3" />
%%STYLE%%
</head>
<body>
  <a class="skip-link" href="#main">Skip to content</a>
  <header class="nav">
    <div class="wrap nav-inner">
      <a class="brand" href="/">rust<span>&middot;</span>pdf</a>
      <nav class="nav-links">
        <a href="/#features">Features</a>
        <a href="/#languages">Languages</a>
        <a href="/docs/">Docs</a>
        <a href="/#pricing">Pricing</a>
        <a href="/#faq">FAQ</a>
      </nav>
      <a href="/#pricing" class="btn btn-sm">Get a license</a>
    </div>
  </header>

  <main id="main">
    <section class="hero">
      <div class="wrap">
        <p class="eyebrow">%%EYEBROW%% &middot; %%LANGLONG%%</p>
        <h1>%%H1%%</h1>
        <p class="lede sp-lede">%%LEDE%%</p>
        <div class="sp-scene" aria-hidden="true">
          <div class="sp-paper">
            <span class="sp-badge">%%LANGNAME%%</span>
            <span class="ln"></span><span class="ln"></span><span class="ln s"></span>
            <span class="ln"></span><span class="ln s"></span><span class="ln"></span>
            <span class="sp-seal">%%SEAL%%</span>
          </div>
        </div>
        <div class="cta-row" style="margin-top:32px">
          <a href="#how" class="btn btn-lg">See the %%LANGNAME%% code</a>
          <a href="%%HUBURL%%" class="btn btn-ghost btn-lg">%%HUBNAME%%, explained</a>
        </div>
      </div>
    </section>

    <section class="band">
      <div class="wrap">
        <h2 class="center">Why %%LANGLONG%% needs this</h2>
        <div class="sp-why">
          <div>
            <p class="muted">%%GAP%%</p>
            <p class="muted">%%WHYP%%</p>
          </div>
          <ul class="check-list">
%%BULLETS%%
          </ul>
        </div>
      </div>
    </section>

    <section id="how" class="section">
      <div class="wrap">
        <h2 class="center">%%HOWTITLE%% with rust-pdf</h2>
        <p class="center muted narrow">Install the package, then call the same idiomatic API every rust-pdf binding shares. The snippet below is real %%LANGNAME%% code from the reference docs.</p>
        <div class="tabs">
          <div class="tab-bar"><span class="tab active">%%LANGNAME%%</span></div>
<pre class="tab-pane active"><code>%%CODE%%</code></pre>
        </div>
%%VALIDATORS%%
%%LICNOTE%%
        <p class="center muted narrow" style="margin-top:22px">Full %%LANGNAME%% reference in the <a href="%%LANGDOCS%%">documentation</a>.</p>
      </div>
    </section>

    <section class="band">
      <div class="wrap narrow">
        <h2 class="center">%%TASKLABEL%% in %%LANGNAME%%: FAQ</h2>
%%FAQHTML%%
      </div>
    </section>

    <section class="section">
      <div class="wrap narrow center">
        <h2>%%CTATITLE%%</h2>
        <p class="muted">One Rust core, the same output across every language. Prototype for free, license the corporate features when you ship.</p>
        <div class="cta-row">
          <a href="%%LANGDOCS%%" class="btn btn-lg">Read the %%LANGNAME%% docs</a>
          <a href="/#pricing" class="btn btn-ghost btn-lg">View pricing &amp; licensing</a>
        </div>
        <div class="sibling-langs">
%%SIBLINGS%%
        </div>
      </div>
    </section>
  </main>

  <footer class="footer">
    <div class="wrap footer-inner">
      <span class="brand">rust<span>&middot;</span>pdf</span>
      <span class="footer-by">by <a href="https://casefy.io" rel="noopener">CaseFy&nbsp;Inc.</a></span>
      <span class="muted">Enterprise PDF for every language. One Rust core, licensed per feature.</span>
      <nav class="footer-links">
        <a href="/docs/">Docs</a>
        <a href="/legal/terms.html">Terms</a>
        <a href="/legal/privacy.html">Privacy</a>
        <a href="mailto:sales@casefy.io">Contact</a>
      </nav>
    </div>
  </footer>

  <script src="/highlight.js"></script>
  <script src="/app.js"></script>
  <script src="/consent.js" defer></script>
</body>
</html>
"""

SEALS = {"sign-pdf": "SIGNED", "pdf-a": "PDF/A", "encrypt-pdf": "AES-256",
         "merge-pdf": "MERGE", "extract-text": "TEXT", "compress-pdf": "OPTIMIZED",
         "generate-pdf": "PDF", "pdf-forms": "FORM"}
HOWTITLE = {"sign-pdf": "Sign a PDF in %L", "pdf-a": "Create PDF/A in %L",
            "encrypt-pdf": "Encrypt a PDF in %L", "merge-pdf": "Merge PDFs in %L",
            "extract-text": "Extract text in %L", "compress-pdf": "Compress a PDF in %L",
            "generate-pdf": "Generate a PDF in %L", "pdf-forms": "Build a form in %L"}
TASKLABEL = {"sign-pdf": "Signing", "pdf-a": "PDF/A", "encrypt-pdf": "Encryption",
             "merge-pdf": "Merging", "extract-text": "Text extraction",
             "compress-pdf": "Compression", "generate-pdf": "Generation", "pdf-forms": "Forms"}


def render(task, lang, code):
    t = TASKS[task]
    L = LANGS[lang]
    ln = L["name"]
    sub = lambda s: s.replace("%L", ln).replace("%l", ln)
    path = f"/{task}/{lang}"
    title = sub(t["title"])
    lede = sub(t["lede"])
    desc = (sub(t["title"]) + ". " + sub(t["lede"]))[:300]
    desc = re.sub(r"\s+", " ", desc).strip()

    # JSON-LD
    crumbs = [
        {"@type": "ListItem", "position": 1, "name": "Home", "item": "https://rustpdf.dev/"},
        {"@type": "ListItem", "position": 2, "name": t["hub_name"], "item": "https://rustpdf.dev" + t["hub_url"]},
        {"@type": "ListItem", "position": 3, "name": title, "item": "https://rustpdf.dev" + path},
    ]
    faq_entities = [{"@type": "Question", "name": sub(q),
                     "acceptedAnswer": {"@type": "Answer", "text": sub(a)}} for q, a in t["faq"]]
    graph = {"@context": "https://schema.org", "@graph": [
        {"@type": "BreadcrumbList", "itemListElement": crumbs},
        {"@type": "TechArticle", "headline": title, "description": desc,
         "url": "https://rustpdf.dev" + path, "inLanguage": "en",
         "proficiencyLevel": "Beginner",
         "about": {"@type": "SoftwareApplication", "name": "rust-pdf", "applicationCategory": "DeveloperApplication"},
         "publisher": {"@type": "Organization", "name": "CaseFy Inc.", "url": "https://casefy.io"}},
        {"@type": "FAQPage", "mainEntity": faq_entities},
    ]}
    jsonld = json.dumps(graph, indent=2, ensure_ascii=False)

    bullets = "\n".join(f"            <li>{esc(sub(b))}</li>" for b in t["bullets"])
    faqhtml = "\n".join(
        f"        <details><summary>{esc(sub(q))}</summary>\n          <p>{esc(sub(a))}</p>\n        </details>"
        for q, a in t["faq"])

    if t["gated"]:
        validators = ('        <div class="validators" style="text-align:center">Validated by: '
                      + "".join(f"<span>{v}</span>" for v in t["validators"]) + "</div>")
        licnote = ('        <p class="center muted narrow" style="margin-top:18px">'
                   f"{ln} basic generation is free. {TASKLABEL[task]} is a corporate feature, unlocked by one offline "
                   'license token. See <a href="/#pricing">pricing &amp; licensing</a>.</p>')
    else:
        validators = ('        <div class="validators" style="text-align:center">Validated by: '
                      + "".join(f"<span>{v}</span>" for v in t["validators"]) + "</div>")
        licnote = ('        <p class="center muted narrow" style="margin-top:18px">'
                   f"This is part of the free tier in {ln}. No license required.</p>")

    # siblings: same task other langs, same lang other tasks, hub, docs
    sib = []
    sib.append(f'          <a href="{t["hub_url"]}">{esc(t["hub_name"])}</a>')
    sib.append(f'          <a href="{L["hub"]}">rust-pdf for {esc(ln)}</a>')
    other_langs = [x for x in MATRIX[task] if x != lang]
    for ol in other_langs:
        sib.append(f'          <a href="/{task}/{ol}">{esc(TASKLABEL[task])} in {esc(LANGS[ol]["name"])}</a>')
    other_tasks = [tk for tk in TASKS if lang in MATRIX[tk] and tk != task]
    for ot in other_tasks[:5]:
        sib.append(f'          <a href="/{ot}/{lang}">{esc(sub2(ot, ln))}</a>')
    sib.append(f'          <a href="{L["docs"]}">{esc(ln)} docs</a>')
    siblings = "\n".join(sib)

    out = PAGE
    repl = {
        "%%TITLE%%": esc(title), "%%DESC%%": esc(desc), "%%PATH%%": path,
        "%%JSONLD%%": jsonld, "%%STYLE%%": STYLE,
        "%%EYEBROW%%": esc(t["eyebrow"]), "%%LANGLONG%%": esc(L["long"]),
        "%%H1%%": sub(t["h1"]), "%%LEDE%%": esc(lede),
        "%%LANGNAME%%": esc(ln), "%%SEAL%%": SEALS[task],
        "%%HUBURL%%": t["hub_url"], "%%HUBNAME%%": esc(t["hub_name"]),
        "%%GAP%%": esc(L["gap"]), "%%WHYP%%": esc(sub(t["why_p"])),
        "%%BULLETS%%": bullets,
        "%%HOWTITLE%%": esc(sub(HOWTITLE[task])),
        "%%CODE%%": esc(code), "%%VALIDATORS%%": validators, "%%LICNOTE%%": licnote,
        "%%LANGDOCS%%": L["docs"],
        "%%TASKLABEL%%": esc(TASKLABEL[task]),
        "%%FAQHTML%%": faqhtml,
        "%%CTATITLE%%": esc(sub(t["title"])),
        "%%SIBLINGS%%": siblings,
    }
    for k, v in repl.items():
        out = out.replace(k, v)
    return path, out


def sub2(task, ln):
    return f"{TASKLABEL[task]} in {ln}"


def main():
    code = harvest()
    pages = []
    for task in TASKS:
        for lang in MATRIX[task]:
            path, out = render(task, lang, code[task][lang])
            if "—" in out:  # em-dash guard
                sys.exit(f"em-dash found in {path}")
            d = os.path.join(PUB, task)
            os.makedirs(d, exist_ok=True)
            with open(os.path.join(d, f"{lang}.html"), "w", encoding="utf-8") as f:
                f.write(out)
            pages.append(path)
    # rewrite sitemap spoke block
    update_sitemap(pages)
    print(f"generated {len(pages)} spoke pages")
    for p in pages:
        print("  ", p)


def update_sitemap(paths):
    sm = os.path.join(PUB, "sitemap.xml")
    src = open(sm, encoding="utf-8").read()
    MARK_A = "<!-- spokes:start -->"
    MARK_B = "<!-- spokes:end -->"
    block = [MARK_A]
    for p in paths:
        block.append("  <url>")
        block.append(f"    <loc>https://rustpdf.dev{p}</loc>")
        block.append(f"    <lastmod>{TODAY}</lastmod>")
        block.append("    <changefreq>weekly</changefreq>")
        block.append("    <priority>0.7</priority>")
        block.append("  </url>")
    block.append(MARK_B)
    blob = "\n".join(block)
    if MARK_A in src and MARK_B in src:
        src = re.sub(re.escape(MARK_A) + ".*?" + re.escape(MARK_B), blob, src, flags=re.S)
    else:
        src = src.replace("</urlset>", blob + "\n</urlset>")
    open(sm, "w", encoding="utf-8").write(src)


if __name__ == "__main__":
    main()
