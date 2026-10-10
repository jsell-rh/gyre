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
Environment fingerprint: `host-b66093bb886dfa090b75d0af8ab21ae06e2f90ee0f9e94718a50425d14d18a54`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/374e273dcda44a4796a3224a8b4963d7/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/374e273dcda44a4796a3224a8b4963d7/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/374e273dcda44a4796a3224a8b4963d7/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/374e273dcda44a4796a3224a8b4963d7/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/374e273dcda44a4796a3224a8b4963d7/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/374e273dcda44a4796a3224a8b4963d7/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/374e273dcda44a4796a3224a8b4963d7/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/374e273dcda44a4796a3224a8b4963d7/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/374e273dcda44a4796a3224a8b4963d7/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/374e273dcda44a4796a3224a8b4963d7/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/374e273dcda44a4796a3224a8b4963d7/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/374e273dcda44a4796a3224a8b4963d7/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 257 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/374e273dcda44a4796a3224a8b4963d7/1/tools/checks.sh
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

  05709c24  task-196  feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/374e273dcda44a4796a3224a8b4963d7/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "05709c242509b89214876339b3c463ede3a31b60", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  05709c24  task-196  feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-y2tso_qc/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Contract restoration (this round, finding 46fad40e, category=contract):** the
prior round's checkpointed task file dropped the assigned contract's
`spec_ref: "GOAL.md — real implementations and meaningful verification"`
frontmatter line, changing the requirement generation (dev-contract.py hashes
`title`/`spec_ref`/`depends_on`/`coverage_sections` + non-operational prose).
The contract is restored byte-for-byte (verified below); `progress: not-started`
→ `ready-for-review` and this `## Shipped` section are the only operational
additions. Product-source repair from the prior round is preserved untouched:
`git diff 05709c242509b89214876339b3c463ede3a31b60 HEAD -- scripts/ crates/
web/src/` is empty; the branch delta vs base is exactly `specs/tasks/task-196.md`
(1 line) + this task file.

**Reproduction:** with the repair removed (task-196 frontmatter SHA stripped via
sed mutation), `bash scripts/check-task-commit-attribution.sh` exits 1 with the
identical violation: `05709c24 task-196 feat(task-196): Ground Briefing Q&A in
real briefing data with sources and history validation`. Evidence:
`/tmp/stage/review-evidence/task-228-r2-attribution-repro.txt`.

**Repair (1 line, `specs/tasks/task-196.md:8`):** appended the full SHA
`05709c242509b89214876339b3c463ede3a31b60` to the existing `commits:` list,
preserving the 3 branch SHAs (`3b90956c`, `abcfff04`, `88b57180`). The check's
own documented remedy — same repair shape as task-224's `a11ba8d3` (task-068
drift) and task-219's `e96d25ab` (task-200 drift): the squashed landing commit
cannot contain its own SHA, so the recorded list stayed one entry short and the
landing surface was invisible to review scoping (task-095 R3-F4 flaw class).
No exemptions added — `scripts/task-commit-attribution-exemptions.txt` frozen at
its 3-entry baseline (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4
task-091`); no gate/skip/check weakened; no Rust/JS source changed.

**Probe after repair:** exit 0 — `OK: every task-labeled product-surface commit
is recorded in its task's commits: frontmatter (or exempted legacy drift).`
Evidence: `/tmp/stage/review-evidence/task-228-r2-attribution-after-repair.txt`.

**Mutation check (test-the-repair):** repair present → exit 0; SHA removed
(mutated out via sed) → identical FAIL exit 1; repair restored → exit 0 again.
The pass is attributable to the recorded SHA, not gate drift. Evidence: both
files above (repro = mutated state, after-repair = restored state).

**Attribution for this task:** `python3 scripts/dev-attribution.py task-228`
derives an empty list — the branch's only commit (`edb132c3`, pipeline
checkpoint) touches only `specs/tasks/`, no product surface — so `commits: []`
is attribution-canonical for this specs-only repair round.

**Contract-hash invariance proof:** `dev-contract.py requirement_parts()` maps
this task file (before vs after this round's operational additions) to
identical `(front, prose)` — the restoration round changed nothing normative;
the prior round's contract finding (dropped `spec_ref`) is repaired. Full
command and output:
`/tmp/stage/review-evidence/task-228-r2-contract-hash-proof.txt`.
