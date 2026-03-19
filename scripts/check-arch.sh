#!/usr/bin/env bash
# Architecture lint: enforce hexagonal boundaries.
#
# Invariant: gyre-domain MUST NOT import gyre-adapters or any infrastructure crate.
# Domain depends only on gyre-ports and gyre-common.
#
# This script scans gyre-domain's Cargo.toml and source for forbidden dependencies.
# Run by pre-commit and CI. On failure, the message includes remediation instructions.

set -euo pipefail

DOMAIN_CARGO="crates/gyre-domain/Cargo.toml"
DOMAIN_SRC="crates/gyre-domain/src"

FORBIDDEN_DEPS=(
    "gyre-adapters"
    "rusqlite"
    "sqlx"
    "reqwest"
    "hyper"
    "axum"
    "tonic"
)

FAIL=0

# Check Cargo.toml for forbidden dependencies
for dep in "${FORBIDDEN_DEPS[@]}"; do
    if grep -q "\"${dep}\"" "$DOMAIN_CARGO" 2>/dev/null; then
        echo "ARCH VIOLATION: gyre-domain/Cargo.toml references forbidden dependency '${dep}'"
        echo "  Remediation: Move infrastructure code to gyre-adapters."
        echo "  See: specs/development/architecture.md - Hexagonal Architecture Invariants"
        FAIL=1
    fi
done

# Check source files for use/extern crate of forbidden modules
for dep in "${FORBIDDEN_DEPS[@]}"; do
    # Convert crate name to module name (hyphens -> underscores)
    mod="${dep//-/_}"
    if grep -rq "use ${mod}" "$DOMAIN_SRC" 2>/dev/null || grep -rq "extern crate ${mod}" "$DOMAIN_SRC" 2>/dev/null; then
        echo "ARCH VIOLATION: gyre-domain source imports forbidden module '${mod}'"
        echo "  Remediation: Move infrastructure code to gyre-adapters."
        echo "  See: specs/development/architecture.md - Hexagonal Architecture Invariants"
        FAIL=1
    fi
done

if [ "$FAIL" -eq 0 ]; then
    echo "Architecture lint passed: gyre-domain has no forbidden dependencies."
fi

exit "$FAIL"
