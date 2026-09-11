# Build the documentation site image.
#
#   docker build -f site/Dockerfile -t rustpdf-site .
#
# The build context must contain the `site/` directory (the repo root works).
FROM node:22-slim AS site
WORKDIR /app

COPY site/package.json site/package-lock.json* ./
RUN npm ci --omit=dev || npm install --omit=dev

COPY site/ ./

ENV NODE_ENV=production
ENV PORT=3000
EXPOSE 3000
CMD ["node", "src/server.js"]
