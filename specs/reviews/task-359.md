# Review — task-359 (Publish reviewed spec backlog for specs/system/authorization-provenance.md)

Spec: `specs/system/authorization-provenance.md` (canonical, unmodified by the candidate).
Candidate under review: `3172a2ebedfd950c1d003ee540b4eac80455263b` (branch `pipeline/task-359/4081d892b28548c6b7512079b751c09d-1`, base `f3ca7db3bf7ee4b35cab550ad18011c13d8680c4`, single commit `chore: publish reviewed spec backlog fdf43e24079d`).
Verdict: **approved** — no findings. Task creation only; no product code touched.

## What the candidate does

Adds the 8 planned task documents `specs/tasks/task-351.md` … `task-358.md` (assigned names from the plan's `assigned_names` map), the assigned publication task document `specs/tasks/task-359.md` (embedding the plan JSON byte-identically, 215164 bytes), and a purely additive update to `specs/task-spec-inputs.json` (+25 `backlog/<section-id>` section entries, +8 task bindings). Nothing else: no product file, spec, coverage matrix, script, or gate is modified (`git diff --name-status` evidence in `/tmp/stage/review-evidence/README.md`).

## Fidelity to the assigned plan (programmatic, not eyeball)

A comparison script checked every planned task against its document field by field: frontmatter (title, `depends_on` mapped through `assigned_names`, `progress: not-started`, `commits: []`), `## Required behavior` == plan `required_behavior` (exact string), acceptance checklist == plan `acceptance` (exact, ordered), `## Verification intent` == plan `negative_probe` (exact string), and the Discovery-evidence JSON == the audit's section records verbatim (id, decision, requirement_quote, detail, evidence). All 8 tasks: OK. The plan JSON embedded in task-359.md is byte-identical to the assignment's plan block.

## Decomposition, dependencies, duplicate-work (challenged, not assumed)

- The 25 added section bindings are exactly the union of the 8 tasks' `covers` lists; the audit's 25 `gap`-decision sections are covered exactly once each, and no `satisfied`/`context` section is assigned. No cover id is duplicated across tasks.
- All 25 heading paths resolve in the canonical spec; every audit `requirement_quote` appears verbatim in the spec text.
- Dependencies are internal to the plan and coherent (`boundary-enforcement` waits on linkage + verification + output signing; everything else roots at `approval-signed-input-root` or is independent).
- No duplicate active assignment: no existing binding or pre-existing task document references any of the 25 section ids. The only other open task mentioning authorization-provenance.md is task-298 (agent-runtime.md plan; touches it only via the ralph-loop supersession cross-reference). task-188 (§2.4) is complete, consistent with the audit classifying §2.4's binding logic `satisfied` while attributing its production unreachability to the §2.1 linkage gap that task-351/352 close.

## Audit challenged against the code

The spec content is identical between the audit base `973068a8` and the review base (only spec/task files differ), so anchors were verified at HEAD. Spot-verified evidence anchors, all accurate: `specs.rs:897/904` SignedInput silent-skip branches; `key_binding.rs:94/110` proof-of-possession over raw public key bytes with server-assigned identity fields; 16 `bail!` stubs in `postgres/authorization_provenance.rs`; `spawn.rs:4307/4474` no-chain skip and empty `commit_sha`; `specs.rs:864` empty commit_sha at approval; `git_http.rs:5932` structural-only parent linkage; `spawn.rs:4453` empty preconditions; `trust_anchors.rs:247` RBAC-only anchor protection; `DetailPanel.svelte:1038/1056` and `ConstraintEditor.svelte:154,167-181` plain-input UI. The audit's reclassifications (notably the §2.3 retraction) are documented in the audit itself with code evidence that checks out.

## Acceptance criteria of the publication task

- New tasks remain `not-started` (verified in frontmatter); task-359 itself is `ready-for-review`, matching the assignment body.
- No coverage status, spec requirement, product behavior, or enforcement changed (diff scope + empty `specs/coverage/` diff).
- Commit message follows the established publication pattern (cf. task-342's merged `c1ad6b98`); spec-only commits are not task-labeled product-surface commits, so commit-attribution is not implicated.

## Verifier notes

No Rust/web probes are required for this candidate — it contains no production code. The mechanical guard suite (`scripts/check-*.sh`) is unaffected by spec-only additions. Observations recorded as non-blocking in `/tmp/stage/review-evidence/README.md`: the task-numbering jump (342→351) comes from the plan's own `assigned_names`, and the coverage matrix rows for reclassified sections stay untouched by design (implementing tasks update their rows with source evidence, per the task text and the task-342 precedent).
