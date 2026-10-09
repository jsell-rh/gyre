#!/usr/bin/env bash
# One agent round; the outer sandbox driver checkpoints every round.
set -euo pipefail
task=$1
[[ "$task" =~ ^task-[0-9]+$ ]] || exit 2
cd /tmp/gyre
file="specs/tasks/$task.md"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/gyre-target}"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="${CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER:-cc}"
export RUSTFLAGS="${RUSTFLAGS:--C link-arg=-fuse-ld=lld}"

run_prompt() {
  local role=$1
  local session_dir="/tmp/stage/omp-sessions/$role"
  local session_file compact_note=''
  local review_base=''
  local -a resume=()
  local -a model=(--model "${GYRE_DEV_MODEL:-enmaas-glm-5-3/rits/zai-org/glm-5-3}")
  if [ "$role" = implementation ]; then
    model=(--model "${GYRE_DEV_IMPLEMENTATION_MODEL:-${GYRE_DEV_MODEL:-enmaas-glm-5-3/rits/zai-org/glm-5-3}}")
  fi
  case "$role" in
    review|audit-review) review_base=$(git merge-base origin/main HEAD) ;;
  esac
  mkdir -p "$session_dir"
  session_file=$(find "$session_dir" -maxdepth 1 -name '*.jsonl' -print | sort | tail -n 1)
  if [ -n "$session_file" ]; then
    if [ "$(stat -c %s "$session_file")" -gt "${GYRE_DEV_COMPACT_BYTES:-400000}" ]; then
      compact_note=$(python3 - "$session_file" <<'PY'
import json, sys
texts = []
for line in open(sys.argv[1], encoding="utf-8", errors="replace"):
    try:
        item = json.loads(line)
    except json.JSONDecodeError:
        continue
    msg = item.get("message", {})
    if item.get("type") != "message" or msg.get("role") != "assistant":
        continue
    body = "".join(c.get("text", "") for c in msg.get("content", []) if c.get("type") == "text")
    if body.strip():
        texts.append(body)
print("\n".join(texts[-3:])[-4000:])
PY
      )
      mkdir -p /tmp/stage/omp-archive
      mv "$session_file" "/tmp/stage/omp-archive/$role-$(date +%s).jsonl"
    else
      resume=(-c)
    fi
  fi
  {
    if [ "${#resume[@]}" -eq 0 ]; then
      cat specs/GOAL.md "/tmp/stage/dev-$role.md"
      if [ -n "$compact_note" ]; then
        printf '\n## Bounded handoff from the previous round\n\n%s\n' "$compact_note"
        printf '\n## Current branch changes\n\n'
        git diff --stat origin/main
        printf '\nThe prior round already mapped the code; avoid repeating broad repository exploration.\n'
      fi
    else
      printf '%s\n' 'Continue this task from the saved session. The checkout and task frontmatter are authoritative. Avoid repeating broad repository exploration.'
    fi
    case "$role" in
      implementation) printf '%s\n' 'Make the next concrete production edit, then run a focused check. If blocked, record the specific blocker in the task file.' ;;
      review|audit-review)
        printf '%s\n' 'Review the remaining concrete findings independently. Write the verdict and task status when supported by evidence; do not restart implementation or broad exploration.'
        printf '\n## Current review scope\n\nComparison base: %s\nCurrent HEAD: %s\n' "$review_base" "$(git rev-parse HEAD)"
        printf '%s\n' "Inspect git diff $review_base for the task changes, including uncommitted repairs. A failed-main Base in the task or an older checkpoint in the handoff is diagnostic provenance, not the review comparison base. Changes inherited from main are outside this task diff."
        git diff --stat "$review_base"
        ;;
    esac
    printf '\n## Assigned task\n\n'
    cat "$file"
    if [ -f /tmp/stage/audit-contract.json ]; then
      printf '\n## Mechanically enforced fidelity scope\n\n'
      cat /tmp/stage/audit-contract.json
    fi
    if [ -f /tmp/stage/repair.md ]; then
      printf '\n## Findings from the rejected integration\n\n'
      cat /tmp/stage/repair.md
    fi
    review=$(bash scripts/task-field.sh "$file" review)
    if [ -n "$review" ] && [ -f "$review" ]; then
      printf '\n## Existing review\n\n'
      cat "$review"
    fi
  } | timeout --signal=INT --kill-after=30s "${GYRE_DEV_ROUND_TIMEOUT:-1800}" \
    omp -p "${model[@]}" "${resume[@]}" --session-dir "$session_dir" --mode=json --approval-mode yolo \
    | node /tmp/stage/dev-stream.mjs "$role"
}

progress=$(bash scripts/task-field.sh "$file" progress)
case "$progress" in
  complete) echo "already complete"; exit 0;;
  not-started|in-progress|needs-revision|ready-for-review) ;;
  *) echo "unsupported task progress: $progress" >&2; exit 1;;
esac

git fetch --quiet origin main
if ! git rebase --autostash origin/main; then
  echo "rebase conflict: asking resolver" >&2
  run_prompt rebase
  rebase_merge=$(git rev-parse --git-path rebase-merge)
  rebase_apply=$(git rev-parse --git-path rebase-apply)
  if [ -d "$rebase_merge" ] || [ -d "$rebase_apply" ]; then
    git rebase --abort || true
    exit 1
  fi
  git merge-base --is-ancestor origin/main HEAD || { echo "resolver did not finish rebase onto main" >&2; exit 1; }
fi

python3 /tmp/stage/dev-attribution.py "$task"
progress=$(bash scripts/task-field.sh "$file" progress)
if [ -f /tmp/stage/audit-contract.json ]; then
  python3 /tmp/stage/dev-audit-check.py /tmp/stage/audit-contract.json --generation-only || exit 80
fi
if [ "$progress" = ready-for-review ]; then
  if [ -f /tmp/stage/audit-contract.json ]; then
    run_prompt audit-review
    python3 /tmp/stage/dev-audit-check.py /tmp/stage/audit-contract.json --base origin/main --replay
  else
    run_prompt review
  fi
  if [ "$(bash scripts/task-field.sh "$file" progress)" = complete ]; then
    touch /tmp/stage/review-approved
  fi
else
  rm -f /tmp/stage/review-approved
  if [ -f /tmp/stage/audit-contract.json ]; then run_prompt audit; else run_prompt implementation; fi
fi
