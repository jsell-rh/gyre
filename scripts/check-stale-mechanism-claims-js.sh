#!/usr/bin/env bash
# Architecture lint: detect stale mechanism claims in JS/TS test comments.
#
# A stale mechanism claim is a test comment that describes a specific
# mechanism, variable, or behavior that is contradicted by the actual
# test code in the same block. The most common failure mode:
#
#   // The blast radius query uses $clicked as scope.node, which makes
#   // ExplorerCanvas store it as an interactive query template.
#   await applyQueryViaEditor(page, BLAST_RADIUS_QUERY);
#
# ...but BLAST_RADIUS_QUERY uses `node: 'fn-spawn-agent'` (a fixed node),
# not `$clicked`. The comment was written for an earlier draft and never
# updated when the query changed. The stale comment misrepresents the
# mechanism, causing future developers to misunderstand the test.
#
# Detection:
#   1. Find comments that assert a template variable ($clicked, etc.)
#      is used as a mechanism, but the variable doesn't appear in code.
#   2. Detect comments claiming "interactive"/"click-activated" behavior
#      in tests where no template variable appears in code.
#
# Exempt with: // mechanism-claim:ok — <reason>
#
# See: specs/reviews/task-057.md R2-F2
#
# Run by pre-commit and CI.

set -euo pipefail

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

echo "Checking for stale mechanism claims in JS/TS test comments..."

TEST_DIRS="web/tests web/src web/e2e"
FILES_FOUND=0

for dir in $TEST_DIRS; do
    [ -d "$dir" ] || continue

    while IFS= read -r file; do
        [ -f "$file" ] || continue
        FILES_FOUND=$((FILES_FOUND + 1))

        # Use awk to detect both patterns within test blocks, checking
        # whether the claimed mechanism appears in actual code (not comments).
        awk -v file="$file" '
        /^\s*(it|test)\s*\(/ {
            in_test = 1
            test_start = NR
            match($0, /['\''"]([^'\''"]*)['\''"]/,  m)
            test_name = m[1]
            brace_depth = 0

            # Reset per-test state
            delete comment_claims       # comment lines claiming $var
            delete comment_claim_lines
            claim_count = 0
            has_interactive_claim = 0
            interactive_claim_line = 0
            interactive_claim_text = ""
            has_exempt = 0

            # Collect all code lines and comment lines
            delete code_lines
            code_count = 0
            next
        }

        in_test {
            n = gsub(/{/, "{")
            brace_depth += n
            n = gsub(/}/, "}")
            brace_depth -= n

            if ($0 ~ /mechanism-claim:ok/) has_exempt = 1

            is_comment = ($0 ~ /^\s*\/\//)

            if (is_comment) {
                # Pattern 1: comment references $clicked/$selected/$hovered/$focused
                # but as a positive claim (not "not $clicked" negation)
                if ($0 ~ /\$clicked/ || $0 ~ /\$selected/ || $0 ~ /\$hovered/ || $0 ~ /\$focused/) {
                    # Skip negation patterns like "not $clicked" or "(not $clicked)"
                    line_lower = tolower($0)
                    if (line_lower !~ /not \$/ && line_lower !~ /without \$/ && line_lower !~ /no \$/) {
                        claim_count++
                        comment_claims[claim_count] = $0
                        comment_claim_lines[claim_count] = NR
                    }
                }

                # Pattern 2: claims about interactive behavior
                line_lower = tolower($0)
                if (line_lower ~ /interactive query template/ || \
                    line_lower ~ /interactive.*template/ || \
                    line_lower ~ /activates after clicking/ || \
                    line_lower ~ /only activates after/ || \
                    line_lower ~ /tiered coloring only activates after/) {
                    has_interactive_claim = 1
                    interactive_claim_line = NR
                    interactive_claim_text = $0
                }
            } else {
                # Non-comment line — track it as code
                code_count++
                code_lines[code_count] = $0
            }

            # End of test block
            if (brace_depth <= 0 && ($0 ~ /\}\s*\)\s*;?\s*$/ || $0 ~ /^\s*\}\s*\)\s*;?\s*$/)) {
                if (!has_exempt) {
                    # Check if any template variables appear in the CODE
                    # (not comments) of this test block
                    code_has_clicked = 0
                    code_has_selected = 0
                    code_has_hovered = 0
                    code_has_focused = 0
                    code_has_any_template = 0
                    for (i = 1; i <= code_count; i++) {
                        if (code_lines[i] ~ /\$clicked/) { code_has_clicked = 1; code_has_any_template = 1 }
                        if (code_lines[i] ~ /\$selected/) { code_has_selected = 1; code_has_any_template = 1 }
                        if (code_lines[i] ~ /\$hovered/) { code_has_hovered = 1; code_has_any_template = 1 }
                        if (code_lines[i] ~ /\$focused/) { code_has_focused = 1; code_has_any_template = 1 }
                    }

                    # Pattern 1: Report comments claiming a template var
                    # that is NOT in the code
                    for (c = 1; c <= claim_count; c++) {
                        var_missing = 0
                        if (comment_claims[c] ~ /\$clicked/ && !code_has_clicked) var_missing = 1
                        if (comment_claims[c] ~ /\$selected/ && !code_has_selected) var_missing = 1
                        if (comment_claims[c] ~ /\$hovered/ && !code_has_hovered) var_missing = 1
                        if (comment_claims[c] ~ /\$focused/ && !code_has_focused) var_missing = 1
                        if (var_missing) {
                            text = comment_claims[c]
                            gsub(/^[ \t]+/, "", text)
                            # Extract the var name
                            match(comment_claims[c], /\$[a-zA-Z_]+/, vn)
                            printf "%s:%d: test \"%s\" comment at line %d claims template variable %s is used, but it does not appear in the test code:\n  %s\n\n", file, test_start, test_name, comment_claim_lines[c], vn[0] ? vn[0] : "$var", text
                        }
                    }

                    # Pattern 2: Report interactive claims when no template
                    # variable appears in the code
                    if (has_interactive_claim && !code_has_any_template) {
                        text = interactive_claim_text
                        gsub(/^[ \t]+/, "", text)
                        printf "%s:%d: test \"%s\" claims interactive/click-activated behavior at line %d but no template variable ($clicked etc.) appears in the test code:\n  %s\n\n", file, test_start, test_name, interactive_claim_line, text
                    }
                }
                in_test = 0
            }
        }
        ' "$file" 2>/dev/null >> "$HITS_FILE" || true

    done < <(find "$dir" -name '*.test.js' -o -name '*.test.ts' -o -name '*.spec.js' -o -name '*.spec.ts' 2>/dev/null | sort)
done

if [ "$FILES_FOUND" -eq 0 ]; then
    echo "No test files found — skipping."
    exit 0
fi

if [ -s "$HITS_FILE" ]; then
    echo ""
    echo "STALE MECHANISM CLAIMS — test comments describing mechanisms not present in the code:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  A stale mechanism claim is a comment that describes a specific"
    echo "  variable (\$clicked), pattern (interactive template), or behavior"
    echo "  (activates after clicking) that is contradicted by the actual test"
    echo "  data. The comment was likely written for an earlier draft and never"
    echo "  updated when the implementation changed."
    echo ""
    echo "  Fix: Update the comment to accurately describe the actual mechanism,"
    echo "  or change the test to actually use the claimed mechanism."
    echo ""
    echo "  If the claim is intentionally approximate, add:"
    echo "    // mechanism-claim:ok — <reason>"
    echo ""
    echo "  See: specs/reviews/task-057.md R2-F2"
    echo ""
    echo "Violation(s) found."
    exit 1
fi

echo "Stale mechanism claim check passed."
exit 0
