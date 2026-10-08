#!/usr/bin/env bash
# Deterministic integration gates. Keep mechanics out of agent prompts.
set -euo pipefail
cd /tmp/gyre
git diff --check HEAD^1 HEAD
BASE=$(git rev-parse HEAD^1)
gate() { python3 /tmp/stage/dev-static-gate.py "$BASE" "$@"; }
gate python3 /tmp/stage/check-rustfmt-diff.py "$BASE"
gate timeout --signal=INT --kill-after=30s "${GYRE_DEV_GATE_TIMEOUT:-1800}" \
  python3 /tmp/stage/check-clippy-diff.py "$BASE"
# The candidate must not grant itself new exemptions, including replacement
# entries that leave the count unchanged.
python3 - <<'PY'
import subprocess
def git(*args):
    return subprocess.check_output(['git', *args], text=True)
def entries(ref, path):
    result = subprocess.run(['git', 'show', f'{ref}:{path}'], capture_output=True, text=True)
    if result.returncode:
        return set()
    return {line.strip() for line in result.stdout.splitlines()
            if line.strip() and not line.lstrip().startswith('#')}
paths = set()
for ref in ('HEAD^1', 'HEAD'):
    paths.update(path for path in git('ls-tree', '-r', '--name-only', ref, 'scripts').splitlines()
                 if path.endswith('-exemptions.txt'))
for path in sorted(paths):
    added = entries('HEAD', path) - entries('HEAD^1', path)
    if added:
        raise SystemExit(f'new verification exemptions forbidden: {path}: {sorted(added)}')
PY
static_gates() {
  gate bash scripts/check-arch.sh
  GYRE_CHECK_HIERARCHY=1 gate bash scripts/check-hierarchy.sh
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
    gate bash "scripts/$check.sh"
  done
}
# Run the upstream verifier against the candidate code first. Run the
# candidate verifier too, so legitimate strengthening is checked. Restoring
# scripts at their original paths preserves checks that derive the repo root
# from their own location; the reviewer sees the original merge tree.
restore_scripts() { git restore --source=HEAD --worktree -- scripts; }
trap restore_scripts EXIT
git restore --source=HEAD^1 --worktree -- scripts
static_gates
restore_scripts
trap - EXIT
static_gates
# OpenShell cannot accept loopback sockets, including those used by library
# tests. Clippy above compiles every Rust target; the controller executes the
# full test suite on the exact merge SHA on the host before promotion.
(cd web && timeout --signal=INT --kill-after=30s "${GYRE_DEV_GATE_TIMEOUT:-1800}" \
  bash -c 'npm ci && npm run build')
git restore --worktree -- web/dist
git clean -fd -- web/dist
