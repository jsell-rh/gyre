---
title: "Define AgentReview and AgentValidation gate types with review protocol"
spec_ref: "agent-gates.md §Part 1"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Gate Types (Extended)"
  - "agent-gates.md §AgentReview Gate"
commits: ["531434a6875f62e1ee17899032e799a0a4d5c11b", "31c35ddb2cf13663cfc4c1ee337eff847e289253", "b3678e2762149ba743e63c604bb618e66dd564ab"]
---

## Spec Excerpt

From `agent-gates.md` §Gate Types (Extended) and §AgentReview Gate:

**Five gate types total:**

| Gate Type | What It Does | Blocks Until |
|---|---|---|
| `TestCommand` | Run a shell command (e.g., `cargo test`) | Exit code 0 |
| `LintCommand` | Run a linter (e.g., `cargo clippy`) | Exit code 0 |
| `RequiredApprovals` | Count review approvals | N approvals received |
| **`AgentReview`** | Spawn a review agent with MR diff + spec context | Agent submits Approved decision |
| **`AgentValidation`** | Spawn a validation agent for domain-specific checks | Agent reports pass/fail |

**AgentReview Gate flow:**

1. Forge spawns review agent with: MR diff (full patch), referenced spec (at pinned SHA), MR description + acceptance criteria, review persona (e.g., `personas/security.md`), scoped OIDC token with `review:submit` permission only
2. Agent reviews against the spec and persona criteria
3. Agent submits verdict via Review API: `Approved` or `ChangesRequested` (comments appear on MR like any other review)
4. Forge updates gate status — merge proceeds or blocks
5. Agent torn down after verdict (single-minded agents)

## Implementation Plan

1. **Extend gate type enum in domain:**
   - Add `AgentReview` variant to existing gate type enum
   - Add `AgentValidation` variant
   - `AgentReview` carries: persona slug, required (bool)
   - `AgentValidation` carries: validation_type (string), persona slug, required (bool)

2. **Gate configuration model:**
   - Extend gate CRUD to accept AgentReview/AgentValidation configuration
   - AgentReview config: `{ persona: String, required: bool }`
   - AgentValidation config: `{ validation_type: String, persona: String, required: bool }`
   - Store in existing gates table with type discriminator

3. **Review agent spawn protocol:**
   - When merge processor encounters AgentReview gate:
     - Resolve persona by slug
     - Gather MR context: diff, spec_ref (at pinned SHA), description, acceptance criteria
     - Spawn agent via existing spawn infrastructure with:
       - `agent_type = "review"`
       - Scoped OIDC token: `scope: ["review:submit"]` (read-only, no push)
       - Context: MR diff + spec content + review persona system_prompt
     - Record gate_agent_id for tracking

4. **Review verdict submission:**
   - Gate agent uses existing Review API (`POST /api/v1/merge-requests/:id/reviews`)
   - Forge watches for review from gate agent
   - Map review decision to gate status: Approved → Passed, ChangesRequested → Failed

5. **Gate agent teardown:**
   - After verdict submission, auto-complete the review agent
   - Clean up: revoke JWT, remove ABAC policies

6. **API: extend gate endpoints:**
   - `POST /api/v1/repos/:id/gates` accepts AgentReview/AgentValidation types
   - `GET /api/v1/repos/:id/gates` returns all gate types including new ones
   - Verify routes in `gyre-server/src/api/mod.rs`

## Acceptance Criteria

- [ ] AgentReview and AgentValidation gate type variants defined
- [ ] Gate CRUD accepts and stores AgentReview configuration (persona, required)
- [ ] Review agent spawn protocol: persona resolution, MR context gathering, scoped token
- [ ] Review agent receives MR diff, spec at pinned SHA, persona system_prompt
- [ ] Agent's OIDC token scoped to `review:submit` only
- [ ] Review verdict maps to gate status (Approved→Passed, ChangesRequested→Failed)
- [ ] Gate agent auto-teardown after verdict
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-gates.md` Part 1 §Gate Types through §AgentReview Gate. Existing gate implementation: `gyre-server/src/api/gates.rs`, gate types in domain. Merge request reviews: `gyre-server/src/api/merge_requests.rs` (submit_review, list_reviews). Agent spawn: `gyre-server/src/api/spawn.rs`. Gate routes: `GET/POST /api/v1/repos/:id/gates` registered in `gyre-server/src/api/mod.rs` at line ~152. Persona resolution: `gyre-server/src/api/personas.rs` (resolve_persona). JWT minting: `gyre-server/src/auth.rs` (mint_with_workload). Check migration numbering: currently at 000049.

## Shipped

Both `agent-gates.md` coverage sections are implemented with real, production
behavior (product commits `42d79754`…`1b40aacb`, recovered across interrupted
runs and completed across assignments, including the review-F1/F2/F3 repairs,
the review-round TTL repair `72c5bb03`, the review-repair round `31c35ddb`,
the rebase onto `fefde294`, the rebase onto task-177's `7747376`, the merge
onto base `fb1f82e31`, and the `459f51b9` review-repair round, all
described below).

- **Gate types**: `AgentReview` and `AgentValidation` variants in
  `gyre-common`'s `GateType`; `QualityGate` carries `persona` (existing) and
  the `validation_type` column (migration `2026-10-09-000059`, renumbered
  from `000056` after review — see the `459f51b9` repair round), persisted by
  both SQLite and Postgres adapters with an upsert round-trip test. Gate CRUD
  (`api/gates.rs`) accepts/returns both types and `validation_type` (tests:
  `create_agent_review_gate`, `create_agent_validation_gate`).
- **Review spawn protocol** (`gate_executor.rs`): persona resolved
  nearest-wins (repo → workspace → tenant, no fabricated tenant scope);
  unresolvable persona/spec_ref fails the gate before any process spawns.
  MR context gathered for real: full diff via `git_ops.diff`, spec content at
  the pinned SHA via the `read_file_at_commit` port (unknown SHA is an error,
  not a silent None), MR title, and task description (acceptance criteria
  carrier) via MR → author agent → task.
- **Scoped token**: `AgentSigningKey::mint_scoped` mints a JWT with
  `scope: review:submit` only (review agents) or `validation:report`
  (validation agents — validators have no review-submission capability).
  Enforcement is real and layered, all exact-match capability comparison:
  `git_http` denies push (403 read-only), the ABAC middleware allow-lists
  each capability's routes only, and `submit_review` binds the reviewer
  identity to the token subject (forged `reviewer_agent_id` in the body is
  ignored). Covered by `tests/task134_review_probe.rs` — oneshot router
  calls, no listener required.
- **Token TTL tracks the configured gate window** (repair `72c5bb03`): both
  mint sites derive the JWT TTL from
  `gate.timeout_secs.unwrap_or(AGENT_GATE_TIMEOUT_SECS) + 60` via the shared
  `gate_token_ttl_secs` helper, so a verdict submitted late in a
  long-timeout gate no longer 401s on an expired JWT. Regression tests on
  both mint paths assert the minted token's `exp − iat` covers a configured
  `timeout_secs=700` window; both fail against the previous fixed-TTL code
  (verified by mutation).
- **Verdict mapping**: Approved → Passed, ChangesRequested → Failed with the
  agent's feedback surfaced in gate output; only the gate's own agent's
  verdict counts; exit-0-without-review fails ("cannot determine state" is
  not "state is fine"). Unit tests plus two end-to-end tests that bind a real
  axum server, a real bare git repo with a spec pinned at a pre-tip SHA, and
  drive the fixture `review_agent_driver.sh` through the live Review API;
  `e2e_review_submission_is_abac_allowed_with_seeded_policies` guards the
  seeded-policies ABAC regression without needing loopback.
- **Teardown**: token revoked (`kv_remove`) on every exit path (verdict,
  timeout, spawn failure) — asserted by tests; temp spec/diff files removed.
- **AgentValidation**: spawns the configured validation agent with
  `GYRE_VALIDATION_TYPE` delivered and attributed in output; pass/fail from
  the exit code; no-command fails closed.

### Repair rounds (`31c35ddb`, rebases onto `fefde294` and `7747376`)

**Review findings `31c35ddb`** (findings b916825b; no product behavior
changed):

- **Exemption freeze slack** (process): `check-fabricated-scope-defaults.sh`
  still read `FROZEN_EXEMPTION_COUNT=4` although the exemption file held 2
  entries — leaving two slots of silent-growth slack. Verified: baseline
  passes, and a growth probe (an extra entry appended temporarily) FAILS
  the check as the any-growth-fails freeze principle requires.
- **Non-discriminating test** (test):
  `scoped_review_token_denied_outside_allowlist` did not seed builtin
  policies, so its 403 came from the empty policy store's default-deny
  regardless of the allow-list. It now calls `seed_builtin_policies` (as its
  siblings do), so the asserted 403 is attributable to the scoped-token
  allow-list. Verified discriminating by mutation: with `is_reviewer`/
  `is_validator` forced false in `abac_middleware.rs`, the test FAILS
  (200 vs 403); previously it passed under the same mutation.

**Rebase onto `fefde294`** (merge `05c99b4e2` + fix `531434a68`):

- **Freeze-file conflicts resolved by combining both sides' fixes**: the
  branch had fixed `gate_executor.rs`'s gate-event ws_id fallback (freeze at
  2); the new base carries the repair removing the `auth.rs` JWT
  claims.tenant_id fallback (task-103, freeze at 3). The merged tree flags
  exactly one site (`speculative_merge.rs:372`, verified by running the
  checker's scan over `crates/`), so the file resolves to 1 entry with
  `FROZEN_EXEMPTION_COUNT=1` — the tightest freeze, no slack. Verified:
  baseline passes; a temporary second entry fails (rc=1); restored baseline
  passes (probe output retained in review evidence).
- **Semantic merge conflict fixed** (`531434a68`): the base's `jti` field on
  `AgentJwtClaims` broke `mint_scoped`'s initializer, which git's textual
  auto-merge left without the field. Every scoped gate-agent token now
  carries a unique JWT ID, so teardown-time `kv_remove` revocation is
  attributable per token, matching the sibling mint functions.

**Rebase onto `7747376`** (merge `a3b639877`, task-177 base):

- Four conflicts resolved by combining both sides' fixes. In
  `abac_middleware.rs`, the branch's early action resolution (required by
  the task-134 scoped-token allow-list) is kept and the base's hardened
  `is_system_principal()` system bypass adopted (task-099 F1: an
  `agent_id == "system"` string match would let a user *named* "system"
  spoof the ABAC bypass). In `git_http.rs`, the branch's scoped-token
  read-only push rejection is kept and the `resolve_repo_by_slug` call
  adapted to the base's new `&auth` signature. The fabricated-scope freeze
  files resolve to the branch's tightest form: 1 entry,
  `FROZEN_EXEMPTION_COUNT=1` — the merged tree keeps the branch's
  `gate_executor.rs` rewrite (no ws_id fallback), so the base's
  `gate_executor.rs:111` entry is dead. Verified on the merged tree:
  baseline passes and a growth probe (an extra entry appended temporarily)
  FAILS; probe output retained in review evidence
  (`rebase-resolution-7747376.txt`).

### Review-repair round (findings `459f51b9`)

Findings `459f51b9` reviewed candidate `9163afc9b` at checkpoint
`873ec0e88`; both findings share one root cause (the rebase onto
`7747376` collided the branch's migration with the base's).

- **F1 (code) — duplicate Diesel migration version `20261009000056`**:
  the branch's `gate_validation_type` migration sat at the same Diesel
  version as the base's (task-177 `774737638`) `user_tenant_id` migration,
  so Diesel would run only one of them — production SQLite deployments
  would either fail the migration or silently lose the `validation_type`
  column, after which every quality-gate save fails. Fixed by renumbering
  to the next unused sequence: `2026-10-09-000059_gate_validation_type`
  (pure `git mv` directory rename; up.sql/down.sql unchanged, history
  preserved). Verified: `check-migration-versions.sh` passes (rc=1 before),
  `check-migration-sql-portability.sh` passes, and the three tests the
  finding named as failing now pass — `save_upsert_updates_all_config_columns`,
  `no_duplicate_migration_version_prefixes`,
  `migration_000058_backfills_unique_url_safe_usernames` — with the full
  `cargo test -p gyre-adapters --lib` suite green (396 passed, 0 failed).
- **F2 (process) — stale test-evidence claim**: the Shipped section
  asserted the upsert round-trip test passed on the merged tree without
  the adapter suite having been re-run after the collision-introducing
  rebase. The suite is now re-run on the current tree (evidence above),
  and the Shipped migration reference corrected to `000059`.

The merge onto the current base `fb1f82e31` (merge `74418dddc`, clean
auto-merge bringing task-141's `git_http.rs` work) was verified to preserve
the branch's scoped-token read-only push rejection; `git_http` 49/49
(including the base's new tests), `task134_review_probe` 7/7, and
`gate_executor` 37/37 on the merged tree. Probe output retained in review
evidence (`repair-459f51b9.txt`).

### Test evidence

On the current tree (merge `74418dddc` onto base `fb1f82e31`, plus the
`459f51b9` migration renumber): `cargo test -p gyre-adapters --lib` 396/396
(including the upsert round-trip and the migration guard tests),
`task134_review_probe` 7/7, `gate_executor` 37/37 (including both TTL
regression tests and both e2e review tests), `git_http` 49/49,
`check-migration-versions.sh` and `check-migration-sql-portability.sh`
pass, and `check-fabricated-scope-defaults.sh` passes with the count
enforced at 1. `cargo test --all` and the full mechanical-check suite are
owned by verification and publication.
