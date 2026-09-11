# rust-pdf — documentation site

Static documentation and marketing site for the free, MIT-licensed `rust-pdf`
library. A small Express server serves the static pages and injects the current
Delphi/Swift package versions into their docs pages.

```
public/  ──►  Express (static + version injection + clean URLs)
```

## What's here

| Path | Role |
|------|------|
| `public/` | Static landing, docs and feature pages |
| `src/server.js` | Express app (static files, clean URLs, docs version injection) |
| `Dockerfile` | Node site image |
| `scripts/deploy.sh` | Build + deploy to the VPS (k3s) |
| `scripts/gen_spokes.py` | Generator for the task × language spoke pages |

## Local development

```sh
cd site
npm install
npm run dev        # http://localhost:3000
```

Optional env (`.env`): `PORT`, `BASE_URL`, `PRODUCT_NAME`, `DELPHI_VERSION`,
`SWIFT_VERSION`. See `.env.example`.

## Deploy (VPS, Docker)

The build context is the **repo root**:

```sh
docker compose -f site/docker-compose.yml up -d --build
```

Put nginx/Caddy (or the existing Traefik/Cloudflare setup) in front for TLS.
