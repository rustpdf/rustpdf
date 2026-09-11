import express from "express";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const publicDir = path.join(__dirname, "..", "public");

const PORT = Number(process.env.PORT || 3000);
const PRODUCT_NAME = process.env.PRODUCT_NAME || "rust-pdf";
const BASE_URL = (process.env.BASE_URL || "http://localhost:3000").replace(/\/+$/, "");

const app = express();
app.disable("x-powered-by");
app.set("trust proxy", true);

// --- Security headers + HTTPS enforcement ------------------------------------
app.use((req, res, next) => {
  res.setHeader("Strict-Transport-Security", "max-age=63072000; includeSubDomains; preload");
  res.setHeader("X-Content-Type-Options", "nosniff");
  res.setHeader("X-Frame-Options", "DENY");
  res.setHeader("Referrer-Policy", "strict-origin-when-cross-origin");
  res.setHeader("Permissions-Policy", "geolocation=(), microphone=(), camera=()");
  // Content-Security-Policy: lock down to self + the only third party we load
  // (Google Analytics, and only after cookie consent — see consent.js).
  res.setHeader(
    "Content-Security-Policy",
    "default-src 'self'; base-uri 'self'; object-src 'none'; frame-ancestors 'none'; frame-src 'none'; " +
      "img-src 'self' data: https:; style-src 'self' 'unsafe-inline'; font-src 'self'; " +
      "script-src 'self' 'unsafe-inline' https://www.googletagmanager.com https://www.google-analytics.com; " +
      "connect-src 'self' https://www.googletagmanager.com https://www.google-analytics.com https://*.google-analytics.com https://*.analytics.google.com",
  );
  const proto = req.headers["x-forwarded-proto"];
  if (proto && proto !== "https" && req.method === "GET") {
    return res.redirect(301, `https://${req.headers.host}${req.originalUrl}`);
  }
  next();
});

// --- Canonical URL form: redirect any `.html` URL to its extensionless path ---
app.use((req, res, next) => {
  if (req.method === "GET" && req.path.endsWith(".html")) {
    const clean = req.path.slice(0, -5);
    const suffix = req.originalUrl.slice(req.path.length); // preserve ?query
    return res.redirect(301, (clean === "/index" ? "/" : clean) + suffix);
  }
  next();
});

app.get("/healthz", (_req, res) => res.json({ ok: true }));

// --- Delphi docs: inject the current package version ------------------------
// The download links/version on /docs/delphi.html carry a __DELPHI_VERSION__
// placeholder so the page never needs a manual edit per release. The version is
// derived from the zip actually present in /downloads (the single source of
// truth — whatever the deploy baked in), falling back to $DELPHI_VERSION. Cached
// at startup; the container restarts on every deploy, so it stays current.
function cmpSemver(a, b) {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) if ((pa[i] || 0) !== (pb[i] || 0)) return (pa[i] || 0) - (pb[i] || 0);
  return 0;
}
function delphiVersion() {
  try {
    const versions = fs
      .readdirSync(path.join(publicDir, "downloads"))
      .map((f) => f.match(/^rustpdf-delphi-(\d+\.\d+\.\d+)\.zip$/))
      .filter(Boolean)
      .map((m) => m[1])
      .sort(cmpSemver);
    if (versions.length) return versions[versions.length - 1];
  } catch {
    /* no downloads dir yet */
  }
  return process.env.DELPHI_VERSION || "0.1.0";
}
function renderDelphiPage() {
  const html = fs.readFileSync(path.join(publicDir, "docs", "delphi.html"), "utf8");
  return html.replace(/__DELPHI_VERSION__/g, delphiVersion());
}
let delphiPageCache = null;
try {
  delphiPageCache = renderDelphiPage();
  console.log(`Delphi docs pinned to v${delphiVersion()}`);
} catch (err) {
  console.error("Delphi docs render failed:", err.message);
}
app.get(["/docs/delphi.html", "/docs/delphi"], (_req, res) => {
  try {
    res.type("html").send(delphiPageCache || renderDelphiPage());
  } catch {
    res.sendFile(path.join(publicDir, "docs", "delphi.html"));
  }
});

// --- Swift docs: inject the current package version + xcframework checksum ----
// /docs/swift.html carries __SWIFT_VERSION__ and __SWIFT_CHECKSUM__ placeholders.
// Both derive from what the deploy baked into /downloads (single source of truth):
// the version from the published zip, the checksum from the .checksum file.
function swiftVersion() {
  try {
    const versions = fs
      .readdirSync(path.join(publicDir, "downloads"))
      .map((f) => f.match(/^rustpdf-swift-(\d+\.\d+\.\d+)\.zip$/))
      .filter(Boolean)
      .map((m) => m[1])
      .sort(cmpSemver);
    if (versions.length) return versions[versions.length - 1];
  } catch {
    /* no downloads dir yet */
  }
  return process.env.SWIFT_VERSION || "0.1.0";
}
function swiftChecksum(version) {
  try {
    return fs
      .readFileSync(
        path.join(publicDir, "downloads", `RustPdfFFI-${version}.xcframework.zip.checksum`),
        "utf8",
      )
      .trim();
  } catch {
    return "PASTE_FROM_THE_.checksum_FILE";
  }
}
function renderSwiftPage() {
  const v = swiftVersion();
  const html = fs.readFileSync(path.join(publicDir, "docs", "swift.html"), "utf8");
  return html.replace(/__SWIFT_VERSION__/g, v).replace(/__SWIFT_CHECKSUM__/g, swiftChecksum(v));
}
let swiftPageCache = null;
try {
  swiftPageCache = renderSwiftPage();
  console.log(`Swift docs pinned to v${swiftVersion()}`);
} catch (err) {
  console.error("Swift docs render failed:", err.message);
}
app.get(["/docs/swift.html", "/docs/swift"], (_req, res) => {
  try {
    res.type("html").send(swiftPageCache || renderSwiftPage());
  } catch {
    res.sendFile(path.join(publicDir, "docs", "swift.html"));
  }
});

// --- Feature hubs that also have a spoke subdirectory ------------------------
const PAGE_CACHE = "public, max-age=300, stale-while-revalidate=86400";
const sendPage = (res, ...parts) => {
  res.setHeader("Cache-Control", PAGE_CACHE);
  res.sendFile(path.join(publicDir, ...parts));
};

const HUBS_WITH_SUBDIR = [
  "pdf-a", "encrypt-pdf", "merge-pdf", "extract-text", "compress-pdf", "pdf-forms",
];
for (const slug of HUBS_WITH_SUBDIR) {
  app.get([`/${slug}`, `/${slug}/`], (req, res) => {
    if (req.path.endsWith("/")) return res.redirect(301, `/${slug}`);
    sendPage(res, `${slug}.html`);
  });
}
app.get(["/sign-pdf", "/sign-pdf/"], (req, res) => {
  if (req.path.endsWith("/")) return res.redirect(301, "/sign-pdf");
  sendPage(res, "sign-pdf", "index.html");
});
app.get(["/generate-pdf", "/generate-pdf/"], (req, res) => {
  if (req.path.endsWith("/")) return res.redirect(301, "/generate-pdf");
  sendPage(res, "generate-pdf", "index.html");
});

// --- Static site -------------------------------------------------------------
app.use(
  express.static(publicDir, {
    extensions: ["html"],
    setHeaders(res, filePath) {
      if (/\.(woff2?|ttf|otf|png|jpe?g|gif|webp|svg|ico)$/i.test(filePath)) {
        res.setHeader("Cache-Control", "public, max-age=31536000, immutable");
      } else if (/\.(css|js)$/i.test(filePath)) {
        res.setHeader("Cache-Control", "public, max-age=86400");
      } else if (/\.html$/i.test(filePath)) {
        res.setHeader("Cache-Control", "public, max-age=300, stale-while-revalidate=86400");
      }
    },
  }),
);
// /zugferd is the canonical hybrid-e-invoice page; serve the same page for the
// French "Factur-X" spelling (the page's <link rel=canonical> points to /zugferd).
app.get(["/factur-x", "/facturx"], (_req, res) => sendPage(res, "zugferd.html"));
// /merge-pdf is the canonical page for combine + split; serve it for /split-pdf
// too (its <link rel=canonical> points to /merge-pdf).
app.get(["/split-pdf"], (_req, res) => sendPage(res, "merge-pdf.html"));

// Task x language spoke pages live at /<task>/<lang>.html and are served by the
// static handler above. Map the common alternate language spellings (golang,
// nodejs, dotnet) to the canonical spoke file.
const SPOKE_TASKS = [
  "sign-pdf", "pdf-a", "encrypt-pdf", "merge-pdf",
  "extract-text", "compress-pdf", "generate-pdf", "pdf-forms",
];
const SPOKE_LANG_ALIAS = { golang: "go", nodejs: "node", dotnet: "csharp" };
app.get("/:task/:lang", (req, res, next) => {
  const { task, lang } = req.params;
  const canonical = SPOKE_LANG_ALIAS[lang];
  if (!canonical || !SPOKE_TASKS.includes(task)) return next();
  const file = path.join(publicDir, task, `${canonical}.html`);
  res.sendFile(file, (err) => (err ? next() : undefined));
});

// 404 — anything unmatched (HTML pages get the styled page; APIs get JSON).
app.use((req, res) => {
  if (req.path.startsWith("/api/")) return res.status(404).json({ error: "not_found" });
  res.status(404).sendFile(path.join(publicDir, "404.html"));
});

app.listen(PORT, () => {
  console.log(`${PRODUCT_NAME} site on ${BASE_URL} (port ${PORT})`);
});
