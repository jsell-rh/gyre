---
title: "Implement admin user impersonation"
spec_ref: "identity-security.md §Impersonation"
depends_on:
  - task-102
progress: complete
coverage_sections:
  - "identity-security.md §Impersonation"
commits: ["e17e493afd734295d9de982617271e0c3891a5cd", "f7e7601f25efbf59d25ea64ede574065e37af73f", "64536dbccf6bb5b1d7fcfef754ab7342ad256cba", "14af7abd7e27ffcc7aa28e8703cdda7e97abe218", "a1194a81a0b22e891d7b215b6b019b4861d27c6c", "2d511c2169c0c038533d9d2ba240318389555c8c", "cad88393d30797174d3ae3bd3ab90f0d8353f2c6"]
---

## Spec Excerpt

From `identity-security.md` §Impersonation:

> Support **user impersonation** for enterprise support scenarios.
> Requirements:
> - Target user must **provide a code or explicit approval** before impersonation begins.
> - Impersonated user is **notified** when someone impersonates them.
> - **Full audit trail** of all actions taken during impersonation — clearly attributed to the impersonator acting as the target.
> - Authorization pattern inspired by AP2: intent → signed mandate → receipt with full audit trail.

From `observability.md` §Audit Event Types:

| Event Type | Trigger | `detail` fields |
|---|---|---|
| `ImpersonationStarted` | Admin begins impersonating user | `impersonator_id`, `target_user_id`, `approval_token` |
| `ImpersonationEnded` | Impersonation session ends | `impersonator_id`, `target_user_id`, `duration_secs`, `actions_taken` |

## Implementation Plan

1. **Impersonation request/approval flow:**
   - `POST /api/v1/admin/impersonate` — Admin requests to impersonate a user
     - Request: `{ target_user_id: String }`
     - Response: `{ request_id: String, approval_code: String }` — a short-lived code sent to the target user
   - `POST /api/v1/admin/impersonate/:request_id/approve` — Target user approves with the code
     - Request: `{ approval_code: String }`
     - Response: `{ impersonation_token: String, expires_at: u64 }` — short-lived JWT with impersonation claims

2. **Impersonation token:**
   - JWT with additional claims: `impersonator_id`, `impersonating: true`, `target_user_id`
   - Short TTL (15 minutes, configurable via `GYRE_IMPERSONATION_TTL`)
   - All API calls with this token are attributed to the impersonator in audit trail
   - The `sub` claim is the target user (so permissions match), but `impersonator_id` is always present

3. **Notification to target user:**
   - When impersonation starts, create a notification for the target user:
     - Type: `ImpersonationAlert` (add to `NotificationType` enum if needed)
     - Message: "Admin {impersonator_name} is impersonating your account"
   - Send via WebSocket if the target user is online
   - Also send email notification if email is configured

4. **Audit trail:**
   - Emit `ImpersonationStarted` audit event when impersonation begins (with impersonator_id, target_user_id, approval_token)
   - Emit `ImpersonationEnded` audit event when impersonation token expires or is explicitly ended
   - Track `actions_taken` count during the impersonation session
   - All API calls during impersonation carry `X-Gyre-Impersonator` header in audit records

5. **End impersonation:**
   - `DELETE /api/v1/admin/impersonate/:request_id` — Admin explicitly ends impersonation
   - Automatic end on token expiry
   - `ImpersonationEnded` event includes duration and action count

6. **Authorization:**
   - Only `Admin` role can request impersonation
   - Impersonation of other Admin users requires `SuperAdmin` or is prohibited
   - Rate limit: max 1 active impersonation per admin at a time

## Acceptance Criteria

- [ ] Admin can request impersonation via POST /api/v1/admin/impersonate
- [ ] Target user receives approval code
- [ ] Target user can approve/deny the impersonation
- [ ] Impersonation token issued with correct claims on approval
- [ ] Target user notified when impersonation starts
- [ ] Full audit trail: ImpersonationStarted/Ended events emitted
- [ ] All actions during impersonation attributed to impersonator in audit
- [ ] Impersonation auto-expires after TTL
- [ ] Only Admin role can impersonate
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/identity-security.md` §Impersonation for the full requirements. Also read `specs/system/observability.md` §Audit Event Types for the impersonation audit events. Auth/JWT handling is in `gyre-server/src/auth.rs`. Notifications are in `gyre-server/src/api/notifications.rs` (or similar — grep for `NotificationType`). WebSocket messaging is in `gyre-server/src/ws.rs`. Register new routes in `gyre-server/src/api/mod.rs`. This depends on task-102 (audit schema) for the ImpersonationStarted/Ended events.

## Shipped

Admin user impersonation is implemented end-to-end on commit `cad88393d` (identity-security.md §Impersonation), with one deliberate deviation from the task plan's response shape, documented below. Follow-up `2d511c216` fixed a merge casualty in the test module; `a1194a81a` and `14af7abd7` repaired review findings; `016b315fd` resolved the rebase onto base `72ad2a128`; merge commit `c66ae2dd8` resolved the rebase onto base `c5deca801`; merge commit `0d1f522c7` resolved the merge of base `fefde2946`; `64536dbcc` repaired the verification round against base `fefde2946` (changed-line rustfmt/clippy + exemption-growth gate); merge commit `ad6a6741b` resolved the merge of base `7747376` (task-177 lineage); merge commit `3f7f9acf8` resolved the clean merge of base `402142b1c` (task-106/task-116 lineage), with `e17e493af` repairing its test-code casualties; merge commit `d605a3b5e` resolved the merge of base `65c470ea` (task-104 access/source-control audit lineage). Acceptance-criteria status is recorded here rather than by editing the protected checkbox text.

### This round (merge repair: merge of base 65c470ea, task-104 lineage)

- The controller repair left two content conflicts, both resolved against the merged tree's ground truth:
  - `crates/gyre-domain/src/audit.rs` (4 hunks): the base (task-104, "Add access control and source control audit event types") appended `GitPushAccepted`/`GitPushRejected`/`GitClone`/`SpecChanged`/`AuthSuccess`/`AuthFailure`/`AbacDenied`/`RbacDenied` at the same enum position where this branch had inserted its impersonation comment. Resolution: **union** — both variant sets kept in the enum declaration, `as_str`, `from_str` and the round-trip test array. Post-resolution, `audit.rs` differs from the base by exactly two lines (the impersonation comment); both sides' variants round-trip (the base's `source_control_and_access_control_event_types_roundtrip` test passes).
  - `crates/gyre-server/src/siem.rs`: CEF severity match arms. This branch carried `ImpersonationStarted => 6` / `ImpersonationEnded => 4`; the base independently added impersonation arms with severity **7/7** (alongside its `Git*`/`Auth*` arms) and a test `cef_severity_source_control_and_access_control_events` asserting 7. This branch's suites assert no impersonation CEF severity anywhere (`api/impersonation.rs` and `siem.rs` tests were checked), so the base's 7/7 is ground truth — the **base side was adopted** and the merged `siem.rs` is byte-identical to the base. Dropping this branch's 6/4 is not a behavior regression: severity for these two event types now matches the shared main lineage's spec'd mapping, which both trees' tests must satisfy.
- No other merge casualties: `cargo check -p gyre-server --all-targets` compiles with zero errors (no E0063 `impersonation` field class this round — the base's new test code in `auth.rs` constructs its `AuthenticatedAgent` literals with struct-update syntax or the full field set), and impersonation content in `auth.rs` (73 references) and `abac_middleware.rs` (20) is identical to the pre-merge verified head `b044276be`.
- Focused probes on the exact merged head `d605a3b5e`: `cargo check -p gyre-server -p gyre-domain` OK; `api::impersonation` 14/14, `siem` 11/11, `auth::` 52/52, `abac_middleware` 14/14, `api::audit` 12/12, `api::spawn` 46/46, `mcp::` 88/88, `jobs::` 6/6, `ws::` 90/90, `messages::` 14/14, `api::users` 17/17, `gyre-domain` 443+8. Guards green: arch, abac-route-registry, abac-exempt-handlers, fabricated-scope-defaults, scope-literal-defaults, dead-message-kinds, in-memory-state-stores, byte-slice-truncation, inert-enforcement, mem-port-contracts, forwarded-header-trust, forged-scope-fields, migration-versions, relative-path-defaults, fail-open-ref-resolution, lossy-secret-conversion, unbounded-external-http, mcp-write-tools, task-commit-attribution. `check-rustfmt-diff.py` and `check-clippy-diff.py` vs base `65c470ea` both clean (21 Rust files). `scripts/` byte-identical to the base. Evidence: `/tmp/stage/review-evidence/task-108-merge-65c470ea.txt`.
- The merge carried the base's `web/dist` assets; per assignment rules those generated assets are restored by the executor and rebuilt remotely — not touched here (cargo's build.rs auto-rebuild in the sandbox produced no tree changes).

### Previous round (checkpoint continuation: base 402142b1c merge-casualty repair)

- The previous assignment on this checkpoint was blocked at the **pre-agent security scan** (scanner stderr: `connect error: transport error`; "critical findings" reported with no findings body) — an infrastructure transport failure in the scanner, not a code defect; no agent work ran there. This round continued from the saved source checkpoint: the controller merge `3f7f9acf8` of base `402142b1c` (task-106 jj-rebase, task-116 meta-spec prompt assembly, task-141 persona-push lineage), which auto-merged cleanly — `auth.rs` and `api/impersonation.rs` are byte-identical to the previously verified head `f7e7601f2`, and the dual-modified siblings (`notification.rs`, `abac_middleware.rs`, `api/mod.rs`, `jobs.rs`, `lib.rs`, `main.rs`, `mem.rs`) unioned base-side additions with this branch's impersonation additions.
- One semantic merge casualty fixed in **test code only** (follow-up commit `e17e493af`, no production change): the base's new test code in `api/spawn.rs` constructs five `AuthenticatedAgent` struct literals that predate this branch's `impersonation` field (E0063 at spawn.rs:2459/2551/2670/2735/2825). Added `impersonation: None` at all five sites — the correct value for direct (non-impersonated) test authentication, the identical repair class as the base-7747376 round.
- Focused probes on `e17e493af`: `cargo check -p gyre-server` OK; `api::impersonation` 14/14, `api::spawn::` 46/46, `auth::` 47/47, `abac_middleware` 12/12, `jobs::` 6/6, `ws::` 90/90, `mcp::` 85/85, `messages::` 14/14, `siem::` 9/9, `gyre-common` 116/116. Guards green: arch, abac-route-registry, abac-exempt-handlers, dead-message-kinds, scope-literal-defaults, fabricated-scope-defaults, in-memory-state-stores, mem-port-contracts, inert-enforcement, byte-slice-truncation, task-commit-attribution, forwarded-header-trust, forged-scope-fields, migration-versions, relative-path-defaults, fail-open-ref-resolution, lossy-secret-conversion, unbounded-external-http. `check-rustfmt-diff.py` and `check-clippy-diff.py` vs base `402142b1c` both clean. Evidence: `/tmp/stage/review-evidence/task-108-merge-402142b1c.txt`.
- Exemption files changed only on the base side (abac-route-registry −4 entries, assertionless-test/ignored-tool-tests/relative-path-defaults adjustments); no new exemptions added, no verifier script touched. `web/dist` carried the base's rebuilt assets; per assignment rules those are executor-restored and rebuilt remotely — not touched here.

### Previous round (merge repair: merge of base 7747376, task-177 lineage)

- The controller repair left three content conflicts (`auth.rs`, `lib.rs`, `scripts/scope-literal-defaults-exemptions.txt`), all resolved against the merged tree's ground truth:
  - The base **fixed the nine task-099 F1 legacy sites in code** — a `system_principal_tenant()` helper documenting the bootstrap-tenant contract for the global/agent/federated token paths, a fail-closed `user.tenant_id` binding on the API-key path, and workspace-record-resolved tenants in `lib.rs` `emit_event`/`emit_telemetry`. Those real fixes collided with this branch's inline `// scope-literal:ok` markers on the same lines (the markers were this branch's workaround for line-number drift in the exemption file). Resolution: **adopted the base's fixes at all nine sites** — the markers are gone, which is strictly better than any exemption — and kept the branch's `ImpersonationContext` struct alongside the base's `system_principal_tenant` helper (dual addition at the same position; both are live in the merged tree).
  - `scope-literal-defaults-exemptions.txt`: took the **base** side. The branch's extra comment block described the inline-marker approach, which is obsolete now that the sites are fixed in code. The file is byte-identical to the base's 12-entry version; `FROZEN_EXEMPTION_COUNT` and every verifier script untouched — `scripts/` as a whole is byte-identical to the base.
- Three semantic merge casualties fixed in **test code only** (follow-up commit `f7e7601f2`, no production behavior change beyond adopting the base's fixes):
  - The base's new `is_system_principal_discriminates_spoof_shapes` test constructs `AuthenticatedAgent` struct literals that predate this branch's `impersonation` field (E0063). Added `impersonation: None` to all three sites — the correct value for direct (non-impersonated) test authentication.
  - The base's new workspace `(tenant_id, slug)` uniqueness port contract (enforced in the mem adapter, `check-mem-port-contracts` class) broke `seed_user`, which seeded every user's workspace with the same slug `imp-test` in tenant `tenant-imp`. The slug is now `imp-{user_id}` per seeded user.
  - The base's fail-closed API-key tenant binding rejects users without a `tenant_id`. Seeded test users now carry `tenant_id = tenant-imp`, so API-key requests resolve the real scope instead of 403.
- Focused probes on the merged tree (`ad6a6741b` + `f7e7601f2`, i.e. the current branch head): `cargo check -p gyre-server` OK; `api::impersonation` 14/14, `auth::` 47/47, `abac_middleware` 12/12, `jobs::` 6/6, `ws::` 90/90, `messages::` 14/14, `mcp::` 85/85, `siem::` 9/9. Guards green: scope-literal-defaults, fabricated-scope-defaults, arch, abac-route-registry, abac-exempt-handlers, dead-message-kinds, in-memory-state-stores, byte-slice-truncation, inert-enforcement, task-commit-attribution. `check-rustfmt-diff.py` and `check-clippy-diff.py` vs base `7747376` both clean (the clippy-diff run **before** committing flagged `git_http.rs:91/135` `result_large_err`, but that file is byte-identical to the base in the merged tree — the gate diffs `base..HEAD` and HEAD was still the pre-merge branch head mid-merge; on the committed merge it is clean). Evidence: `/tmp/stage/review-evidence/task-108-merge-7747376.txt`.
- The merge carried the base's rebuilt `web/dist` assets; per assignment rules those generated assets are restored by the executor and rebuilt remotely — not touched here.

### Previous round (verification repair: changed-line gates vs base fefde2946)

- The verification round (attempt `5179d01122b44f09b72a911c3163196b`, `bash /tmp/stage/checks.sh` exit 1) diffed the candidate against the **bare base** `fefde2946`, so it re-checked the whole task diff, not just the last process commit. Three findings, all repaired in `64536dbcc` with no behavior change:
- **rustfmt changed lines**: `abac_middleware.rs` 951-952 (let-else line not collapsed) and `api/impersonation.rs` test module (12 lines). Fixed by running rustfmt with the gate's own config (`--edition 2021 --config skip_children=true`) on both files. `check-rustfmt-diff.py` vs base: clean.
- **clippy changed lines** in `impersonation.rs`: five `useless_conversion` (the `KvJsonStore` port returns `anyhow::Error`, which `ApiError::Internal` takes directly — the `.into()` in the kv `map_err` closures was a no-op; replaced with `map_err(ApiError::Internal)`, the serde_json mappings keep their real `.into()`) and one `collapsible_if` (the ws notification push's nested `if matches { if try_send(..) }` collapsed to one condition with identical short-circuit semantics). `check-clippy-diff.py` vs base: clean (exit 0).
- **`new verification exemptions forbidden: scope-literal-defaults`**: task-108's insertions in `auth.rs`/`lib.rs` shifted the nine pre-existing task-099 F1 legacy sites' line numbers, so the branch's remapped file entries (from `f57ffefcb`, shifted again by the later base merges) were (a) not a subset of the base's entry strings — the gate compares exact `path:line` strings against the base and forbids any addition — and (b) stale, pointing at non-site lines. Repair: converted the nine sites to the check's documented inline `// scope-literal:ok — task-099 F1 legacy debt` markers at their true locations and deleted the stale file entries. Same nine legacy sites, same debt, no new exemptions — the file count drops 22 -> 13, the frozen ceiling (24) and every verifier script untouched; the entry set is now a strict subset of the base's. The three adapter/domain entries that are stale on the base itself (sites at 166/124 vs entries 163/119 in both trees) are left exactly as the base carries them — pre-existing main debt, not this branch's displacement. Precedent for the remap class: task-236's investigation of the identical finding.
- Focused verification on `64536dbcc`: rustfmt-diff, clippy-diff and the exemption-subset comparison vs base all pass; static guards green (arch, abac-route-registry, abac-exempt-handlers, dead-message-kinds, byte-slice-truncation, inert-enforcement, in-memory-state-stores, scope-literal-defaults, fabricated-scope-defaults, forwarded-header-trust, forged-scope-fields); `api::impersonation` 14/14, `auth::` 45/45, `abac_middleware` 12/12, `messages` 15/15, `ws::` 89/89. Evidence: `/tmp/stage/review-evidence/task-108-verification-repair.txt`.

### Previous round (merge repair: merge of base fefde2946)

- The controller repair left four content conflicts, all resolved by union/deduplication against the merged tree's ground truth:
  - `notification.rs` and `audit.rs`: both sides appended new enum variants at the same tail position (`ImpersonationRequest`/`ImpersonationAlert` + `ImpersonationStarted`/`ImpersonationEnded` here; `SecretExpiring` + the `Agent*` lifecycle variants on the base). Both sets kept in the enum, its `as_str`/`parse`/`from_str`/`default_priority` arms and the round-trip tests — dropping either side would strand the other task's emitters (`check-dead-message-kinds` class) or DB round-trip.
  - `siem.rs`: CEF severity match arms — both sides' arms kept (impersonation 6/4 alongside the base's agent-lifecycle severities).
  - `fabricated-scope-defaults-exemptions.txt`: took the **base** side. The base's task-103 repair round fixed the `auth.rs` JWT `claims.tenant_id` fallback site itself (the "absent claim = default tenant" contract is now a serde schema default `default_tenant_claim` on `JwtClaims`), and the merged `auth.rs` carries that fix — so this branch's `auth.rs:983` remap entry is obsolete and dropped rather than re-remapped. The file is identical to the base's 3-entry version and the auto-merged guard's `FROZEN_EXEMPTION_COUNT=3`; no exemption added, count not raised.
- One semantic merge casualty fixed after the conflicts: the base's new MCP test helpers construct `AuthenticatedAgent` struct literals that predate this branch's `impersonation` field (E0063). Added `impersonation: None` to both sites in `mcp.rs` — the correct value for direct (non-impersonated) test authentication, matching every other non-impersonated construction in `auth.rs`.
- Focused probes on the exact merged head `0d1f522c7`: `cargo check -p gyre-server` OK; `api::impersonation` 14/14; `auth::`+`siem::`+`mcp::`+`jobs::`+`abac_middleware` 156/156; `gyre-common` notification 4/4; `gyre-domain` audit 8/8. Guards green: fabricated-scope, abac-route-registry, mem-port-contracts, in-memory-state-stores, migration-versions, dead-message-kinds, arch. Evidence: `/tmp/stage/review-evidence/task-108-merge-repair-fefde294.txt`.
- The merge carried the base's rebuilt `web/dist` assets; per assignment rules those generated assets are restored by the executor and rebuilt remotely — not touched here.

### Previous round (merge repair: rebase onto base c5deca801)

- The controller repair (merge of base `c5deca801`, task-105 log compression + task-120 audit-detail compression lineage) left one content conflict in `crates/gyre-server/src/main.rs`: this branch had added the `impersonation_expiry` job spawn and the base had added the `log_compression` job spawn at the same site. Resolved by keeping **both** `jobs::spawn_job` registrations — `jobs.rs` registers both job names in the merged tree (`impersonation_expiry` at jobs.rs:558, `log_compression` at jobs.rs:407), so dropping either would strand a registered job without its scheduler spawn.
- Everything else auto-merged cleanly (`auth.rs`, `jobs.rs`, `lib.rs`, `mem.rs`, `siem.rs`, `tty.rs`, adapters, docs, web). The auto-merge did not shift the exempted `auth.rs:983` fabricated-scope site, so `check-fabricated-scope-defaults.sh` passes unchanged on the merged tree.
- Focused probes on the exact merged head `c66ae2dd8`: `cargo check -p gyre-server` OK; `api::impersonation` 14/14, `auth::` 44/44, `jobs::` 6/6, `log_compression` 11/11, `abac_middleware` 11/11, `api::audit` 12/12, `api::users` 14/14, `siem::` 8/8, `tty` 3/3, `gyre-adapters` audit 14/14. Guards green: fabricated-scope, abac-route-registry, in-memory-state-stores, migration-versions, dead-message-kinds, arch. Evidence: `/tmp/stage/review-evidence/task-108-merge-repair.txt`.
- The merge carried the base's rebuilt `web/dist` assets; per assignment rules those generated assets are restored by the executor and rebuilt remotely — not touched here.

### Previous round (rebase repair: exemption-file merge conflict)

- The controller repair (rebase onto base `72ad2a128`) left one content conflict in `scripts/fabricated-scope-defaults-exemptions.txt`: the base had removed the `specs.rs:959` entry (site fixed in task-193's repair round, count lowered 5->4) and no longer carries the `git_http.rs:1495` entry (that site is fixed on the base too), while this branch carried the 5-entry file including its `auth.rs:983` line remap. Resolved against the ground truth of the merged tree: base's specs.rs removal and comment block kept (no `"default"` scope fallback remains in `api/specs.rs`), `git_http.rs:1495` entry dropped (site gone from the merged `git_http.rs`), and this branch's `auth.rs:983` remap kept — the task branch's code above the site shifts it from the base's `773` to `983` in the merged tree. Result: 3 entries, at or below the auto-merged `FROZEN_EXEMPTION_COUNT=4` — no exemption added, count not raised; the only net change vs the base is the line remap. `check-fabricated-scope-defaults.sh` verified green on the merged tree (evidence: `/tmp/stage/review-evidence/task-108-rebase-repair.txt`).
- The other guards touched by the auto-merge (`check-abac-route-registry.sh`, `check-fail-open-ref-resolution.sh`, `check-byte-slice-truncation.sh`, `check-inert-enforcement.sh`) all pass on the merged tree.

### Previous round (review repair: exemption remap, approval critical-section serialization)

- **`check-fabricated-scope-defaults.sh` passes again** (finding: the final base merge inserted lines above the pre-existing exempted site — the JWT `claims.tenant_id` fallback in `auth.rs`, already validated by `validate_tenant_id` before use — moving it from line 945 to 983 without re-updating the exemption remap, so the guard failed on the candidate and broke pre-commit/CI). The exemption entry is re-mapped to `auth.rs:983` (edit, not addition; the line count stays frozen at 5). Guard verified green on the fixed tree.
- **The approval session-creation critical section is serialized** (finding: the Pending-status check, the max-1-active-session check, `save_session` and the request-status save were an unlocked read-modify-write, so two concurrent `POST .../approve` calls from the target both loaded the request as Pending and both minted live impersonation tokens for the same admin — two sessions for one consent, two `ImpersonationStarted` events, violating the max-1-active-per-admin rate limit). `SESSION_WRITE_LOCK` (the lock already held by the action counter and `end_session`) is now held across the whole section and released before token minting, audit emission and notification delivery. The sibling read-modify-writes are covered the same way:
  - `create_request`'s rate-limit read + request save now run inside the lock (moved from the handler, which had checked it unlocked), closing the check-to-save gap against a concurrently approving request.
  - `deny_request`'s terminal transition (a concurrent approve/deny pair can no longer both pass the Pending check and overwrite each other's status).
  - The expiry sweep's stale-request marking (cannot overwrite a request a concurrent approve just marked Approved). The sweep's session-ending loop acquires/releases the lock per session, so taking it for the marking loop cannot deadlock.
- New test `concurrent_approvals_create_exactly_one_session` (yielding kv adapter, same construction as the existing atomicity test): two concurrent approvals → exactly one succeeds, the loser gets 409, exactly one session and one `ImpersonationStarted`. **Mutation-verified**: with the approve lock removed, the test fails with `2` successful approvals (evidence in `/tmp/stage/review-evidence/task-108-mutation-probe.txt`); with the fix it passes.

### Previous round (review repair: attribution coverage, approve ordering, atomic counter)

- **Impersonation attribution now covers every authenticated surface** (finding: the `impersonation_action` recording ran only after policy evaluation, so ABAC-exempt routes, unresolved patterns, git HTTP, MCP and WebSocket left impersonated calls unattributed and uncounted):
  - Repair probe through the full production router (`build_router`: `require_auth_middleware` -> `last_seen` -> `abac` -> handler) exposed that `require_auth_middleware` had **no impersonation branch** — the token was 401-rejected before any handler ran, so impersonated `/api/v1/*` calls were impossible in a default production deployment (the test suites layered only `abac_middleware`, which is why they stayed green). Fixed by mirroring `FromRequestParts` step 1.5 in the middleware (EdDSA signature + active session; inactive session -> 401).
  - `abac_middleware` attributes exempt and unresolved routes **before** the passthrough (`record_impersonated_passthrough`: bearer-token peek -> impersonation-JWT crypto pre-filter -> session resolution -> fire-and-forget record with `surface` = `abac-exempt`/`abac-unresolved`); evaluated routes still record after policy evaluation with the decision outcome (`surface` = `abac`).
  - The auth-resolution layer (`AuthenticatedAgent::from_request_parts`) records impersonated extractions with `surface` = `per-handler`, covering the surfaces that authenticate via the extractor without the ABAC middleware: git smart HTTP, MCP, conversations, explorer and ws-ticket issuance; ws-ticket consumption records with `surface` = `ws-ticket`.
  - WebSocket (`/ws`) and agent TTY connections opened with an impersonation token record one action per connection (`surface` = `websocket`, method `WS`).
  - Exactly-once per HTTP request via a request-extensions marker (`ImpersonationActionRecorded`): `abac_middleware` marks evaluated routes before its extraction (it owns the post-evaluation record with the outcome), the exempt/unresolved passthrough marks before the handler extractor runs, and `last_seen_middleware` defers to abac the same way — no double counting across the require_auth -> last_seen -> abac -> handler-extraction chain (asserted: two impersonated calls -> `actions_taken == 2`).
  - Honest limitation, replacing the previous blanket claim: on per-handler surfaces the event's outcome reflects successful *authentication* (the handler's own response is not visible at the auth-resolution layer); evaluated `/api/v1` routes keep the exact ABAC decision outcome. The audit `detail` carries a `surface` field distinguishing all of the above.
- **`approve_request` checks the caller identity first** (finding: status/expiry were revealed before the target check, letting any authenticated user probe request state — a non-target probing an approved request got 409 instead of 403). The target-user check now runs before any status or expiry is revealed, matching `deny_request`'s ordering. Test: `non_target_cannot_probe_request_status`.
- **Atomic `actions_taken`** (finding: the unlocked load -> increment -> save in the fire-and-forget recorder lost counts under concurrency). The session-record read-modify-write (increment in `record_impersonated_action`, end in `end_session`) is now serialized by a session-write lock; this also prevents an in-flight increment from overwriting an `end_session` write and resurrecting an ended session. Test: `concurrent_impersonated_actions_are_counted_atomically` — 8 concurrent recordings through a yielding kv adapter (models a real async backend; the synchronous mem adapter cannot reproduce the interleaving); verified to fail with `actions_taken == 1` when the lock is removed, passes with `8` and strictly sequential per-event counts.

### Verification (focused, previous rounds)

- `./scripts/check-fabricated-scope-defaults.sh` — OK on the earlier merged tree (rebase conflict resolved; see that round).
- `cargo test -p gyre-server --lib api::impersonation` — 14/14 green; `auth::` 37/37; `jobs::` 6/6 on that merged tree (auth/middleware/jobs/abac_middleware were auto-merged by this repair; their suites confirm no regression).
- `./scripts/check-abac-route-registry.sh`, `check-fail-open-ref-resolution.sh`, `check-byte-slice-truncation.sh`, `check-inert-enforcement.sh` — all OK on that merged tree.
- Evidence: `/tmp/stage/review-evidence/task-108-rebase-repair.txt` (plus prior-round `task-108-mutation-probe.txt`). Current-round evidence: `/tmp/stage/review-evidence/task-108-merge-repair.txt`. Full workspace suites, all-target Clippy and GitHub CI are owned by the verification stage.

### Flow (intent -> signed mandate -> receipt)

- `POST /api/v1/admin/impersonate` (Admin only -- ABAC `admin` resource + in-handler role/user-identity check): creates an approval request. The server generates a one-time 8-digit CSPRNG approval code, stores **only its SHA-256 hash**, and delivers the raw code **only to the target user** -- an `ImpersonationRequest` Inbox notification (priority 2, body carries `request_id`/`approval_code`/`expires_at`) plus a WebSocket `NotificationPush` when the user is online. Request and code are valid 10 minutes. **Deviation from the plan's response shape:** the plan specified `{request_id, approval_code}` in the admin-facing response; that is not shipped because returning the code to the requesting admin would let the admin self-approve, defeating the spec requirement that the *target* provides the code or explicit approval. The response is `{request_id, expires_at}`.
- `POST /api/v1/admin/impersonate/:request_id/approve` (called by the **target user**, any role): per-handler authorization (caller's verified user id must equal the request's target -- checked before any status is revealed), constant-time hash comparison of the code, max-1-active-session rate limit. On success mints a short-lived EdDSA JWT via `AgentSigningKey::mint_impersonation` and persists a session through the `KvJsonStore` port (namespaces `impersonation_requests` / `impersonation_sessions` -- restart-survivable, no in-memory store).
- `POST /api/v1/admin/impersonate/:request_id/deny`: target explicitly rejects; a denied request can no longer be approved.
- `DELETE /api/v1/admin/impersonate/:request_id` (only the impersonating admin): ends the session -> the token is rejected immediately (session-record revocation before `exp`) and `ImpersonationEnded` is emitted.
- Approve/deny are ABAC-exempt routes with per-handler authorization because the caller is the target user who may hold any role (a ReadOnly target must still be able to consent); request/end are ABAC `admin`-resource routes with in-handler defense in depth. All four routes are registered in the `ResourceResolver`.

### Impersonation token

EdDSA JWT with `sub` = target user (permissions match), `impersonator_id`/`target_user_id`/`session_id`/`impersonating: true` claims. TTL from `GYRE_IMPERSONATION_TTL` (default 900 s), validated with **zero leeway** on `exp` (self-minted, same clock). The auth chain (`require_auth_middleware`, `FromRequestParts` and the WS `authenticate_token` path) resolves it to the target's identity with an `ImpersonationContext` attached; a missing/ended/expired session record rejects the token.

### Audit trail

- `AuditEventType::ImpersonationStarted` / `ImpersonationEnded` variants (gyre-domain) with the spec `detail` fields: `impersonator_id`, `target_user_id`, `approval_token` (the consumed code's hash -- the raw single-use code is never persisted) and `duration_secs`, `actions_taken` (+ `reason`). Both events are attributed (`user_id`) to the impersonator.
- Every authenticated call made under an impersonation token is recorded as an `impersonation_action` audit event attributed to the impersonator and counted in the session's `actions_taken` — evaluated `/api/v1` routes via `abac_middleware` after policy evaluation (with the decision outcome), exempt/unresolved `/api/v1` routes via `abac_middleware` before the passthrough, per-handler surfaces (git HTTP, MCP, conversations, explorer, ws-ticket) at the auth-resolution layer, and WS/TTY connections at connection time; see "This round" for the exactly-once marker design. Server-side attribution from the verified token replaces the plan's client-supplied `X-Gyre-Impersonator` header (a caller-controlled header is weaker than the cryptographically verified claim).
- `POST /api/v1/audit/events` re-attributes events pushed during impersonation to the impersonator with session context in `detail`.

### Notifications

- `NotificationType::ImpersonationRequest` (priority 2) and `ImpersonationAlert` (priority 1, "Admin {name} is impersonating your account") variants.
- `WsMessage::NotificationPush` variant; `AppState.ws_connection_users` maps connection id to server-verified user id so pushes are matched by verified identity, never client-supplied ids.
- Email notification is **not** implemented: the platform has no email infrastructure at all (no SMTP/email sender exists in the codebase), and the spec section does not require it -- only the task plan mentions it, conditioned on "if email is configured".

### Authorization constraints

- Only `Admin` role with a real user identity (the shared system token is rejected) may request/end; non-admins get 403.
- Impersonating another Admin is prohibited -- `UserRole` has no SuperAdmin variant, so the plan's "requires SuperAdmin **or is prohibited**" resolves to prohibited.
- Max one active session per admin (409); no requesting/approving/denying while impersonated (prevents privilege chaining); cannot impersonate yourself.
- `impersonation_expiry` background job (30 s interval, registered in the job registry and spawned from main) ends expired sessions and marks stale pending requests; JWT expiry is enforced at validation time regardless of the sweep.
