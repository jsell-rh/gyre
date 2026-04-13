#!/usr/bin/env bash
# Architecture lint: detect Playwright route overrides registered after
# page navigation within the same test function.
#
# Playwright's `page.route()` only intercepts requests that occur AFTER
# the route handler is registered. If a test navigates to a page (via
# `page.goto()` or a navigation helper like `navigateToExplorer(page)`)
# and then registers a route override, the override never fires — the
# page already loaded and the API calls already completed.
#
# Detection:
#   Within each `test(` block in Playwright spec files:
#   1. Track when `page.goto(`, `.goto(`, or a known navigation helper
#      (navigateToExplorer, navigateTo) is called.
#   2. After navigation, check for `page.route(` calls.
#   3. If a route override appears after navigation → violation.
#
# Note: route overrides in `beforeEach` blocks before navigation are fine.
# Route overrides with `{ times: 1 }` after navigation are especially
# problematic — they're clearly intended to intercept a specific request
# that already happened.
#
# Exempt with: // route-lifecycle:ok — <reason>
#
# See: specs/reviews/task-057.md F2 (route override after navigation)
#
# Run by pre-commit and CI.

set -euo pipefail

WEB_TESTS="web/tests"
VIOLATIONS=0

if [ ! -d "$WEB_TESTS" ]; then
    echo "Skipping Playwright route lifecycle check: $WEB_TESTS not found"
    exit 0
fi

echo "Checking for Playwright route overrides registered after navigation..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

# Find Playwright spec files
for file in $(find "$WEB_TESTS" -type f \( -name '*.spec.js' -o -name '*.spec.ts' \) \
    ! -path '*/node_modules/*' \
    | sort); do
    [ -f "$file" ] || continue

    # Use awk to detect route overrides after navigation within test blocks.
    awk -v file="$file" '
    # Skip exempted lines
    /route-lifecycle:ok/ { next }

    # Detect test block start: test( or test.only(
    /^\s*test(\.(only|skip))?\s*\(/ {
        in_test = 1
        brace_depth = 0
        has_navigated = 0
        test_start = NR

        # Count braces
        n = length($0)
        for (i = 1; i <= n; i++) {
            c = substr($0, i, 1)
            if (c == "{") brace_depth++
            if (c == "}") brace_depth--
        }
        next
    }

    in_test {
        # Track brace depth
        n = length($0)
        for (i = 1; i <= n; i++) {
            c = substr($0, i, 1)
            if (c == "{") brace_depth++
            if (c == "}") brace_depth--
        }

        # Detect navigation calls
        if ($0 ~ /\.goto\(/ || $0 ~ /navigateToExplorer\(/ || $0 ~ /navigateTo\(/) {
            has_navigated = 1
        }

        # Detect route override after navigation
        if (has_navigated && $0 ~ /page\.route\(/) {
            printf "%s:%d: page.route() registered after navigation — override will never fire\n  %s\n", file, NR, $0
        }

        # End of test block
        if (brace_depth <= 0) {
            in_test = 0
        }
    }
    ' "$file" 2>/dev/null >> "$HITS_FILE"
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(grep -c "^[^ ]" "$HITS_FILE" || true)
    echo ""
    echo "PLAYWRIGHT ROUTE LIFECYCLE — route overrides registered after navigation:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  Playwright page.route() only intercepts requests that occur AFTER"
    echo "  the handler is registered. If the test navigates first (page.goto,"
    echo "  navigateToExplorer), the page load triggers API calls that the"
    echo "  late-registered route cannot intercept."
    echo ""
    echo "  Fix: Register ALL route overrides BEFORE navigation:"
    echo ""
    echo "    // WRONG — override registered after page load"
    echo "    await navigateToExplorer(page);"
    echo "    await page.route('**/api/v1/views', ...);"
    echo ""
    echo "    // CORRECT — override registered before navigation"
    echo "    await page.route('**/api/v1/views', ...);"
    echo "    await navigateToExplorer(page);"
    echo ""
    echo "  For test-specific overrides that differ from beforeEach, register"
    echo "  them before navigation OR trigger a re-fetch after registration"
    echo "  (e.g., by clicking a reload button or navigating again)."
    echo ""
    echo "  Exempt with: // route-lifecycle:ok — <reason>"
    echo ""
    echo "  See: specs/reviews/task-057.md F2"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "Playwright route lifecycle check passed."
exit 0
