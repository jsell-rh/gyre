#!/usr/bin/env bash
# Architecture lint: detect hardcoded string-literal defaults where runtime
# values should be used.
#
# When a runtime value (e.g., repository.default_branch) is available in the
# calling context but the callee hardcodes a string literal (e.g., "main"),
# the code silently produces wrong results for non-default configurations.
#
# This script detects known hardcoded-default patterns:
#   1. default_branch: "main" — should come from the repository record
#   2. Other patterns can be added as discovered
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

echo "Checking for hardcoded default values..."

# ── Check 1: Hardcoded default_branch ─────────────────────────────────
# Any assignment like `default_branch: "main".to_string()` or
# `default_branch: "main".into()` outside of test functions, test modules,
# and repository creation (where "main" is the genuine default for new repos).

# Exempt patterns:
#   - Inside #[cfg(test)] modules or test_ functions
#   - In repos.rs create_repo handler (where "main" is the default for new repos)

HARDCODED_HITS=$(grep -rn 'default_branch.*"main"' "$SERVER_SRC" \
    --include='*.rs' \
    | grep -v '#\[cfg(test)\]' \
    | grep -v 'fn test_' \
    | grep -v '// hardcoded-default:ok' \
    | grep -v '\.to_string().*// new repo default' \
    || true)

if [ -n "$HARDCODED_HITS" ]; then
    echo ""
    echo "HARDCODED DEFAULT BRANCH found:"
    echo "$HARDCODED_HITS" | while IFS= read -r line; do
        echo "  $line"
        VIOLATIONS=$((VIOLATIONS + 1))
    done
    echo ""
    echo "  default_branch should come from the repository record, not be hardcoded."
    echo "  If this is intentional (e.g., new repo creation), add comment: // hardcoded-default:ok"
    echo "  See: specs/reviews/task-007.md F3 (hardcoded default_branch)"
    echo ""
    FAIL=1
fi

# ── Result ──────────────────────────────────────────────────────────────

if [ "$FAIL" -eq 0 ]; then
    echo "Hardcoded defaults lint passed."
    exit 0
else
    echo "Fix: Pass the runtime value from the calling context instead of hardcoding."
    echo "     Add '// hardcoded-default:ok' comment if genuinely intentional."
    exit 1
fi
