# syntax=docker/dockerfile:1.7
# Rust services: spectra-api, spectra-worker and spectra-db.
# Build from the repository root:  docker build -f docker/engine.Dockerfile .

FROM rust:1-bookworm AS build
WORKDIR /src
# Manifests and sources. examples/ is read at compile time (include_str! of the sample portfolio).
COPY Cargo.toml Cargo.lock rustfmt.toml ./
COPY crates ./crates
COPY examples ./examples
# Optional cargo features, e.g. --build-arg CARGO_FEATURES=spectra-api/s3 for S3 artifact storage.
ARG CARGO_FEATURES=""
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p spectra-api -p spectra-db ${CARGO_FEATURES:+--features "$CARGO_FEATURES"} \
 && mkdir -p /out \
 && cp target/release/spectra-api target/release/spectra-worker target/release/spectra-db /out/

FROM debian:bookworm-slim AS runtime
# curl is only for the container health check.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /var/lib/spectra --create-home spectra \
 && mkdir -p /var/lib/spectra/artifacts /var/lib/spectra/datasets \
 && chown -R spectra /var/lib/spectra
COPY --from=build /out/spectra-api /out/spectra-worker /out/spectra-db /usr/local/bin/
USER spectra
WORKDIR /var/lib/spectra
# Inside the compose network only; the port is never published.
ENV SPECTRA_API_ADDR=0.0.0.0:8000
EXPOSE 8000
HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8000/health || exit 1
CMD ["spectra-api"]
