import express from "express";
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

// --- Static site -------------------------------------------------------------
app.use(express.static(publicDir, { extensions: ["html"] }));
app.get("/success", (_req, res) => res.sendFile(path.join(publicDir, "success.html")));
app.get("/cancel", (_req, res) => res.sendFile(path.join(publicDir, "cancel.html")));

// 404 — anything unmatched (HTML pages get the styled page; APIs get JSON).
app.use((req, res) => {
  if (req.path.startsWith("/api/")) return res.status(404).json({ error: "not_found" });
  res.status(404).sendFile(path.join(publicDir, "404.html"));
});

app.listen(config.port, () => {
  console.log(`${config.productName} site on ${config.baseUrl} (port ${config.port})`);
});
