# Build with the OpenShell worker context (omp and omp-home supplied separately).
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git openssh-client xz-utils build-essential pkg-config \
      libssl-dev sqlite3 jq ripgrep fd-find tmux libpq-dev lld \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
    && ln -s /usr/local/cargo/bin/* /usr/local/bin/
ENV PATH=/usr/local/cargo/bin:$PATH
RUN rustup component add rustfmt clippy

# Match the repository's Playwright lockfile. Focused browser probes must not
# depend on runtime downloads or writes to the sandbox's read-only system.
ENV PLAYWRIGHT_BROWSERS_PATH=/usr/local/share/gyre-playwright
RUN npx --yes --package=playwright@1.58.2 playwright install --with-deps chromium \
    && npm cache clean --force \
    && rm -rf /var/lib/apt/lists/*

# Portable, pinned jj for source-control code and integration tests.
ARG JJ_VERSION=0.39.0
ARG JJ_SHA256=8da8d96e9c8696c21ad47847a63d533e249acb0449d9af0f0562b5ea7b024f04
RUN curl -fsSL "https://github.com/jj-vcs/jj/releases/download/v${JJ_VERSION}/jj-v${JJ_VERSION}-x86_64-unknown-linux-musl.tar.gz" \
      -o /tmp/jj.tar.gz \
    && echo "${JJ_SHA256}  /tmp/jj.tar.gz" | sha256sum -c - \
    && tar -xzf /tmp/jj.tar.gz -C /usr/local/bin ./jj \
    && rm /tmp/jj.tar.gz \
    && jj --version

COPY omp /usr/local/bin/omp
RUN chmod +x /usr/local/bin/omp
COPY omp-home /opt/omp-home
ENV OMP_HOME=/opt/omp-home

WORKDIR /workspace
CMD ["bash"]
