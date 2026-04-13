#!/usr/bin/env bash
# Architecture lint: detect self-contradicting test assertions in JS/TS.
#
# A self-contradicting test contains comments expressing doubt about the
# asserted behavior — phrases like "arguably shouldn't", "technically
# incorrect", "doesn't match spec", or "not ideal" — while the assertion
# codifies the questioned behavior as expected. The test permanently
# rationalizes a spec-violating bug by making it look intentional.
#
# Detection signals:
#   Comments inside test blocks (it(...), test(...)) containing
#   doubt-expressing phrases, unless exempted with:
#     // spec-deviation:ok — <reason>
#
# See: specs/reviews/task-055.md F5
#
# Run by pre-commit and CI.

set -euo pipefail

WEB_SRC="web/src"
VIOLATIONS=0

if [ ! -d "$WEB_SRC" ]; then
    echo "Skipping self-contradicting test comment check: $WEB_SRC not found"
    exit 0
fi

echo "Checking for self-contradicting test comments in JS/TS test files..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

# Doubt-expressing phrases that signal the developer knows the assertion is wrong
# Each pattern is designed to avoid false positives on neutral technical comments
DOUBT_PHRASES=(
    'arguably should'        # "arguably shouldn't be a backward ghost"
    'technically incorrect'
    'technically wrong'
    "doesn't match spec"
    "doesn't match the spec"
    'does not match spec'
    'does not match the spec'
    'not ideal'
    'not quite right'
    'not quite correct'
    'spec says otherwise'
    'spec disagrees'
    'contradicts the spec'
    'violates the spec'
    'against the spec'
    'debatable whether'
    'even though it should'  # "included even though it shouldn't be"
    'even though it shouldn' # contraction variant
    'so arguably'
    'but arguably'
)

for file in $(find "$WEB_SRC" -name '*.test.js' -o -name '*.test.ts' -o -name '*.spec.js' -o -name '*.spec.ts' 2>/dev/null | sort); do
    [ -f "$file" ] || continue

    # Use awk to find test blocks with doubt comments
    awk -v file="$file" '
    # Match test block openers: it(, test(, describe(
    /^\s*(it|test)\s*\(/ {
        in_test = 1
        test_start = NR
        has_doubt = 0
        has_exempt = 0
        doubt_line = ""
        doubt_text = ""
        # Extract test name from the opening line
        match($0, /['\''"]([^'\''"]*)['\''"]/,  m)
        test_name = m[1]
        brace_depth = 0
        next
    }

    in_test {
        # Track brace depth to know when test block ends
        n = gsub(/{/, "{")
        brace_depth += n
        n = gsub(/}/, "}")
        brace_depth -= n

        # Check for exemption comment
        if ($0 ~ /spec-deviation:ok/) has_exempt = 1

        # Check for doubt-expressing phrases in comments
        if ($0 ~ /^\s*\/\//) {
            line = tolower($0)
            if (line ~ /arguably should/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /technically incorrect/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /technically wrong/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /doesn.t match (the )?spec/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /does not match (the )?spec/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /not quite right/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /not quite correct/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /spec says otherwise/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /spec disagrees/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /contradicts the spec/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /violates the spec/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /against the spec/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /debatable whether/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /even though it should/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /so arguably/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
            if (line ~ /but arguably/) { has_doubt = 1; doubt_line = NR; doubt_text = $0 }
        }

        # Detect end of test block (closing paren+semicolon or just closing at depth 0)
        if (brace_depth <= 0 && ($0 ~ /\}\s*\)\s*;?\s*$/ || $0 ~ /^\s*\}\s*\)\s*;?\s*$/)) {
            if (has_doubt && !has_exempt) {
                gsub(/^[ \t]+/, "", doubt_text)
                printf "%s:%d: test \"%s\" has self-contradicting comment at line %d:\n  %s\n", file, test_start, test_name, doubt_line, doubt_text
            }
            in_test = 0
        }
    }
    ' "$file" 2>/dev/null >> "$HITS_FILE"
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(grep -c ':' "$HITS_FILE" | head -1 || echo "0")
    echo ""
    echo "SELF-CONTRADICTING TEST ASSERTIONS — tests with comments acknowledging incorrect behavior:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  A self-contradicting test contains comments expressing doubt about"
    echo "  the asserted behavior ('arguably shouldn't', 'technically wrong',"
    echo "  'doesn't match spec') while the assertion codifies the questioned"
    echo "  behavior as expected. This rationalizes a spec-violating bug by"
    echo "  making it look intentional."
    echo ""
    echo "  Fix: Change the code so the assertion matches the spec, then"
    echo "  update the assertion and remove the doubt comment."
    echo ""
    echo "  If the deviation is intentional and documented in the spec,"
    echo "  add comment: // spec-deviation:ok — <reason>"
    echo ""
    echo "  See: specs/reviews/task-055.md F5"
    echo ""
    echo "Violation(s) found."
    exit 1
fi

echo "Self-contradicting test comment check passed."
exit 0
