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
# Check 1: This script detects API handler functions that call structural-only
# verification functions (verify_attestation_audit_only, verify_chain) without
# also calling constraint evaluation functions (evaluate_all, build_cel_context,
# derive_strategy_constraints, evaluate_constraints_against).
#
# Note: verify_chain calls verify_attestation_audit_only internally for each
# node — it is still structural-only verification (Phase 1 of §6.2). Switching
# from verify_attestation_audit_only to verify_chain does NOT add constraint
# evaluation (Phases 2–5). Both functions must be treated as structural-only.
#
# Check 2: Detects verification endpoints that implement Phases 2-4 (constraint
# evaluation) but omit Phase 5 (output signature verification). Also detects
# false coverage claims — comments asserting "already covered by verify_chain"
# when verify_chain only checks INPUT signatures, not OUTPUT signatures
# (agent_signature, gate result signatures).
#
# An endpoint that is INTENTIONALLY structural-only (e.g., an internal health
# check) can be exempted with: // verification-scope:structural-only
# An endpoint that intentionally defers Phase 5 can use: // phase5-exempt:ok
#
# See: specs/reviews/task-008.md F3, R2 F5 (incomplete verification endpoint)
# See: specs/reviews/task-008.md R3 F5 (Phase 5 output signatures not verified)
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

# ── Check 2: Output signature verification (Phase 5) ─────────────────
#
# A verification endpoint that implements phases 1-4 (structural + constraint
# evaluation) but omits Phase 5 (output signature verification) is incomplete.
# Phase 5 requires:
#   (a) if attestation.output.agent_signature is not null:
#       verify_signature(agent_signature, content_hash)
#   (b) for gate in attestation.output.gate_results:
#       verify_signature(gate.signature, gate.output_hash)
#
# The most common failure mode: a comment claims "Already covered by verify_chain"
# but verify_chain only checks INPUT signatures (SignedInput.signature,
# DerivedInput.signature), not OUTPUT signatures (agent_signature, gate
# signatures). A grep for agent_signature verification in the codebase returns
# zero hits — the field is only set to None in test fixtures.
#
# Detection: A verification handler (functions with "verif" in the name, or
# that return VerificationResult) that calls constraint evaluation functions
# (indicating Phases 2-4 are implemented) but does not reference output
# signature verification patterns (agent_signature.*verify, gate.*signature
# .*verify, output.*verify, verify.*output). Enforcement functions
# (evaluate_push_constraints, enforce_*, etc.) are not flagged — Phase 5
# belongs in the verification pipeline, which enforcement calls into.
#
# See: specs/reviews/task-008.md R3 F5 (Phase 5 not implemented)
#
# Check 2a: False coverage claims — comments that assert Phase 5 is "already
# covered" by a function that does not implement it. These are flagged in ANY
# function, not just verification handlers.

echo ""
echo "=== Check 2: Output signature verification (Phase 5) ==="

for file in $(grep -rl 'verify_attestation_audit_only\|verify_chain\|VerificationResult\|get_verification' "$SERVER_SRC" --include='*.rs' 2>/dev/null | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" '
    # Skip test modules
    /^\s*#\[cfg\(test\)\]/ { in_test = 1; next }
    in_test { next }

    # Detect function boundaries
    /^\s*(pub\s+)?(async\s+)?fn\s+/ {
        # Check previous function
        if (fn_name != "" && is_handler && is_verification_handler && has_constraint_eval && !has_output_sig_verify && !has_exempt) {
            printf "MISSING OUTPUT SIGNATURE VERIFICATION (Phase 5): %s in %s:%d\n", fn_name, file, fn_start
            printf "  Handler implements constraint evaluation (Phases 2-4) but does not\n"
            printf "  verify output signatures (Phase 5 of spec §6.2). Phase 5 requires:\n"
            printf "  (a) verify agent_signature against content_hash using agent key\n"
            printf "  (b) verify gate result signatures against output_hash using gate key\n"
            printf "  Without Phase 5, forged gate results with arbitrary signatures pass.\n"
            printf "  See: specs/reviews/task-008.md R3 F5\n\n"
            violations++
        }
        if (fn_name != "" && has_false_coverage_claim && !has_exempt) {
            printf "FALSE COVERAGE CLAIM: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Comment at line %d claims output signature verification is\n", false_claim_line
            printf "  \"already covered\" by another function. This must be verified by\n"
            printf "  reading the referenced function — do not assert coverage by comment.\n"
            printf "  verify_chain only checks INPUT signatures, not OUTPUT signatures.\n"
            printf "  See: specs/reviews/task-008.md R3 F5\n\n"
            violations++
        }

        # Parse function name
        match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        fn_name = m[1]
        fn_start = NR
        has_constraint_eval = 0
        has_output_sig_verify = 0
        has_false_coverage_claim = 0
        false_claim_line = 0
        has_exempt = 0
        is_handler = 0

        if (fn_name ~ /^test_/) { fn_name = ""; next }
        if ($0 ~ /pub\s+(async\s+)?fn/) is_handler = 1
        # Only flag Phase 5 missing for verification-related handlers,
        # not enforcement functions (enforce_*, evaluate_push/merge_constraints,
        # git_receive_pack) — Phase 5 belongs in the verification pipeline.
        is_verification_handler = 0
        if (fn_name ~ /verif|get_verification|verification/) is_verification_handler = 1
        next
    }
    fn_name != "" {
        if ($0 ~ /verification-scope:structural-only|phase5-exempt:ok/) has_exempt = 1
        # Constraint evaluation (signals Phases 2-4 are present)
        if ($0 ~ /evaluate_all|evaluate_constraints|build_cel_context|derive_strategy_constraints|evaluate_push_constraints|evaluate_merge_constraints|accumulate_chain_constraints/) has_constraint_eval = 1
        # Output signature verification patterns (exclude comments — a comment
        # claiming coverage is not actual verification code)
        if ($0 !~ /^\s*\/\// && $0 ~ /agent_signature.*verify|verify.*agent_signature|output.*signature.*verify|verify.*output.*sign|gate.*signature.*verify|verify.*gate.*sign|verify_output_signatures/) has_output_sig_verify = 1
        # Also detect VerificationResult return type as signal this is a verification handler
        if ($0 ~ /VerificationResult/) is_verification_handler = 1
        # False coverage claims — comments asserting coverage without implementation
        # Flagged in ANY function, not just verification handlers
        if ($0 ~ /[Aa]lready covered by.*verify_chain|[Aa]lready covered by.*verify_attestation/) {
            has_false_coverage_claim = 1
            false_claim_line = NR
        }
    }
    END {
        if (fn_name != "" && is_handler && is_verification_handler && has_constraint_eval && !has_output_sig_verify && !has_exempt) {
            printf "MISSING OUTPUT SIGNATURE VERIFICATION (Phase 5): %s in %s:%d\n", fn_name, file, fn_start
            printf "  Handler implements constraint evaluation (Phases 2-4) but does not\n"
            printf "  verify output signatures (Phase 5 of spec §6.2). Phase 5 requires:\n"
            printf "  (a) verify agent_signature against content_hash using agent key\n"
            printf "  (b) verify gate result signatures against output_hash using gate key\n"
            printf "  Without Phase 5, forged gate results with arbitrary signatures pass.\n"
            printf "  See: specs/reviews/task-008.md R3 F5\n\n"
            violations++
        }
        if (fn_name != "" && has_false_coverage_claim && !has_exempt) {
            printf "FALSE COVERAGE CLAIM: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Comment at line %d claims output signature verification is\n", false_claim_line
            printf "  \"already covered\" by another function. This must be verified by\n"
            printf "  reading the referenced function — do not assert coverage by comment.\n"
            printf "  verify_chain only checks INPUT signatures, not OUTPUT signatures.\n"
            printf "  See: specs/reviews/task-008.md R3 F5\n\n"
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
    echo "     (5) verify output signatures (agent_signature + gate signatures)"
    echo "     The enforcement code (evaluate_push_constraints, evaluate_merge_constraints)"
    echo "     already implements this pipeline -- extract and reuse."
    echo ""
    echo "     Check 2 (Phase 5): If your endpoint implements Phases 2-4 but not Phase 5,"
    echo "     add output signature verification. Do NOT add a comment claiming it is"
    echo "     'already covered' by verify_chain — verify_chain only checks INPUT signatures."
    echo ""
    echo "     Add '// verification-scope:structural-only' or '// phase5-exempt:ok' if the"
    echo "     endpoint is intentionally limited."
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi
