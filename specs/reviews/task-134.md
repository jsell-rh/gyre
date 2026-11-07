# Review — task-134 (agent-gates.md §Gate Types (Extended) + §AgentReview Gate)

Spec: `specs/system/agent-gates.md` Part 1 §Gate Types (Extended) and §AgentReview Gate (five gate types; AgentReview spawn protocol with MR diff, spec at pinned SHA, description + acceptance criteria, persona, scoped `review:submit` token; verdict via Review API; teardown).
Candidate under review: `9248f6e0` on `pipeline/task-134/414bbda22dcc421aa3f2c794d371109a-21` (base `7f8b5d9c`; task-134 source recovered via checkpoint `b3678e27` and merged with base in `d679e3e8`; the merge brought only upstream task-156 content — task-134 surfaces are byte-identical to the checkpoint).
Verdict: **approved** (round 3; rounds 1–2 produced the F1/F2/F3 and TTL repairs cited below).

## Evidence (all on the exact candidate tree; commands and output in `/tmp/stage/review-evidence/task-134-round3-probes.md`)

- Focused suites, all green: `gate_executor` 37/37 (includes both real-server e2e review tests on a live 127.0.0.1 listener driving `tests/fixtures/review_agent_driver.sh` through the Review API, both TTL regression tests, teardown tests), `task134_review_probe` 7/7, `api::gates` 6/6, `api::merge_requests` 30/30, `git_http` 38/38, `git2_ops::read_file_at_commit` 3/3, SQLite gate upsert 1/1.
- Mechanical checks all OK (arch, ABAC route registry, migration versions incl. SQL portability, inert enforcement, fail-open ref resolution, mem port contracts, fabricated/literal scope defaults, forged scope fields, in-memory stores, byte-slice truncation, task commit attribution).
- Mutation check: disabling the reviewer identity binding in `submit_review` (always honor body `reviewer_agent_id`) makes `scoped_review_token_reviewer_identity_is_bound` FAIL — the binding is real and the test is discriminating. Source restored; suites re-ran green on the restored tree.

## Verified against the acceptance criteria

- **Gate type variants**: `GateType::AgentReview`/`AgentValidation` (pre-existing in `gyre-common`, dispatched in `run_gate`); `QualityGate.validation_type` added with migration `2026-10-09-000056_gate_validation_type` (distinct Diesel version — date+sequence prefix; check-migration-versions passes), persisted by SQLite and Postgres with a full-column upsert regression test.
- **Gate CRUD**: `api/gates.rs` accepts/returns both types and `validation_type`; round-trip through list asserted; `api/mod.rs` routes already registered.
- **Spawn protocol**: persona resolved nearest-wins (repo → workspace → tenant, no fabricated tenant scope; unresolvable persona fails the gate before spawn); MR context gathered for real — full diff via `git_ops.diff`, spec at the pinned SHA via the new `read_file_at_commit` port (unknown SHA is an error, missing file at a resolving SHA is an explicit failure), MR title, task description via MR → author agent → task. The old no-command auto-approve fallback is gone — no-command now fails closed.
- **Scoped token**: `AgentSigningKey::mint_scoped` (`review:submit` for reviewers, `validation:report` for validators), TTL derived from the configured gate window (`timeout_secs + 60`) on both mint sites with real-JWT regression tests. Enforcement layered and exact-match: `git_receive_pack` 403 before repo resolution; ABAC middleware allow-lists only each capability's routes (validators are read-only MR context); `submit_review` binds reviewer identity to the token subject. All proven by the probe suite, with the lookalike-scope test as the empirical control that agent-role tokens can otherwise POST reviews under seeded policies.
- **Verdict mapping**: Approved → Passed, ChangesRequested → Failed with the review body surfaced in gate output; only the gate's own agent's verdict counts; exit-0-without-review fails. Unit + both e2e directions.
- **Teardown**: token revoked (`kv_remove`) on every exit path — verdict, timeout, spawn failure — asserted by tests and by the e2e assertion that no `gate-*` tokens remain; temp spec/diff files removed.

## Non-blocking observations

- `scoped_review_token_denied_outside_allowlist` runs without seeded policies, so its 403 alone is not discriminating against empty-store default-deny; the allow-list is nonetheless discriminatingly covered by `validation_scoped_token_cannot_submit_review` (seeded; the lookalike control proves the same POST is 201 for agent-role tokens) and the reviewer-allow side by `scoped_review_token_reviewer_identity_is_bound` (201 inside the allow-list, seeded).
- Implementation-plan deviation, documented and spec-compliant: gate agents are configured subprocess commands (extending the pre-existing gate command infrastructure with an env-var protocol documented in `docs/agent-protocol.md`) rather than `api/spawn.rs` agent records; the canonical spec mandates the context bundle, scoped token, verdict mapping and teardown — all real here.
- `revoke_gate_token` ignores `kv_remove` errors without logging; a failed revoke then relies on token TTL. Harmless in direction (read/submit-only token) but worth a `warn!` someday.

`cargo test --all` and the full mechanical suite are owned by verification/publication; every focused suite touching the diff was run green here.
