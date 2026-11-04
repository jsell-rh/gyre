# Review — task-134 (AgentReview and AgentValidation gate types with review protocol)

Spec: `specs/system/agent-gates.md` §Part 1 §Gate Types (Extended) + §AgentReview Gate.
Candidate under review: `d2fa4e4ca6132bafa98ec748be04893f6182b1cd` (base `e608ac7856209426721d09c5ac35e43e01c2d9b5`), branch `pipeline/task-134/517d29ab7fcb467db2168fddf25ea0ba-1`. All 15 frontmatter commits verified present.
Verdict: **needs revision** — one CI-breaking mechanical-gate regression; the implementation substance itself is real and verified.

Evidence: `/tmp/stage/review-evidence/task-134-review.md` (plus per-run logs in the same directory).

## Round 1

### Verified working (independent probes, this sandbox — loopback HTTP permitted)

- **Fail-closed replaces fakery.** Both auto-approve stubs are gone: an AgentReview or AgentValidation gate with no `command` fails ("no agent command configured") instead of fabricating an approval — the old `run_agent_review_gate_stub` that self-submitted an Approved review is deleted. `gate_executor` 37/37 passed, including both real-server e2e tests (real axum server, real bare git repo with the spec pinned at a pre-tip SHA, real `review_agent_driver.sh` process through the live Review API, http_code=201, context-delivery assertions for persona prompt / MR title / task description / pinned spec content / full diff).
- **Pinned-SHA spec reading is real.** New `read_file_at_commit` port (git2 adapter): a test proves the pinned v1 content is returned after the branch tip advances to v2, and an unresolvable SHA is an `Err`, not `Ok(None)` — fail-closed ref resolution. Adapter tests 3/3.
- **Persona resolution real and fail-closed.** Nearest-wins repo → workspace → tenant; unresolvable persona fails the gate before any token is minted (asserted: no `gate-*` entries in `agent_tokens`).
- **Scoped token enforcement is layered and exact-match.** `mint_scoped` issues `review:submit` (reviewers) / `validation:report` (validators). Three enforcement layers, each probed: git push 403 (read-only), ABAC route allow-list 403 outside each capability's routes (validators read-only MR context; lookalike scopes `review:submit:admin`/`xreview:submit` do NOT inherit — exact-match proven), and `submit_review` binds the reviewer identity to the token subject (forged `reviewer_agent_id` ignored). `task134_review_probe` 7/7.
- **TTL repair regression tests kill the pre-fix bug.** Mutation probe: reverting `gate_token_ttl_secs` to the fixed 360s TTL makes both `*_token_ttl_covers_configured_gate_timeout` tests FAIL (verified, output captured); mutation fully restored afterwards.
- **Teardown.** Token revoked (`kv_remove`) on verdict/timeout/spawn-failure paths (asserted); revoked-token 401 probe; temp spec/diff files removed.
- **Verdict mapping.** Approved → Passed, ChangesRequested → Failed with the agent's feedback surfaced; only the gate's own agent's verdict counts (another agent's approval does not satisfy the gate); exit-0-without-review fails.
- **Persistence.** `validation_type` column (migration 000056, next unused sequence — `check-migration-versions.sh` OK; SQL portable to both backends — `check-migration-sql-portability.sh` OK), SQLite upsert round-trip test covers `validation_type` AND `required` drift; gate CRUD accepts/returns it (6/6 `api::gates` tests, round-trip through list).

Mechanical gates on the candidate: arch, abac-route-registry, abac-exempt-handlers (89 handlers), migration-versions, migration-sql-portability, mem-port-contracts, fail-open-ref-resolution, inert-enforcement, forged-scope-fields, in-memory-state-stores, scope-literal-defaults, unbounded-external-http, task-commit-attribution — all OK.

### Finding F1 (blocking): mechanical gate `check-fabricated-scope-defaults.sh` fails on the candidate

- Candidate: `bash scripts/check-fabricated-scope-defaults.sh` → **EXIT=1**, `FAIL: ... crates/gyre-server/src/gate_executor.rs:122: let ws_id = workspace_id.unwrap_or_else(|| Id::new("default"))`.
- Base `e608ac78` (temporary worktree): same script → **EXIT=0**.
- Cause: the candidate's `gate_executor.rs` edits (the `gate_token_ttl_secs` helper etc.) shifted the pre-existing exempted site `gate_executor.rs:111` ("ws_id fallback for gate event emission") to line 122. The diff updated the exemption file's line-drift entries for `auth.rs` (773→837) and `git_http.rs` (1495→1524) but missed the `gate_executor.rs` entry, so the frozen path:line exemption no longer matches and the legacy site reads as a new violation. The script runs in pre-commit and CI, so this candidate cannot pass verification/publication as-is.
- Fix (one line): update the `gate_executor.rs` entry's line number to 122 — line-number drift of a frozen entry, not a new entry (same maintenance the diff already performed for auth.rs/git_http.rs). Preferably instead: fix the legacy site itself (skip-and-log per `emit_reconciliation_completed`) since this task heavily touched `gate_executor.rs` and the exemption header says "fix when touching".

### Notes (non-blocking)

- `GateType::AgentReview`/`AgentValidation` variants and the gates CRUD route already existed at base; this task's delta is the enforcement hardening above plus `validation_type` — consistent with the recovered-across-runs commit history.
- The `e2e_review_submission_is_abac_allowed_with_seeded_policies` oneshot test correctly guards review-F1 (loss of `seed_builtin_policies` in the e2e setup) for no-loopback sandboxes; here both e2e tests ran for real.
- MR with no `spec_ref` at all proceeds with empty spec content (no error) — reasonable reading of "referenced spec": an MR that references nothing has nothing to pin; a *present but unresolvable* `spec_ref` fails the gate, which is the fail-open-ref-resolution invariant that matters.
- Reviewer identity binding engages only for exact `review:submit` scope; other JWT callers keep the pre-existing body-field behavior (task's anti-forgery requirement is about the scoped gate agent, which is fully bound).

**Verdict: not approved — F1 must be fixed (trivial one-line exemption line-drift update, or better, fix the legacy ws_id fallback site).**
