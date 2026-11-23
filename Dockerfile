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

# ── pyright language server (Python Pass 2, task-074) ────────────────────────
# Pinned to the release the driver was verified against (1.1.414).
FROM node:22-bookworm-slim AS pyright
RUN npm install -g pyright@1.1.414

# ── rust-analyzer (Rust Pass 2, task-073) ────────────────────────────────────
# Pinned via the toolchain image: the rust-analyzer component shipped with the
# pinned rust 1.87 toolchain. Same base as the builder stage, so the binary's
# libc matches the bookworm runtime stage (the compiled server binary relies
# on the same compatibility).
FROM rust:1.87-slim AS rust-analyzer
RUN rustup component add rust-analyzer \
    && cp "$(rustup which rust-analyzer)" /usr/local/bin/rust-analyzer

# ── typescript-language-server + typescript (TypeScript Pass 2, task-075) ────
# Pinned npm packages, following the pyright stage pattern above.
FROM node:22-bookworm-slim AS ts-tools
RUN npm install -g typescript@5.8.3 typescript-language-server@4.3.4

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

# Pass 2 semantic extraction prerequisites (specs/system/lsp-call-graph.md §11):
# the Go toolchain (packages.Load shells out to `go list`) and the committed
# go-callgraph CHA binary. The Rust (rust-analyzer) and TypeScript
# (typescript-language-server + typescript) toolchains are wired in below.
COPY --from=golang:1.24-bookworm-slim /usr/local/go /usr/local/go
ENV PATH="/usr/local/go/bin:${PATH}"
COPY scripts/go-callgraph/go-callgraph /usr/local/bin/gyre-go-callgraph
ENV GO_CALLGRAPH_BIN=/usr/local/bin/gyre-go-callgraph

# Python Pass 2 (task-074) and TypeScript Pass 2 (task-075): node runs the
# LSP driver scripts, pyright provides pyright-langserver, and python3 lets
# pyright resolve the analyzed repo's interpreter. The driver scripts live
# under the server's working directory (/app), where the adapter's workspace
# discovery finds them.
RUN apt-get update && apt-get install -y --no-install-recommends \
    libstdc++6 \
    python3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=node:22-bookworm-slim /usr/local/bin/node /usr/local/bin/node
COPY --from=pyright /usr/local/lib/node_modules/pyright /usr/local/lib/node_modules/pyright
RUN ln -s /usr/local/lib/node_modules/pyright/langserver.index.js /usr/local/bin/pyright-langserver \
    && ln -s /usr/local/lib/node_modules/pyright/index.js /usr/local/bin/pyright
COPY scripts/python-callgraph.sh scripts/python-callgraph-pyright.mjs scripts/ts-callgraph.mjs /app/scripts/
ENV PYRIGHT_LANGSERVER_BIN=/usr/local/bin/pyright-langserver

# Rust Pass 2 (task-073): the rust-analyzer LSP binary, discovered via
# RUST_ANALYZER_BIN then PATH (crates/gyre-adapters/src/call_graph_rust.rs
# find_rust_analyzer). Without it, Rust Pass 2 degrades to zero Calls edges.
COPY --from=rust-analyzer /usr/local/bin/rust-analyzer /usr/local/bin/rust-analyzer
ENV RUST_ANALYZER_BIN=/usr/local/bin/rust-analyzer

# TypeScript Pass 2 (task-075): typescript-language-server drives tsserver
# from the typescript package; scripts/ts-callgraph.mjs discovers them via
# TS_CALLGRAPH_SERVER (the language-server entry point) and
# TS_CALLGRAPH_TSSERVER (the TypeScript lib dir). Node is already installed
# above for the Python driver.
COPY --from=ts-tools /usr/local/lib/node_modules/typescript /usr/local/lib/node_modules/typescript
COPY --from=ts-tools /usr/local/lib/node_modules/typescript-language-server /usr/local/lib/node_modules/typescript-language-server
RUN ln -s /usr/local/lib/node_modules/typescript-language-server/lib/cli.mjs /usr/local/bin/typescript-language-server
ENV TS_CALLGRAPH_SERVER=/usr/local/bin/typescript-language-server
ENV TS_CALLGRAPH_TSSERVER=/usr/local/lib/node_modules/typescript/lib

# Default data directory
RUN mkdir -p /data && chown gyre:gyre /data

USER gyre

ENV GYRE_BASE_URL=http://localhost:3000
ENV GYRE_DATA_DIR=/data
ENV RUST_LOG=info

EXPOSE 3000

# Usage: docker run -p 3000:3000 -v /host/data:/data gyre-server
CMD ["/usr/local/bin/gyre-server"]
