#!/usr/bin/env bash
# CLI Spec Parity Lint: detect common CLI-spec signature drift.
#
# Checks:
# 1. CLI commands whose spec signature shows optional params ([--param])
#    but whose clap struct declares them as required (non-Option<T>).
# 2. CLI commands that require a subcommand for their primary action
#    when the spec defines a flat command.
# 3. CLI parameters that accept raw IDs where the spec says <name>/<slug>.
#
# This script reads spec excerpts from task files and cross-references
# the clap struct in crates/gyre-cli/src/main.rs.
#
# Run during pre-commit and CI. Not exhaustive — supplements the
# implementation checklist item #18.

set -euo pipefail

CLI_MAIN="crates/gyre-cli/src/main.rs"

if [ ! -f "$CLI_MAIN" ]; then
    echo "SKIP: $CLI_MAIN not found"
    exit 0
fi

FAIL=0

# --- Check 1: Required fields that should be Option<T> ---
# These are fields whose spec signature uses [brackets] (optional)
# but the clap struct declares as bare String (required).
#
# Pattern: look for struct fields of type `String` (not Option<String>)
# in command variants where the help text says "Workspace slug" or similar,
# cross-referencing spec-optional params.

# Find non-Option workspace params in CLI command structs.
# The spec universally marks --workspace as optional ([--workspace <slug>]).
# Any `workspace: String` (not Option<String>) in a command variant is a violation.
WORKSPACE_REQUIRED=$(grep -n 'workspace: String' "$CLI_MAIN" 2>/dev/null | grep -v 'Option<String>' | grep -v '//' || true)
if [ -n "$WORKSPACE_REQUIRED" ]; then
    while IFS= read -r line; do
        echo "CLI-SPEC PARITY: --workspace is declared required but spec marks it optional [--workspace <slug>]"
        echo "  $CLI_MAIN:$line"
        echo "  Fix: Change 'workspace: String' to 'workspace: Option<String>'"
        echo ""
        FAIL=1
    done <<< "$WORKSPACE_REQUIRED"
fi

# --- Check 2: Subcommand-required commands that should be flat ---
# Look for commands where a `command: XxxCommands` field exists but the
# spec defines a flat command (no subcommand needed for primary action).
# We check specifically for Inbox since the spec defines `gyre inbox [--flags]`
# as a flat command. The pattern `command: InboxCommands` without a
# `default_subcommand` or `Option<InboxCommands>` forces a subcommand.
INBOX_SUBCOMMAND_REQUIRED=$(grep -n 'command: InboxCommands' "$CLI_MAIN" 2>/dev/null | head -1 || true)
if [ -n "$INBOX_SUBCOMMAND_REQUIRED" ]; then
    # Check if it's wrapped in Option (allowing bare invocation)
    if ! grep -q 'command: Option<InboxCommands>' "$CLI_MAIN" 2>/dev/null; then
        echo "CLI-SPEC PARITY: 'gyre inbox' requires a subcommand but spec defines flat command"
        echo "  $CLI_MAIN:$INBOX_SUBCOMMAND_REQUIRED"
        echo "  Spec: gyre inbox [--workspace <slug>] [--priority <min>-<max>]"
        echo "  Fix: Make subcommand optional or set default_subcommand to 'list'"
        echo ""
        FAIL=1
    fi
fi

# --- Check 3: Help text saying "ID" where spec says name/slug ---
# Catch help strings like "Repository ID" for flags the spec defines as <name>.
REPO_ID_HELP=$(grep -n '"Repository ID"' "$CLI_MAIN" 2>/dev/null || true)
if [ -n "$REPO_ID_HELP" ]; then
    # Check if this is on a --repo flag (where spec says <name>)
    # vs a --repo-id flag (which is explicit about wanting an ID)
    while IFS= read -r line; do
        LINENUM=$(echo "$line" | cut -d: -f1)
        # Check the field name on the next line — if it's `repo:` not `repo_id:`, it's a violation
        FIELD=$(sed -n "$((LINENUM + 1))p" "$CLI_MAIN" 2>/dev/null || true)
        if echo "$FIELD" | grep -q 'repo:' 2>/dev/null && ! echo "$FIELD" | grep -q 'repo_id:' 2>/dev/null; then
            echo "CLI-SPEC PARITY: --repo help says 'Repository ID' but spec says '--repo <name>'"
            echo "  $CLI_MAIN:$line"
            echo "  Fix: Accept a human-readable repo name and resolve to ID (like resolve_workspace_slug)"
            echo ""
            FAIL=1
        fi
    done <<< "$REPO_ID_HELP"
fi

# --- Check 4: --repo-id as required arg where spec doesn't include it ---
# The spec defines `gyre spec assist <path> "<instruction>"` — no --repo-id.
# A mandatory repo_id flag not in the spec is an invented parameter.
SPEC_ASSIST_REPO_ID=$(grep -n 'repo_id: String' "$CLI_MAIN" 2>/dev/null | grep -v 'Option<String>' || true)
if [ -n "$SPEC_ASSIST_REPO_ID" ]; then
    # Only flag if this is inside the SpecCommands/Assist context
    while IFS= read -r line; do
        LINENUM=$(echo "$line" | cut -d: -f1)
        # Check surrounding context for Assist variant
        CONTEXT=$(sed -n "$((LINENUM - 10)),$((LINENUM))p" "$CLI_MAIN" 2>/dev/null || true)
        if echo "$CONTEXT" | grep -q 'Assist' 2>/dev/null; then
            echo "CLI-SPEC PARITY: 'gyre spec assist' has required --repo-id not in spec signature"
            echo "  $CLI_MAIN:$line"
            echo "  Spec: gyre spec assist <path> \"<instruction>\""
            echo "  Fix: Infer repo from context or make --repo-id optional"
            echo ""
            FAIL=1
        fi
    done <<< "$SPEC_ASSIST_REPO_ID"
fi

if [ "$FAIL" -eq 0 ]; then
    echo "CLI-spec parity lint passed."
fi

exit "$FAIL"
