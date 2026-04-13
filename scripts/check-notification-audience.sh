#!/usr/bin/env bash
# Architecture lint: verify that notification recipient sets match spec-defined
# audiences — no unauthorized expansion (e.g., unioning authors with all
# workspace members when the spec says "notify all authors").
#
# Flaw class: Over-broad notification audience (see TASK-027 R1 F2)
#
# When the spec says "notify all authors," the implementation must send
# notifications ONLY to the distinct author set. Adding workspace members
# via list_by_workspace() is an unauthorized expansion that sends irrelevant
# notifications to uninvolved users.
#
# Check 1: Workspace member union with author set
#   Detects functions that have BOTH:
#   (a) An author-collection pattern (author_ids, author_agent_id)
#   (b) A workspace membership query (list_by_workspace) whose results
#       are unioned into the notification target set
#   This suggests the notification audience is expanded beyond "authors."
#
# Exempt a line with: // notification-audience:ok — <reason>
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

echo "Checking for over-broad notification audience expansion..."

# ── Check 1: Workspace member union with author/target set ──

for file in $(find "$SERVER_SRC" -name '*.rs' -type f | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" '
    # Skip test modules
    /^\s*#\[cfg\(test\)\]/ { in_test_module = 1; next }

    # Detect function boundaries
    /^\s*(pub\s+)?(async\s+)?fn\s+/ {
        # Check previous function
        if (fn_name != "" && !has_exempt && has_author_collection && has_member_union) {
            printf "OVER-BROAD NOTIFICATION AUDIENCE: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Function collects author IDs (line %d) and unions them with\n", author_line
            printf "  workspace members via list_by_workspace (line %d).\n", member_line
            printf "  If the spec says \"notify all authors,\" the workspace member\n"
            printf "  union is unauthorized — it sends notifications to uninvolved users.\n"
            printf "  Fix: Notify only the spec-defined audience (e.g., distinct\n"
            printf "  author_agent_id values). Remove the workspace membership lookup\n"
            printf "  unless the spec explicitly includes workspace members.\n"
            printf "  See: specs/reviews/task-027.md R1 F2\n\n"
            violations++
        }
        if (fn_name != "" && has_author_collection) checked++

        # Parse function name
        match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        fn_name = m[1]
        fn_start = NR
        has_author_collection = 0
        author_line = 0
        has_member_union = 0
        member_line = 0
        has_exempt = 0
        # Skip test functions
        if (fn_name ~ /^test_/ || in_test_module) fn_name = ""
        next
    }
    fn_name != "" {
        # Check for exemption
        if ($0 ~ /notification-audience:ok/) has_exempt = 1

        # Detect author collection patterns
        if ($0 ~ /author_ids|author_agent_id/ && $0 ~ /insert|push|collect/) {
            if (!has_author_collection) {
                has_author_collection = 1
                author_line = NR
            }
        }

        # Detect workspace membership lookup unioned into notification target
        if ($0 ~ /list_by_workspace/ && $0 !~ /notification-audience:ok/) {
            if (!has_member_union) {
                has_member_union = 1
                member_line = NR
            }
        }
    }
    END {
        # Check last function
        if (fn_name != "" && !has_exempt && has_author_collection && has_member_union) {
            printf "OVER-BROAD NOTIFICATION AUDIENCE: %s in %s:%d\n", fn_name, file, fn_start
            printf "  Function collects author IDs (line %d) and unions them with\n", author_line
            printf "  workspace members via list_by_workspace (line %d).\n", member_line
            printf "  If the spec says \"notify all authors,\" the workspace member\n"
            printf "  union is unauthorized — it sends notifications to uninvolved users.\n"
            printf "  Fix: Notify only the spec-defined audience (e.g., distinct\n"
            printf "  author_agent_id values). Remove the workspace membership lookup\n"
            printf "  unless the spec explicitly includes workspace members.\n"
            printf "  See: specs/reviews/task-027.md R1 F2\n\n"
            violations++
        }
        if (fn_name != "" && has_author_collection) checked++
        printf "SUMMARY:%d:%d\n", checked, violations
    }
    ' "$file" | while IFS= read -r line; do
        case "$line" in
            SUMMARY:*)
                c=$(echo "$line" | cut -d: -f2)
                v=$(echo "$line" | cut -d: -f3)
                echo "$c $v" >> /tmp/check-notif-audience-$$
                ;;
            *)
                echo "$line"
                ;;
        esac
    done
done

# ── Tally results ─────────────────────────────────────────────────────

if [ -f /tmp/check-notif-audience-$$ ]; then
    while read -r c v; do
        CHECKED=$((CHECKED + c))
        VIOLATIONS=$((VIOLATIONS + v))
    done < /tmp/check-notif-audience-$$
    rm -f /tmp/check-notif-audience-$$
fi

echo ""
if [ "$VIOLATIONS" -eq 0 ]; then
    echo "Notification audience lint passed: ${CHECKED} author-notification functions checked."
    echo "No unauthorized audience expansion detected."
    exit 0
else
    echo "Fix: Verify the spec's stated notification audience. If the spec says"
    echo "     \"notify all authors,\" remove the workspace membership lookup."
    echo "     Only expand the audience if the spec explicitly requires it."
    echo "     Exempt with: // notification-audience:ok — <reason>"
    echo "${VIOLATIONS} violation(s) found out of ${CHECKED} functions checked."
    exit 1
fi
