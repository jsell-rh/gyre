#!/usr/bin/env bash
# Architecture lint: verify that code handling cryptographic material actually
# performs cryptographic verification — not just decode/deserialize or
# structural checks.
#
# Two flaw classes detected:
#
# Class 1 — Decode-without-verify (see TASK-006 F2):
#   A handler that base64-decodes a signature and stores the bytes without ever
#   calling a verify function allows any caller to claim ownership of any key.
#   Scans for: user-submitted crypto fields → decode operations → missing verify.
#
# Class 2 — Structural-only verification (see TASK-006 F4):
#   A function named verify_* or *_verify that references crypto material
#   (signature, public_key, signed) but never calls a cryptographic operation
#   (ring::, Ed25519, verify() on a key, etc.). This catches verification
#   functions that check structural properties (non-empty fields, expiry,
#   chain depth) but skip the actual cryptographic signature check — making
#   audit logs unreliable because forged signatures report as valid.
#
# Scope: all non-test Rust source files under crates/gyre-server/src/ — not
# just api/ handlers. Verification functions in git_http.rs, constraint_check.rs,
# and other modules are equally important.
#
# Exempt a function with: // crypto-verify:ok — <reason>
#
# Run by pre-commit and CI.

set -euo pipefail

SERVER_SRC="crates/gyre-server/src"
FAIL=0
CHECKED=0
VIOLATIONS=0

if [ ! -d "$SERVER_SRC" ]; then
    echo "ERROR: Cannot find $SERVER_SRC"
    exit 1
fi

echo "Checking server source for crypto verification completeness..."

# ── Class 1: Decode-without-verify ────────────────────────────────────

echo ""
echo "=== Class 1: Decode-without-verify ==="

for file in $(find "$SERVER_SRC" -name '*.rs' -type f | sort); do
    [ -f "$file" ] || continue
    bname=$(basename "$file")
    # Skip mod.rs (routing), error.rs (error types)
    [ "$bname" = "mod.rs" ] || [ "$bname" = "error.rs" ] && continue

    # Use awk to find handler functions that decode crypto material but don't verify
    awk -v file="$file" '
    # Track #[cfg(test)] module boundaries — skip test code
    /^\s*#\[cfg\(test\)\]/ { in_test_module = 1; next }

    # Detect start of an async fn (handler boundary)
    /^\s*(pub\s+)?(async\s+)?fn\s+/ {
        # Check previous function
        if (fn_name != "" && has_crypto_decode && !has_verify && !has_exempt) {
            printf "CRYPTO DECODE WITHOUT VERIFY: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Handler decodes cryptographic material but never calls a verify function.\n"
            printf "  Base64-decoding a signature is NOT verification. Call ed25519_verify or equivalent.\n"
            printf "  See: specs/reviews/task-006.md F2 (decode-without-verify)\n\n"
            violations++
        }
        if (fn_name != "" && has_crypto_decode) checked++

        # Parse function name
        match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        fn_name = m[1]
        fn_start = NR
        has_crypto_decode = 0
        has_verify = 0
        has_crypto_field = 0
        has_exempt = 0
        # Skip test functions (test_ prefix or inside #[cfg(test)])
        if (fn_name ~ /^test_/ || in_test_module) fn_name = ""
        next
    }
    fn_name != "" {
        # Check for exemption comment
        if ($0 ~ /crypto-verify:ok/) has_exempt = 1
        # Check for user-submitted crypto field references (not internally-computed)
        # Match: signature, user_signature, _digest, _proof, _certificate
        # Exclude: _hash (too broad — catches internally-computed hashes like hash_api_key)
        if ($0 ~ /signature|_digest|_proof|_certificate/) has_crypto_field = 1
        # Check for decode operations on crypto material
        if (has_crypto_field && ($0 ~ /decode|from_bytes|from_slice|from_base64|STANDARD/)) has_crypto_decode = 1
        # Check for verification calls
        # jsonwebtoken::decode with Validation IS signature verification for JWTs
        if ($0 ~ /\.verify\(|verify_signature|check_signature|validate_signature|UnparsedPublicKey|jsonwebtoken::decode/) has_verify = 1
    }
    END {
        # Check last function
        if (fn_name != "" && has_crypto_decode && !has_verify && !has_exempt) {
            printf "CRYPTO DECODE WITHOUT VERIFY: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Handler decodes cryptographic material but never calls a verify function.\n"
            printf "  Base64-decoding a signature is NOT verification. Call ed25519_verify or equivalent.\n"
            printf "  See: specs/reviews/task-006.md F2 (decode-without-verify)\n\n"
            violations++
        }
        if (fn_name != "" && has_crypto_decode) checked++
        printf "SUMMARY:%d:%d\n", checked, violations
    }
    ' "$file" | while IFS= read -r line; do
        case "$line" in
            SUMMARY:*)
                c=$(echo "$line" | cut -d: -f2)
                v=$(echo "$line" | cut -d: -f3)
                echo "$c $v" >> /tmp/check-crypto-verify-$$
                ;;
            *)
                echo "$line"
                ;;
        esac
    done
done

# ── Class 2: Structural-only verification ─────────────────────────────
#
# A function named verify_* or *_verify* that references crypto material
# (signature, public_key, signed input) but never calls a cryptographic
# operation is performing structural-only verification.
#
# This is different from Class 1: the function doesn't decode user input,
# it reads stored crypto data and is supposed to verify it. A function
# that checks expiry, chain depth, and non-empty fields but never calls
# ring::signature, Ed25519, or .verify() on a key object makes audit logs
# unreliable — forged signatures will report as valid.

echo ""
echo "=== Class 2: Structural-only verification ==="

for file in $(find "$SERVER_SRC" -name '*.rs' -type f | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" '
    # Track #[cfg(test)] module boundaries — skip test code
    /^\s*#\[cfg\(test\)\]/ { in_test_module = 1; next }

    # Detect start of a fn whose name contains "verify"
    /^\s*(pub\s+)?(async\s+)?fn\s+/ {
        # Check previous function
        if (fn_name != "" && is_verify_fn && has_crypto_ref && !has_crypto_op && !has_exempt) {
            printf "STRUCTURAL-ONLY VERIFICATION: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Function name contains \"verify\" and references crypto material\n"
            printf "  (signature/public_key/signed) but never calls a cryptographic operation\n"
            printf "  (ring::, .verify(), UnparsedPublicKey, Ed25519). Structural checks\n"
            printf "  (expiry, chain depth, non-empty fields) are not crypto verification.\n"
            printf "  See: specs/reviews/task-006.md F4 (structural-only-verification)\n\n"
            violations++
        }
        if (fn_name != "" && is_verify_fn && has_crypto_ref) checked++

        # Parse function name
        match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        fn_name = m[1]
        fn_start = NR
        is_verify_fn = 0
        has_crypto_ref = 0
        has_crypto_op = 0
        has_exempt = 0

        # Skip test functions
        if (fn_name ~ /^test_/ || in_test_module) { fn_name = ""; next }

        # Check if function name contains "verify"
        if (fn_name ~ /verify/) is_verify_fn = 1
        next
    }
    fn_name != "" && is_verify_fn {
        # Check for exemption comment
        if ($0 ~ /crypto-verify:ok/) has_exempt = 1
        # Check for crypto material references
        if ($0 ~ /\.signature|public_key|SignedInput|Signed\(/) has_crypto_ref = 1
        # Check for actual cryptographic operations
        if ($0 ~ /ring::|\.verify\(|UnparsedPublicKey|Ed25519|ed25519|verify_signature|SHA256|sha256|digest::digest/) has_crypto_op = 1
    }
    END {
        # Check last function
        if (fn_name != "" && is_verify_fn && has_crypto_ref && !has_crypto_op && !has_exempt) {
            printf "STRUCTURAL-ONLY VERIFICATION: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Function name contains \"verify\" and references crypto material\n"
            printf "  (signature/public_key/signed) but never calls a cryptographic operation\n"
            printf "  (ring::, .verify(), UnparsedPublicKey, Ed25519). Structural checks\n"
            printf "  (expiry, chain depth, non-empty fields) are not crypto verification.\n"
            printf "  See: specs/reviews/task-006.md F4 (structural-only-verification)\n\n"
            violations++
        }
        if (fn_name != "" && is_verify_fn && has_crypto_ref) checked++
        printf "SUMMARY:%d:%d\n", checked, violations
    }
    ' "$file" | while IFS= read -r line; do
        case "$line" in
            SUMMARY:*)
                c=$(echo "$line" | cut -d: -f2)
                v=$(echo "$line" | cut -d: -f3)
                echo "$c $v" >> /tmp/check-crypto-verify-$$
                ;;
            *)
                echo "$line"
                ;;
        esac
    done
done

# ── Tally results ─────────────────────────────────────────────────────

if [ -f /tmp/check-crypto-verify-$$ ]; then
    while read -r c v; do
        CHECKED=$((CHECKED + c))
        VIOLATIONS=$((VIOLATIONS + v))
    done < /tmp/check-crypto-verify-$$
    rm -f /tmp/check-crypto-verify-$$
fi

echo ""
if [ "$VIOLATIONS" -eq 0 ]; then
    echo "Crypto verify lint passed: ${CHECKED} functions with crypto material checked."
    echo "All perform cryptographic verification (not just structural checks)."
    exit 0
else
    echo "Fix: Ensure all functions that handle cryptographic material perform actual"
    echo "     cryptographic operations — not just structural/format checks."
    echo "     Class 1: After decoding crypto material, call ed25519_verify or equivalent."
    echo "     Class 2: Verification functions must call ring::signature, .verify(), etc."
    echo "     Structural checks (expiry, chain depth, non-empty) are necessary but not sufficient."
    echo "${VIOLATIONS} violation(s) found out of ${CHECKED} functions checked."
    exit 1
fi
