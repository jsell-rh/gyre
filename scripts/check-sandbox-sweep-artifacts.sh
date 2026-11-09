#!/usr/bin/env bash
# check-sandbox-sweep-artifacts.sh — generated npm artifacts must never be
# swept into task-labeled product commits by the checkpoint machinery.
#
# Procedural background (specs/reviews/task-092.md, two rejected
# integrations in a row): review/verifier tooling invoked `npm` from the
# repo root. The repo root has no package.json, so npm wrote an empty
# lockfile stub `/package-lock.json` (`{"name":"gyre","packages":{}}`).
# The wip sweep in scripts/dev-remote.sh ("preserve sandbox attempt")
# runs `git add -A`, which committed that generated stub as a tracked
# file inside a task-labeled commit. The integration review then
# rejected the round twice with "review cannot approve its own source
# edits — package-lock.json", because the file looked like the review's
# own tooling mutating the source tree under review.
#
# The stub is now ignored via the anchored rule `/package-lock.json` in
# the root .gitignore, so `git add -A` in the sweep never captures it.
#
# Detection (this script):
#   1. A repo-root `package-lock.json` must be either absent or ignored.
#      If it exists AND is tracked, the sweep will re-commit it — the
#      exact double-rejection failure mode. (Ignored-and-tracked is
#      impossible for a never-committed file, but `git add -f` or a
#      stale branch could resurrect it, so check tracked state.)
#   2. The anchor must stay anchored: an UNanchored `package-lock.json`
#      rule in the root .gitignore would also ignore
#      web/package-lock.json, scripts/package-lock.json, and
#      docker/gyre-agent/package-lock.json — all tracked, all
#      dependency-pinning surface. A future editor "simplifying" the
#      pattern would silently untrack three lockfiles on their next
#      regeneration.
#
# Remediation: keep the anchored `/package-lock.json` rule in the root
# .gitignore and never track a root-level package-lock.json. Do not
# broaden the pattern.
#
# Run by pre-commit and CI.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

FAIL=0

# --- Check 1: root package-lock.json must not be tracked ---
if git ls-files --error-unmatch package-lock.json >/dev/null 2>&1; then
    echo "FAIL: a repo-root package-lock.json is TRACKED."
    echo ""
    echo "The repo root has no package.json; a root lockfile is always a"
    echo "generated npm stub (review/verifier tooling running npm from"
    echo "the repo cwd). Once tracked, the dev-remote.sh wip sweep"
    echo "('preserve sandbox attempt', git add -A) preserves it in every"
    echo "task-labeled commit, and integration review rejects the round"
    echo "with 'review cannot approve its own source edits' (task-092:"
    echo "two consecutive rejections)."
    echo ""
    echo "Fix: git rm --cached package-lock.json && rm -f package-lock.json"
    echo "(the anchored /package-lock.json .gitignore rule keeps it out)."
    FAIL=1
fi

# --- Check 2: the ignore rule must stay root-anchored ---
# Any rule line whose pattern is exactly `package-lock.json` (no leading
# slash, no deeper path) applies at every directory level and would
# ignore the tracked web/, scripts/, and docker/gyre-agent/ lockfiles.
while IFS= read -r rule; do
    [ -n "$rule" ] || continue
    echo "FAIL: unanchored gitignore rule '$rule' in the root .gitignore."
    echo ""
    echo "An unanchored 'package-lock.json' pattern matches at every"
    echo "directory level, so it also ignores web/package-lock.json,"
    echo "scripts/package-lock.json, and"
    echo "docker/gyre-agent/package-lock.json — tracked lockfiles that"
    echo "pin real dependencies. The next regeneration would leave them"
    echo "untracked and unpinnable by review."
    echo ""
    echo "Fix: anchor it to the repo root — '/package-lock.json'."
    FAIL=1
done < <(grep -n '^[[:space:]]*package-lock\.json[[:space:]]*$' .gitignore | sed 's/^\([0-9]*\):[[:space:]]*/\1:/' | grep -v '^[0-9]*:/')

# --- Check 3: if a root stub exists, it must be ignored ---
if [ -f package-lock.json ] && ! git check-ignore -q package-lock.json; then
    echo "FAIL: repo-root package-lock.json exists but is NOT ignored."
    echo ""
    echo "It is a generated npm stub (no root package.json exists); the"
    echo "dev-remote.sh wip sweep (git add -A) will commit it into the"
    echo "next task-labeled commit and the integration review will"
    echo "reject the round (task-092 double rejection)."
    echo ""
    echo "Fix: add the anchored '/package-lock.json' rule to .gitignore."
    FAIL=1
fi

if [ "$FAIL" -ne 0 ]; then
    exit 1
fi

echo "OK: no trackable npm-generated root lockfile; ignore rule is anchored."
