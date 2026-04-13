#!/usr/bin/env bash
# Architecture lint: detect fire-and-forget concurrent writes to shared resources.
#
# When multiple `tokio::spawn` calls in the same function write to the same
# shared resource (git ref, file path) and a JoinHandle is dropped (not
# `.await`ed or stored), the spawned tasks run concurrently with no ordering
# guarantee.  Whichever write completes last determines the final content —
# a non-deterministic race condition.
#
# This script detects functions that contain multiple `tokio::spawn` calls
# where the spawned closures reference the same git notes ref (e.g.,
# `refs/notes/attestations`).  It also detects dropped JoinHandles (spawn
# calls whose result is not bound to a variable or awaited).
#
# See: specs/reviews/task-018.md F3
#
# Run by pre-commit and CI.

set -euo pipefail

RUST_SRC="crates"
FAIL=0

if [ ! -d "$RUST_SRC" ]; then
    echo "Skipping concurrent shared write check: $RUST_SRC not found"
    exit 0
fi

echo "Checking for fire-and-forget concurrent writes to shared resources..."

# --- Check 1: Multiple tokio::spawn calls in the same function that
#     reference the same git notes ref ---
#
# Strategy: For each .rs file, find functions containing 2+ tokio::spawn calls.
# For each such function, check if the spawned closures reference the same
# git notes ref (--ref= pattern).

check_git_notes_race() {
    local file="$1"

    # Skip test files
    if echo "$file" | grep -qE '(/tests/|_test\.rs|#\[test\])'; then
        return 0
    fi

    # Find all functions that contain tokio::spawn
    # We use a simplified approach: find line numbers of tokio::spawn calls,
    # group by enclosing function.

    local spawn_lines
    spawn_lines=$(grep -n 'tokio::spawn' "$file" 2>/dev/null | grep -v '// concurrent-write:ok' || true)

    if [ -z "$spawn_lines" ]; then
        return 0
    fi

    local spawn_count
    spawn_count=$(echo "$spawn_lines" | wc -l)

    if [ "$spawn_count" -lt 2 ]; then
        return 0
    fi

    # For files with 2+ spawn calls, check if any spawned closures reference
    # the same git notes ref.  We look for `--ref=` or `refs/notes/` patterns
    # near each spawn call (within 20 lines after it).
    local refs_found=()
    local spawn_line_nums
    spawn_line_nums=$(echo "$spawn_lines" | cut -d: -f1)

    for line_num in $spawn_line_nums; do
        local end_line=$((line_num + 25))
        local block
        block=$(sed -n "${line_num},${end_line}p" "$file" 2>/dev/null || true)

        # Look for git notes ref patterns in the spawned block
        local ref_match
        ref_match=$(echo "$block" | grep -oE '(--ref=|refs/notes/)[^"'"'"' ),]+' | head -1 || true)

        if [ -n "$ref_match" ]; then
            refs_found+=("$line_num:$ref_match")
        fi
    done

    # Check if any two spawn calls reference the same ref
    if [ ${#refs_found[@]} -lt 2 ]; then
        return 0
    fi

    # Extract just the ref values and check for duplicates
    local ref_values=()
    for entry in "${refs_found[@]}"; do
        local ref_val="${entry#*:}"
        # Normalize: strip --ref= prefix if present
        ref_val="${ref_val#--ref=}"
        ref_values+=("$ref_val")
    done

    # Check for duplicates
    local sorted_refs
    sorted_refs=$(printf '%s\n' "${ref_values[@]}" | sort)
    local unique_refs
    unique_refs=$(printf '%s\n' "${ref_values[@]}" | sort -u)

    if [ "$sorted_refs" != "$unique_refs" ]; then
        # Found duplicate refs across spawn calls
        echo ""
        echo "  CONCURRENT WRITE RACE in $file:"
        for entry in "${refs_found[@]}"; do
            local ln="${entry%%:*}"
            local rv="${entry#*:}"
            echo "    Line $ln: tokio::spawn writes to $rv"
        done
        echo ""
        echo "  Multiple tokio::spawn calls write to the same git notes ref."
        echo "  If any JoinHandle is dropped, the writes race — whichever"
        echo "  completes last determines the note content."
        echo ""
        FAIL=1
    fi
}

# --- Check 2: Dropped JoinHandles on tokio::spawn that perform writes ---
#
# A tokio::spawn whose return value is not assigned to a variable is a
# dropped JoinHandle.  If the spawned closure performs a write operation
# (git notes add, file write, etc.), it races with subsequent code.

check_dropped_write_handles() {
    local file="$1"

    # Skip test files
    if echo "$file" | grep -qE '(/tests/|_test\.rs)'; then
        return 0
    fi

    # Find bare `tokio::spawn(` calls that are not assigned to a variable.
    # Pattern: line starts with whitespace + tokio::spawn (no `let` before it)
    # This catches:  `    tokio::spawn(async move {`
    # But not:       `    let handle = tokio::spawn(async move {`
    local bare_spawns
    bare_spawns=$(grep -n 'tokio::spawn' "$file" 2>/dev/null \
        | grep -v 'let .* = .*tokio::spawn' \
        | grep -v '// concurrent-write:ok' \
        | grep -v '/tests/\|_test\.rs' \
        || true)

    if [ -z "$bare_spawns" ]; then
        return 0
    fi

    # For each bare spawn, check if the closure performs a write operation
    while IFS= read -r spawn_line; do
        local line_num
        line_num=$(echo "$spawn_line" | cut -d: -f1)
        local end_line=$((line_num + 25))
        local block
        block=$(sed -n "${line_num},${end_line}p" "$file" 2>/dev/null || true)

        # Check for write indicators in the spawned block
        local has_write=false
        if echo "$block" | grep -qE '(notes.*add|write_all|write_to|fs::write|\.output\(\))'; then
            has_write=true
        fi

        if [ "$has_write" = true ]; then
            # Check if there are OTHER spawn or write calls later in the same function
            # (simplified: check if any other spawn call exists within 50 lines)
            local extended_end=$((line_num + 60))
            local after_block
            after_block=$(sed -n "$((line_num + 1)),${extended_end}p" "$file" 2>/dev/null || true)

            if echo "$after_block" | grep -q 'tokio::spawn\|notes.*add\|attach_.*note'; then
                echo ""
                echo "  DROPPED JOINHANDLE on write-spawning tokio::spawn in $file:$line_num"
                echo "    The JoinHandle is not stored or awaited, but the closure performs"
                echo "    a write operation.  Subsequent code in the same function also writes"
                echo "    (or spawns writes), creating a potential race condition."
                echo ""
                echo "  Fix options:"
                echo "    1. Await the JoinHandle: let handle = tokio::spawn(...); handle.await.ok();"
                echo "    2. Skip the redundant write if a later write will overwrite it"
                echo "    3. Add '// concurrent-write:ok — <reason>' if the race is intentional"
                echo ""
                FAIL=1
            fi
        fi
    done <<< "$bare_spawns"
}

# Collect results in a temp file to avoid subshell variable scoping
RESULT_FILE=$(mktemp)
echo "0" > "$RESULT_FILE"

# Scan all non-test Rust files
while read -r file; do
    check_git_notes_race "$file"
    check_dropped_write_handles "$file"
done < <(find "$RUST_SRC" -name '*.rs' -not -path '*/tests/*' -not -name '*_test.rs')

if [ "$FAIL" -eq 0 ]; then
    rm -f "$RESULT_FILE"
    echo "Concurrent shared write check passed."
    exit 0
else
    rm -f "$RESULT_FILE"
    echo ""
    echo "Fix: Sequence writes to shared resources or skip redundant writes."
    echo "See: specs/reviews/task-018.md F3 (race condition between legacy and chain note writes)"
    exit 1
fi
