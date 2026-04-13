#!/usr/bin/env bash
# Architecture lint: detect functions that save domain objects and log about
# them with tracing::info! but do not emit domain events via emit_event() —
# in files that already use emit_event() for analogous operations.
#
# When a spec says "log activity event" for a domain action, the code must
# call state.emit_event() — not just tracing::info!. These are different
# systems:
#   - tracing::info!  = infrastructure logging (server logs, not user-facing)
#   - emit_event()    = domain event bus (WebSocket, dashboard, orchestrators)
#
# The most common failure mode: a function creates a domain object via
# .save() or .create() on a port, logs "new X detected" with tracing::info!,
# but never calls emit_event(). The spec's "activity event" requirement is
# silently unsatisfied — orchestrators and dashboard subscribers are never
# notified.
#
# Scope: only flags functions in files that ALREADY have other emit_event()
# calls. If no function in the file uses emit_event(), this check does not
# apply (the file may not handle event-worthy domain actions).
#
# See: specs/reviews/task-048.md F5
#
# Run by pre-commit and CI.

set -euo pipefail

SERVER_SRC="crates/gyre-server/src"
FAIL=0
VIOLATIONS=0

if [ ! -d "$SERVER_SRC" ]; then
    echo "Skipping missing domain event check: $SERVER_SRC not found"
    exit 0
fi

echo "Checking for missing domain event emissions..."

# ── Strategy ────────────────────────────────────────────────────────
#
# 1. Find .rs files in SERVER_SRC that contain at least one emit_event() call
#    (these files already use the domain event pattern).
# 2. In each such file, find non-test functions that:
#    (a) Call .save( or .create( on a domain port (state change)
#    (b) AND have tracing::info! with domain-event language
#        ("detected", "created", "orphaned", "changed", "updated")
#    (c) BUT do NOT call emit_event(
# 3. Flag these as potentially missing domain event emissions.

for file in $(grep -rl 'emit_event(' "$SERVER_SRC" --include='*.rs' 2>/dev/null || true); do
    # This file uses emit_event() somewhere — check for functions that
    # save domain objects and log about it but don't emit events.

    # Use awk to find non-test functions matching the pattern.
    MISSING=$(awk '
        # Skip test modules
        /^\s*#\[cfg\(test\)\]/ { in_test_mod = 1; next }
        /^\s*mod\s+tests\s*\{/ { if (in_test_mod) { test_depth = 1; next } }
        in_test_mod && /\{/ { test_depth++ }
        in_test_mod && /\}/ { test_depth--; if (test_depth <= 0) in_test_mod = 0 }

        # Track non-test function boundaries
        !in_test_mod && /^\s*(pub(\(crate\))?\s+)?(async\s+)?fn\s+/ {
            match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, arr)
            current_fn = arr[1]
            fn_depth = 0
            has_save = 0
            has_tracing_domain = 0
            has_emit_event = 0
            has_exempt = 0
        }
        !in_test_mod && current_fn != "" && /\{/ { fn_depth++ }
        !in_test_mod && current_fn != "" && /\}/ {
            fn_depth--
            if (fn_depth <= 0) {
                if (has_save && has_tracing_domain && !has_emit_event && !has_exempt) {
                    print current_fn
                }
                current_fn = ""
            }
        }

        # Check for domain state changes (.save( or .create( on ports)
        !in_test_mod && current_fn != "" && /\.(save|create)\(/ {
            # Exclude comments
            if (!/\/\/.*\.(save|create)/) has_save = 1
        }

        # Check for tracing::info! (may be multi-line — keyword on a later line)
        !in_test_mod && current_fn != "" && /tracing::(info|debug)!/ {
            if (!/\/\/.*tracing/) in_tracing = 1
        }
        # Track tracing macro scope (look for closing paren+semicolon)
        in_tracing && /;\s*$/ { in_tracing = 0 }

        # Check for domain-event keywords — either on the tracing line itself
        # or on subsequent lines within the same tracing macro invocation
        !in_test_mod && current_fn != "" && (in_tracing || /tracing::(info|debug)!/) {
            if (/detect|creat|orphan|chang|updat|new.*edge|new.*dep|new.*link/) {
                if (!/\/\//) has_tracing_domain = 1
            }
        }

        # Check for emit_event (means the function already emits events)
        !in_test_mod && current_fn != "" && /emit_event/ {
            if (!/\/\/.*emit_event/) has_emit_event = 1
        }

        # Check for exemption comment
        !in_test_mod && current_fn != "" && /\/\/\s*domain-event:ok/ {
            has_exempt = 1
        }
    ' "$file" 2>/dev/null | sort -u)

    if [ -n "$MISSING" ]; then
        while IFS= read -r fn_name; do
            [ -z "$fn_name" ] && continue

            echo ""
            echo "MISSING DOMAIN EVENT: ${file}"
            echo "  Function: ${fn_name}()"
            echo "  This function saves a domain object and logs about it with"
            echo "  tracing::info!, but does not call emit_event()."
            echo "  Other functions in the same file use emit_event() for"
            echo "  analogous domain actions."
            echo ""
            echo "  If the spec says 'log activity event' or 'emit event' for"
            echo "  this action, add state.emit_event() with the appropriate"
            echo "  MessageKind and payload."
            echo ""
            echo "  tracing::info! = infrastructure logging (server logs only)"
            echo "  emit_event()   = domain event bus (dashboard, orchestrators)"
            echo ""
            echo "  Exempt with: // domain-event:ok — <reason>"
            echo ""
            echo "  See: specs/reviews/task-048.md F5"
            echo ""
            VIOLATIONS=$((VIOLATIONS + 1))
            FAIL=1
        done <<< "$MISSING"
    fi
done

# ── Result ──────────────────────────────────────────────────────────────

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "Missing domain event check passed."
    exit 0
else
    echo "Fix: Add state.emit_event() alongside tracing::info! for domain"
    echo "     actions where the spec requires an activity event."
    echo "     Follow the existing emit_event() pattern in the same file."
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi
