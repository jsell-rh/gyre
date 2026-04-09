#!/usr/bin/env bash
# check-wildcard-exclusion.sh — Detect wildcard catch-all match arms on spec-enforced enums
#
# When a spec defines an explicit exclusion list (e.g., "exclude conflicts_with,
# references, supersedes from cycle detection"), the match must enumerate the
# excluded types explicitly and use the wildcard (or explicit match) for the
# INCLUDED set. An empty wildcard `_ => {}` silently excludes any type not
# explicitly listed in the active arms — including types the spec DOES NOT exclude.
#
# Safe default principle: When a spec names specific types to EXCLUDE from a
# behavior, the code should match those types in the skip arm and use the
# wildcard for the INCLUDE arm. This way, new enum variants default to being
# INCLUDED (safe) rather than EXCLUDED (silent omission).
#
# Exempt with: // wildcard:ok — <reason>
#
# Process-revision: TASK-019 F1 — wildcard catch-all excluded `extends` from
# cycle detection; spec only excludes 3 types, wildcard caught 5.

set -euo pipefail

VIOLATIONS=0
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Scan for `_ => {}` in any form in non-test Rust files
while IFS=: read -r file line content; do
    # Skip test files and test modules
    if [[ "$file" == *"/tests/"* || "$file" == *"_test.rs"* ]]; then
        continue
    fi
    # Skip exempted lines
    if echo "$content" | grep -q 'wildcard:ok'; then
        continue
    fi
    # Look backwards from this line for the match expression to determine if it's
    # on a spec-enforced enum type
    match_context=$(sed -n "$((line > 25 ? line - 25 : 1)),${line}p" "$file" 2>/dev/null || true)

    # Check if the match context references a spec-enforced enum type
    # These are types where the spec defines specific inclusion/exclusion rules
    if echo "$match_context" | grep -qE '(SpecLinkType|GateType|GateStatus|ApprovalStatus)'; then
        echo "VIOLATION: $file:$line — empty wildcard catch-all \`_ => {}\` on spec-enforced enum"
        echo "  The wildcard silently excludes any variant not listed in active arms."
        echo "  When a spec defines which types to exclude from a behavior, enumerate"
        echo "  the EXCLUDED types explicitly in the skip arm, and use the wildcard"
        echo "  (or remaining variants) for the INCLUDED set. This ensures new variants"
        echo "  default to inclusion, which is the safe default."
        echo "  Exempt with: // wildcard:ok — <reason>"
        echo ""
        VIOLATIONS=$((VIOLATIONS + 1))
    fi
done < <(grep -rn '_ => {\s*}' "$ROOT/crates/" --include='*.rs' -P 2>/dev/null || true)

if [ "$VIOLATIONS" -gt 0 ]; then
    echo "========================================="
    echo "check-wildcard-exclusion: $VIOLATIONS violation(s) found"
    echo "========================================="
    exit 1
fi

echo "check-wildcard-exclusion: OK (no violations)"
exit 0
