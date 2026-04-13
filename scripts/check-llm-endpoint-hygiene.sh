#!/usr/bin/env bash
# Architecture lint: detect LLM endpoint hygiene violations.
#
# When a handler calls LlmPort::stream_complete (directly or via for_model()),
# two things must happen:
#   1. The workspace budget must be charged via CostEntry (spec: ui-layout.md §3
#      line 158: "LLM calls from generate, briefing/ask, and specs/assist
#      endpoints are charged to the workspace budget as llm_query cost entries.")
#   2. The max_tokens value returned by resolve_llm_model must be forwarded to
#      stream_complete — not discarded with `_`.
#
# Detections:
#   Check 1 — Missing budget tracking: a handler calls stream_complete but the
#             same function does not create a CostEntry. Every LLM endpoint must
#             record its cost.
#   Check 2 — Discarded max_tokens: resolve_llm_model returns (model, max_tokens)
#             but the call site destructures with `_` for max_tokens, then passes
#             None to stream_complete. The configured token limit is lost.
#
# Origin: specs/reviews/task-012.md F2 (missing budget tracking),
#         F3 (resolved max_tokens discarded)
#
# Exempt with: // llm-hygiene:ok — <reason>
#
# Run by pre-commit and CI.

set -euo pipefail

SERVER_SRC="crates/gyre-server/src"
VIOLATIONS=0

if [ ! -d "$SERVER_SRC" ]; then
    echo "Skipping LLM endpoint hygiene check: $SERVER_SRC not found"
    exit 0
fi

echo "Checking LLM endpoint hygiene..."

# ── Check 1: Missing budget tracking ─────────────────────────────────────
# For each file that calls stream_complete (non-test), verify CostEntry
# appears in the same file. This is a coarse check — it doesn't verify
# the CostEntry is in the same function, but catches the most common
# failure mode (entire file has no cost tracking at all).

for src_file in $(grep -rl 'stream_complete' "$SERVER_SRC" --include='*.rs' 2>/dev/null || true); do
    # Skip test files and test modules
    basename_file=$(basename "$src_file")
    if echo "$basename_file" | grep -qE '_test\.rs$|^tests\.rs$'; then
        continue
    fi

    # Find non-test stream_complete call sites
    STREAM_CALLS=$(grep -n 'stream_complete' "$src_file" \
        | grep -v '#\[test\]\|#\[cfg(test)\]\|// llm-hygiene:ok' || true)
    [ -z "$STREAM_CALLS" ] && continue

    # Check if CostEntry appears in the same file (non-test section)
    TEST_BOUNDARY=$(grep -n '#\[cfg(test)\]' "$src_file" | head -1 | cut -d: -f1 || echo "999999")

    while IFS= read -r call_line; do
        [ -z "$call_line" ] && continue
        call_lineno=$(echo "$call_line" | cut -d: -f1)

        # Skip if in test section
        if [ "$call_lineno" -ge "$TEST_BOUNDARY" ]; then
            continue
        fi

        # Find the enclosing function: scan backward for "pub async fn" or "async fn"
        FN_START=$(head -n "$call_lineno" "$src_file" \
            | grep -n 'async fn \|pub fn ' \
            | tail -1 \
            | cut -d: -f1 || echo "1")

        # Find the function's approximate end (next function definition or EOF)
        FN_END=$(tail -n +"$((call_lineno + 1))" "$src_file" \
            | grep -n 'async fn \|pub fn ' \
            | head -1 \
            | cut -d: -f1 || echo "999999")
        FN_END=$((call_lineno + FN_END))

        # Check for CostEntry in the function body
        COST_HITS=$(sed -n "${FN_START},${FN_END}p" "$src_file" \
            | grep -c 'CostEntry\|cost_entry\|costs\.record\|\.record(' || true)

        if [ "${COST_HITS:-0}" -eq 0 ]; then
            fn_name=$(sed -n "${FN_START}p" "$src_file" \
                | grep -oP '(async fn|pub fn) \K[a-z_]+' || echo "unknown")

            echo ""
            echo "MISSING BUDGET TRACKING: $src_file:$call_lineno (fn $fn_name)"
            echo "  This function calls stream_complete (LLM call) but does not create"
            echo "  a CostEntry to charge the workspace budget."
            echo "  Spec: ui-layout.md §3 line 158 requires all LLM calls to be charged"
            echo "  as llm_query cost entries."
            echo ""
            echo "  Fix: After the LLM call completes, create a CostEntry::new(...) and"
            echo "  call state.costs.record(&entry).await. See explorer_views.rs for the"
            echo "  canonical pattern."
            echo ""
            echo "  Exempt with '// llm-hygiene:ok — <reason>' on the stream_complete line."
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    done <<< "$STREAM_CALLS"
done

# ── Check 2: Discarded max_tokens from resolve_llm_model ────────────────
# Pattern: let (model, _) = resolve_llm_model(...)
# or: let (model, _max_tokens) = resolve_llm_model(...)
# The second value is the configured max_tokens and must be forwarded
# to stream_complete, not discarded.

# Multi-line aware: resolve_llm_model calls are often split across two lines:
#   let (model, _) =
#       crate::llm_helpers::resolve_llm_model(...).await;
# So we find all resolve_llm_model call sites and check the preceding line(s)
# for a destructure pattern that discards the second value.

for src_file in $(grep -rl 'resolve_llm_model' "$SERVER_SRC" --include='*.rs' 2>/dev/null || true); do
    # Skip test files
    basename_file=$(basename "$src_file")
    if echo "$basename_file" | grep -qE '_test\.rs$|^tests\.rs$'; then
        continue
    fi

    TEST_BOUNDARY=$(grep -n '#\[cfg(test)\]' "$src_file" | head -1 | cut -d: -f1 || echo "999999")

    while IFS= read -r call_line; do
        [ -z "$call_line" ] && continue
        call_lineno=$(echo "$call_line" | cut -d: -f1)

        # Skip test code
        if [ "$call_lineno" -ge "$TEST_BOUNDARY" ]; then
            continue
        fi

        # Skip exempted
        call_text=$(echo "$call_line" | cut -d: -f2-)
        if echo "$call_text" | grep -q 'llm-hygiene:ok'; then
            continue
        fi

        # Check the preceding 3 lines for `let (model, _)` or `let (_, _)` pattern
        WINDOW_START=$((call_lineno > 3 ? call_lineno - 3 : 1))
        DISCARD_HIT=$(sed -n "${WINDOW_START},${call_lineno}p" "$src_file" \
            | grep -c 'let (.*,\s*_\s*)' || true)

        if [ "${DISCARD_HIT:-0}" -gt 0 ]; then
            echo ""
            echo "DISCARDED MAX_TOKENS: $src_file:$call_lineno"
            echo "  $(sed -n "${WINDOW_START},${call_lineno}p" "$src_file" | tr '\n' ' ')"
            echo ""
            echo "  resolve_llm_model returns (model, max_tokens). The max_tokens value"
            echo "  is the configured token limit for this LLM endpoint and must be"
            echo "  forwarded to stream_complete — not discarded with '_'."
            echo ""
            echo "  Fix: let (model, max_tokens) = resolve_llm_model(...);"
            echo "       ...stream_complete(&system, &user, max_tokens)..."
            echo ""
            echo "  Exempt with '// llm-hygiene:ok — <reason>' on the same line."
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    done < <(grep -n 'resolve_llm_model' "$src_file" || true)
done

echo ""
if [ "$VIOLATIONS" -eq 0 ]; then
    echo "LLM endpoint hygiene check passed."
    exit 0
else
    echo "Fix: Every LLM endpoint must record CostEntry for budget tracking and"
    echo "     forward the resolved max_tokens to stream_complete."
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi
