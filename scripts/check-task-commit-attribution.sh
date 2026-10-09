#!/usr/bin/env bash
# check-task-commit-attribution.sh — Every task-labeled product-surface
# commit must appear in its task's `commits:` frontmatter.
#
# Procedural background (specs/reviews/task-095.md R3-F4): commit 5aaded21
# (+880 lines: circuit breaker, REST recovery surface, CLI) landed on main
# labeled task-095 but was absent from task-095's `commits:` frontmatter.
# The verifier scopes each review round to the task's frontmatter commit
# list — a labeled surface commit missing there is invisible to review
# scoping, and stale scope notes compound ("not implemented on main" was
# re-checked only by luck). Pre-existing drift of the same shape exists for
# 10 more commits; they are seeded in the exemption file below as legacy
# debt and should never grow.
#
# What this check enforces, for every non-merge commit in history whose
# subject mentions `task-NNN` (any of `task-095`, `(task-097+095+102)`,
# `task-095 review`, ...):
#   1. The commit touches product surface (crates/, web/src, web/tests) —
#      docs/review/process-only commits are out of scope (their own
#      subjects label them).
#   2. The subject is not a review/process round-trip itself
#      (`review:`/`process:` conventional type).
#   3. Every referenced task-NNN either has a task file whose `commits:`
#      frontmatter contains the commit's short SHA, or is listed in
#      scripts/task-commit-attribution-exemptions.txt.
#
# The exemption file is FROZEN at the baseline count below — the same rule
# as abac-route-registry: legacy drift recorded, never extended. Fix drift
# by adding the SHA to the task's frontmatter (and removing the exemption
# line), not by growing the file.
FROZEN_EXEMPTION_COUNT=3
#
# Run by CI (needs full git history: checkout with fetch-depth: 0) and
# pre-commit.
#
# Usage: bash scripts/check-task-commit-attribution.sh

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/task-commit-attribution-exemptions.txt"
FAIL=0

if ! command -v git >/dev/null 2>&1; then
    echo "SKIP: git not available."
    exit 0
fi

# ── Collect exemptions: "<short-sha> <task-number>" per line ────────────
declare -A EXEMPT
EXEMPT_TOTAL=0
if [ -f "$EXEMPTIONS_FILE" ]; then
    while read -r sha task; do
        case "$sha" in ''|'#'*) continue ;; esac
        task="${task#task-}"   # normalize: entry may say "task-095" or "095"
        [ -n "$task" ] || continue
        EXEMPT["${sha} ${task}"]=1
        EXEMPT_TOTAL=$((EXEMPT_TOTAL + 1))
    done < "$EXEMPTIONS_FILE"
fi

# Freeze enforcement: the exemption file must never grow.
if [ "$EXEMPT_TOTAL" -gt "$FROZEN_EXEMPTION_COUNT" ]; then
    echo "FAIL: task-commit-attribution-exemptions.txt grew to $EXEMPT_TOTAL entries (baseline: $FROZEN_EXEMPTION_COUNT)."
    echo ""
    echo "The exemption file is legacy drift (task-095 R3-F4), not an approval"
    echo "mechanism. A task-labeled surface commit missing from its task's"
    echo "frontmatter must be RECORDED in the task file's commits: list,"
    echo "never exempted. If you resolved a drift entry, remove the line and"
    echo "lower FROZEN_EXEMPTION_COUNT; never raise it."
    exit 1
fi


# ── Scan history ─────────────────────────────────────────────────────────
VIOLATIONS=""

# ── Frontmatter commit list for a task (short SHAs, lowercase) ──────────
# Handles both forms seen in specs/tasks/: inline arrays
#   commits: ["abc12345", ...]          (possibly wrapped across lines)
# and YAML lists:
#   commits:
#     - abc12345
frontmatter_commits() {
    local task_file="$1"
    [ -f "$task_file" ] || return 1
    awk '
        /^commits:/ {
            if ($0 ~ /\[/) { inline = $0; inarray = ($0 !~ /\]/); next }
            inlist = 1; next
        }
        inarray { inline = inline $0; if ($0 ~ /\]/) inarray = 0; next }
        inlist && /^[\t ]*-[\t ]/ { print; next }
        inlist { inlist = 0 }
        END { if (inline) print inline }
    ' "$task_file" | grep -oE '[0-9a-f]{7,40}' | cut -c1-8
}

while IFS='|' read -r sha subject; do
    [ -n "$sha" ] || continue

    # Skip the check's own bookkeeping rounds.
    case "$subject" in
        review:*|process:*) continue ;;
    esac

    # Task labels: task-NNN, possibly several (task-097+095+102).
    labels=$(echo "$subject" | grep -oE 'task-[0-9]{3}([+][0-9]{3})*' | tr '+' '\n' | sed 's/task-//' | sort -u)
    [ -n "$labels" ] || continue

    # Product surface only: docs/specs-only commits are out of scope —
    # this check is about review SCOPING of code surface.
    files=$(git show --name-only --format= "$sha" 2>/dev/null)
    echo "$files" | grep -qE '^(crates/|web/src|web/tests)' || continue

    short=$(git rev-parse --short=8 "$sha")

    for task in $labels; do
        # Exempt?
        if [ -n "${EXEMPT["$short $task"]:-}" ]; then continue; fi

        task_file="$SCRIPT_DIR/../specs/tasks/task-$task.md"
        if ! frontmatter_commits "$task_file" 2>/dev/null | grep -q "^$short$"; then
            if [ ! -f "$task_file" ]; then
                VIOLATIONS="${VIOLATIONS}  $short  task-$task (NO TASK FILE — label references a nonexistent task)
"
            else
                VIOLATIONS="${VIOLATIONS}  $short  task-$task  $subject
"
            fi
        fi
    done
done < <(git log --no-merges --format='%h|%s' 2>/dev/null)

if [ -n "$VIOLATIONS" ]; then
    echo "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:"
    echo ""
    echo "$VIOLATIONS"
    echo "A task-labeled commit absent from the task's commits: list is invisible"
    echo "to review scoping — the verifier scopes each round to that list"
    echo "(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by"
    echo "adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter."
    echo "Do NOT add entries to $EXEMPTIONS_FILE."
    FAIL=1
fi

if [ "$FAIL" -ne 0 ]; then exit 1; fi

echo "OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift)."
