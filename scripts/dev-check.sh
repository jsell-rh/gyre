#!/usr/bin/env bash
# Deterministic integration gates. Keep mechanics out of agent prompts.
set -euo pipefail
cd /tmp/gyre
git diff --check HEAD^1 HEAD
python3 /tmp/stage/check-rustfmt-diff.py HEAD^1
timeout --signal=INT --kill-after=30s "${GYRE_DEV_GATE_TIMEOUT:-1800}" \
  python3 /tmp/stage/check-clippy-diff.py HEAD^1
bash scripts/check-arch.sh
GYRE_CHECK_HIERARCHY=1 bash scripts/check-hierarchy.sh
for check in \
  check-abac-route-registry \
  check-abac-exempt-handlers \
  check-mcp-write-tools \
  check-migration-versions \
  check-migration-sql-portability \
  check-dead-message-kinds \
  check-byte-slice-truncation \
  check-relative-path-defaults \
  check-fail-open-ref-resolution \
  check-task-commit-attribution \
  check-mem-port-contracts \
  check-fabricated-scope-defaults \
  check-lossy-secret-conversion \
  check-scope-literal-defaults \
  check-inert-enforcement \
  check-forged-scope-fields \
  check-forwarded-header-trust \
  check-in-memory-state-stores \
  check-unbounded-external-http; do
  bash "scripts/$check.sh"
done
# OpenShell cannot accept loopback sockets. Its integration binaries need a
# regular host; the controller runs the full suite on the exact merge SHA
# before promotion. Compile all targets in Clippy above, and execute the
# library and binary tests here.
timeout --signal=INT --kill-after=30s "${GYRE_DEV_GATE_TIMEOUT:-1800}" \
  env SKIP_WEB_BUILD=1 cargo test --all --lib --bins
(cd web && timeout --signal=INT --kill-after=30s "${GYRE_DEV_GATE_TIMEOUT:-1800}" \
  bash -c 'npm ci && npm run build && npm test')
git restore --worktree -- web/dist
git clean -fd -- web/dist
