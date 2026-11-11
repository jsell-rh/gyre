# Review — task-139 (agent-gates §Forge Enforcement Policies + §Integration with Agent Gates)

Task: forge enforcement policies for spec-to-code binding at merge — five
per-repo policies, `require_signed_approval` (the fifth), author-facing
enforcement reporting, and spec-alignment gate delivery of the spec at the
pinned SHA.

Commits under review (candidate `fcdc3d8f`, base `cba416b4`):
`99edd0bca` (feat: forge spec enforcement policies and pinned-sha spec
delivery), `059586a37`/`33cc49415` (rustfmt repairs), `ec05bf9af` (review
repair: GYRE_SPEC_URL fetchable + blob-pinned spec_refs resolvable).
The first four policies (require_spec_ref, require_approved_spec,
warn_stale_spec, require_current_spec), the MR `spec_ref` column, and the
spec-policy API were already merged at base from an earlier round of this
task; this candidate closes the remaining delta.

## Round 2 (this review — repair verification)

Prior-round findings and their resolution:

1. **GYRE_SPEC_URL unfetchable by gate agents** (route missing from
   REVIEWER_ALLOWED_ROUTES / VALIDATOR_ALLOWED_ROUTES, and no test through the
   real auth stack) — **fixed**: `MR_SPEC_ROUTE` added to both allow-lists and
   to the ABAC resolver (abac_middleware.rs), plus
   `scoped_gate_tokens_can_fetch_pinned_mr_spec` (tests/task134_review_probe.rs)
   which drives `gyre_server::build_router` — the real require_auth + ABAC
   layer stack (lib.rs:793-803), not the bare api_router — asserting 200 +
   pinned content for both `review:submit` and `validation:report` scoped
   tokens and 403 on `/api/v1/repos`.
2. **Blob-pinned spec_refs unresolvable by the gate's spec channel**
   (read_file_at_commit did a commit-only lookup that errors on blob OIDs) —
   **fixed and independently verified live**: git2_ops.rs now resolves blob
   pins to the blob's own content (neither commit nor blob ⇒ error,
   fail-closed). Probe: `cargo test -p gyre-adapters --lib blob_pinned` →
   PASS (`test_read_file_at_commit_resolves_blob_pinned_sha`: ledger path +
   v1 blob sha, branch advanced to v2, content resolves to "# Spec v1").
   The candidate also added the parallel `GET /api/v1/merge-requests/:id/spec`
   channel (git cat-file, validated 40-hex SHA, 404 on unreachable blob) and
   `GYRE_SPEC_URL` for both review and validation gate processes.
3. **Byte-slice truncation pattern in the new notification helper** —
   **fixed**: `notify_spec_policy_violation` uses `mr_id.chars().take(8)`
   (char-boundary), unlike the four pre-existing exempted sites.

Independent probes run this round (evidence: /tmp/stage/review-evidence):

- `cargo test -p gyre-adapters --lib spec_policy` → PASS
  (`set_and_get_round_trips_all_five_policies`: fresh SQLite DB runs migration
  `2026-11-09-000059_require_signed_approval`, all five fields round-trip,
  default-all-false, upsert update path). Migration version uniqueness
  confirmed — `20261109000059` vs sibling `20261108000059` differ by date
  prefix; check-migration-versions.sh PASS.
- `cargo test -p gyre-adapters --lib blob_pinned` → PASS (see finding 2).
- `cargo check -p gyre-server` → PASS (4m24s, no warnings on changed code).
- `cd web && npm ci && npx vitest run src/__tests__/RepoSettings.test.js` →
  58/58 PASS, including the new regression test proving the policy toggles
  bind the REAL API field names (the pre-existing `require_approval` /
  `stale_spec_warning` bindings were dead — fields the API never returns) and
  that saving round-trips the full five-field policy object.
- 18 static guard scripts PASS on the candidate tree (arch, ABAC route
  registry, migration versions + SQL portability, byte-slice truncation,
  inert enforcement, fail-open ref resolution, forged scope fields, in-memory
  state stores, lossy secret conversion, relative path defaults, scope literal
  defaults, fabricated scope defaults, dead message kinds, mem port contracts,
  unbounded external HTTP, conditional test guards, forwarded header trust,
  task commit attribution — the last at the candidate commit; the working-tree
  failure is the stage controller's task-file reset, not the candidate).
  Four further scripts fail with awk dialect errors identically at base —
  sandbox environment, not code.

Code review of the enforcement delta:

- **`verify_signed_spec_ref`** (api/gates.rs) queries the real ledger
  (`find_by_spec_sha(path, sha)` — exact-version query, same as the step-8
  forge check) and requires an ACTIVE approval (approved, not revoked/rejected)
  carrying a non-empty, non-whitespace signature; one signed active approval
  among several satisfies. Fail-closed on store error (`unwrap_or_default()` →
  empty ⇒ "no active approval" ⇒ block).
- **Merge processor**: `require_signed_approval` implies spec-ref presence
  (no spec_ref ⇒ no referenced approval ⇒ blocked, conservative and
  documented); blocking policies fail the queue entry with a specific reason,
  emit a GateFailure workspace event, and notify the author's spawning user
  (`notify_spec_policy_violation` — tenant resolved from the workspace record,
  skipped + logged when unresolvable, no fabricated "default" scope).
  `warn_stale_spec` records a deduped advisory MR comment
  ("forge:spec-policy" author) and still merges; `require_current_spec`
  blocks with the drift named. Staleness compares against
  `resolve_spec_blob_sha` (raw path then `specs/`-prefixed — handles ledger
  paths) via real `git rev-parse`.
- **`GET /merge-requests/:id/spec`** is resolver-mapped (middleware ABAC), so
  no per-handler authorization is required by the exempt-handler rule; it
  takes no caller-supplied scope fields.
- Test quality: each new test is discriminating — the unsigned-approval test
  asserts Failed + author notification + a signed control that MERGES; the
  warn-stale test uses a real on-disk git repo (v1 blob pinned, HEAD at v2)
  and asserts both Merged and the advisory; the /spec test asserts v1 content
  and NOT v2; the scoped-token test asserts both 200 on the spec route and
  403 elsewhere.

Deferred to the independent verifier (build wrapper blocks gyre-server test
codegen within the 4 GiB budget — exact commands in
/tmp/stage/review-evidence/verifier-checks.txt): the seven gyre-server test
targets listed there (merge blocking for signed/stale/current, ledger
semantics, /spec endpoint, gate blob-pinned context, scoped-token fetch).
Test code verified by inspection: all symbols and arities exist
(`list_for_user` 7-arg, `ReviewComment::new` 5-arg, `mint_scoped` 6-arg,
`build_router` applies the auth+ABAC stack).

Minor observations (non-blocking, recorded for follow-up):

- `GET /merge-requests/:id/spec` resolves only blob-pinned spec_refs; a
  commit-shaped pin (which `read_file_at_commit` supports and MR creation
  permits) 404s on the URL channel while the env channel
  (GYRE_SPEC_CONTENT) still delivers it. Production spawn always pins blob
  SHAs; documented shape is blob. Edge-case inconsistency only.
- `state.spec_policies.get_for_repo(...).unwrap_or_default()` in the merge
  processor treats a store read error as "all policies disabled"
  (fail-open). Pre-existing at base for the first four policies; the fifth
  inherits it. Follow-up hardening candidate, not introduced by this diff.
- Human-authored MRs (`author_agent_id = None`) get no inbox notification
  for spec-policy violations — the notification path targets the author
  agent's spawning user, mirroring `notify_gate_failure`. The queue-entry
  failure reason and GateFailure workspace event still report the violation.

Verdict: **complete** — the delta is real enforcement (real ledger queries,
real git lookups, fail-closed verification), the prior round's findings are
fixed with discriminating tests, and every probe executable in this sandbox
passed. Subject to the recorded verifier run of the seven deferred server
test targets.
