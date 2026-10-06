# Task review round

Review the assigned task's changed code against the cited spec and its review
history. Inspect the actual diff and call sites. Check whether the behavior is
enforced, scoped, durable, and wired through the real entry point. Look for
tests that could pass without the behavior. Run focused probes where they can
settle a material doubt. The controller separately runs deterministic gates
on the eventual integration commit.

Write concrete findings with file and behavior evidence to the task's review
file. If any material gap remains, set `progress: needs-revision` and leave a
clear repair path. Set `progress: complete` only when the task really meets the
spec. Do not add generic checklist prose to prompts; turn mechanically
detectable failure classes into scripts or focused tests.
