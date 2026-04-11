#!/usr/bin/env bash
# Architecture lint: detect byte-identical baseline screenshots in visual
# regression test suites.
#
# When visual regression tests capture screenshots with `toHaveScreenshot()`,
# each screenshot baseline should represent a visually distinct state. If
# multiple baselines are byte-identical (same MD5 hash), the tests are
# capturing the same default state under different names — providing zero
# visual regression coverage for the claimed scenarios.
#
# The most common failure mode: a filter/view-query/mode test captures the
# canvas BEFORE the filter/query/mode is actually applied (e.g., because
# a route override was registered too late, a custom event was never
# consumed, or the mode activation was silently skipped). The resulting
# screenshot is the default unfiltered canvas — identical to every other
# test that also failed to apply its scenario.
#
# Detection:
#   Find all .png files in Playwright screenshot baseline directories.
#   Group by MD5 hash. If any group has 3+ files with distinct test
#   names, flag it — that many distinct tests shouldn't produce the
#   same pixel-identical image.
#
# Exempt with: # identical-baseline:ok in a file named
#   identical-baseline-exemptions.txt
#
# See: specs/reviews/task-057.md F1 (8 of 15 baselines byte-identical)
#
# Run by pre-commit and CI.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/identical-baseline-exemptions.txt"

# Playwright stores screenshot baselines in directories matching:
#   *.spec.js-snapshots/  or  *.spec.ts-snapshots/
# under the test directory.
BASELINE_DIRS=""

for dir in web/tests web/e2e web/test-results; do
    if [ -d "$dir" ]; then
        while IFS= read -r snap_dir; do
            BASELINE_DIRS="$BASELINE_DIRS $snap_dir"
        done < <(find "$dir" -type d -name '*-snapshots' 2>/dev/null || true)
    fi
done

# Also check web/src for any inline snapshot dirs
if [ -d "web/src" ]; then
    while IFS= read -r snap_dir; do
        BASELINE_DIRS="$BASELINE_DIRS $snap_dir"
    done < <(find "web/src" -type d -name '*-snapshots' 2>/dev/null || true)
fi

if [ -z "$BASELINE_DIRS" ]; then
    echo "No screenshot baseline directories found — skipping identical baseline check."
    exit 0
fi

# Load exemptions
EXEMPTED_FILES=""
if [ -f "$EXEMPTIONS_FILE" ]; then
    EXEMPTED_FILES=$(grep -v '^\s*#' "$EXEMPTIONS_FILE" | grep -v '^\s*$' || true)
fi

echo "Checking for byte-identical screenshot baselines..."

HITS_FILE=$(mktemp)
HASH_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE" "$HASH_FILE"' EXIT

# Collect MD5 hashes of all .png baselines
for dir in $BASELINE_DIRS; do
    find "$dir" -type f -name '*.png' 2>/dev/null | while IFS= read -r png; do
        # Skip exempted files
        is_exempt=0
        if [ -n "$EXEMPTED_FILES" ]; then
            for exempt in $EXEMPTED_FILES; do
                if echo "$png" | grep -qF "$exempt"; then
                    is_exempt=1
                    break
                fi
            done
        fi
        [ "$is_exempt" -eq 1 ] && continue

        hash=$(md5sum "$png" | cut -d' ' -f1)
        echo "$hash $png"
    done
done > "$HASH_FILE"

if [ ! -s "$HASH_FILE" ]; then
    echo "No screenshot baselines found — skipping."
    exit 0
fi

# Group by hash and find duplicates with 3+ files
VIOLATIONS=0
while IFS= read -r hash; do
    count=$(grep -c "^$hash " "$HASH_FILE")
    if [ "$count" -ge 3 ]; then
        files=$(grep "^$hash " "$HASH_FILE" | awk '{print $2}')
        echo "" >> "$HITS_FILE"
        echo "  Hash $hash shared by $count baselines:" >> "$HITS_FILE"
        echo "$files" | while IFS= read -r f; do
            echo "    $f" >> "$HITS_FILE"
        done
        VIOLATIONS=$((VIOLATIONS + 1))
    fi
done < <(awk '{print $1}' "$HASH_FILE" | sort -u)

if [ -s "$HITS_FILE" ]; then
    echo ""
    echo "IDENTICAL BASELINES — screenshot baselines that are byte-identical:"
    cat "$HITS_FILE"
    echo ""
    echo "  When 3+ visual regression test screenshots are pixel-identical,"
    echo "  the tests are capturing the same default state — not the claimed"
    echo "  scenario. This provides zero visual regression coverage."
    echo ""
    echo "  Common causes:"
    echo "    - Route override registered AFTER page navigation (override never fires)"
    echo "    - Custom event dispatched but nothing listens for it"
    echo "    - Mode/filter activation silently skipped (conditional guard)"
    echo "    - Saved view provided via API but never loaded/applied by the UI"
    echo ""
    echo "  Fix: Verify each test actually activates its claimed scenario"
    echo "  before capturing the screenshot. Use Playwright's toHaveScreenshot()"
    echo "  ONLY after confirming the UI reflects the expected state."
    echo ""
    echo "  See: specs/reviews/task-057.md F1"
    echo ""
    echo "${VIOLATIONS} group(s) of identical baselines found."
    exit 1
fi

echo "Identical baseline check passed."
exit 0
