#!/usr/bin/env bash
# Architecture lint: verify domain types enforce the ownership hierarchy.
#
# The ownership hierarchy (Tenant -> Workspace -> Repo) must be non-optional
# in domain types. This script checks that specific fields are declared as
# `Id` (not `Option<Id>`) in domain struct definitions.
#
# Checked fields:
#   - workspace_id on: Task, Agent, MergeRequest, Repository
#   - tenant_id on: Workspace
#
# Run by pre-commit and CI. On failure, the message explains the invariant.
#
# NOTE: This script is DISABLED by default until the non-optional migration
# lands (M34 Slice 3). Enable by removing the guard below.

set -euo pipefail

# ── Guard: remove this block to enable the check ────────────────────────
if [ "${GYRE_CHECK_HIERARCHY:-0}" != "1" ]; then
    echo "Hierarchy lint skipped (GYRE_CHECK_HIERARCHY != 1). Enable after M34 Slice 3."
    exit 0
fi
# ────────────────────────────────────────────────────────────────────────

DOMAIN_SRC="crates/gyre-domain/src"

FAIL=0

# Map of (file, field) pairs that must be non-optional
declare -A REQUIRED_FIELDS
REQUIRED_FIELDS["task.rs:workspace_id"]="Task"
REQUIRED_FIELDS["agent.rs:workspace_id"]="Agent"
REQUIRED_FIELDS["merge_request.rs:workspace_id"]="MergeRequest"
REQUIRED_FIELDS["repository.rs:workspace_id"]="Repository"
REQUIRED_FIELDS["workspace.rs:tenant_id"]="Workspace"

for key in "${!REQUIRED_FIELDS[@]}"; do
    file="${key%%:*}"
    field="${key##*:}"
    entity="${REQUIRED_FIELDS[$key]}"
    filepath="$DOMAIN_SRC/$file"

    if [ ! -f "$filepath" ]; then
        echo "WARNING: Cannot find $filepath — skipping $entity.$field check"
        continue
    fi

    # Check if the field is declared as Option<Id> or Option<gyre_common::Id>
    if grep -P "pub\s+${field}\s*:\s*Option" "$filepath" > /dev/null 2>&1; then
        echo "HIERARCHY VIOLATION: ${entity}.${field} is Option<Id> in ${filepath}"
        echo "  The ownership hierarchy requires this field to be non-optional (Id, not Option<Id>)."
        echo "  See: specs/system/hierarchy-enforcement.md §2 — Non-Optional Hierarchy Fields"
        echo ""
        FAIL=1
    fi
done

if [ "$FAIL" -eq 0 ]; then
    echo "Hierarchy lint passed: all hierarchy fields are non-optional."
fi

exit "$FAIL"
