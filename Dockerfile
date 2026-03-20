# syntax=docker/dockerfile:1
# Multi-stage build: compile in a Rust toolchain image, then copy to a minimal runtime image.

# ── Build stage ───────────────────────────────────────────────────────────────
FROM rust:1.87-slim AS builder

# Install system build dependencies for git2, openssl, etc.
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    libclang-dev \
    cmake \
    git \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies: copy manifests first, build deps-only, then copy src
COPY Cargo.toml Cargo.lock ./
COPY crates/gyre-common/Cargo.toml crates/gyre-common/
COPY crates/gyre-domain/Cargo.toml crates/gyre-domain/
COPY crates/gyre-ports/Cargo.toml crates/gyre-ports/
COPY crates/gyre-adapters/Cargo.toml crates/gyre-adapters/
COPY crates/gyre-server/Cargo.toml crates/gyre-server/
COPY crates/gyre-cli/Cargo.toml crates/gyre-cli/

# Create stub lib.rs / main.rs files so cargo can resolve and cache deps
RUN for crate in gyre-common gyre-domain gyre-ports gyre-adapters; do \
      mkdir -p crates/$crate/src && echo "pub fn _dummy() {}" > crates/$crate/src/lib.rs; \
    done && \
    mkdir -p crates/gyre-server/src && echo "fn main() {}" > crates/gyre-server/src/main.rs && \
    echo "pub fn _dummy() {}" >> crates/gyre-server/src/lib.rs && \
    mkdir -p crates/gyre-cli/src && echo "fn main() {}" > crates/gyre-cli/src/main.rs

RUN cargo build --release -p gyre-server -p gyre-cli 2>/dev/null || true

# Copy real source and do the real build
COPY crates/ crates/

# Touch main files to invalidate cached stub builds
RUN find crates -name "*.rs" -exec touch {} +

RUN cargo build --release -p gyre-server -p gyre-cli

# ── Runtime stage ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    git \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd -ms /bin/bash gyre

WORKDIR /app

# Copy compiled binaries
COPY --from=builder /build/target/release/gyre-server /usr/local/bin/gyre-server
COPY --from=builder /build/target/release/gyre         /usr/local/bin/gyre

# Copy SPA assets if present (build separately with npm/vite)
# COPY web/dist /app/web/dist

# Default data directory
RUN mkdir -p /data && chown gyre:gyre /data

USER gyre

ENV GYRE_BASE_URL=http://localhost:3000
ENV GYRE_DATA_DIR=/data
ENV RUST_LOG=info

EXPOSE 3000

# Usage: docker run -p 3000:3000 -v /host/data:/data gyre-server
CMD ["/usr/local/bin/gyre-server"]
