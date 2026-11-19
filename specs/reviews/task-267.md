# Review — task-267 (Publish reviewed spec backlog for specs/system/identity-security.md)

Spec: `specs/system/identity-security.md` (canonical spec, untouched by this candidate).
Candidate under review: `4d0858a` ("chore: publish reviewed spec backlog 8e132f42330e"), base `321615ec`.
Verdict: **approved** — the publication is exact, complete, and introduces no duplicate assignment, no dependency error, and no change to any product file, spec, coverage status, or gate. Evidence: `/tmp/stage/review-evidence/probe-log.md`.

## Scope of the diff

`git diff 321615ec..4d0858a` touches exactly eight files: seven new task documents (`specs/tasks/task-261.md` … `task-267.md`, per the plan's `assigned_names`) and `specs/task-spec-inputs.json`, which gains 10 `backlog/{section-id}` section entries and 6 `Spec gap spec-31d3ae72e12e22c52442e309 / {key} / b60edce5bc49` binding keys. Nothing is removed; `specs/coverage/` has a zero-line diff; no crate, script, web source, or gate file is touched.

## Fidelity to the reviewed plan

For each of task-261 … task-266 I programmatically compared the document against the plan JSON embedded in the assigned task body: frontmatter (title, `depends_on`, `progress: not-started`, `commits: []`), the full `## Required behavior` text, every acceptance bullet, the `## Verification intent` negative probe, and the embedded `## Discovery evidence` audit JSON. **All match verbatim.** The `covers` arrays equal the plan's per-task section lists — all 10 audited gap sections are covered exactly once across the six tasks, with no overlap and no uncovered gap; the 3 `satisfied` and 3 `context` sections correctly receive no task. `task-267.md` is byte-identical to the assigned publication task body (frontmatter, required behavior, plan JSON, acceptance criteria).

## Bindings and canonical spec

`specs/task-spec-inputs.json` remains valid JSON; the six new binding keys equal the new tasks' `spec_ref` frontmatter values and reference only the new backlog sections. All 10 heading paths were verified to exist in `specs/system/identity-security.md` with exact text and hierarchy (H1 → H2 → H3/H4), and all 16 audited `requirement_quote` strings were found in the canonical spec. The format matches the previously merged publication `f3d157bc` (message-bus backlog, task-247…task-255).

## Audit challenge

- **Freshness:** sha256 of all 20 `source_evidence` files is identical at the audit base `fb1424f`, the publication base `321615ec`, and the candidate — the audit is not stale.
- **Spot-checked claims against production code (all confirmed):** `workload_attestation.rs:1` "G10: Local workload attestation — pragmatic SPIFFE alternative"; `auth.rs` `validate_aud = false` (4 sites); zero `spiffe://` / `ScimGroup` / `scim-mappings` matches across `crates/`; `lib.rs:1429` wires `NoopRekorClient`; `api/tasks.rs` `update_task`/`transition_task_status` take no auth extractor; `git_http.rs:154` admits `user_id.is_none()` principals; `api/scim.rs:143` hardcodes `active: true`; `revoke_token` has no production call path.
- **Decomposition:** coherent and dependency-correct — SPIFFE attestation (261) precedes federation (262); the JWT/task-scope sweep (263) is by-effect and explicitly excludes replaying complete task-130 (declared as dep); the Sigstore delta (264) is narrowly scoped against complete task-107 (declared as dep) and the NoopRekorClient TODO from complete task-144; SCIM users lifecycle (265) precedes groups/mappings (266). The audit's `satisfied` calls I checked (SSO/user management, ABAC engine, impersonation) are backed by real production enforcement in the cited code.
- **Duplicate work:** no other task file references any of the 10 section IDs; grep for topical overlap surfaces only complete tasks (107, 130, 143, 144, 193), all correctly positioned as dependencies or explicitly excluded from replay.

## Mechanical probes

- `PYTHONPATH=scripts python3 -m unittest test_pipeline_spec_backlog` → 40 tests, OK.
- `PYTHONPATH=scripts python3 -m unittest test_pipeline_requirements` → 23 tests, OK.
- New tasks are `not-started`; `task-267` is `ready-for-review` per the assignment (same lifecycle as the prior publication task-255, which later closed as complete with `commits: []`).

No findings. Task creation is not implementation — tasks 261–266 correctly remain `not-started` and no coverage status was altered.
