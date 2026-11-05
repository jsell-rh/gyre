# Review — task-102 (Expand AuditEvent schema to spec-compliant envelope)

Spec: `observability.md` §Audit Event Schema (the 14-field envelope + `AuditOutcome` enum).
Commits under review: `defec35a37` (implementation), `f1340baf` (follow-up migration renumber to 000054, outside `commits:` — acceptable, it is a merge-collision fix that only renamed directories plus one review-doc line, and the attribution check passes).
Verdict: **needs-revision**.

## Round 1

Test runs (shared `target/`):

- `cargo test -p gyre-server --lib api::audit` → **7 passed, 0 failed** (1171 filtered out).
- `cargo test -p gyre-adapters --lib sqlite::audit` → **8 passed, 0 failed** (355 filtered out).
- `cargo test -p gyre-domain --lib audit` → **7 passed, 0 failed**.
- `cargo test -p gyre-server --lib siem::` → **7 passed, 0 failed**.
- `bash scripts/check-migration-versions.sh` → OK (no duplicate versions; the parallel-merge 000053 collision was resolved by `f1340baf` renumbering this task's migration to 000054).
- `bash scripts/check-mem-port-contracts.sh` → OK.
- `bash scripts/check-arch.sh` → OK.
- `bash scripts/check-abac-route-registry.sh` → OK (no routes added or re-registered; commit did not touch `api/mod.rs`/`abac_middleware.rs`).
- `bash scripts/check-task-commit-attribution.sh` → OK.
- Migration fold semantics verified empirically against sqlite3 3.50.2: `json_patch(details, json_object('path', path, 'pid', pid))` with NULL path/pid emits no key (RFC 7396 null-key removal), non-NULL values fold correctly, and a pre-existing key collision is impossible on legacy data (old `details` payload never contained `path`/`pid` keys — they were separate columns). Fold is correct.

Verified working (no findings):

- **Envelope conformance: PASS.** All 14 fields present with spec types in the domain struct (`gyre-domain/src/audit.rs:89-108`: `agent_id: Option<Id>`, `user_id: Option<Id>`, `session_id: Option<String>`, `workspace_id/repo_id: Option<Id>`, `resource_type: String`, `resource_id: Option<String>`, `outcome: AuditOutcome`, `detail: serde_json::Value`, `source_ip/user_agent: Option<String>`, `timestamp: u64`), in `AuditEventRow`/`AuditEventRecord` (both SQLite and Postgres adapters), in migration 000054's 14 columns, and in `AuditEventResponse`. `AuditOutcome` (Success/Failure/Blocked, snake_case serde both ways) matches the spec enum exactly.
- **Agent-identity binding (NEW-31) holds and is regression-tested.** `record_audit_event` derives `agent_id` from `auth.agent_id` (empty-string global-token callers become `None`, satisfying the spec's "Null for server-initiated events") and ignores the body value; `audit_event_agent_id_bound_to_caller` exercises the full JWT path with a forged body `agent_id`.
- **user_id provenance is correct per auth path:** agent tokens and the global token carry `user_id: None` (agent-only/server events — spec: "Null for agent-only events"); API-key and Keycloak-JWT callers carry `user_id: Some(user.id)` from the users store (`auth.rs:577-588, 787-794`). No caller class produces both-empty identification: global-token events correctly keep the `system` agent_id only, and the federation path (`auth.rs:997-999`) has both `agent_id` (remote-subject) and `user_id: None` — coherent with the spec's agent-vs-user distinction.
- **Filter parity across all three adapters: PASS.** `apply_filters` (SQLite, `sqlite/audit.rs:199-228`), the Postgres `query` body (`postgres/audit.rs:120-158`), and the mem adapter (`mem.rs:1175-1206`) each implement all 9 `AuditQueryFilter` dimensions (agent_id, event_type, workspace_id, user_id, resource_type, outcome, since, until, limit) with identical semantics (limit truncation after timestamp-desc sort). SQLite adapter tests cover each dimension individually, including a `Custom` event type.
- **Migration shape:** rebuild-table pattern matches the repo's established approach (000015); down migration reverses column shape, restores legacy `path`/`pid` from detail via `json_extract`, and preserves non-legacy detail content via `json_remove`. Down-migration `COALESCE(agent_id, '')` collapses server-initiated events (agent_id NULL) into empty-string agent_id on rollback — data-shape loss is inherent to reversing a nullable→NOT NULL relaxation and acceptable.
- **Internal emitters populate the envelope meaningfully:** `container_audit.rs` emit() sets `resource_type: "container"` with `resource_id` from detail `container_id` and outcome per event class (started/stopped=Success, crashed/oom=Failure, network_blocked=Blocked — the Blocked variant's only in-tree constructor, so the outcome dimension is real, not decorative); `procfs_monitor.rs` sets `resource_type: "process"`, `resource_id: pid`; `audit_simulator.rs` and `retention.rs` test fixtures updated to the new constructor.
- **SSE/broadcast payloads** are the full envelope (spec's example payload is a subset; superset is conformant). Spec's `ContainerStarted` CamelCase strings vs the implementation's snake_case `as_str()` naming is pre-existing, cross-cutting the whole event-type vocabulary, and owned by task-103/104 per `specs/coverage/system/observability.md` row 5 — out of scope here.
- **SIEM formatters** include the new envelope fields in both syslog JSON (id/event_type/agent_id/user_id/workspace_id/resource_type/resource_id/outcome/detail/source_ip) and CEF extensions (ts/resourceType/outcome/agentId/userId/workspaceId/resourceId/src + detail-derived filePath/pid). `chrono_like_iso8601` returning raw epoch seconds is pre-existing and untouched by this commit (verified via `git show defec35a^`).
- **Web components** read `evt.detail ?? evt.details` with legacy fallback; `agent_id` moved from required to optional in both components' rendering of the actor column (`evt.actor ?? evt.user_id`) — no stale required-field assumptions. Test mocks updated.
- **`docs/api-reference.md`** accurately documents the new POST body, GET filters, and envelope response shape, including the NEW-31 binding and legacy path/pid folding.
- Tests are real, not assertionless: `record_audit_event_returns_201` asserts all 14 response fields present, source_ip first-hop extraction, user_agent, and anti-forgery; the legacy-fold test asserts folded detail keys and absence of top-level path/pid; the outcome-filter test round-trips record→query.

Findings:

- [-] [process-revision-complete] **F1 (major): migration 000054 uses SQLite-only `json_patch`/`json_object`/`json_extract`/`json_remove`, but the migrations directory is embedded and executed by BOTH storage backends — a PostgreSQL deployment now fails at startup.** `crates/gyre-adapters/src/postgres/mod.rs:54` embeds the same `migrations/` dir as `sqlite/mod.rs:58`, and `PgStorage::new_for_tenant` runs `run_pending_migrations(MIGRATIONS)` on every construction (`postgres/mod.rs:84`); `gyre-server/src/lib.rs:795-803` constructs `PgStorage` whenever `GYRE_DATABASE_URL` starts with `postgres://`, and `docs/server-config.md:45` documents PostgreSQL as a supported deployment ("all port traits persist via Diesel ORM with auto-migrations"). PostgreSQL has none of `json_patch`, `json_object(…
- [-] [process-revision-complete] **F2 (moderate): `workspace_id`, `repo_id`, `resource_type`, `resource_id`, `session_id`, and `detail` in the recorded event are caller-supplied and unvalidated — the NEW-31 anti-forgery binding applied to `agent_id` is not applied to the scoping fields, so any authenticated agent can attribute its audit events to another workspace/repo or mislabel the resource.** `record_audit_event` (`api/audit.rs:146-161`) passes `req.workspace_id`/`req.repo_id` straight through (`Id::new` of an arbitrary string — no existence check, no comparison against the caller's own scope) and `resource_type`/`resource_id`/`detail` are free-form. This matters concretely because the new query API filters on exactly these dimensions (`GET /audit/events?workspace_id=...`): an age…
- [-] [process-revision-complete] **F3 (moderate): `source_ip` is taken from client-controlled `X-Forwarded-For`/`X-Real-Ip` headers with no trusted-proxy gate — the spec's compliance-relevant "who made this call" attribute is caller-forgeable, defeating the point of binding it.** `api/audit.rs:112-124` prefers `X-Forwarded-For` (first hop) then `X-Real-Ip`, unconditionally. A direct-connect client (the default deployment; nothing in `docs/server-config.md` configures or requires a reverse proxy, and there is no trusted-proxy list anywhere in the server) can set `X-Forwarded-For: <arbitrary>` and have it persisted as the event's `source_ip`, then forwarded to SIEM as `src=` in CEF and `source_ip` in syslog JSON — i.e. the two attributes the spec adds for forensic attribution (`source_i…
- [-] [process-revision-complete] **F4 (minor): query-time `user_id`/`workspace_id`/`resource_type` filters accept arbitrary strings with no existence or scope validation, and `outcome` filter deserialization returns an unstructured 400 on typo.** `QueryAuditParams.outcome: Option<AuditOutcome>` means `?outcome=succes` produces axum's generic `Query` rejection (400 with a serde message naming the field) rather than the repo's `ApiError` shape used by the other handlers — minor API consistency issue, since `?agent_id=` etc. silently succeed with zero results for nonexistent IDs while `?outcome=` fails loudly. Not a spec violation (spec only lists `?agent_id=&event_type=&since=` as required filters; the implementation superset-filters per the task's own acceptance criteria). Worth a smal…

Non-findings (checked, judged out of scope or acceptable):

- Event-type enum vocabulary (`ContainerCrashed` vs spec's `ContainerKilled`/`ContainerFailed`, `ProcessExec` vs `ProcessSpawned`/`ProcessExited`, `Syscall` vs `SyscallBlocked`, all 25 missing event types) — pre-existing, owned by task-103/104 per the coverage doc's row 5. Not this task's envelope scope.
- `audit_stats` returning all-time counts instead of "last 24 h" — pre-existing gap tracked in coverage row 6, untouched here.
- Mem-adapter filter parity — verified complete, no divergence.
- SSE payload superset of the spec's example — conformant.
- Legacy `path`/`pid` request-body folding — correct and tested; `or_insert` (not overwrite) on the detail keys is the right precedence for old clients that also send detail.
- Down migration data-shape loss (NULL agent_id → `''`) — inherent to reversing a nullability relaxation; acceptable for a rollback path.

Summary: the envelope work itself is thorough and correct — all 14 fields flow end-to-end (domain → port → all three adapters → migration → API → SIEM → web) with genuine filter parity and real tests, and the migration's legacy-data fold is verified correct on real SQLite. But the migration breaks the PostgreSQL deployment path outright (F1), and the two new provenance attributes the spec added for forensic value (`source_ip`, plus workspace/repo attribution) are caller-forgeable in exactly the way the neighboring NEW-31 binding exists to prevent (F2, F3). Revision required.

— Verifier, 2026-09-30

## Round 4 (candidate ca40a808, base 2f215b1e)

Independent review of the F1–F4 revision fixes plus the round-3 repairs. The
envelope implementation itself (commit `defec35a`) is already in main through
the assignment base; this round's delta is `crates/gyre-server/src/api/audit.rs`
(F2/F4), `crates/gyre-server/src/trusted_proxy.rs` (F3, new),
`main.rs`/`lib.rs`/`mem.rs`/`middleware.rs` wiring, migration 000054's portable
rewrite (F1), the exemption shrinks, and the rebuilt `web/dist`.

Verdict: **approved**.

Independent evidence (logs under `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-server --lib api::audit` → 12 passed, 0 failed (includes
  F2 cross-tenant workspace/repo Forbidden + mismatched pair Forbidden +
  unknown-scope NotFound, F3 socket-peer source_ip with forged XFF ignored and
  trusted-proxy XFF honored rightmost-untrusted, F4 ApiError shape).
- `cargo test -p gyre-server --lib trusted_proxy` → 9 passed, 0 failed.
- `cargo test -p gyre-server --lib siem::` → 7 passed, 0 failed.
- `cargo test -p gyre-adapters --lib migration_000054` → 1 passed, 0 failed.
- `cargo test -p gyre-domain --lib audit` → 8 passed, 0 failed.
- Independent of the Rust harness: the real embedded up.sql/down.sql driven
  through the sqlite3 CLI against seeded legacy rows (quote/backslash/newline/
  tab path, pre-existing `path` key, non-object details) — fold output is
  JSON-valid with exact merge-patch semantics; down restores the legacy shape
  keeping `details` verbatim.
- Mechanical checks all OK: check-forwarded-header-trust (0 exemptions),
  check-forged-scope-fields (0 exemptions), check-migration-sql-portability
  (1 pre-existing exemption, 000013), check-migration-versions, check-arch,
  check-mem-port-contracts, check-abac-route-registry, check-inert-enforcement,
  check-task-commit-attribution.
- Round-3 repair R1 verified: `web/dist` has zero conflict markers, both
  bundles parse under `node --check`, the import graph is closed
  (index.html → index-9ZNGTuNm.js + index-GeLbLKhg.css; index → elk.bundled-
  BEFJVH3L.js → back to index-9ZNGTuNm.js), `web/src` is byte-identical to
  base, and a fresh `npm ci && npm run build` in this environment reproduces
  the committed `web/dist` byte-for-byte (`git status --porcelain web/dist`
  empty after rebuild).
- Round-3 repair R2 verified: both exemption files emptied with
  FROZEN_EXEMPTION_COUNT lowered to 0, per the shrink-only protocol.

Notes (non-blocking):

- PostgreSQL runtime probe is not possible in this sandbox (no postgres
  tooling). Static evidence supports F1: the portability scan passes and the
  rewritten SQL uses only `substr`/`length`/`replace`/`||`/`CAST` with
  standard_conforming_strings-compatible literals. Verifier with a PG instance
  should start the server with `GYRE_DATABASE_URL=postgres://...` and confirm
  migration 000054 applies.
- Legacy `path` values containing C0 control characters other than the five
  escaped ones would fold to invalid JSON for that row, degrading to the
  adapters' existing corrupt-JSON fallback (`{}` detail) — documented in the
  migration comment, unreachable from in-repo writers, self-limiting to one
  row's detail payload.
- coverage row 4 (`observability.md §Audit Event Schema`) remains
  `task-assigned` pending the auditor-stage reconciliation, consistent with
  the process precedent that coverage flips come from audit commits, not
  implementation commits.

— Reviewer, round 4
