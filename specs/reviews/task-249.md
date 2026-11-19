# Review — task-249 (Storage-exempt broadcast kinds: kind-based short-circuit and real DataSeeded/QueueUpdated emitters)

Spec: `specs/system/message-bus.md` §Message Kinds / Broadcast-only kinds (line 177): "DataSeeded and QueueUpdated use Destination::Broadcast with workspace_id: None. They are server-only AND storage-exempt — they flow through the in-memory broadcast channel only, never hitting MessageRepository::store(). The send path must short-circuit before attempting storage for these kinds."
Commits under review: `356ddc34` (product), `9bc5cca2` (docs, task file only). Base `fb1424fb`.
Verdict: **approved** — both requirements implemented in production code; no findings.

## Probes run (checkout at candidate `9bc5cca2`)

- `bash scripts/check-dead-message-kinds.sh` → **OK: every MessageKind variant has an emitter (or documented exemption)**, exit 0 — with `QueueUpdated` removed from `scripts/dead-message-kind-exemptions.txt` (list shrank: 4 → 3 entries; no new entries).
- `/tmp/stage/bin/cargo test -p gyre-common broadcast_only` → **`message::tests::broadcast_only_returns_true_for_dataseeded_queueupdated_only ... ok`** (1 passed).
- `/tmp/stage/bin/cargo check -p gyre-server` → Finished, no errors (cold run 7m19s; evidence file records cached re-run exit 0).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → Finished, no errors (cold run 3m56s; check profile only — no codegen/link, per the review budget constraint; **gyre-server test execution deferred to the independent verifier**, list below).
- Full diff inspection `fb1424fb..9bc5cca2` (11 files, +634/−29); grep sweep of every `messages.store(&msg...)` call site repo-wide (production stores at `api/messages.rs:311`, `mcp.rs:3030`, `lib.rs:753` — all three now guarded; all other hits are test code).

Evidence: `/tmp/stage/review-evidence/` (dead-kinds check, gyre-common test run, both cargo checks, diff stat).

## Requirement 1 — kind-based storage short-circuit: verified

`MessageKind::broadcast_only()` (`gyre-common/src/message.rs`) returns true for exactly `DataSeeded | QueueUpdated` (unit-tested in gyre-common, run here: passes). All three send paths append `&& !msg.kind.broadcast_only()` to the storage guard, so no row of these kinds is persisted **regardless of destination**, while signing, broadcast-channel send (`message_broadcast_tx`), and consumer dispatch are unchanged:

- REST `send_message` (`api/messages.rs`): guard now `effective_tier != Telemetry && !Broadcast && !kind.broadcast_only()`.
- MCP `handle_message_send` (`mcp.rs`): identical extension.
- Internal emitter `emit_scoped_event` (`lib.rs`): guard now `!Broadcast && !kind.broadcast_only()`.

Existing tier/destination/server-only validation is untouched for all other kinds (diff confirms the only changes to those blocks are the added conjunct). The optional 400 for non-Broadcast destinations was not taken — permitted; the mandatory "never stored" behavior is enforced by kind.

**Test sensitivity (mutation reasoning, not self-confirmation):** `broadcast_only_kinds_not_persisted_even_forced_to_workspace` (api/messages.rs) sends `data_seeded`/`queue_updated` to `{"workspace": "ws-bo"}` as the Server-origin global token, asserts 201, then asserts `find_by_id` → None and `list_by_workspace("ws-bo")` empty, and that the broadcast subscriber received both. Under the pre-task destination-only guard the mem adapter's `store()` (mem.rs:4839 — unconditional insert, returns Ok; no workspace-existence check) WOULD persist the row, so `find_by_id` would return Some and the test fails — it kills the real bug. The lib.rs test `emit_event_broadcast_only_kind_never_stored_for_workspace_destination` repeats the probe through the internal emitter for both kinds (Workspace destination, workspace pre-created via `ensure_test_workspace`), same discrimination.

## Requirement 2 — real emitters for the formerly-dead kinds: verified

- **DataSeeded** — emitted at the end of `admin_seed` (`api/admin.rs`) via `emit_tenant_event(auth.tenant_id, Destination::Broadcast, DataSeeded, None)`: server origin, Broadcast, `workspace_id: None` (emit_tenant_event passes None by construction), payload none; tenant derived from the authenticated seeding caller — not a fabricated literal. Emitted synchronously before the HTTP response, after the 5 seeded StateChanged telemetry messages (drain loop of 32 in the test is ample). Test `admin_seed_broadcasts_data_seeded` drives the real `/api/v1/admin/seed` route and scans the broadcast channel; pre-task the kind had zero production constructors, so the `expect(...)` could never succeed.
- **QueueUpdated** — new `emit_queue_updated(state, workspace_id)` helper (`merge_processor.rs`): resolves the tenant from the workspace record via `message_tenant_id` (skip-and-log on unresolvable — no fabricated scope, consistent with the check-fabricated-scope-defaults invariant), then `emit_tenant_event(tenant, Broadcast, QueueUpdated, None)`. Called at every merge-queue state transition: enqueue (`enqueue_core` in api/merge_queue.rs and the reconciliation auto-enqueue in reconciliation.rs — the latter enqueues directly, not via enqueue_core, so the separate call is correct), operator cancel (`cancel_entry`, workspace resolved from the entry's MR), circuit-breaker cancel (`trip_circuit_breaker`, mr→workspace map built from the full MR list so every entry resolves), claim (Queued→Processing in `process_next` and `merge_atomic_group`), requeue (paused queue, supervised-trust hold, ABAC fail-closed ×2, atomic-group rollback), merge completion (both the atomic-group and single-entry Merged paths), and failure (`fail_queue_entry` plus the direct conflict/git-error sites). Test `enqueue_and_cancel_broadcast_queue_updated` drives the real REST enqueue and cancel routes and asserts Server origin, Broadcast, `workspace_id: None`, payload none, and no stored row for both transitions.
- `QueueUpdated` removed from `scripts/dead-message-kind-exemptions.txt`; the check passes with the shrunken list (run here). The dead-kind invariant is satisfied by real emitter construction sites (the check counts expression-position uses outside message.rs).

## Acceptance criteria

- Storage exemption incl. forced Workspace destination — implemented + discriminating tests (REST, MCP-mirrored guard, internal emitter). Runtime confirmation deferred to verifier (list below).
- DataSeeded emitter observable by subscriber — implemented at the real seed-completion event; test drives the real route.
- QueueUpdated emitter on queue transitions — implemented across enqueue/dequeue/merge-completion (+ claim/requeue/failure); test covers enqueue + cancel via real routes.
- dead-message-kinds check passes with QueueUpdated de-exempted, no new exemptions — **run here, exit 0**.
- Existing broadcast/destination validation unchanged for other kinds — confirmed by diff (guards only extended; validation blocks untouched).

## Test-inflation check

None found. Each new test asserts the negation of the pre-task behavior (stored row / absent emitter) through real routes or the real emitter; the four adapted merge_processor tests preserve their original no-recovery-event / AtomicGroupFailed assertions and only additionally permit the spec-mandated QueueUpdated broadcasts (any other kind still fails them). The `mr_timeline.rs` QueueUpdated consumer arm reads stored messages and therefore remains unreachable for a storage-exempt kind — pre-existing consumer code, untouched by this task, and the spec's storage exemption is the governing contract; not a defect.

## Scope check

No out-of-scope edits: only the three send paths, the two emitters + their call sites, the exemptions file, coverage row 6 of `specs/coverage/system/message-bus.md` (accurate, evidence-cited, no unrelated rows), and the task file. Commit attribution correct (`356ddc34` product commit recorded in frontmatter; `9bc5cca2` is docs-only).

## Verifier must-run (gyre-server test execution deferred per 4 GiB review budget)

- `cargo test -p gyre-server broadcast_only_kinds_not_persisted_even_forced_to_workspace`
- `cargo test -p gyre-server emit_event_broadcast_only_kind_never_stored_for_workspace_destination`
- `cargo test -p gyre-server admin_seed_broadcasts_data_seeded`
- `cargo test -p gyre-server enqueue_and_cancel_broadcast_queue_updated`
- `cargo test -p gyre-common broadcast_only` (already run here: pass)
- `bash scripts/check-dead-message-kinds.sh` (already run here: pass)
- Full `cargo test --all` per standard verification (the four adapted merge_processor rollback/pass-path tests are exercised by the wider suite).

## Environment observation (non-defect)

The working tree carries a controller-staged modification reverting `specs/tasks/task-249.md` to its pre-task content (`progress: not-started`, no Shipped section); the committed candidate `9bc5cca2` contains the correct ready-for-review state with attribution. This review made no source edits.
