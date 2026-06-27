# rust-pdf — sales site + license automation

Marketing landing page + Stripe checkout + **automated license delivery** for
the `rust-pdf` product. When a customer pays, the backend mints a real Ed25519
license token (by shelling out to the actual `licctl` Rust binary — same crypto
the library verifies with) and emails it via SendGrid.

```
Stripe Checkout ──► webhook ──► licctl (mint token) ──► SendGrid email
                                     │                        │
                                     └──► SQLite (idempotent) ─┘──► success page
```

## What's here

| Path | Role |
|------|------|
| `public/` | Static landing page, success & cancel pages |
| `src/server.js` | Express app (static + `/api/checkout` + Stripe webhook + `/api/license`) |
| `src/stripe.js` | Checkout session creation + idempotent fulfilment |
| `src/license.js` | Mints tokens via the `licctl` binary |
| `src/email.js` | SendGrid delivery of the token + activation steps |
| `src/db.js` | SQLite store (idempotency + token lookup) |
| `scripts/issue.js` | Manual token issuance (support / OEM / comps) |
| `Dockerfile` | Multi-stage: compiles `licctl`, bundles it with the Node app |

## Prerequisites

1. **Vendor keypair.** Generate once and keep the seed secret:
   ```sh
   # from the repo root
   cargo build --release -p license --example licctl
   target/release/examples/licctl keygen
   #   seed   <64 hex>   → VENDOR_SEED  (SECRET, goes in .env)
   #   pubkey <64 hex>   → bake into the shipped library binaries:
   #                       RUSTPDF_LICENSE_PUBKEY=<pubkey> cargo build --release -p pdf-ffi
   ```
   The pubkey baked into the binaries you distribute must match this seed, or the
   emitted tokens won't verify on the customer's machine.

2. **Stripe.** Create one annual Price per self-serve tier — **Pro** and
   **Enterprise** — and copy each `price_…` id into `STRIPE_PRICE_ID_PRO` and
   `STRIPE_PRICE_ID_ENTERPRISE`. (OEM is sales-led, no Stripe price.) Grab your
   secret key and (after creating the webhook endpoint) the signing secret.

3. **SendGrid.** API key + a verified sender identity/domain.

## Local development

```sh
cd site
cp .env.example .env          # fill in the values (see below)
npm install

# Terminal A — run the site (needs the licctl binary built above)
npm run dev

# Terminal B — forward Stripe webhooks to the local server
stripe listen --forward-to localhost:3000/api/stripe/webhook
#   → copies a whsec_… into STRIPE_WEBHOOK_SECRET

# Trigger a test purchase from the site's pricing button, or:
stripe trigger checkout.session.completed
```

### Required env (`.env`)

See `.env.example` for the full annotated list. The essentials:

- `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`, `STRIPE_PRICE_ID_PRO`, `STRIPE_PRICE_ID_ENTERPRISE`
- `VENDOR_SEED` (64-hex Ed25519 seed) — **secret**
- `SENDGRID_API_KEY`, `SENDGRID_FROM` (verified sender)
- `BASE_URL` (public URL for Stripe redirects)

`LICCTL_BIN` defaults to `../target/release/examples/licctl`; override in prod.

## Deploy (VPS, Docker)

The Dockerfile compiles `licctl` from the workspace and bundles it, so the build
context is the **repo root**:

```sh
# on the VPS, in the repo root, with site/.env filled in
docker compose -f site/docker-compose.yml up -d --build
```

Then put nginx/Caddy in front for TLS and point a Stripe webhook endpoint at
`https://yourdomain/api/stripe/webhook` (event `checkout.session.completed`).

### Manual / comp licenses

```sh
node scripts/issue.js "ACME Corp" 365 all
# prints a ready-to-send token
```

## Security notes

- The webhook verifies Stripe's signature (`STRIPE_WEBHOOK_SECRET`) before doing
  anything; fulfilment is **idempotent** (keyed on the checkout session id), so
  Stripe retries can't double-issue.
- `VENDOR_SEED` is the only thing that can mint licenses — treat it like a
  signing key. It never touches a shell (passed via `execFile` argv).
- The success page reveals a token only for a **paid** session whose id the
  caller already holds (`cs_…` ids are unguessable) — and the token is emailed
  regardless, so a lost tab never loses the key.
