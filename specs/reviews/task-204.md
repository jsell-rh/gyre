# Review — task-204 (spec-lifecycle accountability patrol)

Spec: `specs/system/spec-lifecycle.md` §Accountability Integration.
Candidate: `e811b2a217e429e054ca910cff93fd09d2ddcb41` (base `3af80bd21a8a3452420e014e645eefdfd137f9be`).
Commits under review: the 15 SHAs recorded in the task frontmatter (attribution
check passes against the candidate's task file).
Verdict: **approved** — no blocking findings. The gyre-server test suite could
not be executed in this sandbox (build wrapper rejects gyre-server codegen
beyond the 4 GiB review budget); the required probes are recorded below and
must be run by the independent verifier.

## Probes run (evidence in /tmp/stage/review-evidence)

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0
  (7m28s; type-checks the new module, handler, and all new test code).
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo clippy -p gyre-server` → exit 0,
  433 pre-existing warnings, **zero** referencing `spec_lifecycle_patrol.rs`
  or the new `api/specs.rs` handler/tests.
- `bash scripts/check-arch.sh` → PASS (hexagonal boundaries intact; new module
  is server-side, depends only on ports/domain/common).
- `bash scripts/check-abac-route-registry.sh` → PASS —
  `/api/v1/patrol/spec-lifecycle` is registered in the resolver
  (`abac_middleware.rs`, resource `spec`, POST→`write`), NOT added to the
  frozen exemption file (unlike the legacy `spec-links` patrol, which is
  exempt). Gating is therefore stronger than the sibling endpoint, which
  satisfies "same auth/role gating" in the enforcing direction.
- `bash scripts/check-abac-exempt-handlers.sh` → PASS (75 handlers).
- `bash scripts/check-dead-message-kinds.sh` → PASS (no new MessageKind; reuses
  existing `MessageKind::Escalation`, which already has emitters in
  `stale_agents.rs`/`merge_processor.rs`).
- `bash scripts/check-mem-port-contracts.sh`,
  `check-inert-enforcement.sh`, `check-scope-literal-defaults.sh`,
  `check-fabricated-scope-defaults.sh`, `check-lossy-secret-conversion.sh`,
  `check-forwarded-header-trust.sh`, `check-relative-path-defaults.sh`,
  `check-fail-open-ref-resolution.sh`, `check-unbounded-external-http.sh`,
  `check-in-memory-state-stores.sh`, `check-byte-slice-truncation.sh` → all PASS.
- `bash scripts/check-task-commit-attribution.sh` → PASS when run against the
  candidate's `specs/tasks/task-204.md` (the staged working tree resets that
  file to `commits: []`, which makes the same script fail — staging artifact,
  not a candidate defect; all 11 task-labeled product commits in history are
  present in the candidate frontmatter, verified SHA-by-SHA and by re-running
  the script with the candidate file restored, then the staged state restored
  byte-identically).

## Deferred to the verifier (blocked here by the build wrapper, not by code)

The wrapper rejects gyre-server codegen (`cargo test -p gyre-server`):
"gyre-server code generation plus the model exceeds 4 GiB." The following must
be executed by the independent verifier on the integrated tree:

1. `cargo test -p gyre-server --lib spec_lifecycle` — 13 tests: 11 in
   `spec_lifecycle_patrol::tests` + 2 endpoint tests
   (`spec_lifecycle_patrol_endpoint_flags_and_escalates`,
   `spec_lifecycle_patrol_endpoint_honours_thresholds`).
2. `cargo test --all` (task acceptance criterion).
3. `bash scripts/check-arch.sh` (already green here; re-run for the record).

## Verified working

- **All three checks run against real storage.** `run_spec_lifecycle_patrol`
  queries `TaskRepository::list()` and `SpecLedgerRepository::list_all()`
  (the ledger the post-receive hook's `sync_spec_ledger` writes), and check 3
  resolves task coverage via `TaskRepository::list_by_spec_path` per spec —
  no in-memory stand-ins beyond the standard mem adapters behind the ports.
  Storage failures propagate as `Err` → HTTP 500, never an empty finding list.
- **Check 1** flags non-terminal (`Done`/`Cancelled` excluded) tasks labeled
  `spec-drift-review` (exact label the hook writes, `git_http.rs:1752`) past
  `drift_review_max_age_secs` (default 24 h, documented as the loop-cycle
  proxy). **Check 2** flags `spec-implementation` + `status == Backlog` past
  `implementation_backlog_max_age_secs` (default 7 d). Thresholds are request
  parameters (`SpecLifecyclePatrolRequest`), never hardcoded in the check
  bodies — task-109 config can feed them without a rewrite.
- **Check 3** (defense in depth) handles the real path-spelling split: the
  ledger stores manifest-relative paths (`system/x.md`) while the hook records
  git-rooted task `spec_path`s (`specs/system/x.md`); the patrol normalizes
  and queries both spellings, requires repo match when both sides know the
  repo (a same-named spec in another repo cannot mask the gap), treats
  `Cancelled` tasks as non-covering, skips `Deprecated`/no-`current_sha`
  entries, and applies a grace window keyed off ledger `updated_at` so
  same-tick pushes are not flagged. Watched paths come from the repo's
  `SpecLifecycleConfig` via `state.spec_lifecycle_configs.get_for_repo` — the
  same config `process_spec_lifecycle` consults — and `enabled = false` repos
  are skipped, mirroring the hook's early return. The patrol and hook can
  never disagree about which specs should have tasks.
- **Escalation is real enforcement.** Every finding goes through
  `state.emit_event(Some(ws), Destination::Workspace(ws), MessageKind::Escalation,
  Some(payload))` — the exact pattern prescribed from `api/specs.rs:684-706`.
  `emit_event` signs the message, persists it via `MessageRepository::store`,
  broadcasts to WS clients, and dispatches to consumers. The payload
  identifies the finding (`source`, `finding_type`, `severity`, `spec_path`,
  `task_id`, `message`, `suggested_action`) and satisfies the message-bus
  escalation schema (`reason` required, extension fields pass through —
  validated in-test via `MessageKind::Escalation::validate_payload`). Missing
  workspace scope routes to `Destination::Broadcast` (and nothing is persisted
  under a fabricated `"default"` scope), matching the platform's F1
  anti-forgery invariant.
- **Tests are real guards, not self-confirming.**
  `flags_and_escalates_only_the_accountability_gaps` seeds all three gap
  shapes plus fresh/covered/terminal controls, asserts the *exact* flagged set
  (sorted `type:id` equality), then asserts exactly 4 persisted `escalation`
  messages on the seeded workspace stream with identifying payload content —
  it fails if any check is a no-op, thresholds are inverted, escalation is
  skipped, or emit_event's tenant resolution is bypassed.
  `flags_watched_specs_without_tasks` kills the repo-masking bug and both
  path-spelling coverages; `check3_watched_paths_follow_per_repo_config`
  kills a hardcoded watched-path list and verifies disabled-repo skip;
  `thresholds_are_not_hardcoded_in_the_checks` kills hardcoded/inverted
  thresholds; `workspaceless_finding_broadcasts` asserts delivery on the real
  broadcast channel plus no persistence under fabricated scopes; the endpoint
  tests drive the real router (`api_router().oneshot`) and assert both
  threshold directions and persisted escalation. I traced every assertion
  through the actual mem adapters and `emit_event` → `message_tenant_id` →
  `messages.store` path; semantics line up.
- **Route/ABAC/docs/coverage bookkeeping is complete**: route registered in
  `api/mod.rs` next to `spec-links`, resolver entry added (not an exemption),
  `docs/api-reference.md` row added with accurate defaults, coverage row 8 →
  `implemented` with evidence (count 8/13 → 9/13), task frontmatter
  `ready-for-review` with all product commits recorded.

## Non-blocking observations (no action required for this task)

1. The patrol response (like the entire specs API surface — `list_specs`
   likewise lists all tasks/ledger entries unscoped) is not tenant-filtered:
   a Developer+ caller sees findings from all tenants. Consistent with the
   platform's current design and with the task plan's prescribed signature;
   escalation itself is correctly per-workspace scoped. If the specs surface
   ever gets tenant scoping, this endpoint should follow.
2. Check 3's grace window reuses `drift_review_max_age_secs`; a caller passing
   a very small drift threshold also shrinks the hook grace window. Documented
   in the module; acceptable until task-109 feeds a dedicated config value.
3. `escalate_findings`'s return count increments even when `emit_event` skips
   an unresolvable-workspace message (skip-and-log is the platform's F1
   invariant, not a forgery). The count is used only for logging; persisted
   emission is what the tests assert.

Note on the working tree: the staged checkout resets
`specs/tasks/task-204.md` to its pre-assignment content (`progress:
not-started`, `commits: []`) as an uncommitted modification; the candidate
commit itself carries `ready-for-review` and the full commits list. No
production files are dirty.
