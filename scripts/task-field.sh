#!/usr/bin/env bash
# Extracts a field from YAML frontmatter in a task file.
# Usage: task-field.sh <file> <field>
#
# Scalar fields print the value on one line.
# List fields (depends_on, commits, coverage_sections) print one item per line.
# Quoted strings are unquoted automatically.
# Returns empty (exit 0) if the field is missing or an empty list [].
set -euo pipefail

FILE="$1"
FIELD="$2"

awk '/^---$/{n++; next} n==1' "$FILE" | awk -v field="$FIELD" '
  $0 ~ "^"field":" {
    val = $0
    sub("^"field":[ ]*", "", val)
    # Strip surrounding quotes
    gsub(/^"/, "", val)
    gsub(/"$/, "", val)
    if (val == "[]") { exit }       # explicit empty list
    if (val ~ /^\[\]/) { exit }     # inline empty array
    if (val == "" || val ~ /^[[:space:]]*$/) {
      # Empty value — list items follow on next lines
      found = 1
      next
    }
    print val
    found = 1
    next
  }
  found && /^  - / {
    val = $0
    sub(/^  - /, "", val)
    # Strip surrounding quotes
    gsub(/^"/, "", val)
    gsub(/"$/, "", val)
    print val
    next
  }
  found { exit }
'
