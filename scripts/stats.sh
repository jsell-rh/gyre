#!/usr/bin/env bash
# Gyre Build Stats - tracks velocity, quality, and cost metrics across tasks.
# Usage: ./scripts/stats.sh [--json]
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TASKS_DIR="$REPO_ROOT/specs/tasks"
REVIEWS_DIR="$REPO_ROOT/specs/reviews"
WORKTREE_BASE="$REPO_ROOT/worktrees/workers"
JSON_MODE=false
[[ "${1:-}" == "--json" ]] && JSON_MODE=true

# --- Colors (disabled for JSON) ---
if $JSON_MODE; then
    BOLD="" DIM="" RESET="" GREEN="" YELLOW="" RED="" CYAN="" BLUE="" MAGENTA=""
else
    BOLD=$'\033[1m' DIM=$'\033[2m' RESET=$'\033[0m'
    GREEN=$'\033[32m' YELLOW=$'\033[33m' RED=$'\033[31m'
    CYAN=$'\033[36m' BLUE=$'\033[34m' MAGENTA=$'\033[35m'
fi

# --- Launch slow background jobs immediately so they overlap with local parsing ---
OUTLIER_GAP=3600  # 1 hour

# git log: single pass for per-task timing + global stats.
# Cache result in /tmp keyed by HEAD hash — valid until the next commit.
_HEAD_HASH=$(cd "$REPO_ROOT" && git rev-parse HEAD 2>/dev/null || echo "none")
_GIT_CACHE="/tmp/gyre-stats-git-${_HEAD_HASH}.tsv"
_git_tmp=$(mktemp)
if [[ -f "$_GIT_CACHE" ]]; then
    cp "$_GIT_CACHE" "$_git_tmp" &
    _pid_git=$!
else
    (cd "$REPO_ROOT" && git log --all --format="%at %s" --reverse 2>/dev/null | awk -v gap="$OUTLIER_GAP" '
{
    ts = $1
    gl_count++
    if (gl_first == "") gl_first = ts
    gl_last = ts
    subj = substr($0, index($0, " ") + 1)
    if (index(subj, "fix(process)") > 0) gl_checklist++
    lc = tolower(subj)
    while (match(lc, /task-[0-9]+/)) {
        tid = substr(lc, RSTART, RLENGTH)
        if (tid in first_ts) {
            diff = ts - last_ts[tid]
            if (diff > 0 && diff < gap) active[tid] += diff
        } else {
            first_ts[tid] = ts
            cnt[tid] = 0
        }
        last_ts[tid] = ts
        cnt[tid]++
        lc = substr(lc, RSTART + RLENGTH)
    }
}
END {
    print "_SUMMARY_\t" gl_first "\t" gl_last "\t" gl_checklist+0 "\t" gl_count+0
    for (t in first_ts)
        if (cnt[t] >= 2)
            print t "\t" first_ts[t] "\t" last_ts[t] "\t" active[t] "\t" cnt[t]
}' | tee "$_GIT_CACHE") > "$_git_tmp" &
    _pid_git=$!
fi

# Rust code metrics: cache by HEAD hash (code changes land in commits)
_CODE_CACHE="/tmp/gyre-stats-code-${_HEAD_HASH}.tsv"
_rust_tmp=$(mktemp)
_svelte_tmp=$(mktemp)
if [[ -f "$_CODE_CACHE" ]]; then
    # Split cached lines: line 1 = rust totals, line 2 = svelte total
    { sed -n '1p' "$_CODE_CACHE" > "$_rust_tmp"; sed -n '2p' "$_CODE_CACHE" > "$_svelte_tmp"; } &
    _pid_rust=$!; _pid_svelte=$!
else
    # Rust: single find+awk pass (total + test lines)
    (find "$REPO_ROOT/crates" -name "*.rs" -not -path "*/.git/*" -not -path "*/target/*" 2>/dev/null \
        | xargs awk '
            FNR==1 { cur=FILENAME; has_test[cur]=0 }
            /\#\[cfg\(test\)\]|\#\[test\]/ { has_test[cur]=1 }
            { line_count[cur]++ }
            END {
                total=0; test_total=0
                for (f in line_count) {
                    total += line_count[f]
                    if (has_test[f]) test_total += line_count[f]
                }
                print total "\t" test_total
            }' 2>/dev/null) > "$_rust_tmp" &
    _pid_rust=$!

    # Svelte/TS line count
    find "$REPO_ROOT/web/src" \( -name "*.svelte" -o -name "*.ts" \) 2>/dev/null \
        | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}' > "$_svelte_tmp" &
    _pid_svelte=$!
fi

# --- Task status counts (fast local awk, runs while git/find work in background) ---
total_tasks=0; complete=0; in_review=0; in_progress=0; not_started=0; needs_revision=0
declare -a task_names=() task_statuses=() task_titles=()

declare -A _task_status_map=() _task_title_map=()
while IFS=$'\t' read -r name status title; do
    _task_status_map["$name"]="$status"
    _task_title_map["$name"]="$title"
done < <(awk '
    FNR == 1 {
        if (NR > 1 && file != "") _flush()
        file = FILENAME
        sub(".*/", "", file)
        sub(/\.md$/, "", file)
        in_fm = 0; fm_count = 0; status = "unknown"; title = ""
    }
    /^---$/ { fm_count++; in_fm = (fm_count == 1); next }
    !in_fm { next }
    /^progress:/ {
        status = substr($0, index($0, $2))
        gsub(/^[ \t"]+|[ \t"]+$/, "", status)
    }
    /^title:/ {
        title = substr($0, index($0, $2))
        gsub(/^[ \t"]+|[ \t"]+$/, "", title)
    }
    function _flush() { print file "\t" status "\t" title }
    END { if (file != "") _flush() }
' "$TASKS_DIR"/task-*.md)

for f in "$TASKS_DIR"/task-*.md; do
    [[ -f "$f" ]] || continue
    total_tasks=$((total_tasks + 1))
    name=$(basename "$f" .md)
    status="${_task_status_map[$name]:-unknown}"

    # Override with live worktree status if a worker is active for this task
    wt_file="$WORKTREE_BASE/$name/specs/tasks/$name.md"
    if [[ -d "$WORKTREE_BASE/$name" ]] && [[ -f "$wt_file" ]]; then
        wt_status=$(awk '/^---$/{c++; if(c==2) exit} c==1 && /^progress:/{s=$2; gsub(/^[ \t"]+|[ \t"]+$/, "", s); print s}' "$wt_file" 2>/dev/null)
        [[ -z "$wt_status" ]] && wt_status="$status"
        # Worktree exists but task not yet updated — treat as in-progress
        [[ "$wt_status" == "not-started" ]] && wt_status="in-progress"
        _task_status_map[$name]="$wt_status"
        status="$wt_status"
    fi
    title="${_task_title_map[$name]:-}"
    [[ -z "$title" ]] && title=$(head -1 "$f" | sed -E 's/^# (TASK-[0-9]+|Task [0-9]+): //')
    # Replace em-dashes with plain dashes for consistent column width
    title="${title//—/-}"
    # Truncate to 28 chars with ellipsis in the middle
    if [[ ${#title} -gt 38 ]]; then
        title="${title:0:18}..${title: -18}"
    fi
    task_names+=("$name")
    task_statuses+=("$status")
    task_titles+=("$title")
    case "$status" in
        complete) complete=$((complete + 1)) ;;
        ready-for-review|in-review) in_review=$((in_review + 1)) ;;
        in-progress) in_progress=$((in_progress + 1)) ;;
        not-started) not_started=$((not_started + 1)) ;;
        needs-revision) needs_revision=$((needs_revision + 1)) ;;
    esac
done

if [[ $total_tasks -eq 0 ]]; then
    echo "No tasks found in $TASKS_DIR"
    exit 0
fi

# --- Review metrics per task (single awk pass over all review files) ---
declare -A review_rounds=() review_findings=()
while IFS=$'\t' read -r name rounds findings; do
    review_rounds["$name"]=$rounds
    review_findings["$name"]=$findings
done < <(awk '
    FNR == 1 {
        if (NR > 1 && file != "") print file "\t" rounds "\t" findings
        file = FILENAME
        sub(".*/", "", file)
        sub(/\.md$/, "", file)
        rounds = 0; findings = 0
    }
    /^\#\# Round [0-9]/ { rounds++ }
    /^\#\# R[0-9]/       { rounds++ }
    /^\#\# Findings/     { rounds++ }
    /process-revision-complete/ { findings++ }
    END { if (file != "") print file "\t" rounds "\t" findings }
' "$REVIEWS_DIR"/task-*.md 2>/dev/null || true)

# --- Harvest background results ---
wait $_pid_git $_pid_rust $_pid_svelte

declare -A task_wall_clock=() task_first_commit=() task_last_commit=()
declare -A task_active_seconds=() task_commits=()
total_commits=0; first_commit_ts=0; last_commit_ts=0; checklist_items=0

while IFS=$'\t' read -r name first last active count; do
    if [[ "$name" == "_SUMMARY_" ]]; then
        first_commit_ts=$first
        last_commit_ts=$last
        checklist_items=$active
        total_commits=$count
    else
        task_first_commit["$name"]=$first
        task_last_commit["$name"]=$last
        task_wall_clock["$name"]=$((last - first))
        task_active_seconds["$name"]=$active
        task_commits["$name"]=$count
    fi
done < "$_git_tmp"
rm -f "$_git_tmp"

total_wall_seconds=$((last_commit_ts - first_commit_ts))

IFS=$'\t' read -r total_rust_lines test_rust_lines < "$_rust_tmp"
total_rust_lines=${total_rust_lines:-0}
test_rust_lines=${test_rust_lines:-0}
prod_rust_lines=$((total_rust_lines - test_rust_lines))

total_svelte_lines=$(cat "$_svelte_tmp" 2>/dev/null || echo 0)
total_svelte_lines=${total_svelte_lines:-0}

# Persist code cache if not already cached
if [[ ! -f "$_CODE_CACHE" ]]; then
    { cat "$_rust_tmp"; cat "$_svelte_tmp"; } > "$_CODE_CACHE"
fi

rm -f "$_rust_tmp" "$_svelte_tmp"

# --- Format helpers ---
fmt_duration() {
    local secs=$1
    local hrs=$((secs / 3600))
    local mins=$(( (secs % 3600) / 60 ))
    local s=$((secs % 60))
    if [[ $hrs -gt 0 ]]; then
        printf "%dh %dm %ds" "$hrs" "$mins" "$s"
    else
        printf "%dm %ds" "$mins" "$s"
    fi
}

progress_bar() {
    local total=$1 n_complete=$2 n_review=$3 n_progress=$4 n_not_started=$5
    local width=30
    local w_complete=$((n_complete * width / total))
    local w_review=$((n_review * width / total))
    local w_progress=$((n_progress * width / total))
    local w_not_started=$((width - w_complete - w_review - w_progress))

    local seg=""
    for ((i=0; i<w_complete; i++)); do seg+="#"; done
    printf "%s%s" "${GREEN}" "$seg"
    seg=""
    for ((i=0; i<w_review; i++)); do seg+="#"; done
    printf "%s%s" "${MAGENTA}" "$seg"
    seg=""
    for ((i=0; i<w_progress; i++)); do seg+="#"; done
    printf "%s%s" "${BLUE}" "$seg"
    seg=""
    for ((i=0; i<w_not_started; i++)); do seg+="-"; done
    printf "%s%s%s" "${DIM}" "$seg" "${RESET}"
}

# --- JSON output ---
if $JSON_MODE; then
    echo "{"
    echo "  \"summary\": {"
    echo "    \"total_tasks\": $total_tasks,"
    echo "    \"complete\": $complete,"
    echo "    \"in_review\": $in_review,"
    echo "    \"in_progress\": $in_progress,"
    echo "    \"not_started\": $not_started,"
    echo "    \"needs_revision\": $needs_revision,"
    echo "    \"progress_pct\": $((complete * 100 / total_tasks)),"
    echo "    \"total_commits\": $total_commits,"
    echo "    \"prod_lines\": $prod_rust_lines,"
    echo "    \"test_lines\": $test_rust_lines,"
    echo "    \"svelte_lines\": $total_svelte_lines,"
    if [[ $total_rust_lines -gt 0 ]]; then
        echo "    \"test_ratio\": \"$(printf '%.1f' "$(echo "scale=1; $test_rust_lines * 100 / $total_rust_lines" | bc)")%\","
    else
        echo "    \"test_ratio\": \"0%\","
    fi
    echo "    \"total_wall_clock_seconds\": $total_wall_seconds,"
    echo "    \"checklist_items\": $checklist_items"
    echo "  },"
    echo "  \"tasks\": ["
    first=true
    for i in "${!task_names[@]}"; do
        name="${task_names[$i]}"
        status="${task_statuses[$i]}"
        rounds=${review_rounds[$name]:-0}
        findings=${review_findings[$name]:-0}
        active=${task_active_seconds[$name]:-0}
        wall=${task_wall_clock[$name]:-0}
        commits=${task_commits[$name]:-0}
        $first || echo ","
        first=false
        printf '    {"task": "%s", "status": "%s", "commits": %d, "review_rounds": %d, "findings": %d, "active_seconds": %d, "wall_seconds": %d}' \
            "$name" "$status" "$commits" "$rounds" "$findings" "$active" "$wall"
    done
    echo ""
    echo "  ]"
    echo "}"
    exit 0
fi

# --- Human output ---
echo ""
echo "${BOLD}Gyre Build Stats${RESET}"
echo "${DIM}$(date '+%Y-%m-%d %H:%M')${RESET}"
echo ""

# Progress
pct=$((complete * 100 / total_tasks))
echo "${BOLD}Progress${RESET}"
printf "  [$(progress_bar $total_tasks $complete $in_review $in_progress $not_started)] %d%% (%d/%d tasks)\n" "$pct" "$complete" "$total_tasks"
echo "  ${GREEN}$complete complete${RESET}  ${MAGENTA}$in_review in review${RESET}  ${BLUE}$in_progress in progress${RESET}  ${RED}$needs_revision needs revision${RESET}  ${DIM}$not_started not started${RESET}"
echo ""

# Timeline
echo "${BOLD}Timeline${RESET}"
echo "  Total wall clock:  $(fmt_duration $total_wall_seconds)"
echo "  Total commits:     $total_commits"
echo "  Checklist items:   $checklist_items (accumulated process learnings)"
echo ""

# Code
echo "${BOLD}Code${RESET}"
echo "  Rust production:   $prod_rust_lines lines"
echo "  Rust test:         $test_rust_lines lines"
echo "  Svelte/TS:         $total_svelte_lines lines"
if [[ $total_rust_lines -gt 0 ]]; then
    echo "  Test ratio:        $(printf '%.0f' "$(echo "scale=1; $test_rust_lines * 100 / $total_rust_lines" | bc)")% of Rust"
fi
if [[ $total_wall_seconds -gt 0 && $prod_rust_lines -gt 0 ]]; then
    echo "  Throughput:        ~$(( prod_rust_lines * 3600 / total_wall_seconds )) prod lines/hr"
fi
echo ""

# Per-task breakdown
echo "${BOLD}Task Breakdown${RESET}"
printf "  ${DIM}%-8s  %-38s  %-16s  %7s  %6s  %8s  %14s${RESET}\n" "TASK" "TITLE" "STATUS" "COMMITS" "ROUNDS" "FINDINGS" "ACTIVE TIME"
printf "  %s%s%s\n" "${DIM}" "------------------------------------------------------------------------------------------------------" "${RESET}"

total_rounds=0; total_findings=0; total_active=0; total_task_commits=0
done_rounds=0; done_findings=0; done_active=0; done_commits=0

print_task_row() {
    local name="$1" status="$2" title="$3" commits="$4" rounds="$5" findings="$6" active="$7"
    local status_label status_color sc rpad rc fpad at cpad

    case "$status" in
        complete)         status_label="complete";         status_color="$GREEN" ;;
        ready-for-review) status_label="ready-for-review"; status_color="$MAGENTA" ;;
        needs-revision)   status_label="needs-revision";   status_color="$RED" ;;
        in-review)        status_label="in-review";        status_color="$YELLOW" ;;
        in-progress)      status_label="in-progress";      status_color="$BLUE" ;;
        not-started)      status_label="not-started";      status_color="$DIM" ;;
        *)                status_label="$status";          status_color="" ;;
    esac
    sc="${status_color}$(printf '%-16s' "$status_label")${RESET}"

    rpad=$(printf "%6d" "$rounds")
    if [[ $rounds -eq 0 ]]; then
        rc="${DIM}${rpad}${RESET}"
    elif [[ $rounds -le 3 ]]; then
        rc="${GREEN}${rpad}${RESET}"
    elif [[ $rounds -le 7 ]]; then
        rc="${YELLOW}${rpad}${RESET}"
    else
        rc="${RED}${rpad}${RESET}"
    fi

    fpad=$(printf "%8d" "$findings")
    cpad=$(printf "%7d" "$commits")

    if [[ $active -gt 0 ]]; then
        at=$(printf "%14s" "$(fmt_duration $active)")
    else
        at="$(printf '%12s' '')${DIM}--${RESET}"
    fi

    printf "  %-8s  %-38s  %s  %s  %s  %s  %s\n" "$name" "$title" "$sc" "$cpad" "$rc" "$fpad" "$at"
}

for i in "${!task_names[@]}"; do
    name="${task_names[$i]}"
    status="${task_statuses[$i]}"
    rounds=${review_rounds[$name]:-0}
    findings=${review_findings[$name]:-0}
    active=${task_active_seconds[$name]:-0}
    commits=${task_commits[$name]:-0}

    total_rounds=$((total_rounds + rounds))
    total_findings=$((total_findings + findings))
    total_active=$((total_active + active))
    total_task_commits=$((total_task_commits + commits))

    if [[ "$status" == "complete" ]]; then
        # Accumulate completed tasks into a single deferred row
        done_rounds=$((done_rounds + rounds))
        done_findings=$((done_findings + findings))
        done_active=$((done_active + active))
        done_commits=$((done_commits + commits))
    else
        print_task_row "$name" "$status" "${task_titles[$i]}" "$commits" "$rounds" "$findings" "$active"
    fi
done

# Print collapsed completed row (at end, after active tasks)
if [[ $complete -gt 0 ]]; then
    print_task_row "${complete} done" "complete" "--- completed tasks (${complete}) ---" \
        "$done_commits" "$done_rounds" "$done_findings" "$done_active"
fi

printf "  %s%s%s\n" "${DIM}" "------------------------------------------------------------------------------------------------------" "${RESET}"
printf "  ${BOLD}%-8s  %-38s  %-16s  %7d  %6d  %8d  %14s${RESET}\n" "TOTAL" "" "" "$total_task_commits" "$total_rounds" "$total_findings" "$(fmt_duration $total_active)"
echo ""

# Review efficiency
if [[ $total_findings -gt 0 && $complete -gt 0 ]]; then
    echo "${BOLD}Review Efficiency${RESET}"
    echo "  Total findings:        $total_findings"
    echo "  Total review rounds:   $total_rounds"
    if [[ $total_rounds -gt 0 ]]; then
        echo "  Findings per round:    $(printf '%.1f' "$(echo "scale=1; $total_findings / $total_rounds" | bc)")"
    fi
    echo "  Avg rounds per task:   $(printf '%.1f' "$(echo "scale=1; $total_rounds / $complete" | bc)") (completed tasks only)"
    echo ""

    echo "  ${DIM}Highest review effort:${RESET}"
    for name in "${task_names[@]}"; do
        r=${review_rounds[$name]:-0}
        f=${review_findings[$name]:-0}
        [[ $r -gt 0 ]] && echo "    $r rounds, $f findings  $name"
    done | sort -rn | head -3
    echo ""
fi

# Spec coverage
if [ -f "$REPO_ROOT/specs/coverage/SUMMARY.md" ]; then
    echo "${BOLD}Spec Coverage${RESET}"
    total_line=$(grep 'TOTAL' "$REPO_ROOT/specs/coverage/SUMMARY.md" 2>/dev/null || true)
    if [ -n "$total_line" ]; then
        cov_pct=$(echo "$total_line" | grep -oP '\d+%' | tail -1 || echo "0%")
        # Parse pipe-delimited fields, strip bold markers ** and whitespace
        cov_total=$(echo "$total_line" | awk -F'|' '{v=$3; gsub(/[* ]/, "", v); print v}')
        cov_ns=$(echo "$total_line" | awk -F'|' '{v=$5; gsub(/[* ]/, "", v); print v}')
        echo "  Overall:           ${GREEN}${cov_pct}${RESET} (${cov_total} sections, ${cov_ns} not started)"
    fi
    echo "  ${DIM}Lowest coverage:${RESET}"
    # Parse non-header, non-total rows; field 3=Total, field 9=Coverage%
    grep '^|' "$REPO_ROOT/specs/coverage/SUMMARY.md" | grep -v 'TOTAL\|Spec\|---' | \
        while IFS='|' read -r _ spec total _ _ _ _ _ cov _; do
            spec=$(echo "$spec" | xargs)
            total=$(echo "$total" | xargs)
            cov=$(echo "$cov" | xargs)
            [ "$total" = "0" ] && continue
            printf "    %-6s %s\n" "$cov" "$spec"
        done | sort -n | head -5
    echo ""
fi

# Loop status
echo "${BOLD}Loop${RESET}"
if [ -f /tmp/gyre-loop.log ]; then
    active_workers=0
    worker_names=""
    if [ -d "$WORKTREE_BASE" ]; then
        for wt in "$WORKTREE_BASE"/task-*/; do
            [ -d "$wt" ] || continue
            wt_name=$(basename "$wt")
            if [ ! -f "$wt/.done" ]; then
                active_workers=$((active_workers + 1))
                phase=$(grep "\[$wt_name\]" /tmp/gyre-loop.log 2>/dev/null | tail -1 | grep -oP '>>>\s*\K\w+' || echo "working")
                worker_names+="    ${CYAN}$wt_name${RESET}  ${DIM}($phase)${RESET}\n"
            fi
        done
    fi

    if [ $active_workers -gt 0 ]; then
        echo "  Mode:               ${CYAN}parallel${RESET} ($active_workers workers)"
        echo -e "$worker_names"
    else
        last_line=$(tail -1 /tmp/gyre-loop.log)
        echo "  Last action:        $last_line"
    fi

    iterations=$(grep "Orchestrator cycle" /tmp/gyre-loop.log 2>/dev/null | wc -l)
    echo "  Iterations:         $iterations"

    merges=$(grep -E "Merge successful|Conflict auto-resolved" /tmp/gyre-loop.log 2>/dev/null | wc -l)
    [ "$merges" -gt 0 ] && echo "  Worktree merges:    $merges"
else
    echo "  ${DIM}(loop not running — start with: tmux new-session -d -s gyre-loop && bash scripts/loop.sh)${RESET}"
fi
echo ""

# Experiment comparison
echo "${BOLD}Experiment Comparison${RESET}"
echo "  ${DIM}Run this script after a second build to compare:${RESET}"
echo "  ${DIM}  ./scripts/stats.sh --json > run1.json${RESET}"
echo "  ${DIM}  # rebuild with updated prompts${RESET}"
echo "  ${DIM}  ./scripts/stats.sh --json > run2.json${RESET}"
echo "  ${DIM}  diff <(jq . run1.json) <(jq . run2.json)${RESET}"
echo ""
