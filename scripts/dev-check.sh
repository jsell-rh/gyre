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
# OpenShell cannot accept loopback sockets, including those used by library
# tests. Clippy above compiles every Rust target; the controller executes the
# full test suite on the exact merge SHA on the host before promotion.
(cd web && timeout --signal=INT --kill-after=30s "${GYRE_DEV_GATE_TIMEOUT:-1800}" \
  bash -c 'npm ci && npm run build')
git restore --worktree -- web/dist
git clean -fd -- web/dist
