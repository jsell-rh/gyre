#!/usr/bin/env bash
# Regenerates specs/coverage/SUMMARY.md from individual coverage files.
# Mechanical — no LLM. Called by auditor, PM, and bootstrap.
set -euo pipefail

cd "$(dirname "$0")/.."

COV_DIR="specs/coverage/system"
OUT="specs/coverage/SUMMARY.md"

[ -d "$COV_DIR" ] || { echo "No coverage dir: $COV_DIR"; exit 1; }

{
  echo "# Spec Coverage Summary"
  echo ""
  echo "**Last updated:** $(date -u +%Y-%m-%d)"
  echo ""
  echo "| Spec | Total | n/a | Not Started | Assigned | Implemented | Verified | Coverage |"
  echo "|------|-------|-----|-------------|----------|-------------|----------|----------|"

  total_all=0; na_all=0; ns_all=0; ta_all=0; impl_all=0; ver_all=0

  for f in "$COV_DIR"/*.md; do
    [ -f "$f" ] || continue
    name=$(basename "$f" .md)
    total=$(grep -cP '^\| \d+' "$f" 2>/dev/null || true)
    total=${total:-0}
    na=$(grep -c '| n/a |' "$f" 2>/dev/null || true)
    na=${na:-0}
    ns=$(grep -c '| not-started |' "$f" 2>/dev/null || true)
    ns=${ns:-0}
    ta=$(grep -c '| task-assigned |' "$f" 2>/dev/null || true)
    ta=${ta:-0}
    impl=$(grep -c '| implemented |' "$f" 2>/dev/null || true)
    impl=${impl:-0}
    ver=$(grep -c '| verified |' "$f" 2>/dev/null || true)
    ver=${ver:-0}

    denom=$((total - na))
    if [ "$denom" -gt 0 ]; then
      cov=$(( (impl + ver) * 100 / denom ))
    else
      cov=0
    fi

    printf "| %s.md | %d | %d | %d | %d | %d | %d | %d%% |\n" \
      "$name" "$total" "$na" "$ns" "$ta" "$impl" "$ver" "$cov"

    total_all=$((total_all + total))
    na_all=$((na_all + na))
    ns_all=$((ns_all + ns))
    ta_all=$((ta_all + ta))
    impl_all=$((impl_all + impl))
    ver_all=$((ver_all + ver))
  done

  denom_all=$((total_all - na_all))
  [ "$denom_all" -gt 0 ] && cov_all=$(( (impl_all + ver_all) * 100 / denom_all )) || cov_all=0

  printf "| **TOTAL** | **%d** | **%d** | **%d** | **%d** | **%d** | **%d** | **%d%%** |\n" \
    "$total_all" "$na_all" "$ns_all" "$ta_all" "$impl_all" "$ver_all" "$cov_all"

} > "$OUT"

echo "Updated $OUT"
