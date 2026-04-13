#!/usr/bin/env bash
# Architecture lint: detect pipeline data flow truncation.
#
# When code executes an external command with a format string and then parses
# the output for specific patterns, the format string must include the data
# those patterns appear in. The most common failure mode:
#
#   git log --format="%H %s"   ← %s = subject line only
#   ...
#   message.contains("BREAKING CHANGE:")   ← footer appears in the BODY, not subject
#
# The detection path is dead code — it can never match because %s discards
# the commit body where BREAKING CHANGE: footers appear.
#
# git format placeholders:
#   %s = subject only (first line)
#   %b = body only (after first blank line)
#   %B = full message (subject + body)
#   %s will NEVER contain: BREAKING CHANGE: footers, Signed-off-by: trailers,
#   or any multi-line content.
#
# See: specs/reviews/task-020.md F2
#
# Run by pre-commit and CI.

set -euo pipefail

CRATE_SRC="crates"
VIOLATIONS=0

if [ ! -d "$CRATE_SRC" ]; then
    echo "Skipping pipeline data flow check: $CRATE_SRC not found"
    exit 0
fi

echo "Checking for pipeline data flow truncation..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

# --- Check 1: git log %s format with body-pattern parsing ---
#
# Find files that use git log --format with %s (subject-only) AND also
# search for patterns that appear only in the commit body.
#
# Body-only patterns:
#   BREAKING CHANGE:     — conventional commit footer
#   Signed-off-by:       — git trailer
#   Co-authored-by:      — git trailer
#   Reviewed-by:         — git trailer
#   BREAKING-CHANGE:     — alternate conventional commit footer

for file in $(find "$CRATE_SRC" -name '*.rs' -print 2>/dev/null | sort); do
    [ -f "$file" ] || continue

    # Check if file contains a git log format with %s (but not %B or %b)
    # and also contains a search for body-only patterns
    has_subject_only_format=false
    has_body_pattern_search=false

    # Look for git log format strings that use %s but NOT %B or %b
    # Common patterns:
    #   --format="%H %s"
    #   --format='%H %s'
    #   --pretty=format:"%H %s"
    if grep -qE '(--format=|--pretty=format:).*%s' "$file" 2>/dev/null; then
        # Verify the format does NOT also include %B or %b (which would include the body)
        if ! grep -E '(--format=|--pretty=format:).*%s' "$file" 2>/dev/null | grep -qE '%[Bb]'; then
            has_subject_only_format=true
        fi
    fi

    if [ "$has_subject_only_format" = false ]; then
        continue
    fi

    # Check if the same file searches for body-only patterns
    # These patterns only appear in the commit body, never in the subject
    body_patterns=(
        'BREAKING CHANGE:'
        'BREAKING-CHANGE:'
        'Signed-off-by:'
        'Co-authored-by:'
        'Co-Authored-By:'
        'Reviewed-by:'
        'Acked-by:'
        'Tested-by:'
    )

    for pattern in "${body_patterns[@]}"; do
        if grep -qF "\"$pattern\"" "$file" 2>/dev/null || \
           grep -qF "'$pattern'" "$file" 2>/dev/null; then
            has_body_pattern_search=true
            # Find the line numbers
            format_line=$(grep -nE '(--format=|--pretty=format:).*%s' "$file" 2>/dev/null | head -1 | cut -d: -f1)
            pattern_line=$(grep -nF "$pattern" "$file" 2>/dev/null | head -1 | cut -d: -f1)
            echo "${file}:${format_line}: git log format uses %s (subject-only) but ${file}:${pattern_line} searches for '${pattern}' which appears only in the commit body" >> "$HITS_FILE"
        fi
    done

    # Also check for pipeline-data-flow:ok exemption
    if [ "$has_body_pattern_search" = true ] && grep -q 'pipeline-data-flow:ok' "$file" 2>/dev/null; then
        # Remove hits for this file (exempted)
        sed -i "\|^${file}:|d" "$HITS_FILE" 2>/dev/null || true
    fi
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(wc -l < "$HITS_FILE")
    echo ""
    echo "PIPELINE DATA FLOW TRUNCATION — format string discards data the parser needs:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  The git log format uses %s which outputs only the commit"
    echo "  subject line. Patterns like BREAKING CHANGE: appear in the"
    echo "  commit BODY, which %s discards. The detection path is dead"
    echo "  code — it can never match."
    echo ""
    echo "  Fix: Use %B (full message = subject + body) instead of %s"
    echo "  to include the commit body in the output."
    echo ""
    echo "  git format placeholders:"
    echo "    %s = subject only"
    echo "    %b = body only"
    echo "    %B = full message (subject + body)"
    echo ""
    echo "  Exempt with: // pipeline-data-flow:ok — <reason>"
    echo ""
    echo "  See: specs/reviews/task-020.md F2"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "Pipeline data flow check passed."
exit 0
