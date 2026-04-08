#!/usr/bin/env bash
# Architecture lint: verify that verification/audit endpoints implement the
# full spec algorithm — not just structural checks.
#
# When a spec defines a multi-phase verification algorithm (e.g., §6.2 defines
# 5 phases: verify input chain, collect constraints, build CEL context, evaluate
# constraints, verify output signatures), an endpoint that returns a
# VerificationResult must implement ALL phases.
#
# The most common failure mode: an endpoint calls verify_attestation_audit_only
# (which performs structural checks + Ed25519 signature verification — phase 1
# only) and returns the result as the "full VerificationResult tree." This
# omits constraint collection, CEL context building, and constraint evaluation
# (phases 2–5). Consumers get valid: true even when constraints are violated.
#
# This script detects API handler functions that call structural-only
# verification functions (verify_attestation_audit_only, verify_chain) without
# also calling constraint evaluation functions (evaluate_all, build_cel_context,
# derive_strategy_constraints, evaluate_constraints_against).
#
# Note: verify_chain calls verify_attestation_audit_only internally for each
# node — it is still structural-only verification (Phase 1 of §6.2). Switching
# from verify_attestation_audit_only to verify_chain does NOT add constraint
# evaluation (Phases 2–5). Both functions must be treated as structural-only.
#
# An endpoint that is INTENTIONALLY structural-only (e.g., an internal health
# check) can be exempted with: // verification-scope:structural-only
#
# See: specs/reviews/task-008.md F3, R2 F5 (incomplete verification endpoint)
#
# Run by pre-commit and CI.

set -euo pipefail

SERVER_SRC="crates/gyre-server/src"
FAIL=0
VIOLATIONS=0

if [ ! -d "$SERVER_SRC" ]; then
    echo "ERROR: Cannot find $SERVER_SRC"
    exit 1
fi

echo "Checking verification endpoint completeness..."

# Find non-test source files that call any structural-only verification function
# (verify_attestation_audit_only OR verify_chain — both are structural-only)
for file in $(grep -rl 'verify_attestation_audit_only\|verify_chain' "$SERVER_SRC" --include='*.rs' 2>/dev/null | sort); do
    [ -f "$file" ] || continue

    # Use awk to find handler functions (pub async fn) that call
    # verify_attestation_audit_only but not any constraint evaluation function.
    awk -v file="$file" '
    # Skip test modules
    /^\s*#\[cfg\(test\)\]/ { in_test = 1; next }
    in_test { next }

    # Detect function boundaries — look for pub async fn (handlers)
    /^\s*(pub\s+)?(async\s+)?fn\s+/ {
        # Check previous function
        if (fn_name != "" && has_structural_verify && is_handler && !has_constraint_eval && !has_exempt) {
            printf "INCOMPLETE VERIFICATION ENDPOINT: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Handler calls structural-only verification (verify_attestation_audit_only or verify_chain)\n"
            printf "  but does not call constraint evaluation functions. The spec §6.2 defines\n"
            printf "  a 5-phase verification algorithm; structural checks are only phase 1.\n"
            printf "  Missing: collect constraints, build CEL context, evaluate constraints,\n"
            printf "  verify output signatures (phases 2-5).\n"
            printf "  See: specs/reviews/task-008.md F3\n\n"
            violations++
        }

        # Parse function name
        match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        fn_name = m[1]
        fn_start = NR
        has_structural_verify = 0
        has_constraint_eval = 0
        has_exempt = 0
        is_handler = 0

        # Skip test functions
        if (fn_name ~ /^test_/) { fn_name = ""; next }

        # Mark as handler if it is a pub async fn (API handler pattern)
        if ($0 ~ /pub\s+(async\s+)?fn/) is_handler = 1
        next
    }
    fn_name != "" {
        if ($0 ~ /verification-scope:structural-only/) has_exempt = 1
        if ($0 ~ /verify_attestation_audit_only|verify_chain/) has_structural_verify = 1
        # Constraint evaluation functions — any of these indicate the endpoint
        # does more than structural verification
        if ($0 ~ /evaluate_all|evaluate_constraints|build_cel_context|derive_strategy_constraints|enforce_push_constraints|evaluate_push_constraints|evaluate_merge_constraints/) has_constraint_eval = 1
    }
    END {
        if (fn_name != "" && has_structural_verify && is_handler && !has_constraint_eval && !has_exempt) {
            printf "INCOMPLETE VERIFICATION ENDPOINT: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Handler calls structural-only verification (verify_attestation_audit_only or verify_chain)\n"
            printf "  but does not call constraint evaluation functions. The spec §6.2 defines\n"
            printf "  a 5-phase verification algorithm; structural checks are only phase 1.\n"
            printf "  Missing: collect constraints, build CEL context, evaluate constraints,\n"
            printf "  verify output signatures (phases 2-5).\n"
            printf "  See: specs/reviews/task-008.md F3\n\n"
            violations++
        }
        printf "SUMMARY:%d\n", violations
    }
    ' "$file" | while IFS= read -r line; do
        case "$line" in
            SUMMARY:*)
                v=$(echo "$line" | cut -d: -f2)
                echo "$v" >> /tmp/check-verification-$$
                ;;
            *)
                echo "$line"
                ;;
        esac
    done
done

# Tally results
if [ -f /tmp/check-verification-$$ ]; then
    while read -r v; do
        VIOLATIONS=$((VIOLATIONS + v))
    done < /tmp/check-verification-$$
    rm -f /tmp/check-verification-$$
fi

echo ""
if [ "$VIOLATIONS" -eq 0 ]; then
    echo "Verification completeness lint passed."
    exit 0
else
    echo "Fix: Verification endpoints must implement the full spec algorithm,"
    echo "     not just structural checks. Extend the handler to:"
    echo "     (1) load the full attestation chain"
    echo "     (2) collect all constraints (explicit + strategy-implied + gate)"
    echo "     (3) build a CEL evaluation context from actual output"
    echo "     (4) evaluate all constraints"
    echo "     (5) verify output signatures"
    echo "     The enforcement code (evaluate_push_constraints, evaluate_merge_constraints)"
    echo "     already implements this pipeline -- extract and reuse."
    echo ""
    echo "     Add '// verification-scope:structural-only' if the endpoint is intentionally"
    echo "     limited to structural verification (not for user-facing verification endpoints)."
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi
