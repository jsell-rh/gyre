#!/usr/bin/env bash
# Type Simplification Lint: detect String fields in gyre-common types
# where a named enum or struct type exists elsewhere in the codebase.
#
# When a spec defines a field as `gate_type: GateType` and the codebase
# has a `pub enum GateType`, using `pub gate_type: String` in gyre-common
# is spec-type simplification drift. The correct fix is either to use
# the existing enum type (moving it to gyre-common if needed) or to define
# a new enum in gyre-common.
#
# Check strategy:
# 1. Find `pub field_name: String` fields in gyre-common types.
# 2. Derive candidate type names from the field name using PascalCase
#    (e.g., gate_type → GateType, status → Status).
# 3. Also derive a context-qualified name by prepending the enclosing
#    struct's prefix (e.g., status inside GateAttestation → GateStatus).
# 4. Search the codebase for `pub enum <CandidateName>` or
#    `pub struct <CandidateName>`.
# 5. If found, flag it — the field should use the typed version.
#
# Supplements implementation checklist item #23 (spec-type fidelity).

set -euo pipefail

COMMON_SRC="crates/gyre-common/src"

if [ ! -d "$COMMON_SRC" ]; then
    echo "SKIP: $COMMON_SRC not found"
    exit 0
fi

FAIL=0

# Convert snake_case to PascalCase
snake_to_pascal() {
    echo "$1" | sed -E 's/(^|_)([a-z])/\U\2/g'
}

# Find all `pub field_name: String` fields in gyre-common source files.
# Exclude fields that are clearly meant to be freeform strings (names,
# descriptions, paths, hashes, identifiers).
FREEFORM_FIELDS="name|description|title|body|path|hash|sha|signature|id|content|text|url|message|comment|reason|label|value|key|expression"

# Build a map of struct contexts: for each line in gyre-common, find the
# enclosing struct name. This lets us derive context-qualified candidates
# (e.g., status inside GateAttestation → GateStatus).
while IFS= read -r line; do
    [ -z "$line" ] && continue
    FILE=$(echo "$line" | cut -d: -f1)
    LINENUM=$(echo "$line" | cut -d: -f2)
    # Extract field name
    FIELD=$(echo "$line" | grep -oP 'pub\s+\K[a-z_]+(?=:\s*String)')
    [ -z "$FIELD" ] && continue

    # Skip freeform string fields
    if echo "$FIELD" | grep -qE "^($FREEFORM_FIELDS)$"; then
        continue
    fi
    # Skip fields ending in common freeform suffixes
    if echo "$FIELD" | grep -qE '_(name|id|path|sha|hash|url|ref)$'; then
        continue
    fi

    # Derive candidate type name from field name alone
    CANDIDATE=$(snake_to_pascal "$FIELD")

    # Derive context-qualified candidate from enclosing struct name.
    # Find the most recent `pub struct Foo` before this line.
    ENCLOSING_STRUCT=$(head -n "$LINENUM" "$FILE" 2>/dev/null | \
        grep -oP 'pub struct \K\w+' | tail -1 || true)

    QUALIFIED_CANDIDATE=""
    if [ -n "$ENCLOSING_STRUCT" ]; then
        # Extract a prefix from the struct name — everything before the
        # last "word" (e.g., GateAttestation → Gate, SignedInput → Signed).
        # For a field like "status" inside "GateAttestation", we try "GateStatus".
        PREFIX=$(echo "$ENCLOSING_STRUCT" | sed -E 's/[A-Z][a-z]+$//')
        if [ -n "$PREFIX" ] && [ "$PREFIX" != "$ENCLOSING_STRUCT" ]; then
            QUALIFIED_CANDIDATE="${PREFIX}${CANDIDATE}"
        fi
    fi

    # Search for existing enum or struct with either the qualified or direct name.
    # Try the context-qualified name first (more specific, fewer false positives).
    # E.g., for `status` in `GateAttestation`, try `GateStatus` before `Status`.
    EXISTING=""
    for cand in $QUALIFIED_CANDIDATE $CANDIDATE; do
        [ -z "$cand" ] && continue
        # Skip very short/generic type names that would produce false positives
        if [ ${#cand} -le 4 ]; then
            continue
        fi
        HITS=$(grep -rn "pub enum ${cand}\b\|pub struct ${cand}\b" crates/ 2>/dev/null | \
            grep -v "$FILE:$LINENUM" | head -3 || true)
        if [ -n "$HITS" ]; then
            EXISTING="$HITS"
            CANDIDATE="$cand"
            break
        fi
    done

    if [ -n "$EXISTING" ]; then
        echo "TYPE SIMPLIFICATION: $FILE:$LINENUM — '$FIELD: String' but type '$CANDIDATE' exists in the codebase"
        echo "  Field: pub $FIELD: String (in struct $ENCLOSING_STRUCT)"
        echo "  Existing type(s):"
        while IFS= read -r existing_line; do
            echo "    $existing_line"
        done <<< "$EXISTING"
        echo "  If the spec defines this field as $CANDIDATE, use the typed version."
        echo "  If $CANDIDATE is in gyre-domain, move it to gyre-common (pure value enums belong there)."
        echo ""
        FAIL=1
    fi
done < <(grep -rn 'pub [a-z_]*: String' "$COMMON_SRC"/*.rs 2>/dev/null | \
    grep -v 'Option<String>' | \
    grep -v '//' || true)

if [ "$FAIL" -eq 0 ]; then
    echo "Type simplification lint passed."
fi

exit "$FAIL"
