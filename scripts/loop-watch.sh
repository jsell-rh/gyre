#!/usr/bin/env bash
# Watch all loop workers in one tiled tmux window.
#
# Moves every worker window's pane into a single "watch" window and tiles it.
# Re-run to pick up newly spawned workers (their windows are not auto-joined).
#
# Usage: bash scripts/loop-watch.sh [window-name]
# Then:  tmux attach -t gyre-loop   (select the "watch" window)
set -euo pipefail

SESSION=${GYRE_TMUX_SESSION:-gyre-loop}
WATCH=${1:-watch}

command -v tmux >/dev/null 2>&1 || { echo "!!! tmux not installed"; exit 1; }
tmux has-session -t "$SESSION" 2>/dev/null || { echo "!!! no tmux session '$SESSION' (is the loop running?)"; exit 1; }

# (Re)create the watch window
tmux kill-window -t "$SESSION:$WATCH" 2>/dev/null || true
tmux new-window -d -t "$SESSION" -n "$WATCH" "exec sleep infinity"

# Join every worker window (by name — indices shift as windows are destroyed).
# Skip the watch window itself and the loop's idle session-anchor window.
while read -r name; do
  case "$name" in
    "$WATCH"|tmux) continue ;;
  esac
  tmux join-pane -s "$SESSION:$name" -t "$SESSION:$WATCH" 2>/dev/null || true
done < <(tmux list-windows -t "$SESSION" -F '#{window_name}')

# Drop the placeholder pane (the `sleep` anchor the watch window was created with)
tmux list-panes -t "$SESSION:$WATCH" -F '#{pane_index} #{pane_current_command}' \
  | awk '$2=="sleep"{print $1}' \
  | while read -r idx; do tmux kill-pane -t "$SESSION:$WATCH.$idx" 2>/dev/null || true; done

tmux select-layout -t "$SESSION:$WATCH" tiled
tmux select-window -t "$SESSION:$WATCH"

echo "Watch window '$WATCH' ready — tmux attach -t $SESSION"
