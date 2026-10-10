---
title: "Repair verified failure on main 27bd585ca7eb"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `27bd585ca7eb429905ccbded1f48b4d0167c0c20`
Environment fingerprint: `host-14665ca7f6538829033a7fb7478c2129ea62280c7c766883c2a7691b0d0ebabc`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/b8560bd57a694cf7bb09ad10a4beb156/2/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/b8560bd57a694cf7bb09ad10a4beb156/2/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/b8560bd57a694cf7bb09ad10a4beb156/2/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/b8560bd57a694cf7bb09ad10a4beb156/2/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/b8560bd57a694cf7bb09ad10a4beb156/2/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/b8560bd57a694cf7bb09ad10a4beb156/2/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/b8560bd57a694cf7bb09ad10a4beb156/2/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/b8560bd57a694cf7bb09ad10a4beb156/2/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/b8560bd57a694cf7bb09ad10a4beb156/2/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/b8560bd57a694cf7bb09ad10a4beb156/2/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/b8560bd57a694cf7bb09ad10a4beb156/2/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/b8560bd57a694cf7bb09ad10a4beb156/2/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 257 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/b8560bd57a694cf7bb09ad10a4beb156/2/tools/checks.sh
rustfmt: changed lines clean (0 Rust files checked)
clippy: changed lines clean (0 Rust files, 343 existing warnings outside changes)
Architecture lint passed: gyre-domain has no forbidden dependencies or I/O.
Hierarchy lint passed: all hierarchy fields are non-optional.
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

  27bd585c  task-155  feat(task-155): Implement gyre search CLI command

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/b8560bd57a694cf7bb09ad10a4beb156/2/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "27bd585ca7eb429905ccbded1f48b4d0167c0c20", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  27bd585c  task-155  feat(task-155): Implement gyre search CLI command\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-8pnuhaum/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Reproduction:** `bash scripts/check-task-commit-attribution.sh` at base
`27bd585ca7eb429905ccbded1f48b4d0167c0c20` exits 1 with exactly the durable
finding's violation: `27bd585c task-155 feat(task-155): Implement gyre search
CLI command`. Evidence:
`/tmp/stage/review-evidence/task-232-attribution-repro.txt`.

**Root cause:** commit `27bd585c` is task-155's squashed product landing
(+975 lines: `crates/gyre-cli/src/client.rs`, `crates/gyre-cli/src/main.rs`,
`docs/cli.md`, `specs/tasks/task-155.md`) — the candidate itself, checkpointed
and published to main by the executor while task-155's `commits:` frontmatter
still listed only the 7 branch SHAs from the working attempt
(`4c0df440`, `2b6f3372`, `1292303a`, `951037f8`, `0196a149`, `4e5b20d2`,
`6bc9a54d`). The squashed landing commit cannot contain its own SHA, so the
recorded list stayed one entry short and the whole +975-line landing surface
was invisible to review scoping — the exact task-095 R3-F4 flaw class the gate
was built to catch. Same drift shape as task-224's `a11ba8d3` (task-068) and
task-228's `05709c24` (task-196).

**Repair (1 line, `specs/tasks/task-155.md:8`):** appended the full SHA
`27bd585ca7eb429905ccbded1f48b4d0167c0c20` to the existing `commits:` list,
preserving all 7 branch SHAs. The gate's own documented remedy. No exemptions
added — `scripts/task-commit-attribution-exemptions.txt` unchanged at its
frozen 3-entry baseline; no gate, skip, or check weakened; no Rust/JS product
source changed (this repair's only code-tree change is the one frontmatter
line; `git diff` shows nothing else).

**Probe after repair:** exit 0 — `OK: every task-labeled product-surface
commit is recorded in its task's commits: frontmatter (or exempted legacy
drift).` Evidence: `/tmp/stage/review-evidence/task-232-attribution-after-repair.txt`
(fresh re-run: `task-232-attribution-final.txt`).

**Mutation check (test-the-repair):** repair present → exit 0; SHA removed
(working tree stashed back to base state via `git stash push`) → identical
FAIL exit 1 with the same `27bd585c task-155` violation; repair restored
(`git stash pop`) → exit 0 again. The pass is attributable to the recorded
SHA, not gate drift. Evidence: `task-232-attribution-repro.txt` (mutated
state) and `task-232-attribution-final.txt` (restored state).

**Attribution for this task:** `python3 /tmp/stage/dev-attribution.py
task-232` derives `commits: []` — this branch's only commit will touch only
`specs/tasks/`, no product surface — so `commits: []` is
attribution-canonical for this specs-only repair round, matching the
task-228/task-224 repair-round pattern.

**Contract invariance:** the only normative-content change is task-155's
`commits:` frontmatter line, which `dev-contract.py`'s `requirement_parts`
does not hash (it hashes `title`/`spec_ref`/`depends_on`/`coverage_sections`
+ non-Shipped prose); `## Shipped` sections and `progress:`/`commits:` are
operational fields. Task-155's requirement hash is unchanged by this repair.

**Sandbox limitation (recorded, not a code defect):** the TCP `accept()`
listener probe is unsupported here (errno 95, see
`/tmp/stage/capabilities.json`), so no live-server probes were attempted;
this repair needs none — the failing gate is a pure git-history check that
runs and passes in this sandbox (exit 0 above). Full verification suites and
GitHub CI remain for the verification/publication stage, as with every
repair round.
