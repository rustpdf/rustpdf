# Multi-stage build. BUILD CONTEXT MUST BE THE REPO ROOT so we can compile
# the real licctl binary from the Rust workspace and bundle it with the site:
#
#   docker build -f site/Dockerfile -t rustpdf-site .
#
# ---- Stage 1: build the licctl license-minting binary ----------------------
FROM rust:1-slim AS licctl
WORKDIR /build
# Copy just what the license crate + workspace resolution need.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# Building one example still needs the workspace to resolve; this is fine.
RUN cargo build --release -p license --example licctl

# ---- Stage 2: the Node site ------------------------------------------------
FROM node:22-slim AS site
WORKDIR /app
# better-sqlite3 compiles a native addon → needs a toolchain.
RUN apt-get update && apt-get install -y --no-install-recommends \
      python3 build-essential \
    && rm -rf /var/lib/apt/lists/*

COPY site/package.json site/package-lock.json* ./
RUN npm ci --omit=dev || npm install --omit=dev

COPY site/ ./
# Bundle the license minter and point the app at it.
COPY --from=licctl /build/target/release/examples/licctl /app/bin/licctl
ENV LICCTL_BIN=/app/bin/licctl
ENV NODE_ENV=production
ENV PORT=3000
ENV DB_PATH=/app/data/licenses.db

# Persist issued licenses across restarts.
VOLUME ["/app/data"]
EXPOSE 3000
CMD ["node", "src/server.js"]
