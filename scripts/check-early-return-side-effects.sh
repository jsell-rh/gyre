#!/usr/bin/env bash
# Architecture lint: verify that early returns in multi-effect functions do not
# skip critical independent operations.
#
# Flaw class: Early-return side-effect regression (see TASK-008 R3 F1)
#
# When a function performs multiple independent operations (e.g., generate
# child keypair AND create DerivedInput), an early return added to guard one
# operation must not skip the other. The most common failure mode: a fix
# changes the signing entity for DerivedInput and adds an early return when
# the spawner's key is unavailable — but this return exits before the child
# agent's keypair is generated. The child key generation is independent of
# the DerivedInput signing and must always execute.
#
# Check 1: Key generation after early return
#   Detects functions that have BOTH:
#   (a) An early return (return Ok/return Err/return None/return;/?)
#   (b) Key generation code (Ed25519KeyPair::generate, generate_pkcs8)
#   where the early return appears BEFORE the key generation in the source.
#   This suggests the early return may skip key generation.
#
# Check 2: Key storage after early return
#   Detects functions that have BOTH:
#   (a) An early return
#   (b) Key storage code (kv_set.*signing_key, store.*key)
#   where the early return appears BEFORE the key storage.
#
# Exempt a function with: // early-return:ok — <reason>
#
# Scope: all non-test Rust source files under crates/gyre-server/src/
#
# Run by pre-commit and CI.

set -euo pipefail

SERVER_SRC="crates/gyre-server/src"
VIOLATIONS=0
CHECKED=0

if [ ! -d "$SERVER_SRC" ]; then
    echo "ERROR: Cannot find $SERVER_SRC"
    exit 1
fi

echo "Checking for early returns that skip critical side effects..."

# ── Check 1 & 2: Early return before key generation/storage ─────────

for file in $(find "$SERVER_SRC" -name '*.rs' -type f | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" '
    # Skip test modules
    /^\s*#\[cfg\(test\)\]/ { in_test_module = 1; next }

    # Detect function boundaries
    /^\s*(pub\s+)?(async\s+)?fn\s+/ {
        # Check previous function
        if (fn_name != "" && !has_exempt) {
            if (has_keygen && early_return_before_keygen) {
                printf "EARLY RETURN BEFORE KEY GENERATION: %s in %s:%d\n", fn_name, file, fn_start
                printf "  Function has an early return at line %d that exits before key\n", early_return_line
                printf "  generation at line %d. If key generation is independent of the\n", keygen_line
                printf "  guarded operation, it should execute unconditionally.\n"
                printf "  Fix: Move key generation before the early return, or restructure\n"
                printf "  the function so independent operations are not skipped.\n"
                printf "  See: specs/reviews/task-008.md R3 F1 (early-return regression)\n\n"
                violations++
            }
            if (has_key_store && early_return_before_key_store) {
                printf "EARLY RETURN BEFORE KEY STORAGE: %s in %s:%d\n", fn_name, file, fn_start
                printf "  Function has an early return at line %d that exits before key\n", early_return_line_store
                printf "  storage at line %d. If key storage is independent of the guarded\n", key_store_line
                printf "  operation, it should execute unconditionally.\n"
                printf "  Fix: Move key storage before the early return, or restructure\n"
                printf "  the function so independent operations are not skipped.\n"
                printf "  See: specs/reviews/task-008.md R3 F1 (early-return regression)\n\n"
                violations++
            }
        }
        if (fn_name != "" && (has_keygen || has_key_store)) checked++

        # Parse function name
        match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        fn_name = m[1]
        fn_start = NR
        has_keygen = 0
        has_key_store = 0
        has_early_return = 0
        early_return_before_keygen = 0
        early_return_before_key_store = 0
        early_return_line = 0
        early_return_line_store = 0
        keygen_line = 0
        key_store_line = 0
        has_exempt = 0
        # Skip test functions
        if (fn_name ~ /^test_/ || in_test_module) fn_name = ""
        next
    }
    fn_name != "" {
        # Check for exemption
        if ($0 ~ /early-return:ok/) has_exempt = 1

        # Detect early return patterns (return Ok/Err/None, return;, ? operator on its own line)
        # Only match explicit returns, not ? on method chains
        if ($0 ~ /return\s+(Ok|Err|None|;)/ || $0 ~ /^\s*return;/) {
            if (!has_early_return) {
                has_early_return = 1
                early_return_line = NR
                early_return_line_store = NR
            }
        }

        # Detect key generation
        if ($0 ~ /generate_pkcs8|Ed25519KeyPair::generate|KeyPair::generate|ed25519.*generate/) {
            has_keygen = 1
            keygen_line = NR
            if (has_early_return) early_return_before_keygen = 1
        }

        # Detect key storage
        if ($0 ~ /kv_set.*signing_key|kv_set.*agent_signing|store.*signing_key|insert.*signing_key/) {
            has_key_store = 1
            key_store_line = NR
            if (has_early_return) early_return_before_key_store = 1
        }
    }
    END {
        # Check last function
        if (fn_name != "" && !has_exempt) {
            if (has_keygen && early_return_before_keygen) {
                printf "EARLY RETURN BEFORE KEY GENERATION: %s in %s:%d\n", fn_name, file, fn_start
                printf "  Function has an early return at line %d that exits before key\n", early_return_line
                printf "  generation at line %d. If key generation is independent of the\n", keygen_line
                printf "  guarded operation, it should execute unconditionally.\n"
                printf "  Fix: Move key generation before the early return, or restructure\n"
                printf "  the function so independent operations are not skipped.\n"
                printf "  See: specs/reviews/task-008.md R3 F1 (early-return regression)\n\n"
                violations++
            }
            if (has_key_store && early_return_before_key_store) {
                printf "EARLY RETURN BEFORE KEY STORAGE: %s in %s:%d\n", fn_name, file, fn_start
                printf "  Function has an early return at line %d that exits before key\n", early_return_line_store
                printf "  storage at line %d. If key storage is independent of the guarded\n", key_store_line
                printf "  operation, it should execute unconditionally.\n"
                printf "  Fix: Move key storage before the early return, or restructure\n"
                printf "  the function so independent operations are not skipped.\n"
                printf "  See: specs/reviews/task-008.md R3 F1 (early-return regression)\n\n"
                violations++
            }
        }
        if (fn_name != "" && (has_keygen || has_key_store)) checked++
        printf "SUMMARY:%d:%d\n", checked, violations
    }
    ' "$file" | while IFS= read -r line; do
        case "$line" in
            SUMMARY:*)
                c=$(echo "$line" | cut -d: -f2)
                v=$(echo "$line" | cut -d: -f3)
                echo "$c $v" >> /tmp/check-early-return-$$
                ;;
            *)
                echo "$line"
                ;;
        esac
    done
done

# ── Tally results ─────────────────────────────────────────────────────

if [ -f /tmp/check-early-return-$$ ]; then
    while read -r c v; do
        CHECKED=$((CHECKED + c))
        VIOLATIONS=$((VIOLATIONS + v))
    done < /tmp/check-early-return-$$
    rm -f /tmp/check-early-return-$$
fi

echo ""
if [ "$VIOLATIONS" -eq 0 ]; then
    echo "Early-return side-effect lint passed: ${CHECKED} functions checked."
    echo "No early returns skip critical key generation or storage operations."
    exit 0
else
    echo "Fix: When adding early returns to multi-effect functions, verify that"
    echo "     independent operations (key generation, key storage) are not skipped."
    echo "     Move independent operations before the early return, or restructure"
    echo "     the function to separate independent and dependent operations."
    echo "     Exempt with: // early-return:ok — <reason>"
    echo "${VIOLATIONS} violation(s) found out of ${CHECKED} functions checked."
    exit 1
fi
