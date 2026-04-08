#!/usr/bin/env bash
# Architecture lint: detect client-side stub implementations of operations
# that require server-side evaluation.
#
# When a spec requires "evaluate constraints against repo state" or "validate
# CEL expression," the implementation must call a server endpoint.  A client-
# side handler that performs string heuristics (checking for '.', '==', etc.)
# instead of calling the server is a stub — it produces false positives and
# false negatives.
#
# This script detects known stub patterns in Svelte components:
#   1. Functions named dryRun/dry_run/validate that don't make fetch/API calls
#   2. Expression "validation" via string includes/regex instead of server eval
#
# See: specs/reviews/task-007.md F5
#
# Run by pre-commit and CI.

set -euo pipefail

WEB_SRC="web/src"
FAIL=0

if [ ! -d "$WEB_SRC" ]; then
    echo "Skipping stub evaluation check: $WEB_SRC not found"
    exit 0
fi

echo "Checking for client-side stub evaluations..."

# ── Check 1: dryRun / dry_run / validate functions without fetch ──────
# Find Svelte/TS files that define a dryRun-like function.  If the function
# body doesn't contain 'fetch(' or 'api.' or '$api' or 'invoke(' (common
# patterns for server calls), it's likely a stub.

# Extract function blocks that look like evaluation handlers
STUB_FILES=""

for file in $(grep -rl 'dryRun\|dry_run\|dryrun' "$WEB_SRC" --include='*.svelte' --include='*.ts' 2>/dev/null || true); do
    # Check if the file has a function named dryRun (or similar) that
    # does NOT contain a fetch/API call
    if grep -qE 'async\s+function\s+dryRun|function\s+dryRun|const\s+dryRun|let\s+dryRun' "$file" 2>/dev/null; then
        if ! grep -qE 'fetch\(|\.post\(|\.get\(|\.put\(|\$api|api\.|invoke\(' "$file" 2>/dev/null; then
            STUB_FILES="$STUB_FILES  $file\n"
        fi
    fi
done

if [ -n "$STUB_FILES" ]; then
    echo ""
    echo "CLIENT-SIDE STUB EVALUATION found:"
    echo -e "$STUB_FILES"
    echo "  These files define dryRun/validate functions but make no server API calls."
    echo "  If the spec says 'evaluate against repo state' or 'validate expression,'"
    echo "  the handler must call a server endpoint — client-side string checks are"
    echo "  not equivalent to server-side CEL/constraint evaluation."
    echo ""
    echo "  See: specs/reviews/task-007.md F5 (stub dry-run)"
    echo ""
    FAIL=1
fi

# ── Check 2: String heuristic validation of CEL expressions ──────────
# Detect patterns like `expression.includes('.')` or `expression.includes('==')`
# which are string-level heuristics, not real CEL parsing/evaluation.

HEURISTIC_HITS=$(grep -rn "\.includes('\\.')\|\.includes('==')\|\.includes('>=')\|\.includes('<=')\|\.includes('!=')" "$WEB_SRC" \
    --include='*.svelte' --include='*.ts' \
    | grep -i 'expression\|constraint\|cel\|valid' \
    | grep -v '// stub-check:ok' \
    || true)

if [ -n "$HEURISTIC_HITS" ]; then
    echo ""
    echo "STRING HEURISTIC VALIDATION of expressions found:"
    echo "$HEURISTIC_HITS" | while IFS= read -r line; do
        echo "  $line"
    done
    echo ""
    echo "  Checking if a string contains '.' or '==' is not CEL validation."
    echo "  Valid CEL like 'output.changed_files.size() < 50' may be flagged as invalid,"
    echo "  while invalid syntax like 'foo == bar == baz' passes.  Use the server's"
    echo "  CEL evaluator for real validation."
    echo ""
    echo "  Add '// stub-check:ok' if this is genuinely not a validation stub."
    echo ""
    FAIL=1
fi

# ── Check 3: Hardcoded server-derived display data ─────────────────────
# Detect Svelte components that locally construct arrays representing
# server-derived data (strategy-implied constraints, derived policies,
# etc.) instead of fetching them from the server.
#
# The pattern: a file pushes items into a "strategy" or "implied" array
# inside a $derived block or function body.  This means the component
# is building a local approximation of what the server's derivation
# function produces.  Even if the file has OTHER API calls (e.g., for
# dry-run evaluation), the strategy/implied data itself must be fetched.
#
# See: specs/reviews/task-007.md F6

HARDCODED_DISPLAY_HITS=""

# Find files that push items into arrays named *implied* or *strategy*
# (the hallmark of locally-constructed derived data).
for file in $(grep -rl 'implied\.push\|strategyConstraints\s*=\s*\[' "$WEB_SRC" --include='*.svelte' --include='*.ts' 2>/dev/null || true); do
    # Skip files with an explicit opt-out comment
    if grep -q '// hardcoded-display:ok' "$file" 2>/dev/null; then
        continue
    fi

    # Check if the file fetches strategy/implied data from the server.
    # A valid fetch looks like: api.getStrategyConstraints, api.fetchImplied,
    # fetch(...strategy...), etc.  We check for any fetch/API call whose
    # context mentions strategy or implied or derived constraints.
    HAS_STRATEGY_FETCH=0
    if grep -qE '(fetch|api\.|getStrategy|fetchImplied|fetchDerived|getImplied).*([Ss]trateg|[Ii]mplied|[Dd]erived)' "$file" 2>/dev/null; then
        HAS_STRATEGY_FETCH=1
    fi
    # Also check the reverse order (strategy keyword before fetch call on same/nearby lines)
    if grep -qE '([Ss]trateg|[Ii]mplied|[Dd]erived).*(fetch|api\.|await)' "$file" 2>/dev/null; then
        HAS_STRATEGY_FETCH=1
    fi

    if [ "$HAS_STRATEGY_FETCH" -eq 0 ]; then
        HARDCODED_DISPLAY_HITS="$HARDCODED_DISPLAY_HITS  $file\n"
    fi
done

if [ -n "$HARDCODED_DISPLAY_HITS" ]; then
    echo ""
    echo "HARDCODED SERVER-DERIVED DISPLAY DATA found:"
    echo -e "$HARDCODED_DISPLAY_HITS"
    echo "  These files construct strategy-implied/derived data arrays locally"
    echo "  instead of fetching them from the server.  When a UI section displays"
    echo "  data that is derived by server-side logic (e.g., strategy-implied"
    echo "  constraints from workspace config, trust levels, attestation policies),"
    echo "  the component must fetch the full set via an API call."
    echo ""
    echo "  A hardcoded subset (e.g., only 'meta-spec set match') hides constraints"
    echo "  the user needs to see at approval time.  The server's derive function"
    echo "  produces the authoritative set."
    echo ""
    echo "  See: specs/reviews/task-007.md F6 (hardcoded strategy-implied display)"
    echo ""
    echo "  Add '// hardcoded-display:ok' if genuinely intentional."
    echo ""
    FAIL=1
fi

# ── Check 4: Server-side syntax-only validation posing as evaluation ──
#
# When a spec says "dry-run evaluates constraints against repo state," the
# server endpoint must actually *evaluate* constraints — not just *compile*
# them.  CEL compilation (Program::compile / validate_cel_expression) only
# checks syntax.  Evaluation requires building a context from repo state
# and calling evaluate_all / evaluate / eval.
#
# A handler that calls validate_cel_expression or Program::compile but NOT
# evaluate_all / evaluate / build_cel_context is a syntax-only validation
# endpoint masquerading as a dry-run evaluator.
#
# See: specs/reviews/task-007.md F10

SYNTAX_ONLY_HITS=""

RUST_SRC="crates"
if [ -d "$RUST_SRC" ]; then
    # Find handlers/functions that reference "dry-run" or "validate" in doc
    # comments and call validate_cel_expression but not evaluate_all/evaluate.
    for file in $(grep -rl 'validate_cel_expression\|validate_constraints' "$RUST_SRC" --include='*.rs' 2>/dev/null || true); do
        # Skip test modules and the constraint_evaluator itself (where the fn is defined)
        if echo "$file" | grep -q 'constraint_evaluator\.rs$'; then
            continue
        fi

        # Check if any function in this file:
        # (a) has "dry-run" or "dry_run" or "validate" in a doc comment
        # (b) calls validate_cel_expression (syntax-only check)
        # (c) does NOT call evaluate_all, evaluate, or build_cel_context
        if grep -qE '///.*([Dd]ry.?[Rr]un|validate|§7\.6)' "$file" 2>/dev/null; then
            if grep -qE 'validate_cel_expression' "$file" 2>/dev/null; then
                if ! grep -qE 'evaluate_all|evaluate_constraints_against|build_cel_context|\.evaluate\(' "$file" 2>/dev/null; then
                    # This file has a dry-run/validate handler that only does syntax validation
                    # Find the specific function(s)
                    FUNCS=$(grep -n 'validate_cel_expression' "$file" 2>/dev/null | grep -v '// syntax-only:ok' || true)
                    if [ -n "$FUNCS" ]; then
                        while IFS= read -r funcline; do
                            SYNTAX_ONLY_HITS="$SYNTAX_ONLY_HITS  $file:$funcline (syntax-only: calls validate_cel_expression but not evaluate)\n"
                        done <<< "$FUNCS"
                    fi
                fi
            fi
        fi
    done
fi

if [ -n "$SYNTAX_ONLY_HITS" ]; then
    echo ""
    echo "SYNTAX-ONLY VALIDATION posing as evaluation/dry-run:"
    echo -e "$SYNTAX_ONLY_HITS"
    echo "  These server handlers validate CEL syntax (compile) but do not evaluate"
    echo "  constraints against actual repo state.  The spec §7.6 requires dry-run"
    echo "  to 'evaluate the constraint set against the current repo state to preview"
    echo "  what would pass/fail.'  Compilation only checks if the expression is"
    echo "  parseable — it does NOT check whether the constraint would pass or fail"
    echo "  against real data."
    echo ""
    echo "  A user entering 'agent.attestation_level >= 99' sees 'valid' (it parses)"
    echo "  even though it would fail for every agent.  This is a false confidence"
    echo "  bug — the user proceeds to approve, then discovers failures at push time."
    echo ""
    echo "  Fix: The endpoint must accept repo_id + workspace_id, build an evaluation"
    echo "  context from actual repo/workspace state, call evaluate_all(), and return"
    echo "  per-constraint pass/fail results."
    echo ""
    echo "  Add '// syntax-only:ok' on the validate_cel_expression call if syntax-only"
    echo "  validation is genuinely the intended behavior (not for dry-run endpoints)."
    echo ""
    echo "  See: specs/reviews/task-007.md F10"
    echo ""
    FAIL=1
fi

# ── Result ──────────────────────────────────────────────────────────────

if [ "$FAIL" -eq 0 ]; then
    echo "Stub evaluation check passed."
    exit 0
else
    echo "Fix: Replace client-side heuristics with server-side API calls."
    echo "     Add a server endpoint that uses the domain's evaluator if one doesn't exist."
    exit 1
fi
