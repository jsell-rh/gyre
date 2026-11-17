#!/usr/bin/env bash
# Architecture lint: verify domain types enforce the ownership hierarchy.
#
# specs/system/hierarchy-enforcement.md §2 — Non-Optional Hierarchy Fields:
# a hierarchy edge that is Option<Id> allows an entity with NO parent to
# bypass the tenant check entirely (hierarchy-enforcement review F1: 35 of
# 50 breached tasks had workspace_id = None). The M34 Slice 3 migration made
# the fields below non-optional (workspace_id: Id / tenant_id: Id). This
# script scans every domain struct definition and fails if any of those
# fields is declared as Option<...>, or if a struct that must carry the
# field has lost the field entirely (a deleted field silently re-opens the
# same bypass by removing the edge, not just its optionality).
#
# Scanned (struct, required non-optional fields):
#   Task            — workspace_id, repo_id
#   Agent           — workspace_id
#   MergeRequest    — workspace_id
#   Repository      — workspace_id
#   Workspace       — tenant_id
#
# Scanning by struct NAME across the whole domain crate (not a fixed file
# map) means the check survives file moves and catches a struct of the same
# name defined anywhere, while leaving legitimate Option<Id> fields on
# other structs untouched (audit events, tenant-level LLM/prompt defaults,
# user preferences — those are not hierarchy edges).
#
# Enabled by default since the M34 Slice 3 non-optional migration landed
# (spec §2 note). Set GYRE_CHECK_HIERARCHY=0 only to temporarily bypass.
#
# Run by pre-commit, dev-check and CI. On failure, the message explains
# the invariant.

set -euo pipefail

if [ "${GYRE_CHECK_HIERARCHY:-1}" = "0" ]; then
    echo "Hierarchy lint skipped (GYRE_CHECK_HIERARCHY=0)."
    exit 0
fi

DOMAIN_SRC="crates/gyre-domain/src"

if [ ! -d "$DOMAIN_SRC" ]; then
    echo "check-hierarchy: ERROR — domain source directory '$DOMAIN_SRC' not found (run from repo root)"
    exit 2
fi

FAIL=0

# awk state machine over every domain .rs file. mawk-compatible: no match()
# array capture, no \\s, no word-boundary escapes.
while IFS= read -r file; do
    if ! out=$(awk '
        BEGIN {
            req["Task"]        = "workspace_id repo_id"
            req["Agent"]       = "workspace_id"
            req["MergeRequest"]= "workspace_id"
            req["Repository"]  = "workspace_id"
            req["Workspace"]   = "tenant_id"
        }
        function flushstruct(   nf, j, f) {
            if (cur == "") return
            nf = split(req[cur], fields, " ")
            for (j = 1; j <= nf; j++) {
                f = fields[j]
                if (!(f in found)) {
                    printf "HIERARCHY VIOLATION: %s.%s is missing from the struct at %s\n", cur, f, FILENAME
                    printf "  The M34 hierarchy migration made this edge mandatory; deleting the field\n"
                    printf "  silently re-opens the no-parent bypass (same flaw class as Option<Id>).\n"
                    printf "  See: specs/system/hierarchy-enforcement.md section 2 - Non-Optional Hierarchy Fields\n"
                    bad = 1
                }
            }
            cur = ""
        }
        # struct header: `pub struct Name {` / `pub(crate) struct Name {`
        /struct[ \t]/ {
            flushstruct()
            n = split($0, w, /[ \t]+/)
            name = ""
            for (i = 1; i < n; i++) {
                if (w[i] == "struct") { name = w[i + 1]; break }
            }
            gsub(/[^A-Za-z0-9_]/, "", name)
            if (name != "" && name in req) {
                cur = name
                delete found
            }
            next
        }
        # top-level closing brace ends the struct body
        /^\}/ { flushstruct() }
        cur != "" {
            nf = split(req[cur], fields, " ")
            for (j = 1; j <= nf; j++) {
                f = fields[j]
                if ($0 ~ ("^[ \t]*pub[ \t]+" f "[ \t]*:[ \t]*Option")) {
                    printf "HIERARCHY VIOLATION: %s.%s is declared Option at %s:%d\n", cur, f, FILENAME, FNR
                    printf "  The ownership hierarchy requires this field to be non-optional (Id, not Option<Id>).\n"
                    printf "  An Option parent edge lets an entity bypass the tenant check entirely (review F1).\n"
                    found[f] = 1
                    printf "  See: specs/system/hierarchy-enforcement.md section 2 - Non-Optional Hierarchy Fields\n"
                    bad = 1
                } else if ($0 ~ ("^[ \t]*pub[ \t]+" f "[ \t]*:")) {
                    found[f] = 1
                }
            }
            next
        }
        END { flushstruct() }
    ' "$file"); then
        echo "check-hierarchy: ERROR — scanner failed on $file"
        exit 2
    fi
    if [ -n "$out" ]; then
        echo "$out"
        FAIL=1
    fi
done <<FILES
$(find "$DOMAIN_SRC" -name '*.rs' | sort)
FILES

if [ "$FAIL" -eq 0 ]; then
    echo "Hierarchy lint passed: all hierarchy fields are non-optional."
fi

exit "$FAIL"
