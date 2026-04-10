#!/usr/bin/env bash
# Architecture lint: detect dead code in frontend test files.
#
# When a test function calls render() and destructures the result into
# a variable (e.g., `const { container } = render(...)`), that variable
# must be used in at least one assertion or query within the same test
# block (`it()` or `test()`). A render result that is never referenced
# after its declaration is dead code — it was created but never asserted
# on, indicating either:
#   (a) An abandoned first attempt that was replaced by a second render
#       call (the F3 pattern from task-042)
#   (b) A test that renders a component but never verifies anything
#       about the rendered output
#
# Detection:
#   Within each `it(` or `test(` block, find `render()` calls that
#   destructure into named variables. Check if each variable name
#   appears again in the block (in querySelector, textContent, expect,
#   or any other reference). If the variable appears only at the
#   declaration site, it is dead.
#
# Exempt with: // dead-test-code:ok — <reason>
#
# See: specs/reviews/task-042.md F3 (dead render() in test)
#
# Run by pre-commit and CI.

set -euo pipefail

WEB_SRC="web/src"
VIOLATIONS=0

if [ ! -d "$WEB_SRC" ]; then
    echo "Skipping dead test code check: $WEB_SRC not found"
    exit 0
fi

echo "Checking for dead render() results in test files..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

# Find test files
for file in $(find "$WEB_SRC" -type f \( -name '*.test.js' -o -name '*.test.ts' -o -name '*.test.jsx' -o -name '*.test.tsx' -o -name '*.spec.js' -o -name '*.spec.ts' \) \
    ! -path '*/node_modules/*' \
    | sort); do
    [ -f "$file" ] || continue

    # Use awk to process test blocks and find unused render results.
    # Strategy:
    #   1. Track when we enter an it() or test() block (by counting braces)
    #   2. Within each block, record variables destructured from render()
    #   3. At block end, check if each variable was referenced after declaration
    awk -v file="$file" '
    BEGIN {
        in_test = 0
        brace_depth = 0
        test_start = 0
        num_vars = 0
    }

    # Detect test block start: it( or test(
    !in_test && /^\s*(it|test)\s*\(/ {
        in_test = 1
        # Count opening braces on this line to set initial depth
        line = $0
        gsub(/[^{]/, "", line)
        opens = length(line)
        line = $0
        gsub(/[^}]/, "", line)
        closes = length(line)
        brace_depth = opens - closes
        test_start = NR
        num_vars = 0
        delete var_names
        delete var_lines
        delete var_used
        next
    }

    in_test {
        # Track brace depth
        line = $0
        gsub(/[^{]/, "", line)
        opens = length(line)
        line = $0
        gsub(/[^}]/, "", line)
        closes = length(line)
        brace_depth += opens - closes

        # Skip lines with exemption comment
        if ($0 ~ /dead-test-code:ok/) {
            # Mark all current vars as used (exempted)
            for (i = 1; i <= num_vars; i++) {
                var_used[i] = 1
            }
        }

        # Detect render() destructuring:
        #   const { container } = render(...)
        #   const { container: c2 } = render(...)
        #   const result = render(...)
        # Use \b word boundary to match standalone render(), not rerender()
        if ($0 ~ /[^a-zA-Z_]render\s*\(/ || $0 ~ /^render\s*\(/) {
            # Pattern 1: const { varName } = render(
            if (match($0, /const\s*\{\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*\}\s*=\s*render/, m)) {
                num_vars++
                var_names[num_vars] = m[1]
                var_lines[num_vars] = NR
                var_used[num_vars] = 0
            }
            # Pattern 2: const { container: aliasName } = render(
            else if (match($0, /const\s*\{\s*[a-zA-Z_][a-zA-Z0-9_]*\s*:\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*\}\s*=\s*render/, m)) {
                num_vars++
                var_names[num_vars] = m[1]
                var_lines[num_vars] = NR
                var_used[num_vars] = 0
            }
            # Pattern 3: const varName = render(
            else if (match($0, /const\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*render/, m)) {
                num_vars++
                var_names[num_vars] = m[1]
                var_lines[num_vars] = NR
                var_used[num_vars] = 0
            }
        }
        # Check if any previously declared variable is referenced on this line
        # (but not on the same line as its declaration).
        # Skip lines that are render() destructuring declarations — property
        # names in destructuring patterns (e.g., { container: c2 }) are NOT
        # references to previously declared variables with the same name.
        is_render_decl = ($0 ~ /const\s*[\{].*=\s*render\s*\(/)
        if (!is_render_decl) {
            for (i = 1; i <= num_vars; i++) {
                if (!var_used[i] && NR != var_lines[i]) {
                    # Check if the variable name appears as a word boundary
                    if ($0 ~ ("\\<" var_names[i] "\\>")) {
                        var_used[i] = 1
                    }
                }
            }
        }

        # End of test block
        if (brace_depth <= 0) {
            for (i = 1; i <= num_vars; i++) {
                if (!var_used[i]) {
                    printf "%s:%d: render() result \"%s\" is never referenced after declaration (dead code)\n", file, var_lines[i], var_names[i]
                }
            }
            in_test = 0
            num_vars = 0
        }
    }
    ' "$file" 2>/dev/null >> "$HITS_FILE"
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(wc -l < "$HITS_FILE")
    echo ""
    echo "DEAD RENDER RESULTS in test files:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  A render() call whose result variable is never referenced is dead code."
    echo "  This typically indicates an abandoned first attempt that was replaced"
    echo "  by a second render() call, leaving the first as unreachable dead code"
    echo "  that confuses readers and adds execution overhead."
    echo ""
    echo "  Fix: Remove the dead render() call and its destructured variable."
    echo "  If the render is intentionally separate (e.g., testing mount lifecycle),"
    echo "  add '// dead-test-code:ok — <reason>' on the same line."
    echo ""
    echo "  See: specs/reviews/task-042.md F3"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "Dead test code check passed."
exit 0
