#!/usr/bin/env bash
# check-noop-code.sh — Detect arithmetic no-op patterns in Rust source code.
#
# These patterns are always dead code:
#   += 0     (additive identity)
#   -= 0     (additive identity)
#   *= 1     (multiplicative identity)
#   /= 1     (multiplicative identity)
#   |= false (bitwise/boolean identity)
#   &= true  (bitwise/boolean identity)
#
# They indicate iterative development artifacts — code that was written during
# an intermediate attempt and never cleaned up. The surrounding code (the variable
# being mutated) is likely also dead — populated but never read.
#
# Exempt a line with: // noop:ok — <reason>

set -euo pipefail

ERRORS=0

# Check 1: Arithmetic no-ops in non-test Rust code
# We check both test and non-test code because no-ops are never intentional.
while IFS=: read -r file line content; do
    # Skip lines with exemption comment
    if echo "$content" | grep -q '// noop:ok'; then
        continue
    fi
    echo "ERROR: Arithmetic no-op at $file:$line"
    echo "  $content"
    echo "  This operation has no effect. The variable is likely dead code from iterative development."
    echo "  Remove the no-op and its surrounding dead variable, or exempt with // noop:ok — <reason>"
    echo ""
    ERRORS=$((ERRORS + 1))
done < <(grep -rn --include='*.rs' -E '\+=\s*0\b|\-=\s*0\b|\*=\s*1\b|/=\s*1\b|\|=\s*false\b|&=\s*true\b' crates/ 2>/dev/null || true)

if [ "$ERRORS" -gt 0 ]; then
    echo "check-noop-code: FAILED — $ERRORS arithmetic no-op(s) found"
    exit 1
fi

echo "check-noop-code: OK"
exit 0
