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

## Round 2 — revision review (candidate 0b045ff4, base 1ae18d1c)

Candidate delta vs base is exactly the F1–F4 revision round (the 14-field envelope itself was already merged into the base via `defec35a` and re-verified unchanged at HEAD): portable rewrite of migration 000054 + a real fold regression test, `validate_workspace_scope`/`validate_repo_scope` in `api/audit.rs`, new `trusted_proxy.rs` + `ConnectInfo` wiring in `main.rs`, manual `outcome` filter parsing, exemption-file shrinkage with frozen counts lowered (2→0, 2→0, 5→1 — the sanctioned shrink direction, never growth), and a web/dist restore-to-base after the checkpoint merge committed conflict markers.

Independent probes on the exact candidate (logs: `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-server --lib api::audit` → **12 passed, 0 failed** — includes mutually differential assertions: own-tenant workspace 201 vs cross-tenant 403 vs unknown 404 (F2 fails if removed); socket-peer source_ip 203.0.113.9 with forged XFF ignored vs XFF 198.51.100.7 honored only from a trusted-proxy peer (one of the two necessarily fails if the F3 gate is removed or widened); forged body `agent_id` ignored on the full JWT path (NEW-31); failed-closed `source_ip: null` without socket info; `?outcome=succes` → 400 `{"error": ...}` ApiError shape (F4); legacy path/pid fold.
- `cargo test -p gyre-server --lib trusted_proxy` → **9 passed, 0 failed** (CIDR masking, malformed-entry fail-closed, rightmost-untrusted XFF, all-trusted-chain fallback, untrusted-peer-cannot-buy-trust).
- `cargo test -p gyre-domain --lib audit` → **8 passed, 0 failed**; `cargo test -p gyre-server --lib siem::` → **7 passed, 0 failed** (workspaceId/resourceType/outcome in CEF + syslog JSON).
- `cargo test -p gyre-adapters --lib migration_000054` → **1 passed, 0 failed** — drives the REAL embedded up.sql through the real diesel runner (un-marks 000054, re-applies onto a recreated legacy-shape table seeded with JSON-hostile paths `/tmp/we"ird\pa\nth\t`, non-object details, null path/pid variants) and asserts the folded JSON parses back to merge-patch semantics. Real migration execution, not mirrored logic.
- `cargo test -p gyre-adapters --lib sqlite::audit` → **11 passed, 0 failed** (all 9 filter dimensions).
- Mechanical: `check-migration-sql-portability`, `check-migration-versions` (000054 fits 000053<000054<000055), `check-forwarded-header-trust`, `check-forged-scope-fields`, `check-arch`, `check-mem-port-contracts`, `check-task-commit-attribution`, `check-abac-route-registry` — all OK.
- web/dist repair: `git diff 1ae18d1c..0b045ff4 -- web/dist` empty; `node --check` on both bundles OK; zero conflict markers. Web sources unchanged vs base, so the restored base build matches current sources.

Verified fixes:

- **F1 fixed.** up.sql/down.sql rewritten with portable constructs only (CASE WHEN + `substr`/`length`/`||`/`replace`/`CAST`/`COALESCE`); NULL propagation through `||` is guarded by `CASE WHEN ... IS NULL` branches on every nullable source; backslash literals rely on `standard_conforming_strings` (PG default since 9.1, documented in-file). The prior json_patch/json_extract forms are gone (portability grep green). Down migration is shape-restoring only (path/pid come back NULL, folded keys remain readable inside `detail`) — an acceptable, documented trade for portability. *Deferred check for verification:* no PostgreSQL server exists in the review sandbox; run the server with `GYRE_DATABASE_URL=postgres://...` to confirm the migration executes (the original F1 failure mode was a PG startup abort). Manual construct review + SQLite execution + dialect grep all support portability.
- **F2 fixed.** `record_audit_event` looks up every caller-supplied `workspace_id` (tenant compare → 403/404) and `repo_id` (repo's workspace tenant compare + workspace/repo consistency → 403/404) before persisting; `agent_id`/`user_id` are bound server-side from auth. `resource_type`/`resource_id`/`detail`/`session_id` remain free-form — correct per spec (they are labels/payloads, not scoping identities), and round 1 already judged the query-time counterpart acceptable.
- **F3 fixed.** New `trusted_proxy.rs`: socket peer (`ConnectInfo`, wired via `into_make_service_with_connect_info` in `main.rs`) is the source of truth; XFF/X-Real-Ip honored only when the peer matches `GYRE_TRUSTED_PROXIES` (empty default = direct), parsed rightmost-untrusted, malformed proxy entries skipped (never widened), fails closed to `None` without socket info. Documented in `docs/server-config.md` + `docs/api-reference.md`.
- **F4 fixed.** `outcome` parsed manually → repo ApiError shape; message names the field and bad value.

Non-findings:

- Exemption-file edits only shrink lists and lower frozen counts (2→0, 5→1) with the fix protocol documented in-file — the sanctioned direction; the one remaining portability entry (000013 down.sql `INSERT OR IGNORE`) is pre-existing debt, unchanged by this candidate.
- 403-vs-404 on scope probes follows the repo's documented per-handler authorization convention (AGENTS.md), consistent with every other scoped handler.
- Task frontmatter `commits:` (4fe453df, 99de714b, 9e70fa7f — pre-rebase checkpoints) vs the shipped section naming 7f1e7362 (web/dist repair): `check-task-commit-attribution.sh` passes; web/dist is generated artifacts restored byte-identical to base, and this review scoped the full base→candidate diff regardless.

Verdict: **approved**. All four revision findings are fixed in production code with differential tests that fail if the behavior is removed; the envelope contract (14 fields, `AuditOutcome`, nullable `agent_id`, migration, 9-dimension query filters, SIEM fields) holds end-to-end at HEAD.

— Reviewer, round 2
