# Review — task-295 (Activity Dashboard completion), rebase-repair round

**Candidate:** `10adedeccea3a4ea041bb5f07fcee9a2fa876506` (base `82aed3dbddb5a5be8a195b8ffb789bcb2030c644`)
**Reviewer round:** rebase repair — published head `c70b8879` merged with main `82aed3db` (task-280 cascade testing) as `64e7f26c`, then process commits `72a18558`, `10adedec`.
**Verdict:** approved.

## Scope of this round

The diff from the previously reviewed head `c70b8879` to the candidate contains
only main's task-280 cascade-testing surface, docs, regenerated `web/dist`, and
two merge-repair resolutions:

1. `crates/gyre-adapters/src/schema.rs` — both sides added a `diesel::table!` at
   the same location; the resolution keeps both `preview_environments` (task-295)
   and `cascade_test_results` (task-280). Verified: `cargo test -p gyre-adapters
   preview_environment` (7/7) and `cargo test -p gyre-adapters cascade` (5/5)
   both pass against the merged schema.
2. Migration version collision — main's `2026-11-21-000070_cascade_test_results`
   duplicated the branch's `000070_preview_environments`; the latter was
   renumbered to `2026-11-21-000071_preview_environments` as a byte-identical
   rename (git `R100`). `scripts/check-migration-versions.sh` passes;
   `durability_across_reopen` re-runs all embedded migrations against a fresh
   temp SQLite DB and passes.

All task-295 authored sources (`web/src`, `api/preview_envs.rs`,
`gyre-ports/src/preview_environment.rs`, `api/activity.rs`,
`gyre-ports/src/notification.rs`) are byte-identical to the previously reviewed
head: `git diff c70b8879..10adedec -- <those paths>` is empty.

## Independent probes run (evidence: /tmp/stage/review-evidence/rebase-repair-round-10adedec.txt)

- Mechanical invariants, all exit 0: check-migration-versions,
  check-abac-route-registry, check-arch, check-mem-port-contracts,
  check-abac-exempt-handlers. No exemption-file entries for the new try-it /
  preview-environment routes.
- `cargo test -p gyre-adapters preview_environment` — 7/7 (durable rows, tenant
  scoping, duplicate-id and missing-row contract enforcement, expiry query,
  migration on fresh DB).
- `cargo test -p gyre-adapters cascade` — 5/5 (main's merged surface intact).
- `cargo check -p gyre-server` — clean (validates the merged server sources
  compile).
- `npm ci` + vitest on the five new frontend test files — 25/25.
- **Negative control:** mutating `activityStream.svelte.js` so the bus-message
  mapper ignores the message's workspace (`workspace_id ?? null` → `'ws-1'`)
  fails 3 tests (per-workspace isolation on both sidebar surfaces and the
  no-scope rejection) — the real-time isolation tests are not self-confirming.
  Source restored byte-identical; tree clean.

## Acceptance criteria status (re-verified against the candidate)

- `since` filter in the repository layer of every NotificationRepository adapter
  (mem, SQLite, PostgreSQL, broadcasting wrapper) with `created_at >= since`,
  DESC, limit; handler dispatches to `list_since`; `api.js activity()` forwards
  `since`. Handler test seeds more rows than the limit, killing post-fetch
  truncation.
- Real-time sidebars ride the single shared `/ws` connection
  (`attachActivityStream` in App.svelte), per-workspace buckets, id dedupe,
  reconnect re-sync via `GET /api/v1/activity?since=`; server-side ws.rs
  membership isolation unchanged.
- Try-it: durable `PreviewEnvironment` rows through the port with `store!`
  wiring (no in-memory AppState map), compute-target dispatch, real X25519
  WireGuard keypair + mesh IP + NetworkPeer registration, TTL/liveness GC job on
  the jobs.rs scheduler, per-handler membership check before any side effect
  (non-member 403 with zero rows/peers, killed by handler tests), ABAC registry
  entries for every new route.
- Trial analytics: `preview.started` / `preview.ended` / `preview.expired` /
  `preview.failed` via `state.analytics.record`, queryable through the analytics
  API; approve/reject half untouched.
- TTY attach: `AgentTty.svelte` consumes `api.agentTtyUrl` (no longer dead
  code), mounted in DetailPanel's agent view; vitest asserts the socket URL from
  `agentTtyUrl` is actually consumed and that replayed-then-live lines render.
- Artifact deep links incl. gate-failure/test-result entries landing on the MR's
  gates tab on both dashboard surfaces.

## Non-blocking observations (for the verifier / next round)

- The coverage note in `specs/coverage/system/activity-dashboard.md` still cites
  "SQLite migration 000070"; the migration is now `000071` after this round's
  renumber. Documentation drift in the note only — the matrix status is accurate
  and the migration file itself is correctly numbered and tested.
- `web/dist` differences vs the previously reviewed head are generated output
  owned by the deterministic verifier (rebuild + test happens remotely after
  integration).

## Verifier-owed execution

gyre-server test binaries exceed the in-sandbox 4 GiB budget; the verifier must
run `cargo test --all`, minimally `cargo test -p gyre-server --lib try_it_`,
`cargo test -p gyre-server --lib api::activity`, plus re-runs of
`cargo test -p gyre-adapters preview_environment`, `cargo test -p gyre-adapters
cascade`, and `cd web && npm ci && npm test`.
