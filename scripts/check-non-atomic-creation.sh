#!/usr/bin/env bash
# Architecture lint: detect non-atomic entity creation with dependent records.
#
# When a handler creates a parent entity (e.g., workspace) and then separately
# creates dependent records (e.g., trust policies, bindings) via individual
# repository calls, a failure mid-sequence leaves a partially-initialized entity.
# The parent exists without all its required dependencies — violating domain
# invariants silently.
#
# This script detects handler functions that contain multiple distinct
# `state.<repo>.create(` calls without using a transactional wrapper.  When
# entity A's integrity depends on records B1..Bn, all creations must happen
# atomically — either through a single domain service method that uses a
# transaction, or by wrapping the calls in a transaction block.
#
# The pattern detected:
#   1. A function has 2+ distinct `state.<repo>.create(` calls where <repo>
#      names differ (e.g., `state.workspaces.create` + `state.policies.create`).
#   2. There is no transaction wrapper (`transaction`, `begin_transaction`,
#      `apply_trust_transition`, or similar atomic domain method) enclosing both.
#
# Legitimate patterns (non-findings):
#   - A function that creates multiple records of the SAME type in a loop
#     (e.g., seeding initial data) where each record is independent.
#   - Functions that use a transactional domain method for the dependent creation.
#
# Exempt a line with: // non-atomic-create:ok — <reason>
#
# See: specs/reviews/task-077.md F2 (workspace creation policy seeding)
#
# Run by pre-commit and CI.

set -euo pipefail

SERVER_SRC="crates/gyre-server/src"
FAIL=0

if [ ! -d "$SERVER_SRC" ]; then
    echo "Skipping non-atomic creation check: $SERVER_SRC not found"
    exit 0
fi

echo "Checking for non-atomic entity creation with dependent records..."

# Strategy: For each non-test .rs file, find functions that contain
# `state.<repo1>.create(` AND `state.<repo2>.create(` where repo1 != repo2,
# without a transaction wrapper between them.

check_non_atomic_creation() {
    local file="$1"

    # Skip test files
    if echo "$file" | grep -qE '(/tests/|_test\.rs)'; then
        return 0
    fi

    # Find all create calls: state.<repo>.create(
    local create_calls
    create_calls=$(grep -n 'state\.[a-z_]*\.create(' "$file" 2>/dev/null \
        | grep -v '// non-atomic-create:ok\|#\[test\]\|#\[cfg(test)\]' \
        || true)

    if [ -z "$create_calls" ]; then
        return 0
    fi

    # Extract unique repository names from create calls
    local repo_names
    repo_names=$(echo "$create_calls" | grep -oP 'state\.(\K[a-z_]+)(?=\.create\()' | sort -u || true)

    local repo_count
    repo_count=$(echo "$repo_names" | grep -c '.' || true)

    if [ "$repo_count" -lt 2 ]; then
        return 0
    fi

    # Multiple distinct repos have .create() calls in this file.
    # Now check per-function: find functions with 2+ distinct repo create calls.

    awk -v file="$file" '
    /^\s*(pub\s+)?(async\s+)?fn\s+/ {
        # Emit previous function results if any
        if (fn_name != "" && !has_exempt && distinct_repos > 1 && !has_transaction) {
            printf "NON-ATOMIC CREATION: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Creates entities via %d different repositories without a transaction:\n", distinct_repos
            for (r in repos_seen) {
                printf "    - state.%s.create() at line %s\n", r, repos_seen[r]
            }
            printf "\n"
            printf "  If any creation fails mid-sequence, the parent entity exists without\n"
            printf "  all its required dependent records — silently violating domain invariants.\n"
            printf "\n"
            printf "  Fix options:\n"
            printf "    1. Use a transactional domain service method for the entire creation\n"
            printf "    2. Wrap all creations in a single database transaction\n"
            printf "    3. If intentional: add '\''// non-atomic-create:ok — <reason>'\'' on each create line\n"
            printf "\n"
            violations++
        }

        # Reset for new function
        match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        fn_name = m[1]
        fn_start = NR
        has_exempt = 0
        has_transaction = 0
        distinct_repos = 0
        delete repos_seen

        # Skip test functions
        if (fn_name ~ /^test_/) fn_name = ""
        next
    }
    fn_name != "" {
        if ($0 ~ /non-atomic-create:ok/) has_exempt = 1

        # Detect transaction wrappers
        if ($0 ~ /transaction|begin_transaction|apply_trust_transition|\.atomic\(/) {
            has_transaction = 1
        }

        # Detect state.<repo>.create( calls
        if (match($0, /state\.([a-z_]+)\.create\(/, m)) {
            repo = m[1]
            if (!(repo in repos_seen)) {
                repos_seen[repo] = NR
                distinct_repos++
            }
        }
    }
    END {
        # Check last function
        if (fn_name != "" && !has_exempt && distinct_repos > 1 && !has_transaction) {
            printf "NON-ATOMIC CREATION: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Creates entities via %d different repositories without a transaction:\n", distinct_repos
            for (r in repos_seen) {
                printf "    - state.%s.create() at line %s\n", r, repos_seen[r]
            }
            printf "\n"
            printf "  If any creation fails mid-sequence, the parent entity exists without\n"
            printf "  all its required dependent records — silently violating domain invariants.\n"
            printf "\n"
            printf "  Fix options:\n"
            printf "    1. Use a transactional domain service method for the entire creation\n"
            printf "    2. Wrap all creations in a single database transaction\n"
            printf "    3. If intentional: add '\''// non-atomic-create:ok — <reason>'\'' on each create line\n"
            printf "\n"
            violations++
        }
        printf "VIOLATIONS:%d\n", violations
    }
    ' "$file"
}

# Collect violations
TOTAL_VIOLATIONS=0
while read -r file; do
    output=$(check_non_atomic_creation "$file")
    if [ -n "$output" ]; then
        # Extract violation count from last line
        v=$(echo "$output" | grep '^VIOLATIONS:' | cut -d: -f2)
        if [ -n "$v" ] && [ "$v" -gt 0 ]; then
            # Print everything except the VIOLATIONS: line
            echo "$output" | grep -v '^VIOLATIONS:'
            TOTAL_VIOLATIONS=$((TOTAL_VIOLATIONS + v))
        fi
    fi
done < <(find "$SERVER_SRC" -name '*.rs' -type f | sort)

echo ""
if [ "$TOTAL_VIOLATIONS" -eq 0 ]; then
    echo "Non-atomic creation check passed."
    echo "No handlers found with multi-repository creation without transaction wrapping."
    exit 0
else
    echo "Fix: Wrap related entity creations in a single transaction or use a"
    echo "     transactional domain service method."
    echo "     Exempt with: // non-atomic-create:ok — <reason>"
    echo "See: specs/reviews/task-077.md F2 (non-atomic workspace+policy creation)"
    echo "${TOTAL_VIOLATIONS} violation(s) found."
    exit 1
fi
