#!/usr/bin/env bash
# check-dead-collections.sh — Detect mutable collection variables that are
# populated but never read.
#
# When a function declares a mutable collection (HashSet, HashMap, Vec,
# BTreeMap, BTreeSet) and populates it via .insert(), .push(), .extend(),
# but the collection's final state is never consumed by downstream code
# (not returned, not passed to another function, not read in a condition,
# not iterated), the variable is dead code.
#
# The compiler and clippy do NOT flag this because the mutations are valid
# operations on a local variable — the variable IS "used" from the
# compiler's perspective, but its accumulated state is discarded.
#
# This pattern commonly occurs during iterative algorithm development:
# the old tracking variable is replaced by a new one but not deleted.
#
# Detection strategy:
#   For each `let mut VAR: Collection<` in non-test Rust code, check if
#   VAR appears in any READ context (.contains(), .get(), .len(),
#   .is_empty(), .iter(), for x in &VAR, return VAR, &VAR as argument).
#   If the only operations are WRITES (.insert(), .push(), .extend(),
#   .entry()), the variable is dead.
#
# Origin: specs/reviews/task-028.md F6 — speculated_clean: HashSet<String>
#         was populated via .insert() but never read. Only speculated_mr_ids
#         was used for dependency satisfaction checks.
#
# Exempt a line with: // dead-collection:ok — <reason>
#
# Run by pre-commit and CI.

set -euo pipefail

VIOLATIONS=0

# Collection type patterns to match
COLLECTION_TYPES="HashSet|HashMap|Vec|BTreeMap|BTreeSet|VecDeque"

# Scan all non-test Rust source files
for src_file in $(find crates/ -name '*.rs' -not -path '*/tests/*' 2>/dev/null); do
    # Find test boundary (skip test modules)
    TEST_BOUNDARY=$(grep -n '#\[cfg(test)\]' "$src_file" 2>/dev/null | head -1 | cut -d: -f1 || echo "999999")

    # Find all `let mut VAR: CollectionType<` or `let mut VAR = CollectionType::new()`
    while IFS=: read -r lineno content; do
        [ -z "$lineno" ] && continue

        # Skip test code
        if [ "$lineno" -ge "$TEST_BOUNDARY" ]; then
            continue
        fi

        # Skip exempted lines
        if echo "$content" | grep -q 'dead-collection:ok'; then
            continue
        fi

        # Extract the variable name
        var_name=$(echo "$content" | grep -oP 'let\s+mut\s+\K[a-z_][a-z_0-9]*' | head -1)
        [ -z "$var_name" ] && continue

        # Find the enclosing function boundaries
        fn_start=$(head -n "$lineno" "$src_file" \
            | grep -n '^\s*pub\s\+async\s\+fn\|^\s*pub\s\+fn\|^\s*async\s\+fn\|^\s*fn ' \
            | tail -1 | cut -d: -f1 || echo "1")

        # Find function end: look for the next function definition after our line
        fn_end_offset=$(tail -n +"$((lineno + 1))" "$src_file" \
            | grep -n '^\s*pub\s\+async\s\+fn\|^\s*pub\s\+fn\|^\s*async\s\+fn\|^\s*fn ' \
            | head -1 | cut -d: -f1 || echo "500")
        fn_end=$((lineno + fn_end_offset))

        # Cap at test boundary
        if [ "$fn_end" -gt "$TEST_BOUNDARY" ]; then
            fn_end=$TEST_BOUNDARY
        fi

        # Extract function body from the variable declaration to function end
        fn_body=$(sed -n "${lineno},${fn_end}p" "$src_file")

        # Count READ operations on this variable (excluding the declaration line itself)
        # Read patterns: .contains(), .get(), .len(), .is_empty(), .iter(),
        # for x in &VAR, &VAR (as function arg), VAR. (method calls that aren't writes)
        read_count=$(echo "$fn_body" | tail -n +2 | grep -cP \
            "${var_name}\.(contains|get|len|is_empty|iter|into_iter|values|keys|drain)\b|&${var_name}\b|for\s+.*\s+in\s+(&?${var_name}|\$\{${var_name}\})|return\s+${var_name}\b|\b${var_name}\s*$" \
            2>/dev/null || true)
        read_count=${read_count:-0}

        # Count WRITE operations (to confirm the variable IS being written to)
        write_count=$(echo "$fn_body" | tail -n +2 | grep -cP \
            "${var_name}\.(insert|push|extend|entry|push_back|push_front|append)\b" \
            2>/dev/null || true)
        write_count=${write_count:-0}

        # If there are writes but no reads, the variable is dead
        if [ "$write_count" -gt 0 ] && [ "$read_count" -eq 0 ]; then
            echo ""
            echo "DEAD COLLECTION VARIABLE: $src_file:$lineno"
            echo "  $content"
            echo "  Variable '$var_name' is populated ($write_count write(s)) but never read."
            echo "  The accumulated state is discarded — this is dead code from iterative"
            echo "  development. Remove the variable and its write operations."
            echo ""
            echo "  Exempt with '// dead-collection:ok — <reason>' on the declaration line."
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    done < <(grep -n "let mut [a-z_][a-z_0-9]*\s*:\s*\(${COLLECTION_TYPES}\)\|let mut [a-z_][a-z_0-9]*\s*=\s*\(${COLLECTION_TYPES}\)::new" "$src_file" 2>/dev/null \
        | grep -vE '^\s*//' || true)
done

echo ""
if [ "$VIOLATIONS" -eq 0 ]; then
    echo "check-dead-collections: OK"
    exit 0
else
    echo "check-dead-collections: FAILED — $VIOLATIONS dead collection variable(s) found"
    echo "  Remove dead collection variables or exempt with // dead-collection:ok — <reason>"
    exit 1
fi
