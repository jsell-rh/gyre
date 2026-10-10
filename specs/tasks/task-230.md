---
title: "Repair verified failure on main 05709c242509"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `05709c242509b89214876339b3c463ede3a31b60`
Environment fingerprint: `host-e53f6e99463f742f5191d2de35170eae5d1c03d8e75b34f4753624d18897d1e4`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 257 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/tools/checks.sh
rustfmt: changed lines clean (20 Rust files checked)
clippy: changed lines clean (20 Rust files, 343 existing warnings outside changes)
Architecture lint passed: gyre-domain has no forbidden dependencies or I/O.
Hierarchy lint passed: all hierarchy fields are non-optional.
WARN: stale exemptions in /home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/checkout/scripts/abac-route-registry-exemptions.txt (route now covered or unregistered — remove the line):
  /api/v1/repos/:id/spec-assertions/check
OK: all registered /api/v1/ routes resolve in the ABAC registry (or are exempted legacy entries).
check-abac-exempt-handlers: OK (89 handler(s) checked)
check-mcp-write-tools: OK (8 write-capable tool(s) checked, all gated)
OK: no duplicate Diesel migration versions.
OK: no dialect-only SQL in shared migrations.
OK: every MessageKind variant has an emitter (or documented exemption).
check-byte-slice-truncation: OK
check-relative-path-defaults: OK
OK: no fail-open .unwrap_or_default()/.unwrap_or("") on resolve_ref() results.
FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:

  05709c24  task-196  feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/a77ec9048d8f453d98a830c38102a0ca/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "05709c242509b89214876339b3c463ede3a31b60", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  05709c24  task-196  feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-6teg99bw/scripts/task-commit-attribution-exemptions.txt.\n"}

```
## Shipped

Reproduced and repaired the verified upstream failure on main `05709c24`.

- **Reproduction at assignment HEAD** (tree = base `05709c242509b89214876339b3c463ede3a31b60` + untracked task file): `bash scripts/check-task-commit-attribution.sh` exited 1 listing `05709c24 task-196 feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation` — a product-surface commit (touches `crates/gyre-server/src/api/graph.rs`, `web/src/__tests__/Briefing.test.js`, `web/src/components/Briefing.svelte`, `web/src/lib/InlineChat.svelte`) absent from `specs/tasks/task-196.md`'s `commits:` frontmatter. Root cause is the same squash-drift class repaired by tasks 213/219/222/224: the squashed landing commit cannot contain its own SHA, so task-196's recorded list kept only the pre-squash branch SHAs (`3b90956c`, `abcfff04`, `88b57180`) and the landing surface was invisible to review scoping (task-095 R3-F4 flaw class). Evidence: `task-230-attribution-before.txt` (script exit 1).
- **Repair**: appended the landing SHA `05709c242509b89214876339b3c463ede3a31b60` to `specs/tasks/task-196.md`'s `commits:` frontmatter, joining the three pre-squash SHAs. This is the check's own documented remedy and the task-224 precedent (`a11ba8d3` for task-068): pre-squash SHAs are kept as historical review-scoping records, the landing SHA is appended, not substituted. Label-scoped re-derivation (mirroring `dev-attribution.py`'s history scan, read-only) confirms `05709c24` is the only reachable product-surface commit labeled task-196. No exemptions added; `scripts/task-commit-attribution-exemptions.txt` untouched at its frozen 3-entry baseline; no check, skip, or gate weakened; no Rust/JS source changed (`git diff 05709c24` touches only `specs/tasks/`, 1 line).
- **Probe after repair**: exit 0 — `OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift).` Evidence: `task-230-attribution-after.txt`.
- **Mutation check (test-the-repair)**: with the repair present, reverting task-196.md via `git checkout --` re-fails the gate with the identical violation (exit 1), and re-applying the patch re-passes (exit 0) — the pass is attributable to the recorded SHA, not gate drift. Evidence: `task-230-mutation-check.txt`, `task-230-mutation-restore-check.txt`.
- **Baseline log's ABAC WARN investigated**: the baseline runner's log showed `WARN: stale exemptions ... /api/v1/repos/:id/spec-assertions/check` from a different checkout (`/tmp/gyre-gate-baseline-6teg99bw`). In this tree `check-abac-route-registry.sh` exits 0 with no WARN both before (stashed) and after the repair — at this tree that route is registered in `api/mod.rs` and absent from the resolver, so its exemption line is live, not stale. Advisory-only either way; the assigned failing probe is the attribution gate. Evidence: `task-230-abac-warn-investigation.txt`.
- **Attribution for this task**: this repair round adds only task records (`specs/tasks/task-196.md`, `specs/tasks/task-230.md`), no product surface, so `commits: []` is correct (`dev-attribution.py task-230` produced no change).
