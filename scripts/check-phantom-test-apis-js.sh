#!/usr/bin/env bash
# Architecture lint: detect phantom test APIs — custom events dispatched
# or window globals set by test code that no production code consumes.
#
# When a test interacts with an application by dispatching a custom event
# (e.g., `window.dispatchEvent(new CustomEvent('explorer-apply-query'))`)
# or setting a window global (e.g., `window.__testViewQuery = ...`), the
# application must actually listen for that event or read that global.
# If nothing in the production code handles it, the test's "interaction"
# is a no-op — the test captures the unmodified default state and passes
# by comparing that state against itself.
#
# This script also detects test helper functions defined in E2E/spec test
# files that are never called — dead test helpers indicate abandoned
# interaction approaches (e.g., the agent wrote applyViewQuery() but
# never called it, using a broken custom event dispatch instead).
#
# Detection:
#   1. Find custom event names dispatched in test files via
#      `new CustomEvent('event-name', ...)`.
#   2. Search production code (web/src/) for addEventListener or
#      on:event-name for that event name.
#   3. If zero production listeners exist → phantom event.
#
#   4. Find window.__xxx assignments in test files.
#   5. Search production code for window.__xxx reads.
#   6. If zero production reads exist → phantom global.
#
#   7. Find `async function funcName(` or `function funcName(` declarations
#      in test files (outside describe/test/it blocks).
#   8. Search the SAME file for calls to funcName(.
#   9. If zero calls exist → dead test helper.
#
# Exempt with: // phantom-api:ok — <reason>
#
# See: specs/reviews/task-057.md F3 (phantom CustomEvent)
# See: specs/reviews/task-057.md F6 (dead applyViewQuery helper)
#
# Run by pre-commit and CI.

set -uo pipefail

WEB_SRC="web/src"
WEB_TESTS="web/tests"

if [ ! -d "$WEB_SRC" ] && [ ! -d "$WEB_TESTS" ]; then
    echo "Skipping phantom test API check: no web directories found"
    exit 0
fi

echo "Checking for phantom test APIs (events, globals, dead helpers)..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

# Build list of test files
SEARCH_DIRS=""
[ -d "$WEB_SRC" ] && SEARCH_DIRS="$WEB_SRC"
[ -d "$WEB_TESTS" ] && SEARCH_DIRS="$SEARCH_DIRS $WEB_TESTS"

TEST_FILES=$(find $SEARCH_DIRS -type f \( -name '*.test.js' -o -name '*.test.ts' -o -name '*.spec.js' -o -name '*.spec.ts' \) ! -path '*/node_modules/*' 2>/dev/null | sort)

for file in $TEST_FILES; do
    [ -f "$file" ] || continue

    # Skip files with blanket exemption
    if grep -q 'phantom-api:ok' "$file" 2>/dev/null; then
        continue
    fi

    # ── Check 1: Phantom custom events ──────────────────────────────────
    # Use awk to extract CustomEvent names and check them
    awk -v file="$file" -v web_src="$WEB_SRC" '
    /phantom-api:ok/ { next }
    /new CustomEvent\(/ {
        # Extract event name from quotes
        line = $0
        if (match(line, /new CustomEvent\(\s*['\''"]([^'\''"]+)['\''"]/, m)) {
            event_name = m[1]
            printf "EVENT %s %d %s\n", file, NR, event_name
        }
    }
    /window\.__[a-zA-Z]/ {
        line = $0
        if (match(line, /window\.(__[a-zA-Z_][a-zA-Z0-9_]*)/, m)) {
            global_name = m[1]
            printf "GLOBAL %s %d %s\n", file, NR, global_name
        }
    }
    ' "$file" 2>/dev/null | while IFS=' ' read -r type fpath lineno name; do
        [ -z "$name" ] && continue

        if [ "$type" = "EVENT" ]; then
            # Search production code for this event name
            hits=$(grep -r "$name" "$WEB_SRC" --include='*.svelte' --include='*.js' --include='*.ts' \
                   ! -name '*.test.*' ! -name '*.spec.*' 2>/dev/null | wc -l) || hits=0
            if [ "$hits" -eq 0 ]; then
                echo "$fpath:$lineno: phantom custom event '$name' — no production code listens for it" >> "$HITS_FILE"
            fi
        elif [ "$type" = "GLOBAL" ]; then
            hits=$(grep -r "window.$name" "$WEB_SRC" --include='*.svelte' --include='*.js' --include='*.ts' \
                   ! -name '*.test.*' ! -name '*.spec.*' 2>/dev/null | wc -l) || hits=0
            if [ "$hits" -eq 0 ]; then
                echo "$fpath:$lineno: phantom window global 'window.$name' — no production code reads it" >> "$HITS_FILE"
            fi
        fi
    done

    # ── Check 3: Dead test helper functions ──────────────────────────────
    # Find top-level function declarations and check if they're called
    awk -v file="$file" '
    /^(async )?function [a-zA-Z_][a-zA-Z0-9_]*\(/ {
        match($0, /function ([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        if (m[1] != "") {
            printf "FUNC %s %d %s\n", file, NR, m[1]
        }
    }
    ' "$file" 2>/dev/null | while IFS=' ' read -r type fpath lineno func_name; do
        [ -z "$func_name" ] && continue

        # Count calls to this function in the same file (excluding the definition)
        call_count=$(grep -c "$func_name(" "$file" 2>/dev/null) || call_count=0
        # Subtract the definition line itself (at least 1 occurrence)
        if [ "$call_count" -le 1 ]; then
            echo "$fpath:$lineno: dead test helper function '$func_name' — defined but never called" >> "$HITS_FILE"
        fi
    done
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(wc -l < "$HITS_FILE")
    echo ""
    echo "PHANTOM TEST APIs — test code using APIs that production code doesn't consume:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  A phantom test API is a mechanism (custom event, window global,"
    echo "  or helper function) that a test uses to interact with or configure"
    echo "  the application, but which no production code handles."
    echo ""
    echo "  Common causes:"
    echo "    - Agent invented a custom event (e.g., 'explorer-apply-query')"
    echo "      that no Svelte component listens for"
    echo "    - Agent set a window global (e.g., window.__testViewQuery) that"
    echo "      no production code reads"
    echo "    - Agent defined a helper function (e.g., applyViewQuery) but"
    echo "      never called it, using a broken mechanism instead"
    echo ""
    echo "  Fix: Use the application's actual API to trigger the behavior:"
    echo "    - For saved views: provide via route intercept BEFORE navigation,"
    echo "      then click the view in the UI to load it"
    echo "    - For filters: click the actual filter buttons in the UI"
    echo "    - For mode activation: use the actual UI controls"
    echo "    - Remove dead helper functions that are never called"
    echo ""
    echo "  Exempt with: // phantom-api:ok — <reason>"
    echo ""
    echo "  See: specs/reviews/task-057.md F3, F6"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "Phantom test API check passed."
exit 0
