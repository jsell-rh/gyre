#!/usr/bin/env bash
# Architecture lint: verify handlers that accept cryptographic input actually
# perform cryptographic verification — not just decode/deserialize.
#
# A handler that base64-decodes a signature and stores the bytes without ever
# calling a verify function allows any caller to claim ownership of any key.
# This is the "decode is not verify" flaw class (see TASK-006 F2).
#
# The script scans non-test API handler functions for:
#   1. User-submitted crypto fields (signature, digest, proof, certificate)
#   2. Decode operations on those fields (base64 decode, from_bytes, etc.)
#   3. Missing verification calls (verify, verify_signature, check_signature)
#
# A handler that decodes user-submitted crypto material but never verifies
# it is a violation. Internally-computed hashes (e.g., hash_api_key) are
# excluded — they are not user-submitted and don't need verification.
#
# Exempt a handler with: // crypto-verify:ok — <reason>
#
# Run by pre-commit and CI.

set -euo pipefail

API_DIR="crates/gyre-server/src/api"
FAIL=0
CHECKED=0
VIOLATIONS=0

if [ ! -d "$API_DIR" ]; then
    echo "ERROR: Cannot find $API_DIR"
    exit 1
fi

echo "Checking API handlers for crypto decode-without-verify..."

for file in "$API_DIR"/*.rs; do
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
        if ($0 ~ /verify|check_signature|validate_signature/) has_verify = 1
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

# Tally results
if [ -f /tmp/check-crypto-verify-$$ ]; then
    while read -r c v; do
        CHECKED=$((CHECKED + c))
        VIOLATIONS=$((VIOLATIONS + v))
    done < /tmp/check-crypto-verify-$$
    rm -f /tmp/check-crypto-verify-$$
fi

if [ "$VIOLATIONS" -eq 0 ]; then
    echo "Crypto verify lint passed: ${CHECKED} handlers with crypto decode checked."
    echo "All perform cryptographic verification after decoding."
    exit 0
else
    echo "Fix: After decoding crypto material, call the appropriate verify function"
    echo "     (e.g., ed25519_verify(public_key, message, signature))."
    echo "     Decode proves format validity. Verify proves cryptographic validity."
    echo "${VIOLATIONS} violation(s) found out of ${CHECKED} handlers with crypto decode."
    exit 1
fi
