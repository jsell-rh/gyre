# Review: TASK-077 — HSI Trust Gradient

## R1 Findings

- [-] [process-revision-complete] **F1: Silent policy creation error swallowing in `create_workspace` — `let _ =` on async operation.**
  `crates/gyre-server/src/api/workspaces.rs` (commit `1d599ac9`), in the `create_workspace` handler, lines after workspace creation:
  ```rust
  for policy in initial_policies {
      let _ = state.policies.create(&policy).await;
  }
  ```
  The `let _ =` discards the `Result` from `policies.create()`. If the policy store fails (DB connection error, constraint violation), the workspace is created and returned as `Supervised` — but the `trust:require-human-mr-review` policy does not exist. The merge processor is then unblocked in a workspace the user believes is Supervised. This silently violates the trust invariant. The handler should propagate the error (using `?` or `.map_err(...)`) so the caller gets an error response instead of a silently broken workspace.

- [-] [process-revision-complete] **F2: Workspace creation policy seeding is not atomic with workspace creation.**
  `crates/gyre-server/src/api/workspaces.rs`, `create_workspace` handler: the workspace is created via `state.workspaces.create(&ws).await?`, then trust policies are created in a separate loop of individual `state.policies.create()` calls. If policy creation fails mid-loop, the workspace exists without all its trust policies. The `update_workspace` handler correctly uses `apply_trust_transition` (single DB transaction) for trust transitions. The `create_workspace` handler should use the same atomic approach for initial policy seeding — either calling `apply_trust_transition` or a similar transactional method — to prevent partially-initialized workspaces.

- [-] [process-revision-complete] **F3: Double workspace write — `apply_trust_transition` and `workspaces.update` both write `trust_level`, creating a partial-update failure window.**
  `crates/gyre-server/src/api/workspaces.rs`, `update_workspace` handler: `apply_trust_transition` atomically commits `trust_level` + policy changes (step 1), then `state.workspaces.update(&ws)` writes ALL workspace fields including `trust_level` again (step 2). If step 1 succeeds but step 2 fails (e.g., DB error on the second write), the trust transition is committed but other field changes (name, description, budget) are lost. The handler returns an error, so the user thinks the entire update failed — but the trust level was already changed. On retry, the trust_level change is a no-op (same value), but the user doesn't know the trust transition succeeded on the first attempt. Fix: either (a) move all workspace field writes into the `apply_trust_transition` transaction, or (b) perform `state.workspaces.update()` first (for non-trust fields), then `apply_trust_transition` last — so if the trust transition fails, the 409 is returned cleanly and the user can retry with the non-trust changes already persisted.

- [-] [process-revision-complete] **F4: No test for the 409 error path on failed trust transition.**
  Acceptance criterion: "409 returned on failed trust transition." The code maps `apply_trust_transition` errors to `ApiError::Conflict` (which returns HTTP 409), but no test exercises this path. The `MemPolicyRepository::apply_trust_transition` always returns `Ok(())` — there is no mechanism to make it fail in tests. Without a test, the 409 status code behavior is unverified. At minimum, add a test that verifies the error mapping produces a 409 status code, or document why the in-memory adapter cannot simulate transaction failure.

---

## Round: R2

**Reviewer:** Verifier
**Date:** 2026-09-30
**Commits reviewed:** 2db3f1ef, 85830fa4, 4cd20f3b, 7d0019ae, 545e986f (post-R1 revision commits), verified at HEAD
**Spec ref:** human-system-interface.md §2 (Trust Gradient, incl. Mechanical Implementation), abac-policy-engine.md

### R1 fix verification

All four R1 findings verified as genuinely resolved at HEAD:

- [-] [process-revision-complete] **F1 (resolved R2): error propagation in `create_workspace`.** `crates/gyre-server/src/api/workspaces.rs:136-139` seeds trust policies via `state.workspaces.apply_trust_transition(&ws, false, &seed_policies)` with `?` — no `let _ =` remains in the handler.
- [-] [process-revision-complete] **F2 (resolved R2): atomic workspace + policy creation.** `apply_trust_transition` upserts the workspace row, deletes old `trust:` policies, and inserts new ones inside one `conn.transaction` (`crates/gyre-adapters/src/sqlite/workspace.rs:240-288`; Postgres equivalent at `postgres/workspace.rs:229`). The mem adapter shares the policy store so writes are visible to the ABAC engine, and honors the fail hook (`mem.rs:1620-1648`).
- [-] [process-revision-complete] **F3 (resolved R2): no double write.** `update_workspace`'s trust path calls only `apply_trust_transition` (workspaces.rs:249-255) — the follow-up `workspaces.update` was removed; failures map to `ApiError::Conflict` (409).
- [-] [process-revision-complete] **F4 (resolved R2): 409 path tested with rollback assertions.** `update_workspace_trust_transition_failure_returns_409` (workspaces.rs:822-872) drives a failing repo via `test_state_failing_trust()` and asserts 409, trust_level unchanged (still Guided), and no partially-applied `trust:` policies for the workspace.

Test suites run: `cargo test -p gyre-server --lib api::workspaces` (12 passed), `cargo test -p gyre-server --lib policy_engine` (19 passed), `cargo test -p gyre-domain --lib policy` (0 tests — see F8).

### Findings

- [x] [process-revision-complete] **F5: The Supervised trust policy is never enforced — the merge processor does not evaluate ABAC, so `trust:require-human-mr-review` is inert data — process surface patched; product fix owned by task-077's revision round.** Process surface patched: checklist item 139 (specs/prompts/implementation.md) requires the full enforcement chain (subject constructed, evaluator invoked, result branches the action) and mandates grepping the spec's subject literal into existence; `scripts/check-inert-enforcement.sh` mechanically flags statement-position evaluate_/enforce_/verify_ calls whose results are discarded, and the two discard sites (merge_processor.rs:1331, :1353) are recorded in `scripts/inert-enforcement-exemptions.txt` with ownership pointers so the check runs green on main until task-077's revision round fixes them; verifier.md names the class (inert enforcement). HSI §2 Mechanical Implementation: "The merge processor evaluates ABAC with `action: "merge"` ... The merge processor uses `subject.type: "system"`, `subject.id: "merge-processor"` ... it is subject to the Supervised trust policy." Reality: grep across `crates/` for `set("subject.type", "system")` returns **zero** matches — the merge-processor ABAC identity is constructed nowhere in the codebase. The only merge-time ABAC evaluation is `evaluate_attestation_abac` (`merge_processor.rs:2343`), which (a) is documented "Audit-only — logged but not enforced (merge proceeds regardless)" (`merge_processor.rs:1313-1315`), (b) evaluates `resource_type "attestation"`, not `"mr"` (`:2379`), and (c) sets `subject.type: "agent"` (`:2363`). The processor then force-transitions MRs `Open → Approved → Merged` (`merge_processor.rs:1382-1388`) with no human-approval gate: the `Approved` status is set by the processor itself, and no human-review/approved-by requirement exists anywhere on the merge path. Net effect: in a workspace the user set to Supervised ("I review everything before it merges"), MRs merge autonomously as soon as gates pass — the exact behavior the level exists to prevent. The code even documents the intended mechanism as if it existed: `policy.rs:323-326` comments "The merge processor uses subject.type: 'system', subject.id: 'merge-processor'", and `workspaces.rs:131-133` justifies atomic seeding with "otherwise the merge processor would be unblocked" — but nothing consults the policy at merge time. Fix: before executing a merge, the processor must evaluate ABAC with the specced identity (subject.type "system", subject.id "merge-processor", action "merge", resource_type "mr") and hold/skip the entry on Deny, plus provide the human-approval path that lets a Supervised MR eventually merge. Coverage rows 11–13 (Trust Levels / What Each Level Controls / Mechanical Implementation) are assigned to this task.
- [x] [process-revision-complete] **F6: Fix-class exhaustion — the R1 F1 flaw class (silent policy-creation failure leaves an entity the user believes is restricted) survives in `create_interrogation_policies` — process surface patched; product fix owned by task-077's revision round.** Process surface patched: checklist item 140 (specs/prompts/implementation.md) requires restriction-bearing creation to fail closed (propagate/abort, never warn-and-continue) and classifies every warn-and-continue site by whether the skipped artifact restricts an actor; `scripts/check-warn-continue-creation.sh` mechanically flags `state.policies.(create|save)` followed by warn-and-continue, and the exact site (spawn.rs:219) is recorded in `scripts/warn-continue-creation-exemptions.txt` with an ownership pointer; verifier.md names the class (warn-and-continue on restriction creation). `crates/gyre-server/src/api/spawn.rs:218-227`: each `state.policies.create()` failure is swallowed with `tracing::warn!` and the loop continues; the function returns only the successfully created IDs, and the caller (`spawn.rs:454-455`) proceeds to spawn the interrogation agent regardless. If the `interrogation-restrict-{agent_id}` Deny (priority 200 — the policy that makes interrogation agents read-only: denies write/delete/spawn/approve/merge on task/mr/repo/agent/spec/persona/worktree) fails to create, the agent runs unrestricted with only a log line. The revision's `scripts/check-non-atomic-creation.sh` does not catch this instance (it only flags functions with 2+ *distinct* `state.<repo>.create(` repos; this function only touches `policies`), and `check-silent-result-discard.sh` doesn't match the `match`-with-warn shape. Fix: fail the spawn (or at minimum fail closed) when the restrict-Deny cannot be created; sweep `cleanup_interrogation_policies` (`spawn.rs:259-267`) for the same swallowed-error shape on delete.
- [x] [process-revision-complete] **F7: Untested fix-introduced branches — both Custom trust-transition directions have zero test coverage — process surface patched; product fix owned by task-077's revision round.** Process surface patched: checklist item 148 (specs/prompts/implementation.md) requires every spec-defined variant and transition direction to have a constructing test (grep the variant string in test files; zero hits = uncovered); verifier.md names the class (uncovered spec-defined variants). Not mechanically decidable, so no check script; product fix owned by task-077's revision round. The transition logic's Custom semantics (`delete_trust_policies = !is_now_custom`, workspaces.rs:249-255) implement two spec-defined directions: Preset → Custom preserves existing `trust:` policies as the starting point for user-managed ABAC; Custom → Preset deletes all `trust:` policies then seeds the preset's (HSI §2 "Trust transitions"). No test exercises either direction: the integration test covers Guided → Supervised → Guided only (workspaces.rs:734-813), and no test in the suite ever sets `trust_level: "Custom"` (the string appears only in handler code and comments). If `!is_now_custom` were inverted, or the Custom arm of `trust_policies_for_level` regressed, no test would fail. Both directions need dedicated tests asserting preservation (Preset→Custom) and deletion+reseed (Custom→Preset).
- [x] [process-revision-complete] **F8: Acceptance criterion "Unit tests for trust policy generation and transition logic" is only partially satisfied — no test asserts the generated policy's fields, and `from_db_str`'s changed fallback is untested — process surface patched; product fix owned by task-077's revision round.** Process surface patched: checklist item 148 (specs/prompts/implementation.md) requires generator functions to get field-level output assertions (effect, priority band, actions, resource_types, conditions - the fields downstream enforcement consumes), not existence checks; verifier.md names the class (existence-only generator assertions). Not mechanically decidable, so no check script; product fix owned by task-077's revision round. `trust_policies_for_level` (`crates/gyre-domain/src/policy.rs:311-361`) has no unit tests: `cargo test -p gyre-domain --lib policy` runs 0 tests, and policy.rs has no test module. The only assertions on the generated Supervised policy are name-existence checks (`trust_policy.is_some()`, workspaces.rs:781-787). No test asserts effect=Deny, priority=150 (spec band 100-199), actions=["merge"], resource_types=["mr"], or the `subject.type == "system"` condition — the exact fields merge-time enforcement (F5) depends on. A typo in the condition attribute (e.g. "subject.id") would pass every existing test. Additionally, `TrustLevel::from_db_str` (`crates/gyre-domain/src/workspace.rs:37-44`) — whose semantics revision commit 2db3f1ef changed to fall back to Supervised on unknown values ("safest level on ambiguity") — has no unit test for the fallback or the legacy-Guided arm. Add a domain test module covering `trust_policies_for_level` per level (field-level assertions) and `from_db_str` (four arms + unknown fallback).

### Verdict

needs-revision — F5 means the task's central deliverable (Supervised trust actually requiring human merge approval, per HSI §2 Mechanical Implementation) is not enforced end-to-end: the Deny policy is created but never consulted. F6 is the same silent-restriction-loss class R1 F1 established, surviving in an adjacent handler. F7/F8 are coverage gaps on fix-introduced branches and an explicit acceptance criterion.

---

## Round: R3

**Candidate:** `3b98f0f74f1c67023261c3817991896cf1a2bf96` (base `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f`)
**Reviewer:** Independent verifier (fresh model, per assignment retry)
**Commits in range:** `ed36c1b` (checkpoint recovery), `fdd3721` (merge base), `1b07c83b` (revision round F5–F8), `3adc47f`, `3b98f0f` (attribution)

### Scope note

The candidate range contains a checkpoint commit (`ed36c1b`) carrying the bulk
of the implementation (2160 insertions, 46 files) plus the revision round
(`1b07c83b`). Earlier task-077 commits (`545e986f` … `2db3f1ef`) are ancestors
of the assigned base and were verified as landed by R2; this round verifies the
full base→candidate diff, re-confirming the R2 fixes hold and auditing the R2
F5–F8 revision work. Evidence saved under `/tmp/stage/review-evidence/`.

### Acceptance criteria — independent confirmation

| Criterion | Evidence |
|---|---|
| `TrustLevel` enum | Four variants; `trust_policies_for_level` matches on all |
| Workspace `trust_level`, default Supervised | Migration `000050_workspace_trust_default_supervised` sets `DEFAULT 'Supervised'` (table rebuild, correct SQLite pattern) |
| `immutable` on policies | Migration `000028_policy_immutable` (`ALTER TABLE policies ADD COLUMN immutable INTEGER NOT NULL DEFAULT 0`); `Policy.immutable` field |
| Trust preset sets | Supervised → one `trust:require-human-mr-review` Deny (priority 150, `[merge]`/`[mr]`, `subject.type == "system"`, Workspace-scoped); Guided/Autonomous/Custom → empty |
| Single-transaction transitions | `apply_trust_transition` (sqlite/workspace.rs:229-292): one `conn.transaction` upserting workspace row + delete/insert of `trust:` policies; handler never double-writes (`workspaces.update` only on the no-trust-change path) |
| ABAC engine: immutable Deny first | `policy_engine::evaluate` Step 1 returns immutable Deny before any priority sort; `immutable_deny_blocks_even_when_high_priority_allow_matches` passed |
| `builtin:require-human-spec-approval` seeded | Spliced from `gyre_domain::builtin_policies` into `seed_builtin_policies`; `main.rs:50` calls with `?` (fail-closed startup). Tests: priority 999, immutable, idempotent |
| CRUD rejects `trust:`/`builtin:` (400) | create (policies.rs:84) + update-rename (:167); prefix tests passed |
| `PUT /workspaces/:id` accepts `trust_level` | Strict `TrustLevel::parse` → 400 on typo (create AND update); state-untouched tests passed |
| 409 on failed transition | `update_workspace_trust_transition_failure_returns_409` passed; message verbatim HSI §2 |
| ABAC cache invalidation | No ABAC policy-result cache exists in gyre-server (policies loaded per request, abac_middleware.rs:833-840); transitions write the same store in one transaction — no intermediate-state window, requirement satisfied |
| Unit tests, generation + transition | Field-level generator assertions (F8); Custom-direction transitions (F7) |
| Integration: transition → policies | `trust_transition_to_supervised_creates_trust_policy`, `trust_transition_custom_to_preset_deletes_and_reseeds`, `trust_transition_preset_to_custom_preserves_trust_policies` |
| fmt + mechanical gates | `cargo fmt --all --check` clean; 20+ `scripts/check-*.sh` green, incl. both gates whose task-077 exemption entries were deleted (inert-enforcement, warn-continue-creation) and task-commit-attribution (repaired by recording `f4acb4eb` in task-189 frontmatter — legitimate: that merge commit IS task-189's landed work) |

### R2 findings F5–F8 — verified resolved

- **F5 (merge-time enforcement):** `evaluate_merge_abac` builds the processor's
  service identity (`subject.type "system"`, `subject.id "merge-processor"`,
  tenant resolved repo→workspace), filters cross-workspace policies by
  `scope_id`, excludes the HTTP-pipeline catch-all `builtin-default-deny` by id
  (in scope it would make Guided/Autonomous unrepresentable — documented
  in-code and regression-tested), and is invoked before BOTH merge paths
  (`merge_branches` calls exist only at merge_processor.rs:706 group and :1593
  single; both gated). Hold is requeue-not-fail (single) / rollback-group
  (atomic), keeping the human-approval path live. The escape
  (`mr.status == Approved`) is closed against agent self-approval:
  `transition_mr_status` rejects Agent-role callers with 403; agent JWTs and
  legacy agent tokens both authenticate with `roles: [Agent]` (auth.rs:549,653).
- **Mutation probe (test quality):** disabling the single-entry gate condition
  (`if false && mr.status != …`) makes both hold tests FAIL (`left: Merged,
  right: Open`, exit 101) — the tests kill the real bug, not self-confirming.
  Source restored after the probe; tree verified clean at the candidate.
- **F6 (fail-closed interrogation policies):** creation propagates errors;
  spawn aborts and rolls back agent record + token; `seed_builtin_policies`
  fails closed (startup propagates via `?`). Both former exemption entries
  deleted — checks green with zero task-077-owned entries.
  `create_interrogation_policies_fails_closed_on_duplicate` and
  `builtin_policy_seeding_fails_closed_on_store_error` passed.
- **F7 (Custom directions):** preset→Custom preserves `trust:` policies;
  Custom→preset deletes all `trust:` (including operator-created), preserves
  non-trust user policies, Guided reseeds nothing.
- **F8 (field-level assertions):** every field the gate consumes is asserted;
  `from_db_str` round-trip + fallback and strict `parse` rejection tested.

### Focused test results (this review, at candidate)

- `cargo test -p gyre-server --lib merge_processor` — 55 passed, 0 failed
- `cargo test -p gyre-server --lib api::workspaces` — 16 passed, 0 failed
- `cargo test -p gyre-server --lib -- policy_engine abac_middleware` — 30 passed
- `cargo test -p gyre-server --lib -- api::policies api::merge_requests` — 24+34 passed (incl. prefix 400s, agent-cannot-approve)
- `cargo test -p gyre-server --lib -- spawn::tests::create_interrogation` — 1 passed
- `cargo test -p gyre-domain --lib` — 371 passed, 0 failed

### Diff hygiene (base→candidate, 47 files)

Substantive changes are confined to the task: trust/ABAC (domain policy +
workspace, policy engine, abac_middleware seed, workspaces/spawn/policies/
merge_requests handlers, merge_processor gate, mem test hooks), migrations,
docs, mechanical check hardening, task specs. Everything else (gyre-cli,
retention, sqlite audit/notification tests, mr_timeline, git2_ops,
integration test files, …) is pure `cargo fmt` reformatting — verified via
`-w` diff: whitespace/line-join only, no semantic deltas.

The `check-non-atomic-creation.sh` rewrite (awk→python3) is a legitimate
portability repair: the base script aborts on mawk hosts (reproduced: `awk:
line 23: syntax error`, rc=2 — gawk-only 3-arg `match()`). The new
`non-atomic-creation-exemptions.txt` baselines 6 sites, all verified
pre-existing at base (reproduced the check's multi-repo-create scan on the
base tree — identical site list). No new violations: the candidate's own
trust-policy seeding goes through `apply_trust_transition` (transactional),
and spawn.rs dropped from 4 to 3 create-repo kinds (F6 split). Line
re-anchoring in other frozen-count exemption files matches actual line
shifts; no new entries.

### Notes (non-blocking)

- §12 rows beyond MR merge/spec approval (notification gradients, briefing
  detail, inbox priority ranges) are not implemented by this task's
  policy-set mapping — consistent with the task contract, whose Mechanical
  Implementation section defines level→policy mapping as the mechanism; the
  downstream surface behavior is owned by other sections/tasks and the
  coverage matrix keeps §12 assigned until the auditor re-verifies. No
  misrepresentation in the Shipped summary.
- Merge-time attestation-ABAC remains audit-only (warn on Deny) — pre-existing
  task-061 behavior baselined in authorization-provenance coverage row 35,
  not part of task-077's trust gate. The candidate's change there only binds
  the result to a warn (which is what let its inert-enforcement exemption
  entries be deleted legitimately).
- 409 body field name is `detail` (ApiError convention), not the spec's
  literal `error`; message string verbatim. Cosmetic API-shape divergence
  consistent with the repo-wide error envelope.

### Verdict

**approved** — the task contract (HSI §9–13 trust gradient: levels, storage,
atomic transitions, immutable-Deny-first evaluation, builtin seeding, prefix
guards, merge-time Supervised enforcement with the human-approval escape
closed to agents, fail-closed restriction creation) is implemented with real
production code and tests that demonstrably fail when the behavior is
disabled. All mechanical gates green at the candidate; no new exemptions; no
spec gaps introduced.
