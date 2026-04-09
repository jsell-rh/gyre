#!/usr/bin/env bash
# Architecture lint: detect push handlers that only process the tip commit.
#
# When a push delivers multiple commits (e.g., 5 commits pushed at once),
# push-time detection logic must process ALL pushed commits — not just the
# tip. Using `git log -1` limits processing to the single newest commit,
# silently missing breaking change markers, conventional commit tags, or
# other patterns in interior commits.
#
# The correct pattern uses the full push range:
#   git log --format=... OLD_SHA..NEW_SHA
#
# Other push-time functions in the same file (e.g., commits_since) already
# use this range pattern. The -1 flag contradicts the spec's "on push"
# language, which refers to all pushed commits (plural).
#
# Detection:
#   Find .arg("-1") in Rust command builder patterns near "log" args,
#   within files containing push-related function names.
#   Also detects inline patterns like `git log -1`.
#
# See: specs/reviews/task-020.md R4 F1
#
# Run by pre-commit and CI.

set -euo pipefail

CRATE_SRC="crates"
VIOLATIONS=0

if [ ! -d "$CRATE_SRC" ]; then
    echo "Skipping push commit range check: $CRATE_SRC not found"
    exit 0
fi

echo "Checking for tip-only commit processing in push handlers..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

# Push-related function name patterns (case-insensitive)
PUSH_FN_PATTERNS=(
    'detect_breaking'
    'on_push'
    'push_handler'
    'post_receive'
    'changes_on_push'
    'on_receive'
)

for file in $(find "$CRATE_SRC" -name '*.rs' -print 2>/dev/null | sort); do
    [ -f "$file" ] || continue

    # Skip test-only files — tests may legitimately use git log -1
    case "$file" in
        */tests.rs|*/tests/*.rs|*_test.rs) continue ;;
    esac

    # Check if file has push-related function names
    has_push_fn=false
    for pattern in "${PUSH_FN_PATTERNS[@]}"; do
        if grep -qi "$pattern" "$file" 2>/dev/null; then
            has_push_fn=true
            break
        fi
    done

    [ "$has_push_fn" = true ] || continue

    # Check 1: Inline pattern — `git log -1` or `git log --max-count=1`
    while IFS= read -r line_info; do
        line_num=$(echo "$line_info" | cut -d: -f1)
        line_content=$(echo "$line_info" | cut -d: -f2-)

        if grep -q 'push-range:ok' <<< "$line_content" 2>/dev/null; then
            continue
        fi

        echo "${file}:${line_num}: git log with -1/--max-count=1 in push handler — only processes the tip commit" >> "$HITS_FILE"
    done < <(grep -nE 'git.*log.*(-1|--max-count[= ]1)' "$file" 2>/dev/null || true)

    # Check 2: Command builder pattern — .arg("log") ... .arg("-1")
    # Detect .arg("-1") or .arg("--max-count=1") near .arg("log") in the same
    # function block. We look for .arg("-1") lines and check if a nearby
    # .arg("log") exists (within 15 lines above).
    while IFS= read -r line_info; do
        line_num=$(echo "$line_info" | cut -d: -f1)
        line_content=$(echo "$line_info" | cut -d: -f2-)

        if grep -q 'push-range:ok' <<< "$line_content" 2>/dev/null; then
            continue
        fi

        # Look for .arg("log") within 15 lines above
        start_line=$((line_num > 15 ? line_num - 15 : 1))
        if sed -n "${start_line},${line_num}p" "$file" 2>/dev/null | grep -qE '\.arg[s]?\(.*"log"'; then
            echo "${file}:${line_num}: .arg(\"-1\") in git log command builder — only processes the tip commit, misses interior commits in multi-commit pushes" >> "$HITS_FILE"
        fi
    done < <(grep -nE '\.arg[s]?\([^)]*"-1"' "$file" 2>/dev/null | grep -v 'push-range:ok' || true)

done

# De-duplicate hits (a line might be caught by both checks)
if [ -s "$HITS_FILE" ]; then
    sort -u "$HITS_FILE" -o "$HITS_FILE"
    VIOLATIONS=$(wc -l < "$HITS_FILE")
    echo ""
    echo "PUSH HANDLER TIP-ONLY DETECTION — git log -1 misses interior commits:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  Push handlers must process ALL pushed commits, not just the tip."
    echo "  A multi-commit push (e.g., 5 commits) with a breaking change"
    echo "  in commit #2 will be silently missed by git log -1."
    echo ""
    echo "  Fix: Replace 'git log -1 ... NEW_SHA' with"
    echo "       'git log ... OLD_SHA..NEW_SHA'"
    echo "  Use the same old_sha..new_sha range pattern as other push-time"
    echo "  functions in the same file (e.g., commits_since)."
    echo ""
    echo "  Handle new-branch case (old_sha = 0000...): use just NEW_SHA"
    echo "  without range when old_sha is all zeros."
    echo ""
    echo "  Exempt with: // push-range:ok — <reason>"
    echo ""
    echo "  See: specs/reviews/task-020.md R4 F1"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "Push commit range check passed."
exit 0
