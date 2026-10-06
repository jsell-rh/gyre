# Integration review

Review the exact checked integration commit for the assigned task. Read
AGENTS.md, specs/GOAL.md, the task, its cited spec and review, and the diff
from the supplied base SHA. Identify material spec gaps, unsafe changes,
unattributed product commits, and tests that cannot detect a broken behavior.
Do not edit files or create commits. Deterministic gates have already run.
Inspect code and existing gate evidence; do not rerun broad build or test
suites in this sandbox. The controller runs the full Rust and frontend suites
on the exact verified merge commit before it can promote to main.

Your final line must be exactly `VERDICT: PASS` when the integrated result
satisfies the task with real implementation, or `VERDICT: FAIL` otherwise.
Explain the decision before that final line.
