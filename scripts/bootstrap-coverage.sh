#!/usr/bin/env bash
# Generates specs/coverage/system/*.md from spec headings.
# Mechanical — no LLM. Every section starts as not-started.
# The auditor agent classifies n/a and implemented later.
# Idempotent — skips files that already exist.
set -euo pipefail

cd "$(dirname "$0")/.."

SPEC_DIR="specs/system"
COV_DIR="specs/coverage/system"
mkdir -p "$COV_DIR"

generated=0
skipped=0

for spec in "$SPEC_DIR"/*.md; do
  [ -f "$spec" ] || continue
  name=$(basename "$spec" .md)
  out="$COV_DIR/$name.md"

  if [ -f "$out" ]; then
    skipped=$((skipped + 1))
    continue
  fi

  # Extract title from first heading
  title=$(head -1 "$spec" | sed 's/^# //')
  title=$(echo "$title" | sed 's/"/\\"/g')

  # Extract ## and ### headings
  sections=()
  while IFS= read -r line; do
    depth=$(echo "$line" | sed 's/[^#]//g' | wc -c)
    depth=$((depth - 1))  # ## = 2, ### = 3
    heading=$(echo "$line" | sed 's/^#* //')
    # Escape pipes in heading text
    heading=$(echo "$heading" | sed 's/|/\\|/g')
    sections+=("${depth}|${heading}")
  done < <(grep -E '^#{2,3} ' "$spec")

  total=${#sections[@]}

  {
    echo "# Coverage: ${title}"
    echo ""
    echo "**Spec:** [\`system/${name}.md\`](../../system/${name}.md)"
    echo "**Last audited:** -"
    echo "**Coverage:** 0/${total}"
    echo ""
    echo "| # | Section | Depth | Status | Task | Notes |"
    echo "|---|---------|-------|--------|------|-------|"

    i=0
    for entry in "${sections[@]}"; do
      i=$((i + 1))
      depth="${entry%%|*}"
      heading="${entry#*|}"
      printf "| %d | %s | %d | not-started | - | |\n" "$i" "$heading" "$depth"
    done
  } > "$out"

  generated=$((generated + 1))
  echo "OK: $out ($total sections)"
done

echo ""
echo "Generated: $generated  Skipped: $skipped"

# Regenerate summary
bash scripts/update-coverage-summary.sh
