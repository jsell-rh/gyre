# Review — task-139 (Forge enforcement policies for spec-to-code binding at merge)

Spec: `specs/system/agent-gates.md` §Forge Enforcement Policies (lines 418–428) and §Integration with Agent Gates (lines 442–451).
Commits under review (scoping per `commits:` frontmatter): `99edd0bca` (feat), `93f0e718a` (progress chore). Base `22f275e26` (task-138 lineage).
Verdict: **complete** — approved with no findings.

## Probes (evidence: /tmp/stage/review-evidence/)

Focused tests, this checkout at `93f0e718a`, debug build via /tmp/stage/bin/cargo:

- `cargo test -p gyre-server --lib signed_spec_ref` → **1 passed** (`signed_spec_ref_requires_signature_on_active_approval`: approved+signed verifies; approved-unsigned and whitespace-only signature fail naming the missing signature; no-approval fails; one signed active row among several satisfies).
- `cargo test -p gyre-server --lib require_signed_approval_blocks` → **1 passed** (unsigned approval → entry Failed with reason naming "no Sigstore signature"; author notification asserted via `NotificationRepository::list_for_user`; control MR with signed approval reaches Merged).
- `cargo test -p gyre-server --lib warn_stale_spec_adds` → **1 passed** (real git repo, HEAD moved past referenced blob; merge proceeds Merged; advisory MR comment from `forge:spec-policy` present and names both SHAs).
- `cargo test -p gyre-server --lib get_mr_spec_serves` → **1 passed** (real git repo, spec v1 committed then v2 at HEAD; endpoint returns v1 content at the pinned SHA; no-spec_ref and unreachable-SHA MRs 404).
- `cargo test -p gyre-server --lib set_require_signed_approval_round_trips` → **1 passed** (PUT then GET round-trips the field over the real route).
- `cargo test -p gyre-adapters --lib set_and_get_round_trips_all_five_policies` → **1 passed** on a fresh SQLite DB — proves migration `2026-11-09-000059_require_signed_approval` runs and all five policy fields persist, including the update path.
- Regression control (task-138 lineage): `require_current_spec_blocks_stale_spec_ref` → **1 passed**.
- Frontend: `cd web && npx vitest run src/__tests__/RepoSettings.test.js` → **54 passed** (incl. the new bind-to-real-API-field-names regression test and full-policy save assertion).

Mutation probes (temporary edits, restored; tree verified clean via `git status --porcelain` after each):

- Disable the signature check in `verify_signed_spec_ref` (early `Ok(())`) → both `signed_spec_ref_requires_signature_on_active_approval` and `require_signed_approval_blocks_unsigned_spec_approval` **FAIL**. The tests are anchored to the enforcement, not to structure.
- Serve HEAD instead of the pinned SHA in `GET /merge-requests/:id/spec` (resolve via `resolve_spec_blob_sha`, the same helper the merge processor uses) → `get_mr_spec_serves_pinned_sha_not_head` **FAILS**. (A first attempt at this mutation used `resolve_blob_sha` with the ledger path and was a no-op — that was a probe defect, not a test weakness; the ledger path `system/identity.md` does not resolve without the `specs/` prefix, so the mutation fell through to the pinned SHA.)

Mechanical invariants (all exit 0, outputs saved): check-arch, check-migration-versions, check-migration-sql-portability, check-abac-route-registry (new route `/api/v1/merge-requests/:id/spec` is registered with a `merge_request` resource mapping), check-byte-slice-truncation, check-inert-enforcement, check-mem-port-contracts, check-dead-message-kinds, check-relative-path-defaults, check-fail-open-ref-resolution, check-in-memory-state-stores, check-task-commit-attribution.

## Verified against the acceptance criteria

- **5 policies configurable per repo** — `SpecPolicy.require_signed_approval` added (domain struct with `#[serde(default)]`, opt-in false); SQLite + Postgres adapters round-trip the column (portable `ALTER TABLE ... INTEGER NOT NULL DEFAULT 0`); mem store stores the struct whole; `GET/PUT /api/v1/repos/:id/spec-policy` round-trips it (PUT Admin-only preserved); the other four fields and the `spec_ref` MR field (path@sha) shipped with the task-138 base and are regression-tested here. UI: RepoSettings now binds the real API field names (the old `require_approval`/`stale_spec_warning` bindings were dead — this fix is real and covered by the new frontend test) and adds `require_current_spec` + `require_signed_approval` toggles.
- **Merge-time enforcement** — merge processor blocks on `require_spec_ref`/`require_approved_spec`/`require_signed_approval` before executing the merge (an MR with no spec_ref is correctly rejected under `require_signed_approval` too: no spec_ref → no referenced approval), `require_current_spec` rejects when HEAD ≠ referenced SHA, `warn_stale_spec` warns without blocking. `verify_signed_spec_ref` performs the same exact-(path, sha) ledger query as `verify_spec_ref` with the additional non-empty-signature condition on the active approval.
- **Enforcement errors reported to the author** — every blocking rejection fails the queue entry with the specific reason, emits a `GateFailure` workspace event, and delivers an inbox notification to the author's spawning user (`notify_spec_policy_violation`, same delivery rule as `notify_gate_failure`); the notification test asserts delivery through the real repository. `warn_stale_spec` records a deduplicated advisory MR comment (author `forge:spec-policy`) visible via the comments endpoint.
- **Spec-alignment gate receives spec at pinned SHA** — new `GET /api/v1/merge-requests/:id/spec` serves the blob content at the MR's `spec_ref` SHA via `git cat-file blob` (SHA validated 40-hex before reaching the command line — no argument injection; 404 on missing spec_ref or unreachable blob). `gate_executor` passes `GYRE_SPEC_URL` to AgentReview and AgentValidation gate agent processes alongside the pre-existing `GYRE_DIFF_URL`/`GYRE_PERSONA`, documented in `docs/agent-protocol.md`. The route is registered in `api/mod.rs` with an ABAC `RouteResourceMapping`.
- **Migration numbering** — `000059` is the next unused sequence after `000058_spec_approval_ledger`.

## Notes (not findings)

- `mr_id[..8.min(mr_id.len())]` in the new `notify_spec_policy_violation` (notifications.rs:163) copies the pre-existing pattern at lines 107/217/271 of the same file; `mr_id` is a server-generated ASCII Id, the index expression is not constant so `check-byte-slice-truncation.sh` does not flag it, and no new hazard class is introduced. Opportunistic cleanup candidate only.
- Coverage sections 22/24 remain `task-assigned` in `specs/coverage/system/agent-gates.md`, consistent with the task-138 precedent (sections 19–21 also remain `task-assigned` post-approval); coverage-matrix flipping is a separate stage, not this candidate's scope.
- Cargo builds regenerate `web/dist` via build.rs; regenerated artifacts were removed to keep the tree clean during review.

Findings:

- (none)
