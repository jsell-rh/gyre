#!/usr/bin/env bash
# Architecture lint: verify that code handling cryptographic material actually
# performs cryptographic verification — not just decode/deserialize or
# structural checks.
#
# Five flaw classes detected:
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
#
# Class 3 — Delegation signing entity mismatch (see TASK-008 F1):
#   A spawn/delegation function that generates a NEW keypair for the child
#   agent and uses that key to sign a DerivedInput — when the spec requires
#   the PARENT/SPAWNER's existing key to sign it. The newly generated key
#   should be stored for the child's future use (push-time signing), not
#   used to sign the delegation structure. Detection: function has keygen +
#   sign + DerivedInput construction but no spawner key lookup from storage.
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

# ── Class 3: Delegation signing entity mismatch ──────────────────────
#
# When a function creates a delegation structure (DerivedInput), the spec
# requires the PARENT/SPAWNER to sign it — not the entity being spawned.
# The most common failure mode: a spawn function generates a new keypair
# for the child agent and uses THAT key to sign the DerivedInput, making
# the delegation circular (the child authorizes its own spawn).
#
# Detection heuristic: if a function both (a) generates a new Ed25519
# keypair (Ed25519KeyPair::generate) AND (b) signs something AND (c)
# constructs a DerivedInput — all in the same function — that is
# suspicious. The new key should be stored for the agent's future use,
# while the DerivedInput should be signed with the SPAWNER's existing key
# loaded from storage.
#
# Exempt with: // signing-entity:ok — <reason>
#
# See: specs/reviews/task-008.md F1 (signing entity mismatch)

echo ""
echo "=== Class 3: Delegation signing entity mismatch ==="

for file in $(find "$SERVER_SRC" -name '*.rs' -type f | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" '
    # Skip test modules
    /^\s*#\[cfg\(test\)\]/ { in_test_module = 1; next }

    # Detect function boundaries
    /^\s*(pub\s+)?(async\s+)?fn\s+/ {
        # Check previous function
        if (fn_name != "" && has_keygen && has_sign && has_derived_input && !has_key_lookup && !has_exempt) {
            printf "DELEGATION SIGNING ENTITY MISMATCH: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Function generates a new keypair AND signs a DerivedInput in the same\n"
            printf "  function WITHOUT loading an existing key from storage. This suggests the\n"
            printf "  newly generated key (for the child) is being used to sign the delegation\n"
            printf "  structure, when the SPAWNER/PARENT key should sign it.\n"
            printf "  Spec: DerivedInput.signature = orchestrator signs derivation (§4.1, §4.5)\n"
            printf "  See: specs/reviews/task-008.md F1 (signing entity mismatch)\n\n"
            violations++
        }
        if (fn_name != "" && has_derived_input) checked++

        # Parse function name
        match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        fn_name = m[1]
        fn_start = NR
        has_keygen = 0
        has_sign = 0
        has_derived_input = 0
        has_key_lookup = 0
        has_exempt = 0
        # Skip test functions
        if (fn_name ~ /^test_/ || in_test_module) fn_name = ""
        next
    }
    fn_name != "" {
        if ($0 ~ /signing-entity:ok/) has_exempt = 1
        # Detect new keypair generation
        if ($0 ~ /generate_pkcs8|Ed25519KeyPair::generate|KeyPair::generate/) has_keygen = 1
        # Detect signing operations
        if ($0 ~ /\.sign\(|sign_bytes/) has_sign = 1
        # Detect DerivedInput construction
        if ($0 ~ /DerivedInput\s*\{|DerivedInput {/) has_derived_input = 1
        # Detect loading an EXISTING key from storage (spawner key lookup)
        # Pattern: kv_get("agent_signing_keys", spawner...) or similar storage reads
        if ($0 ~ /kv_get.*signing_key|kv_get.*spawner|load_signing_key|get_signing_key|spawner.*signing/) has_key_lookup = 1
    }
    END {
        if (fn_name != "" && has_keygen && has_sign && has_derived_input && !has_key_lookup && !has_exempt) {
            printf "DELEGATION SIGNING ENTITY MISMATCH: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Function generates a new keypair AND signs a DerivedInput in the same\n"
            printf "  function WITHOUT loading an existing key from storage. This suggests the\n"
            printf "  newly generated key (for the child) is being used to sign the delegation\n"
            printf "  structure, when the SPAWNER/PARENT key should sign it.\n"
            printf "  Spec: DerivedInput.signature = orchestrator signs derivation (§4.1, §4.5)\n"
            printf "  See: specs/reviews/task-008.md F1 (signing entity mismatch)\n\n"
            violations++
        }
        if (fn_name != "" && has_derived_input) checked++
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

# ── Class 4: Asymmetric branch verification ──────────────────────────
#
# A verify function that handles MULTIPLE crypto-bearing input types
# (e.g., SignedInput and DerivedInput via match arms) where SOME branches
# perform cryptographic verification and OTHERS skip it. The most common
# failure mode: the Signed branch calls Ed25519 verify, but the Derived
# branch only checks structural properties (has_parent, parent_ref non-empty)
# without verifying the DerivedInput's signature.
#
# The spec §4.4 step 1 requires BOTH input types to be cryptographically
# verified: "verify_signature(attestation.input.signature,
# attestation.input.key_binding)" applies to DerivedInputs just as to
# SignedInputs.
#
# Detection: a verify function handles both Signed( and Derived( match arms,
# has crypto operations (ring, .verify(), UnparsedPublicKey), BUT the
# Derived section lacks crypto operations. Class 2 misses this because it
# checks at the function level — if any branch has crypto ops, Class 2
# passes. Class 4 checks at the branch level.
#
# Exempt with: // branch-verify:ok — <reason>
#
# See: specs/reviews/task-009.md F1 (DerivedInput signature not verified)

echo ""
echo "=== Class 4: Asymmetric branch verification ==="

for file in $(find "$SERVER_SRC" -name '*.rs' -type f | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" '
    # Skip test modules
    /^\s*#\[cfg\(test\)\]/ { in_test_module = 1; next }

    # Detect function boundaries
    /^\s*(pub\s+)?(async\s+)?fn\s+/ {
        # Check previous function
        if (fn_name != "" && is_verify_fn && has_signed_branch && has_derived_branch && has_crypto_in_signed && !has_crypto_in_derived && !has_exempt) {
            printf "ASYMMETRIC BRANCH VERIFICATION: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Function handles both Signed and Derived input types but only\n"
            printf "  performs cryptographic verification for the Signed branch.\n"
            printf "  The Derived branch must also verify Ed25519 signature and key\n"
            printf "  binding — DerivedInput.signature proves who authorized the delegation.\n"
            printf "  Spec: §4.4 step 1 requires verify_signature for all input types.\n"
            printf "  See: specs/reviews/task-009.md F1 (asymmetric branch verification)\n\n"
            violations++
        }
        if (fn_name != "" && is_verify_fn && has_signed_branch && has_derived_branch) checked++

        # Parse function name
        match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        fn_name = m[1]
        fn_start = NR
        is_verify_fn = 0
        has_signed_branch = 0
        has_derived_branch = 0
        has_crypto_in_signed = 0
        has_crypto_in_derived = 0
        in_signed_section = 0
        in_derived_section = 0
        has_exempt = 0
        # Skip test functions
        if (fn_name ~ /^test_/ || in_test_module) fn_name = ""
        if (fn_name ~ /verify/) is_verify_fn = 1
        next
    }
    fn_name != "" && is_verify_fn {
        if ($0 ~ /branch-verify:ok/) has_exempt = 1

        # Track which branch we are in based on match arm patterns
        if ($0 ~ /Signed\(|SignedInput/) {
            has_signed_branch = 1
            in_signed_section = 1
            in_derived_section = 0
        }
        if ($0 ~ /Derived\(|DerivedInput/) {
            has_derived_branch = 1
            in_derived_section = 1
            in_signed_section = 0
        }

        # Check for crypto operations in the current section
        if ($0 ~ /ring::|\.verify\(|UnparsedPublicKey|Ed25519|ed25519|verify_signature|digest::digest/) {
            if (in_signed_section) has_crypto_in_signed = 1
            if (in_derived_section) has_crypto_in_derived = 1
        }
    }
    END {
        if (fn_name != "" && is_verify_fn && has_signed_branch && has_derived_branch && has_crypto_in_signed && !has_crypto_in_derived && !has_exempt) {
            printf "ASYMMETRIC BRANCH VERIFICATION: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Function handles both Signed and Derived input types but only\n"
            printf "  performs cryptographic verification for the Signed branch.\n"
            printf "  The Derived branch must also verify Ed25519 signature and key\n"
            printf "  binding — DerivedInput.signature proves who authorized the delegation.\n"
            printf "  Spec: §4.4 step 1 requires verify_signature for all input types.\n"
            printf "  See: specs/reviews/task-009.md F1 (asymmetric branch verification)\n\n"
            violations++
        }
        if (fn_name != "" && is_verify_fn && has_signed_branch && has_derived_branch) checked++
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

# ── Class 5: Sign/verify message mismatch ───────────────────────────
#
# When verification code passes a raw hash field (output_hash, content_hash)
# directly to `.verify()` as the message, but the corresponding signing code
# constructs a JSON structure containing that hash plus other fields, the
# Ed25519 messages don't match — verification ALWAYS fails for legitimate
# signatures (false negatives). This is a silent correctness failure: the
# code appears to verify, the function is named correctly, crypto operations
# are present, and all prior Classes pass — but the wrong bytes are checked.
#
# Detection heuristic: within a verify function, if `.verify(` is called
# with a raw `output_hash` or `content_hash` field as the message (not
# wrapped in a JSON reconstruction), and the function does NOT reconstruct
# a JSON signable structure (serde_json::json!, serde_json::to_vec, or
# a signable_bytes() call) before the verify call for that specific entity,
# the verify message likely doesn't match what was signed.
#
# Exempt with: // sign-verify-parity:ok — <reason>
#
# See: specs/reviews/task-008.md F6 (sign/verify message mismatch)

echo ""
echo "=== Class 5: Sign/verify message mismatch ==="

for file in $(find "$SERVER_SRC" -name '*.rs' -type f | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" '
    # Skip test modules
    /^\s*#\[cfg\(test\)\]/ { in_test_module = 1; next }

    # Detect function boundaries
    /^\s*(pub\s+)?(async\s+)?fn\s+/ {
        # Check previous function
        if (fn_name != "" && is_verify_fn && has_raw_hash_verify && !has_signable_reconstruction && !has_exempt) {
            printf "SIGN/VERIFY MESSAGE MISMATCH: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Function verifies a signature against a raw hash field (output_hash or\n"
            printf "  content_hash) but does not reconstruct the JSON signable structure that\n"
            printf "  the signing code uses. Ed25519 requires the exact same message bytes for\n"
            printf "  sign and verify. If the signer serializes a JSON object containing the\n"
            printf "  hash (plus other fields), the verifier must reconstruct that same JSON —\n"
            printf "  not pass the raw hash bytes directly.\n"
            printf "  Fix: reconstruct the signable JSON, or use a shared signable_bytes() helper.\n"
            printf "  See: specs/reviews/task-008.md F6 (sign/verify message mismatch)\n\n"
            violations++
        }
        if (fn_name != "" && is_verify_fn && has_raw_hash_verify) checked++

        # Parse function name
        match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        fn_name = m[1]
        fn_start = NR
        is_verify_fn = 0
        has_raw_hash_verify = 0
        has_signable_reconstruction = 0
        has_exempt = 0
        # Skip test functions
        if (fn_name ~ /^test_/ || in_test_module) fn_name = ""
        if (fn_name ~ /verify/) is_verify_fn = 1
        next
    }
    fn_name != "" && is_verify_fn {
        if ($0 ~ /sign-verify-parity:ok/) has_exempt = 1
        # Detect .verify() called with a raw hash field as the message.
        # Pattern: .verify(&something.output_hash, or .verify(&something.content_hash,
        # or .verify(&gate.output_hash, etc.
        if ($0 ~ /\.verify\(.*[._]output_hash|\.verify\(.*[._]content_hash/) has_raw_hash_verify = 1
        # Detect JSON signable structure reconstruction before verify.
        # If the function builds serde_json::json!({...}) or calls serde_json::to_vec
        # or calls a signable_bytes() helper, the message is being reconstructed.
        if ($0 ~ /serde_json::json!|serde_json::to_vec|signable_bytes/) has_signable_reconstruction = 1
    }
    END {
        if (fn_name != "" && is_verify_fn && has_raw_hash_verify && !has_signable_reconstruction && !has_exempt) {
            printf "SIGN/VERIFY MESSAGE MISMATCH: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Function verifies a signature against a raw hash field (output_hash or\n"
            printf "  content_hash) but does not reconstruct the JSON signable structure that\n"
            printf "  the signing code uses. Ed25519 requires the exact same message bytes for\n"
            printf "  sign and verify. If the signer serializes a JSON object containing the\n"
            printf "  hash (plus other fields), the verifier must reconstruct that same JSON —\n"
            printf "  not pass the raw hash bytes directly.\n"
            printf "  Fix: reconstruct the signable JSON, or use a shared signable_bytes() helper.\n"
            printf "  See: specs/reviews/task-008.md F6 (sign/verify message mismatch)\n\n"
            violations++
        }
        if (fn_name != "" && is_verify_fn && has_raw_hash_verify) checked++
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
    echo "     cryptographic operations -- not just structural/format checks."
    echo "     Class 1: After decoding crypto material, call ed25519_verify or equivalent."
    echo "     Class 2: Verification functions must call ring::signature, .verify(), etc."
    echo "     Structural checks (expiry, chain depth, non-empty) are necessary but not sufficient."
    echo "     Class 3: Delegation structures (DerivedInput) must be signed by the PARENT's"
    echo "     existing key from storage, not a newly generated child key."
    echo "     Class 4: Verify functions that handle multiple input types (Signed/Derived)"
    echo "     must perform crypto verification for ALL branches, not just Signed."
    echo "     Class 5: Verify functions must reconstruct the same signable message the signer"
    echo "     used. Passing raw output_hash/content_hash to .verify() when the signer serialized"
    echo "     a JSON structure containing the hash = message mismatch = always-failing verification."
    echo "${VIOLATIONS} violation(s) found out of ${CHECKED} functions checked."
    exit 1
fi
