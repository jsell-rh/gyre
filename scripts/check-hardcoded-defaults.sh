#!/usr/bin/env bash
# Architecture lint: detect hardcoded or empty-default values in evaluation
# contexts where runtime values should be used.
#
# When a runtime value (e.g., repository.default_branch, agent meta_spec_set_sha)
# is available in the calling context but the callee hardcodes a string literal,
# uses a wrong variable, or leaves an empty default (String::new(), 0), the code
# silently produces wrong results — or in the case of empty defaults feeding
# unconditional constraints, produces false violations on every evaluation.
#
# This script detects known hardcoded/empty-default patterns:
#   1. default_branch: "main" — should come from the repository record
#   2. default_branch: target_branch — wrong variable (MR target ≠ repo default)
#   3. default_branch from suspicious non-repo source
#   4. Empty defaults in AgentContext fields that feed constraint evaluation
#
# See: specs/reviews/task-007.md F3, F4, F8
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

# ── Check 2: default_branch assigned from wrong variable ─────────────
# A TargetContext that sets `default_branch: target_branch.to_string()` or
# `default_branch: branch.to_string()` is using the MR/push target branch,
# not the repo's actual default branch.  This is silently wrong when the MR
# targets a non-default branch.
#
# Valid patterns:
#   default_branch: default_branch.to_string()  (parameter named default_branch)
#   default_branch: repo.default_branch...       (field from repo record)
#   default_branch: default_branch_clone...      (clone of the above)
# Invalid patterns:
#   default_branch: target_branch.to_string()
#   default_branch: branch.to_string()
#   default_branch: branch_name.to_string()

WRONG_VAR_HITS=$(grep -rn 'default_branch:\s*target_branch\|default_branch:\s*branch\.to_string\|default_branch:\s*branch_name\.to_string' "$SERVER_SRC" \
    --include='*.rs' \
    | grep -v '#\[cfg(test)\]' \
    | grep -v 'fn test_' \
    | grep -v '// hardcoded-default:ok' \
    || true)

if [ -n "$WRONG_VAR_HITS" ]; then
    echo ""
    echo "WRONG VARIABLE for default_branch found:"
    echo "$WRONG_VAR_HITS" | while IFS= read -r line; do
        echo "  $line"
    done
    echo ""
    echo "  default_branch must come from the repository's actual default_branch field,"
    echo "  NOT from the MR target branch or push ref.  target_branch is the branch the"
    echo "  MR targets — often the default branch, but not always (feature branches,"
    echo "  release branches, etc.).  Using target_branch makes the constraint"
    echo "  'target.branch == target.default_branch' always true, which is meaningless."
    echo ""
    echo "  See: specs/reviews/task-007.md F4 (same-class as F3)"
    echo ""
    FAIL=1
fi

# ── Check 3: default_branch in TargetContext not from repo record ─────
# Any TargetContext construction that has a `default_branch:` field should
# reference either a parameter named `default_branch` or `repo.default_branch`
# (or a variable derived from them).  This catches future instances where
# someone invents a new wrong source.

TARGETCTX_HITS=$(grep -rn 'default_branch:' "$SERVER_SRC" \
    --include='*.rs' \
    | grep -v '#\[cfg(test)\]' \
    | grep -v 'fn test_' \
    | grep -v '// hardcoded-default:ok' \
    | grep -v 'default_branch:\s*default_branch' \
    | grep -v 'default_branch:\s*repo\.default_branch' \
    | grep -v 'default_branch:\s*&repo\.default_branch' \
    | grep -v 'default_branch:\s*default_branch_clone' \
    | grep -v 'default_branch:\s*[a-z_]*\.default_branch' \
    | grep -v 'pub default_branch' \
    | grep -v '///\|//!' \
    | grep -v 'default_branch:\s*String' \
    | grep -v 'default_branch:\s*&str' \
    | grep -v 'default_branch:\s*Option' \
    | grep -v 'default_branch:\s*"main"' \
    || true)

if [ -n "$TARGETCTX_HITS" ]; then
    echo ""
    echo "SUSPICIOUS default_branch source found:"
    echo "$TARGETCTX_HITS" | while IFS= read -r line; do
        echo "  $line"
    done
    echo ""
    echo "  default_branch should be sourced from repo.default_branch or a parameter"
    echo "  named default_branch that was passed from the repo record."
    echo "  Add '// hardcoded-default:ok' if genuinely intentional."
    echo ""
    FAIL=1
fi

# ── Check 4: Empty defaults in evaluation context construction ──────────
# When building evaluation context structs (AgentContext, TargetContext),
# fields set to String::new() or literal 0 that feed into constraint
# generation will produce false violations on every evaluation.
#
# The pattern: build_agent_context sets `meta_spec_set_sha: String::new()`
# while derive_strategy_constraints unconditionally generates
# `agent.meta_spec_set_sha == input.meta_spec_set_sha`.  The empty string
# always mismatches, causing false violations and (under fail-closed
# evaluation) blocking all subsequent constraint checks.
#
# Exempt: test functions, cfg(test) modules, and lines with // empty-default:ok
#
# See: specs/reviews/task-007.md F8

EMPTY_DEFAULT_HITS=$(grep -rn 'AgentContext\|agent_context' "$SERVER_SRC" \
    --include='*.rs' -A 20 \
    | grep -E '(meta_spec_set_sha|attestation_level|stack_hash|image_hash|container_id):\s*(String::new\(\)|0[^-9x]|0$|"".to_string)' \
    | grep -v '#\[cfg(test)\]' \
    | grep -v 'fn test_' \
    | grep -v 'mod tests' \
    | grep -v '// empty-default:ok' \
    | grep -v '// hardcoded-default:ok' \
    || true)

if [ -n "$EMPTY_DEFAULT_HITS" ]; then
    echo ""
    echo "EMPTY DEFAULT in evaluation context found:"
    echo "$EMPTY_DEFAULT_HITS" | while IFS= read -r line; do
        echo "  $line"
    done
    echo ""
    echo "  Evaluation context fields set to String::new() or 0 will cause false"
    echo "  violations when constraint generators produce constraints against them."
    echo "  Either populate the field from runtime data (workspace record, KV store,"
    echo "  agent claims) or guard the constraint generation (don't generate constraints"
    echo "  for fields with empty/default context values)."
    echo ""
    echo "  See: specs/reviews/task-007.md F8 (empty agent context fields)"
    echo ""
    echo "  Add '// empty-default:ok' if genuinely intentional (field is not used in"
    echo "  constraint evaluation or the constraint is properly guarded)."
    echo ""
    FAIL=1
fi

# ── Check 5: Hardcoded policy/level return values ────────────────────────
# When a function's purpose is to look up or derive a configurable value
# (e.g., required attestation level, trust threshold, policy tier) from
# stored data, but it always returns the same hardcoded constant (e.g.,
# `Some(2)`, `Some(1)`, `Ok(3)`), it silently caps the value range and
# prevents higher (or lower) tiers from ever being expressed.
#
# Detection: functions named `get_*_level` or `get_*_required_*` whose
# bodies contain `Some(<integer>)` as a return without reading a level
# value from the data source.
#
# See: specs/reviews/task-059.md R1 F2
#
# Exempt: test functions, cfg(test) modules, and lines with // hardcoded-default:ok

# Strategy: find function definitions matching the pattern, extract their
# bodies (up to the closing brace), and check for hardcoded Some(N) returns
# — but only in non-test code.
POLICY_RETURN_HITS=""
while IFS= read -r fn_hit; do
    fn_file=$(echo "$fn_hit" | cut -d: -f1)
    fn_line=$(echo "$fn_hit" | cut -d: -f2)

    # Check if the function is inside a #[cfg(test)] module.
    # Find the last `mod tests {` line in the file; any function definition
    # after that line is test code.
    test_mod_line=$(grep -n 'mod tests' "$fn_file" | tail -1 | cut -d: -f1 || true)
    if [ -n "$test_mod_line" ] && [ "$fn_line" -gt "$test_mod_line" ]; then
        continue
    fi

    # Look for hardcoded Some(N) in the function body (next 20 lines).
    body_hit=$(sed -n "$((fn_line+1)),$((fn_line+20))p" "$fn_file" \
        | grep -n 'Some([0-9]\+)' \
        | grep -v '// hardcoded-default:ok' \
        || true)

    if [ -n "$body_hit" ]; then
        while IFS= read -r bline; do
            bline_num=$(echo "$bline" | cut -d: -f1)
            actual_line=$((fn_line + bline_num))
            POLICY_RETURN_HITS="${POLICY_RETURN_HITS}  ${fn_file}:${actual_line}: $(sed -n "${actual_line}p" "$fn_file")
"
        done <<< "$body_hit"
    fi
done < <(grep -rn 'fn get_.*\(level\|required\|policy\|threshold\)' "$SERVER_SRC" \
    --include='*.rs' | grep -v 'fn test_' || true)

if [ -n "$POLICY_RETURN_HITS" ]; then
    echo ""
    echo "HARDCODED POLICY/LEVEL RETURN VALUE found:"
    echo "$POLICY_RETURN_HITS" | while IFS= read -r line; do
        echo "  $line"
    done
    echo ""
    echo "  A function whose purpose is to look up or derive a configurable value"
    echo "  (attestation level, trust threshold, policy tier) should read the value"
    echo "  from stored data — not return a hardcoded constant."
    echo "  e.g., if the spec defines Level 2 AND Level 3, returning Some(2) always"
    echo "  means Level 3 can never be expressed."
    echo ""
    echo "  Fix: Store a structured policy value (e.g., {\"required_level\": 3}) and"
    echo "  parse the level from it, or add // hardcoded-default:ok if genuinely"
    echo "  intentional (e.g., a fixed minimum floor that cannot vary)."
    echo ""
    echo "  See: specs/reviews/task-059.md R1 F2 (hardcoded attestation level)"
    echo ""
    FAIL=1
fi

# ── Result ──────────────────────────────────────────────────────────────

if [ "$FAIL" -eq 0 ]; then
    echo "Hardcoded defaults lint passed."
    exit 0
else
    echo "Fix: Pass the runtime value from the calling context instead of hardcoding."
    echo "     Add '// hardcoded-default:ok' or '// empty-default:ok' comment if genuinely intentional."
    exit 1
fi
