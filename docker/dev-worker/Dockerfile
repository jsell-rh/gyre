# Rust/browser tools from Gyre's previously qualified worker; Pi and fullsend
# runtime hooks from the pinned upstream image. No OMP or build-context secrets.
FROM ghcr.io/jsell-rh/gyre-worker@sha256:a3856204f3b23b3694564ab34fa76d588f2d09ea39060a71967e5ab645c2fa7d AS gyre-tools
FROM ghcr.io/fullsend-ai/fullsend-sandbox@sha256:16c312057410c980ca4f9741364d2336bf43ac9e2c97de58dd24e78348f77dd5
USER root
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config libssl-dev libpq-dev sqlite3 lld python3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=gyre-tools /usr/local/rustup /usr/local/rustup
COPY --from=gyre-tools /usr/local/cargo /usr/local/cargo
COPY --from=gyre-tools /usr/local/share/gyre-playwright /usr/local/share/gyre-playwright
RUN for tool in /usr/local/cargo/bin/*; do ln -sf "$tool" "/usr/local/bin/$(basename "$tool")"; done
ENV RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH \
    PLAYWRIGHT_BROWSERS_PATH=/usr/local/share/gyre-playwright
RUN npx --yes --package=playwright@1.58.2 playwright install-deps chromium \
    && rm -rf /var/lib/apt/lists/* \
    && cargo --version && pi --version
WORKDIR /sandbox/workspace
USER sandbox
