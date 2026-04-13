#!/usr/bin/env bash
# Converts existing task files from markdown metadata to YAML frontmatter.
# One-time migration. Idempotent — skips files that already have frontmatter.
set -euo pipefail

cd "$(dirname "$0")/.."

for f in specs/tasks/task-*.md; do
  [ -f "$f" ] || continue

  # Skip if already has frontmatter
  if head -1 "$f" | grep -q '^---$'; then
    echo "SKIP (already migrated): $f"
    continue
  fi

  name=$(basename "$f" .md)
  num=$(echo "$name" | sed 's/task-//')

  # --- Extract metadata from markdown ---

  # Title: first line, strip "# TASK-NNN: "
  title=$(head -1 "$f" | sed -E 's/^# TASK-[0-9]+: //')
  # Escape double quotes in title
  title=$(echo "$title" | sed 's/"/\\"/g')

  # Spec reference: grab everything after the bold marker, strip backticks
  spec_ref=$(grep -oP '\*\*Spec reference:\*\*\s*\K.*' "$f" | head -1 || echo "")
  spec_ref=$(echo "$spec_ref" | sed 's/`//g; s/[[:space:]]*$//')
  # Clean trailing whitespace
  spec_ref=$(echo "$spec_ref" | sed 's/[[:space:]]*$//')

  # Progress
  progress=$(grep -oP '\*\*(Progress|Status):\*\*\s*`\K[^`]+' "$f" | head -1 || echo "not-started")

  # Review path: extract from markdown link
  review=$(grep -oP '\*\*Review:\*\*\s*\[.*?\]\(\K[^)]+' "$f" | head -1 || echo "")
  # Normalize: strip leading ../
  review=$(echo "$review" | sed 's|^\.\./|specs/|')

  # Depends on: extract TASK-NNN references
  deps_line=$(grep -oP '\*\*Depends on:\*\*\s*\K.*' "$f" | head -1 || echo "")
  deps_yaml="[]"
  if echo "$deps_line" | grep -qoP 'TASK-\d+'; then
    deps=$(echo "$deps_line" | grep -oP 'TASK-\d+' | \
      sed 's/TASK-/task-/' | \
      awk '!seen[$0]++')  # deduplicate preserving order
    deps_yaml=""
    while IFS= read -r dep; do
      deps_yaml="${deps_yaml}"$'\n'"  - ${dep}"
    done <<< "$deps"
  fi

  # Commits: extract hex SHAs (7-40 chars) from backtick-wrapped refs
  commits_yaml="[]"
  commit_lines=$(grep -oP '`[0-9a-f]{7,40}`' "$f" | tr -d '`' | awk '!seen[$0]++' || true)
  if [ -n "$commit_lines" ]; then
    commits_yaml=""
    while IFS= read -r sha; do
      [ -z "$sha" ] && continue
      commits_yaml="${commits_yaml}"$'\n'"  - ${sha}"
    done <<< "$commit_lines"
  fi

  # --- Strip metadata lines from body ---
  # Remove: title line, metadata lines, first --- separator (if any before line 15)
  body=$(awk '
    NR==1 && /^# TASK-/ { next }
    /^\*\*(Spec reference|Depends on|Progress|Status|Review|Git commits?):\*\*/ { next }
    /^---$/ && NR < 15 && !body_started { next }
    /^[[:space:]]*$/ && !body_started { next }
    { body_started=1; print }
  ' "$f")

  # --- Write new file ---
  {
    echo "---"
    echo "title: \"${title}\""
    echo "spec_ref: \"${spec_ref}\""
    echo "depends_on: ${deps_yaml}"
    echo "progress: ${progress}"
    if [ -n "$review" ]; then
      echo "review: ${review}"
    fi
    echo "coverage_sections: []"
    echo "commits: ${commits_yaml}"
    echo "---"
    echo ""
    echo "${body}"
  } > "$f.tmp"
  mv "$f.tmp" "$f"
  echo "OK: $f"
done

echo ""
echo "Migration complete. Verify with:"
echo "  bash scripts/task-field.sh specs/tasks/task-052.md progress"
echo "  bash scripts/task-field.sh specs/tasks/task-052.md depends_on"
