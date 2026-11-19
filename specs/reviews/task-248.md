# Review: task-248 — Message send-path guards

**Candidate:** `5a3274d42551732c6f2e51cc026e184ec76206e9` (base
`321615eced877c4839ccb98b78fae10778559933`, rebased across task-250's
WebSocket replay-window work)
**Reviewer:** independent review round (round 7), model gateway/rits/zai-org/glm-5-3
**Verdict:** **approved** — no blocking findings; server test execution deferred
to the independent verifier (build wrapper blocks gyre-server codegen in the
review sandbox; probes recorded in `/tmp/stage/review-evidence/`).

Note: the working tree carries an uncommitted controller reset of
`specs/tasks/task-248.md` (frontmatter → not-started/empty commits). This
review examined the committed candidate, whose frontmatter lists all four
product commits (3bb9c9bd, 873d0821, 9dd4a1f0, 955c134a);
`check-task-commit-attribution.sh` passes against the committed state.

## What was reviewed

Full diff base→candidate (17 files, ~1.5k insertions) against the task's
five required behaviors and the canonical spec
(`specs/system/message-bus.md`, Message Envelope + Scoping Rules).

## Findings: none blocking

All five deviations are genuinely closed in production code:

1. **Scopeless-message guard** — `emit_scoped_event` (lib.rs) validates
   `destination != Broadcast && workspace_id.is_none()` FIRST: logs at
   error level and returns `Err` before the Message is constructed, so
   nothing reaches `store()`, the broadcast channel, or dispatch. The sole
   `emit_tenant_event` production caller (reconciliation.rs:1509) passes
   Broadcast, so no production emission regresses; `emit_event`/
   `emit_tenant_event` wrap the Result best-effort.
2. **Mem store contract** — `MemMessageRepository::store` returns `Err` on
   `workspace_id: None` (never panics), matching the SQLite/PostgreSQL
   `message_to_new_row` contract. All three production `store()` callers
   skip Broadcast, so no production path stores a scopeless message.
3. **WS `acknowledged` exclusion** — exactly two bus-Message serialization
   sites exist in ws.rs (replay arm and live delivery arm); both route
   through `ws_message_payload` (same manual-removal pattern as
   `send_response`). The other `to_string` sites serialize `WsMessage`
   control frames, not bus Messages. GET inbox responses keep the field.
4. **Origin table** — `origin_from_auth` now matches on
   `AuthenticatedAgent.auth_mechanism`, set by the extractor at every
   production construction site (verified all of them): global token →
   Server; API key / OIDC user JWT / impersonation → `User(user_id)`;
   agent token / federated JWT → `Agent`. No spoofable string or role
   membership is consulted. The Broadcast guard keeps its separate
   Admin-role check (rule 6 preserved for Admin API keys), while the
   server_only gate and the rule-3 User-origin membership check now apply
   to Admin API-key/OIDC callers. The global token keeps Server origin
   with server_only and Broadcast authority.
5. **Rule 1 tenant isolation** — both the REST handler and the mirrored
   MCP `handle_message_send` resolve the target agent's workspace record,
   compare its tenant with the caller's verified tenant, and reject
   (403 / tool error) on mismatch — including Server-origin global-token
   callers; an unresolvable workspace record rejects fail-closed (round-4
   repair), consistent with the repo's no-fabricated-scope invariants.

## Test quality

Each acceptance criterion has a test that fails if the repaired behavior is
reverted (traced individually): the origin-forge tests return 201 under the
old role-based mapping; the rule-6 probes use a non-server_only Custom kind
so the 403 can only come from the Broadcast guard (round-5 fix); the
cross-tenant tests store and broadcast if the comparison is removed; the
scopeless-guard test uses a counting MessageRepository wrapper plus
broadcast/dispatch receivers and a capturing tracing layer (ERROR level);
both WS arms are covered over a real TCP listener, replay via
`last_seen=0`; the mem contract test mirrors the SQLite adapter's; GET
inbox inclusion is asserted. The rule-3 membership consequence for the OIDC
caller is covered transitively (same `origin_from_auth` mechanism match
proven by the OIDC server_only probe) — noted, not blocking.

## Probes run in this review

- `cargo check -p gyre-server --lib` and `--tests`: exit 0 (all new tests
  type-check).
- `cargo fmt --check -p gyre-server`: clean.
- `cargo test -p gyre-common --lib`: 128 passed.
- Invariant scripts: check-arch, check-mem-port-contracts,
  check-abac-route-registry, check-mcp-write-tools, check-inert-enforcement,
  check-task-commit-attribution (at HEAD): all exit 0.
- gyre-server test execution is blocked by the review sandbox's build
  wrapper (4 GiB budget) — the verifier must run
  `cargo test -p gyre-server --lib`,
  `--test api_integration`, and `--test message_persistence`
  (list in `/tmp/stage/review-evidence/verifier-probes.txt`).

Coverage rows 5, 9, 10 in `specs/coverage/system/message-bus.md` were
updated to `implemented` with task-248 attribution and concrete source
evidence — honest labeling (no `verified` claim before independent
verification).
