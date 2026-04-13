#!/usr/bin/env bash
# Architecture lint: detect prompt template variable substitution gaps.
#
# When a prompt template declares variables (e.g., {{spec_path}}, {{spec_content}},
# {{graph_context}}, {{instruction}}), EVERY consumer of that template must
# substitute ALL declared variables. If a consumer only substitutes a subset,
# the unsubstituted variables appear as literal text in the LLM prompt (e.g.,
# "Knowledge graph context: {{graph_context}}"), producing garbage output.
#
# This is especially dangerous when a template is modified to add new variables:
# the file that was modified substitutes the new variables, but other consumers
# of the same template are not updated.
#
# Detection: for each prompt template constant (PROMPT_*) in llm_defaults.rs,
# extract all {{variable}} placeholders. Then find every consumer that loads
# or references that constant and verify it calls .replace("{{variable}}", ...)
# for ALL declared variables.
#
# Origin: specs/reviews/task-012.md F4 — TASK-012 added {{spec_content}} and
#         {{graph_context}} to PROMPT_SPECS_ASSIST but the MCP handler still
#         only substituted {{spec_path}}, {{draft_content}}, and {{instruction}}.
#
# Exempt with: // template-sub:ok — <reason>
#
# Run by pre-commit and CI.

set -euo pipefail

DEFAULTS_FILE="crates/gyre-server/src/llm_defaults.rs"
SERVER_SRC="crates/gyre-server/src"
VIOLATIONS=0

if [ ! -f "$DEFAULTS_FILE" ]; then
    echo "Skipping template substitution check: $DEFAULTS_FILE not found"
    exit 0
fi

echo "Checking prompt template variable substitution completeness..."

# Extract all PROMPT_* constant names from llm_defaults.rs
PROMPT_CONSTS=$(grep -oP 'pub const (PROMPT_[A-Z_]+)' "$DEFAULTS_FILE" | awk '{print $3}')

for const_name in $PROMPT_CONSTS; do
    # Extract all {{variable}} placeholders from this constant's definition.
    # The constant may span multiple lines; collect from the definition line
    # until the next `pub const` or end of file.
    CONST_LINE=$(grep -n "pub const $const_name" "$DEFAULTS_FILE" | head -1 | cut -d: -f1)
    [ -z "$CONST_LINE" ] && continue

    NEXT_CONST_LINE=$(tail -n +"$((CONST_LINE + 1))" "$DEFAULTS_FILE" \
        | grep -n 'pub const PROMPT_' \
        | head -1 \
        | cut -d: -f1 || echo "")

    if [ -n "$NEXT_CONST_LINE" ]; then
        END_LINE=$((CONST_LINE + NEXT_CONST_LINE - 1))
    else
        END_LINE=$(wc -l < "$DEFAULTS_FILE")
    fi

    # Extract all {{...}} variables from the constant body
    TEMPLATE_VARS=$(sed -n "${CONST_LINE},${END_LINE}p" "$DEFAULTS_FILE" \
        | grep -oP '\{\{[a-z_]+\}\}' \
        | sort -u)

    [ -z "$TEMPLATE_VARS" ] && continue

    # Find all files that reference this constant (consumers)
    CONSUMERS=$(grep -rl "$const_name" "$SERVER_SRC" --include='*.rs' 2>/dev/null \
        | grep -v "llm_defaults.rs" || true)

    for consumer_file in $CONSUMERS; do
        [ -z "$consumer_file" ] && continue

        # Find the line(s) where the constant is referenced
        CONST_REFS=$(grep -n "$const_name" "$consumer_file" | grep -v '// template-sub:ok' || true)
        [ -z "$CONST_REFS" ] && continue

        # For each reference, find the enclosing function and check substitutions
        while IFS= read -r ref_line; do
            [ -z "$ref_line" ] && continue
            ref_lineno=$(echo "$ref_line" | cut -d: -f1)

            # Find the enclosing function
            FN_START=$(head -n "$ref_lineno" "$consumer_file" \
                | grep -n 'async fn \|pub fn \|fn ' \
                | tail -1 \
                | cut -d: -f1 || echo "1")

            # Find function end (next fn definition or +200 lines, whichever is first)
            FN_END_SEARCH=$(tail -n +"$((ref_lineno + 1))" "$consumer_file" \
                | grep -n 'async fn \|pub fn ' \
                | head -1 \
                | cut -d: -f1 || echo "200")
            FN_END=$((ref_lineno + FN_END_SEARCH))

            fn_name=$(sed -n "${FN_START}p" "$consumer_file" \
                | grep -oP '(async fn|pub fn|fn) \K[a-z_]+' || echo "unknown")

            # Extract the function body text
            FN_BODY=$(sed -n "${FN_START},${FN_END}p" "$consumer_file")

            # Check each template variable
            for var in $TEMPLATE_VARS; do
                # Check if the function body contains .replace("{{var}}", ...)
                # The var includes {{ and }}, e.g., {{spec_content}}
                if ! echo "$FN_BODY" | grep -q "replace(\"$var\""; then
                    echo ""
                    echo "MISSING TEMPLATE SUBSTITUTION: $consumer_file:$ref_lineno (fn $fn_name)"
                    echo "  Template $const_name declares variable $var"
                    echo "  but this consumer does not call .replace(\"$var\", ...)."
                    echo "  The literal text '$var' will appear in the LLM prompt."
                    echo ""
                    echo "  Fix: Add .replace(\"$var\", &value) to the template substitution"
                    echo "  chain in this function. Load the value from the appropriate source"
                    echo "  (see the other consumers of $const_name for the pattern)."
                    echo ""
                    echo "  Exempt with '// template-sub:ok — <reason>' on the $const_name line."
                    VIOLATIONS=$((VIOLATIONS + 1))
                fi
            done
        done <<< "$CONST_REFS"
    done
done

echo ""
if [ "$VIOLATIONS" -eq 0 ]; then
    echo "Template substitution check passed."
    exit 0
else
    echo "Fix: Every consumer of a prompt template must substitute ALL declared"
    echo "     {{variables}}. When adding new variables to a template, update every"
    echo "     consumer — grep for the constant name to find them all."
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi
