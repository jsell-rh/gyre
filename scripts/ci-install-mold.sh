#!/usr/bin/env bash
# ci-install-mold.sh — best-effort mold install for CI jobs.
#
# mold (wired in via .cargo/config.toml) is a link-performance
# optimization: it speeds up linking and avoids GNU-ld OOM on memory-
# constrained machines. It is not a correctness requirement, and the apt
# fetch is subject to runner network reliability — E2E run 43046986838
# failed the whole job at the "Install mold linker" step with
# `Temporary failure resolving 'azure.archive.ubuntu.com'` before any
# build or test step ran.
#
# This script installs mold when the runner network allows it and,
# otherwise, degrades the job to the default toolchain linker so a
# transient apt/DNS failure cannot fail an otherwise-green run. The
# fallback removes .cargo/config.toml — which contains only the
# mold/clang linker pinning — after which cargo uses cc/GNU ld, the
# same configuration GitHub CI built with before mold was adopted.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if command -v mold >/dev/null 2>&1; then
    echo "mold already present: $(mold --version)"
    exit 0
fi

# Refresh the package index first: a stale or missing index fails the
# install even when the network is otherwise healthy.
sudo apt-get update -qq || true

for attempt in 1 2 3; do
    if sudo apt-get install -y mold && command -v mold >/dev/null 2>&1; then
        echo "mold installed (attempt ${attempt}): $(mold --version)"
        exit 0
    fi
    echo "mold install attempt ${attempt} failed"
    sleep $((attempt * 10))
done

echo "::warning::mold unavailable (apt/network failure); falling back to the default GNU linker"
rm -f "${REPO_ROOT}/.cargo/config.toml"
exit 0
