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

## Round 2 — revision review (candidate 319cc46c, base 5196f444)

Scope: the F1–F4 revision fixes (`9e70fa7f`, `99de714b`). The envelope itself (14 fields, all three adapters, SIEM, web) was verified in round 1 and is unchanged by this diff apart from the fixes below.

Independent probes (logs and edge-case SQL under `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-adapters --lib migration_000054` → **1 passed** (cold compile 5m26s). The test drives the real embedded up.sql against seeded legacy rows (JSON-hostile path with quote/backslash/newline/tab, non-object details, pre-existing keys) and asserts exact merged payloads.
- `cargo test -p gyre-server --lib api::audit` → **12 passed, 0 failed** (includes F2 cross-tenant workspace/repo + mismatch + unknown-id, F3 both directions, F4 ApiError shape).
- `cargo test -p gyre-server --lib trusted_proxy` → **9 passed, 0 failed**; `siem::` → **7 passed**; `cargo test -p gyre-domain --lib audit` → **8 passed**.
- `bash scripts/check-migration-sql-portability.sh` → OK; exemptions file genuinely shrunk 5→1 (single remaining entry is the pre-existing 000013 `INSERT OR IGNORE`).
- `check-forwarded-header-trust`, `check-forged-scope-fields`, `check-migration-versions`, `check-arch`, `check-inert-enforcement`, `check-scope-literal-defaults`, `check-mem-port-contracts`, `check-abac-route-registry`, `check-task-commit-attribution` → all OK.
- Adversarial sqlite3 3.45.1 probe of the fold SQL beyond the shipped test (multibyte path `/tmp/日本語/ファ"イル`, closing brace inside a string value, empty object, trailing whitespace, pre-existing key): all rows fold to valid JSON with correct escaping; the brace-in-string and multibyte cases are handled correctly. The trailing-whitespace and duplicate-key corners cannot occur in real legacy data (in-repo writers emitted canonical `serde_json::to_string` output; legacy `details` never contained `path`/`pid` keys — established in round 1).

Findings from round 1 — all fixed and regression-tested:

- **F1 fixed.** Migration 000054's up/down rewritten from SQLite-only JSON1 functions to portable SQL (`CASE WHEN` + `substr`/`length`/`replace`/`CAST`/`||`). Static portability audit: every construct is valid on both SQLite and PostgreSQL; the backslash/quote escape literals rely on `standard_conforming_strings` (PG default since 9.1), where they denote the same two-character strings as in SQLite; raw newline/CR/tab inside single-quoted literals are legal in both engines. The regression test would not catch a JSON1 regression on SQLite by itself, but the portability script (frozen count 1, exemptions shrunk) now gates that class mechanically. **Verifier item:** no PostgreSQL server exists in the review sandbox — execution of migration 000054 on a real PostgreSQL instance (e.g. `GYRE_DATABASE_URL=postgres://...` boot + legacy-row fold check) should be run in verification where a PG server is available. This is an infrastructure restriction, not a code defect.
- **F2 fixed.** `validate_workspace_scope`/`validate_repo_scope` look up every caller-supplied `workspace_id`/`repo_id`, compare tenant against the caller, and reject cross-tenant (403), unknown (404), and repo/workspace-mismatch (403) attributions before the write. `user_id`/`agent_id` remain server-bound. The tests assert specific failure statuses, so they fail if the validation is removed.
- **F3 fixed.** New `gyre-server/src/trusted_proxy.rs`: source of truth is the socket peer (`ConnectInfo`, attached via `into_make_service_with_connect_info` in main.rs); `X-Forwarded-For`/`X-Real-Ip` honored only when the peer is in `GYRE_TRUSTED_PROXIES` (empty by default), parsed rightmost-untrusted; resolution fails closed to `None` without socket info; malformed proxy-list entries are skipped, never widened. Grep confirms no unguarded forwarded-header read remains anywhere in the server. The handler tests assert concrete values (`203.0.113.9` from the peer, `null` for forged headers without a socket), so they kill the old header-trusting behavior.
- **F4 fixed.** `?outcome=` parses as a plain string; a typo returns the repo's `ApiError` 400 JSON naming the valid values.

Non-findings (checked, acceptable):

- Down migration restores `path`/`pid` as NULL (shape-restoring, detail payload kept verbatim in `details`) — documented in the file; the same one-way character round 1 already accepted for `agent_id`.
- The up.sql comment "a pre-existing key is never overwritten" is imprecise for the theoretical non-null-`path`-plus-existing-`path`-key corner (it emits a duplicate JSON key; serde_json last-wins gives the folded value, matching RFC 7396 merge-patch and the old `json_patch` behavior anyway) — no behavioral defect, and the corner is unreachable in legacy data.
- GET query filters still accept arbitrary `workspace_id` strings without read-scope validation — pre-existing, explicitly judged acceptable in round 1, unchanged by this diff (the ABAC layer gates the route).
- `specs/coverage/system/observability.md` row 4 remains `task-assigned task-102` — correct process state pending merge (matches the pattern of approved-but-unmerged tasks elsewhere in the coverage docs).
- `web/dist` bundle churn with no `web/src` change — rebuild artifact of the pipeline checkpoint; the committed bundle builds cleanly (`build.rs` ran `npm ci` + `npm run build` successfully during the test compile) and the working tree stayed clean afterward.

Verdict: **approved**. All four round-1 findings are fixed with genuine, bug-killing regression coverage; the migration fold is independently verified correct on real SQLite with adversarial edge cases, and the portability claim is supported by static analysis plus the now-passing mechanical gate (live PostgreSQL execution noted as a verifier item above).

— Reviewer, round 2
