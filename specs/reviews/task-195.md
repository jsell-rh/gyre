# Review — task-195 (Manifest-driven gate selection for MRs referencing a spec)

Spec: `specs/system/spec-registry.md` §16 Agent Gates Integration (+ §Agent
Approver / Gate Schema for `persona` / `min_attestation_level` / `stack_hash`).
Candidate: `836c5d7d` (base `603cc2f7`). Evidence: `/tmp/stage/review-evidence/`.

Verdict: **approved** — real implementation, no fakery found; server test
execution deferred to the independent verifier per the review-stage build
budget (recorded below and in the evidence summary).

## What was verified (independently, this round)

- **Selection is real and pure.** `manifest_gates_for_mr(Option<&SpecManifest>, Option<&str>)`
  (gate_executor.rs:101) resolves the manifest entry whose normalized path
  matches the MR's `spec_ref` path (strips `./`, `/`, `specs/` prefixes;
  accepts bare no-`@sha` paths, which spawn.rs records for un-ledgered
  specs), and returns each `gates:` entry as
  `ManifestGateSpec {persona, min_attestation_level (default 1), stack_hash, spec_path}`.
  Empty for no manifest / no spec_ref / no matching entry / no gates;
  repeated personas within an entry dedupe (first wins). Unit tests assert
  the resolved set directly, including defaults and constraint carry-through.
- **Sourcing reuses task-193's reader.** `resolve_manifest_gates_for_mr`
  loads the MR and repo and calls `crate::spec_registry::read_manifest(&repo.path, "HEAD")`
  — no second manifest reader. HEAD (not the spec_ref blob SHA) matches the
  documented justification: the ledger SHA is a blob SHA, not a treeish, and
  HEAD is the same policy source task-193's approval resolver uses. Reading
  the manifest at repo HEAD also means an MR author cannot inject or remove
  gates via the source branch — only reviewed default-branch policy selects
  gates.
- **Union without double-spawn.** `trigger_gates_for_mr` keeps running all
  repo-level gates except those whose persona is covered by a manifest gate
  (persona-name comparison handles both `personas/security.md` paths and
  bare names). On persona overlap the manifest gate wins and the repo-level
  duplicate is skipped — documented and test-asserted
  (`trigger_gates_manifest_gate_wins_over_repo_gate_for_same_persona`
  requires exactly one security result, and it is the manifest gate, whose
  attestation constraints the repo gate cannot enforce).
- **No regression paths.** No `spec_ref` → exactly the repo gates (test);
  repo without `specs/manifest.yaml` at HEAD → exactly the repo gates (test).
- **Real persona-driven gate, never the auto-pass stub.** Manifest gates are
  materialized as `AgentReview` `QualityGate`s (deterministic id
  `manifest-gate:<repo>:<spec_path>:<persona>`, not persisted to the repo
  gate store) with `GateResult` rows in the same table the merge processor
  reads. `run_agent_review_gate` routes constraint-carrying gates to
  `run_persona_review_gate`, which passes ONLY on an `Approved` review from
  an agent running the named persona (KV `agent_personas`) whose workload
  attestation satisfies `min_attestation_level` (level derived by
  `constraint_check::derive_attestation_level` from the real
  `workload_attestations` record — never invented) and, when pinned, an
  exact `stack_hash` match. No qualifying review → Failed at timeout
  (fail-closed; `persona_gate_fails_without_qualifying_review` explicitly
  kills the auto-pass-stub regression). Latest-verdict-per-reviewer with
  approval-wins semantics, tested in both directions. Unknown gate ids
  default to `required = true` in `check_gates_for_mr`, so failures block
  the merge (`trigger_gates_failed_manifest_gate_blocks_merge`).
- **Forge really spawns the gate agents (spec §16).**
  `spawn_manifest_gate_agent` creates a real `Agent` record (idempotent per
  MR+persona while running — re-enqueue reuse is test-asserted), records the
  persona in `agent_personas`, mints a `review:submit`-scoped EdDSA JWT
  (fail-closed on mint/registration failure), dispatches a `ReviewRequest`
  carrying persona + constraints, and launches the runtime through the
  platform's compute-target machinery (`api::spawn::launch_agent_workload` →
  `dispatch_agent_launch`, the same four-backend dispatch
  `launch_agent_process` uses), delivering the token via `GYRE_AUTH_TOKEN`
  and recording a workload attestation.
  `trigger_gates_launches_gate_agent_runtime_with_token` asserts the launched
  process actually received the token and review context env, and that the
  attestation derives at level ≥ 1. Teardown kills the runtime via the
  registered owning backend (not LocalTarget — a dedicated test proves
  backend routing), revokes the token with audit, and transitions to
  Stopped keeping the record for attribution.
- **Reviewer identity is not forgeable.** `submit_review` now binds the
  reviewer to the verified token subject for EVERY agent-token principal
  (not just `review:submit`-scoped ones), and human principals may use a
  body `reviewer_agent_id` only when it equals their own verified `user_id`
  (Admin may record on behalf; comparison against `user_id`, never the
  user-controlled display name). Without this, any spawned agent or
  non-admin human could forge a review attributed to a persona-holding,
  attested agent and defeat the manifest gate constraints. Three router-
  driving tests cover the forger, the non-admin human, and the admin paths;
  `task134_review_probe` lookalike-scope discriminators were updated
  accordingly. The web review flows (`Inbox.svelte`, `DetailPanel.svelte`)
  now resolve the caller's own id via `selfReviewerId()` instead of the
  legacy `'human'`/`'human-reviewer'` pseudo-ids (vitest Inbox 74/74 pass).
- **Spawn-side persona trust is closed.** The new `persona` field on
  `SpawnAgentRequest`/MCP `gyre_spawn_worker` is validated fail-closed
  (must resolve to an Approved persona in the repo→workspace→tenant chain)
  BEFORE any state is created, so a caller cannot mint a gate-satisfiable
  persona identity by labeling a worker `security`. The forge's own
  manifest-gate writer is server-internal and sources the persona from the
  reviewed manifest.
- **Reviewer allow-list addition is safe.** `REVIEWER_ALLOWED_ROUTES` gained
  `/api/v1/agents/:id/messages` so a gate agent can poll its ReviewRequest
  over REST; the handler enforces own-inbox for non-admin callers
  (messages.rs:356), and the route was already in the ABAC
  `RouteResourceMapping` registry (constant refactor only, no exemption
  growth).
- **Mechanical gates pass**: `check-arch.sh`, plus fabricated-scope-defaults,
  scope-literal-defaults, inert-enforcement, dead-message-kinds,
  in-memory-state-stores, unbounded-external-http, lossy-secret-conversion,
  forwarded-header-trust, byte-slice-truncation, relative-path-defaults,
  fail-open-ref-resolution, migration-versions, forged-scope-fields — all
  exit 0. `fabricated-scope-defaults-exemptions.txt` shrinks 2→1 (the
  gate_executor entry was fixed by this branch); no exemption entries added.
- **Rebase integrity**: `git diff 8b3f3002 836c5d7d -- gate_executor.rs
  api/spawn.rs api/merge_requests.rs` is empty — the final rebase onto
  `603cc2f7` changed none of the key implementation files relative to the
  previously verified tree; only docs/web-helper unions and main's `time`
  crate feature moved. Commit attribution passes at the committed HEAD
  (`check-task-commit-attribution.sh` in a worktree at `836c5d7d`); the
  live-checkout failure is the stage harness's baseline reset of the task
  file, not a candidate defect.

## Probes run here

`cargo check -p gyre-server --tests` (exit 0); `cargo test -p gyre-domain
-p gyre-ports` (503+8 pass); `cargo test -p gyre-adapters --lib` (499 pass);
`cd web && npm ci && npx vitest run src/__tests__/Inbox.test.js` (74/74);
`check-arch.sh` + 12 mechanical checkers (all exit 0). The build wrapper
blocks gyre-server test compilation in the review sandbox (4 GiB budget).

## Deferred to the independent verifier (must run)

`cargo build --all`; `cargo test -p gyre-server --lib gate_executor`;
`cargo test -p gyre-server --lib submit_review`;
`cargo test -p gyre-server --test task134_review_probe`;
`cargo test -p gyre-server --lib spawn`; `cargo test --all` + CI script set.
Full list with test names in `/tmp/stage/review-evidence/summary.md`.

## Non-blocking observations (recorded, no action required)

- `resolve_manifest_gates_for_mr` returns empty (repo gates only) on an
  MR/repo `find_by_id` storage error without a warn log. This mirrors the
  pre-existing error posture of the function it extends
  (`quality_gates.list_by_repo_id(...).unwrap_or_default()` in the base
  also silently degrades to zero gates on storage error), so it is not a
  regression; a warn on the error arms would still be a cheap improvement.
- `trigger_gates_launches_gate_agent_runtime_with_token` uses
  `std::env::set_var("GYRE_AGENT_COMMAND", ...)` — safe under the wrapper's
  `RUST_TEST_THREADS=1`, but a parallel default `cargo test` run could race
  the env var with sibling gate-spawn tests (they would merely launch the
  dump script; no assertion depends on its absence). Test-hygiene only.
- Generated-output note (verifier-owned): the working tree's `web/dist/`
  churn vs HEAD is untracked/uncommitted asset drift; per stage policy the
  verifier rebuilds and commits dist after integration. Not reviewed as a
  source defect.
