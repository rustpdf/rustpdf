import express from "express";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { config, assertRuntime } from "./config.js";
import {
  createCheckoutSession,
  constructEvent,
  fulfilCheckout,
  fulfilRenewal,
  getPricing,
  simulatePurchase,
  stripe,
} from "./stripe.js";
import { getLicenseByRef } from "./db.js";

assertRuntime();

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const publicDir = path.join(__dirname, "..", "public");

const app = express();
app.disable("x-powered-by");
app.set("trust proxy", true);

// --- Security headers + HTTPS enforcement ------------------------------------
// Cloudflare sits in front, but we set HSTS and the standard security headers at
// the origin too (defense in depth; Cloudflare passes them through) and redirect
// any plain-HTTP hit to HTTPS as a safety net. Also enable "Always Use HTTPS" in
// the Cloudflare dashboard so the edge handles http:// before it reaches origin.
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
// Every page's <link rel=canonical> is extensionless; the static handler below
// also serves the bare filename, so `.html` would be a 200 duplicate. 301 it to
// the clean URL so link equity and crawl budget consolidate on one form.
app.use((req, res, next) => {
  if (req.method === "GET" && req.path.endsWith(".html")) {
    const clean = req.path.slice(0, -5);
    const suffix = req.originalUrl.slice(req.path.length); // preserve ?query
    return res.redirect(301, (clean === "/index" ? "/" : clean) + suffix);
  }
  next();
});

// --- Stripe webhook: MUST receive the raw body, so mount it before json() ----
app.post(
  "/api/stripe/webhook",
  express.raw({ type: "application/json" }),
  async (req, res) => {
    let event;
    try {
      event = constructEvent(req.body, req.headers["stripe-signature"]);
    } catch (err) {
      console.error("Webhook signature verification failed:", err.message);
      return res.status(400).send(`Webhook Error: ${err.message}`);
    }

    try {
      switch (event.type) {
        case "checkout.session.completed":
        case "checkout.session.async_payment_succeeded": {
          const license = await fulfilCheckout(event.data.object);
          console.log(`Fulfilled ${event.data.object.id} → license for ${license.email}`);
          break;
        }
        case "invoice.paid":
        case "invoice.payment_succeeded": {
          // Only renewals; the signup invoice is handled by checkout.session.completed.
          const invoice = event.data.object;
          if (invoice.billing_reason === "subscription_cycle") {
            const license = await fulfilRenewal(invoice);
            console.log(`Renewed ${invoice.id} → fresh license for ${license.email}`);
          }
          break;
        }
        default:
          break;
      }
      res.json({ received: true });
    } catch (err) {
      // Return 500 so Stripe retries; fulfilment is idempotent.
      console.error("Fulfilment error:", err);
      res.status(500).json({ error: "fulfilment_failed" });
    }
  }
);

app.use(express.json());

// --- Create a Checkout Session -----------------------------------------------
app.post("/api/checkout", async (req, res) => {
  const tier = req.body?.tier;
  try {
    if (config.testCheckout) {
      // Simulate the whole purchase locally — no Stripe needed.
      const row = await simulatePurchase({
        email: req.body?.email,
        licensee: req.body?.licensee,
        tier,
      });
      console.log(`[test] simulated ${row.tier} purchase ${row.ref} → ${row.email}`);
      return res.json({ url: `/success?session_id=${row.ref}`, test: true });
    }
    const session = await createCheckoutSession(tier);
    res.json({ url: session.url });
  } catch (err) {
    console.error("Checkout creation failed:", err);
    res.status(500).json({ error: "checkout_failed" });
  }
});

// --- Look a token up for the success page ------------------------------------
// Only returns the token once the session is paid AND the webhook has minted it.
app.get("/api/license", async (req, res) => {
  const sessionId = String(req.query.session_id || "");
  if (!sessionId.startsWith("cs_")) return res.status(400).json({ error: "bad_session" });

  // In test mode, serve straight from the store (no real Stripe session exists).
  if (!config.testCheckout) {
    try {
      const session = await stripe.checkout.sessions.retrieve(sessionId);
      if (session.payment_status !== "paid" && config.stripe.mode === "payment") {
        return res.json({ status: "pending" });
      }
    } catch {
      return res.status(404).json({ error: "not_found" });
    }
  }

  const row = getLicenseByRef(sessionId);
  if (!row) return res.json({ status: "processing" }); // webhook not in yet

  res.json({
    status: "ready",
    token: row.token,
    licensee: row.licensee,
    email: row.email,
    expires_at: row.expires_at,
    tier: row.tier,
    features: row.features,
  });
});

// --- Live price for the pricing card (kept in sync with the Stripe Price) ----
app.get("/api/pricing", async (_req, res) => {
  try {
    res.json(await getPricing());
  } catch (err) {
    console.error("Pricing fetch failed:", err.message);
    res.status(502).json({ error: "pricing_unavailable" });
  }
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
// e.g. both `pdf-a.html` and the `pdf-a/` directory exist. express.static would
// see the directory first, 301 the canonical extensionless URL `/pdf-a` to
// `/pdf-a/`, find no index there and return 404 — silently dropping the hub page
// from the index. Serve the hub .html explicitly (200 at the canonical URL)
// BEFORE the static handler runs. /pdf-a/<lang> spokes are unaffected.
// HTML pages served via explicit routes (below) bypass the static handler's
// setHeaders, so give them the same short-TTL cache policy here.
const PAGE_CACHE = "public, max-age=300, stale-while-revalidate=86400";
const sendPage = (res, ...parts) => {
  res.setHeader("Cache-Control", PAGE_CACHE);
  res.sendFile(path.join(publicDir, ...parts));
};

const HUBS_WITH_SUBDIR = [
  "pdf-a", "encrypt-pdf", "merge-pdf", "extract-text", "compress-pdf", "pdf-forms",
];
for (const slug of HUBS_WITH_SUBDIR) {
  // `/slug/` (trailing slash) 301s to the canonical `/slug`; canonical serves 200.
  app.get(`/${slug}/`, (_req, res) => res.redirect(301, `/${slug}`));
  app.get(`/${slug}`, (_req, res) => sendPage(res, `${slug}.html`));
}
// sign-pdf and generate-pdf have spoke directories but no concept page of their
// own; serve the dedicated hub index we generate into each directory.
app.get("/sign-pdf/", (_req, res) => res.redirect(301, "/sign-pdf"));
app.get("/sign-pdf", (_req, res) => sendPage(res, "sign-pdf", "index.html"));
app.get("/generate-pdf/", (_req, res) => res.redirect(301, "/generate-pdf"));
app.get("/generate-pdf", (_req, res) => sendPage(res, "generate-pdf", "index.html"));

// --- Static site -------------------------------------------------------------
app.use(
  express.static(publicDir, {
    extensions: ["html"],
    setHeaders(res, filePath) {
      // Fingerprinted/rarely-changing assets: cache hard for a year. Fonts,
      // images and the favicon never change at a given URL. CSS/JS are not
      // content-hashed yet, so give them a safe one-day TTL (still 6x the old
      // 4h) — bump to a year once they carry a ?v= or hashed filename.
      if (/\.(woff2?|ttf|otf|png|jpe?g|gif|webp|svg|ico)$/i.test(filePath)) {
        res.setHeader("Cache-Control", "public, max-age=31536000, immutable");
      } else if (/\.(css|js)$/i.test(filePath)) {
        res.setHeader("Cache-Control", "public, max-age=86400");
      } else if (/\.html$/i.test(filePath)) {
        // HTML: short edge/browser TTL with background revalidation so a page
        // navigation can serve instantly while fetching a fresh copy.
        res.setHeader("Cache-Control", "public, max-age=300, stale-while-revalidate=86400");
      }
    },
  }),
);
app.get("/success", (_req, res) => sendPage(res, "success.html"));
app.get("/cancel", (_req, res) => sendPage(res, "cancel.html"));
// /zugferd is the canonical hybrid-e-invoice page; serve the same page for the
// French "Factur-X" spelling (the page's <link rel=canonical> points to /zugferd).
app.get(["/factur-x", "/facturx"], (_req, res) => sendPage(res, "zugferd.html"));
// /merge-pdf is the canonical page for combine + split; serve it for /split-pdf
// too (its <link rel=canonical> points to /merge-pdf).
app.get(["/split-pdf"], (_req, res) => sendPage(res, "merge-pdf.html"));

// Task x language spoke pages live at /<task>/<lang>.html and are served by the
// static handler above. Map the common alternate language spellings (golang,
// nodejs, dotnet) to the canonical spoke file; each spoke's <link rel=canonical>
// points at the primary slug (go / node / csharp).
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

app.listen(config.port, () => {
  console.log(`${config.productName} site on ${config.baseUrl} (port ${config.port})`);
});
