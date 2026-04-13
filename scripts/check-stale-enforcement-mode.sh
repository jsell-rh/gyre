#!/usr/bin/env bash
# Architecture lint: detect stale "audit-only" messaging in production code
# after enforcement has been activated.
#
# When the system transitions from audit-only mode (Phase 1–2) to enforcement
# mode (Phase 3+), user-facing strings like "audit-only" and "not rejecting"
# become misleading. A verification result that says "one or more checks failed
# (audit-only, not rejecting)" in a system that IS rejecting pushes/merges
# confuses API consumers and Explorer users.
#
# This is distinct from the function NAME (verify_attestation_audit_only) —
# the function name describes its verification scope, which is fine. The
# problem is user-facing STRINGS in result messages, API responses, and
# log entries that claim the system won't reject when it will.
#
# Detection: scan non-test Rust source files for string literals containing
# "audit-only" or "not rejecting" in contexts that produce user-facing output
# (result messages, API response fields, format!/println! calls).
#
# Exempt with: // enforcement-mode:ok — <reason>
# (e.g., for actual audit-only code paths that remain non-enforcing)
#
# See: specs/reviews/task-009.md F4 (stale audit-only messages)
#
# Run by pre-commit and CI.

set -euo pipefail

SERVER_SRC="crates/gyre-server/src"
VIOLATIONS=0

if [ ! -d "$SERVER_SRC" ]; then
    echo "ERROR: Cannot find $SERVER_SRC"
    exit 1
fi

echo "Checking for stale enforcement-mode messaging..."

# ── Check 1: "audit-only" in user-facing string literals ─────────────
#
# Match string literals (inside quotes) containing "audit-only" or
# "audit only" in non-test Rust code. Skip:
# - Function/variable names (verify_attestation_audit_only is fine)
# - Comments that aren't part of string construction
# - Lines with the exemption marker

echo ""
echo "=== Check 1: audit-only in user-facing strings ==="

for file in $(find "$SERVER_SRC" -name '*.rs' -type f | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" '
    /^\s*#\[cfg\(test\)\]/ { in_test = 1 }
    in_test { next }

    # Match lines with "audit-only" or "audit only" inside string literals
    # (between double quotes). Skip exempted lines.
    /"[^"]*audit[- ]only[^"]*"/ {
        if ($0 !~ /enforcement-mode:ok/) {
            printf "%s:%d: %s\n", file, NR, $0
        }
    }
    ' "$file"
done > /tmp/check-enforcement-mode-$$

if [ -s /tmp/check-enforcement-mode-$$ ]; then
    echo ""
    echo "STALE AUDIT-ONLY MESSAGING found in production code:"
    while IFS= read -r line; do
        echo "  $line"
        VIOLATIONS=$((VIOLATIONS + 1))
    done < /tmp/check-enforcement-mode-$$
    echo ""
    echo "  String literals containing 'audit-only' in user-facing messages are stale"
    echo "  now that enforcement is active (Phase 3+). The system DOES reject invalid"
    echo "  pushes and merges — messages saying otherwise are misleading."
    echo ""
    echo "  Add '// enforcement-mode:ok' if the code path is genuinely non-enforcing."
    echo ""
fi
rm -f /tmp/check-enforcement-mode-$$

# ── Check 2: "not rejecting" in user-facing string literals ──────────

echo ""
echo "=== Check 2: not-rejecting in user-facing strings ==="

for file in $(find "$SERVER_SRC" -name '*.rs' -type f | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" '
    /^\s*#\[cfg\(test\)\]/ { in_test = 1 }
    in_test { next }

    /"[^"]*not rejecting[^"]*"/ {
        if ($0 !~ /enforcement-mode:ok/) {
            printf "%s:%d: %s\n", file, NR, $0
        }
    }
    ' "$file"
done > /tmp/check-enforcement-mode2-$$

if [ -s /tmp/check-enforcement-mode2-$$ ]; then
    echo ""
    echo "STALE NOT-REJECTING MESSAGING found in production code:"
    while IFS= read -r line; do
        echo "  $line"
        VIOLATIONS=$((VIOLATIONS + 1))
    done < /tmp/check-enforcement-mode2-$$
    echo ""
    echo "  String literals containing 'not rejecting' in verification results are stale"
    echo "  now that enforcement is active. Remove or update these messages."
    echo ""
    echo "  Add '// enforcement-mode:ok' if the code path is genuinely non-enforcing."
    echo ""
fi
rm -f /tmp/check-enforcement-mode2-$$

# ── Result ──────────────────────────────────────────────────────────────

echo ""
if [ "$VIOLATIONS" -eq 0 ]; then
    echo "Stale enforcement mode check passed."
    exit 0
else
    echo "Fix: Update user-facing messages to remove 'audit-only' and 'not rejecting'"
    echo "     language. Use accurate descriptions like 'structural checks passed' or"
    echo "     'one or more structural checks failed'. The function name can keep"
    echo "     'audit_only' (it describes verification scope), but result messages"
    echo "     must not claim the system won't reject."
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi
