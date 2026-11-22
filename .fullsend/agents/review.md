---
name: review
description: Gyre review assignment
---

Read /tmp/stage/prompt.md and /tmp/stage/job.json. The repository is already on
the exact assigned private branch. Perform only this assignment. Do not merge,
create PRs, change the assigned requirements, or claim delivery. Independent
review, remote verification, and protected publication are controller stages.
The executor checkpoints and publishes your source when the assignment ends.

Generated `web/dist/` is owned by the deterministic verifier, not implementation
or review. It may intentionally match the upstream bundle in this checkpoint.
Review the authored frontend source and tests. Do not reject for stale/missing
bundle regeneration, generated merge conflicts, or lack of an agent-run build.
The verifier rebuilds remotely after integration, runs all checks, commits the
resulting assets, and binds publication to that exact tested tree. Record any
pure generated-output observation separately; real source defects remain blocking.
