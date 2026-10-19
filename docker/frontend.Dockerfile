# syntax=docker/dockerfile:1.7
# Next.js dashboard, built as a standalone server.
# Build from the repository root:  docker build -f docker/frontend.Dockerfile .

FROM node:20-bookworm-slim AS deps
WORKDIR /app
COPY web/frontend/package.json web/frontend/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci

FROM node:20-bookworm-slim AS build
WORKDIR /app
ENV NEXT_TELEMETRY_DISABLED=1 NEXT_OUTPUT=standalone
COPY --from=deps /app/node_modules ./node_modules
COPY web/frontend ./
RUN npm run build

FROM node:20-bookworm-slim AS runtime
WORKDIR /app
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 PORT=3000 HOSTNAME=0.0.0.0
RUN useradd --system --uid 10001 --create-home spectra \
 && mkdir -p /app/.data && chown spectra /app/.data
COPY --from=build --chown=spectra /app/.next/standalone ./
COPY --from=build --chown=spectra /app/.next/static ./.next/static
COPY --from=build --chown=spectra /app/public ./public
USER spectra
EXPOSE 3000
HEALTHCHECK --interval=10s --timeout=3s --start-period=20s --retries=3 \
    CMD node -e "fetch('http://127.0.0.1:3000/').then(r=>process.exit(r.status<500?0:1)).catch(()=>process.exit(1))"
CMD ["node", "server.js"]
