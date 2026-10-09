# Rebase conflict resolution

The assigned task branch is stopped in a rebase onto current `origin/main`.
Inspect each conflict and preserve both the task's intended behavior and newer
mainline work. Resolve, stage, and continue the rebase. Run focused checks for
the touched surface. Do not discard changes merely to finish the rebase. If
the conflict cannot be resolved safely, leave it unresolved and explain why.
Commit attribution is refreshed mechanically by the driver after rebasing;
do not reconstruct historical commit chains. Once the rebase is complete,
end this role. Implementation and independent review follow separately.
