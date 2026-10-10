# Review — task-069 (Explorer WebSocket Protocol & Server Handler)

Spec: `explorer-implementation.md` §3–6, §20–21.
Comparison base `05709c24` → candidate `2f092bcd` (assignment). Implementation
commit `30dc2826` is attributed in the task frontmatter; subsequent commits
(`a338b7e5` checkpoint, `b666a474` base merge, `2f092bcd` attribution repair)
leave `crates/gyre-server/` + `scripts/` byte-identical to `30dc2826` except
the task-196 frontmatter line — verified by direct diff (`git diff 30dc2826
2f092bcd -- crates/gyre-server/ scripts/` shows only the base merge's
`api/graph.rs`, which is task-196 territory, not task-069 files).

## What this task actually changed

The handler surface (route, four client messages + `delete_view`/`cancel`
extensions, four server messages, view CRUD, auth chain, rate/session limits)
pre-existed at base and was audited, not written, by this task. The real delta
in `explorer_ws.rs` is narrow and correct:

1. **Off-contract status values (the one real defect).** Spec §6 fixes Status
   to exactly `"thinking" | "refining" | "ready"`. At base, three send sites
   emitted free-form strings (`"Thinking..."`/`"Analyzing..."` per-turn,
   `"Synthesizing answer..."` at max-tool-turns). The frontend
   (`web/src/lib/ExplorerChat.svelte:408-410`) maps only the three spec
   strings and silently drops anything else — the defect was user-visible
   (stale status indicator), and the Shipped section's claim about the
   frontend mapping is accurate (independently verified against the Svelte
   source). The fix introduces `STATUS_THINKING`/`STATUS_REFINING`/
   `STATUS_READY` constants used at all nine send sites — verified by grep:
   the only `ExplorerServerMessage::Status` constructions outside the
   `send_status_full` helper are in tests, and the only remaining free-form
   literals are in doc comments and `normalize_status` tests.
2. **`normalize_status()` on the SDK subprocess forward path** — the bundled
   `scripts/explorer-agent.mjs` emits only `thinking`/`refining` (verified:
   lines 74, 158), but `GYRE_EXPLORER_SDK_PATH` is operator-swappable, so
   normalizing before the wire is genuine hardening, not theater. Substring
   matching can false-positive on exotic alternate-script statuses (e.g.
   "not ready yet" → `ready`), but every output is a valid protocol value,
   which is the contract that matters; bundled values pass through exactly.
3. **`&raw_preview[..500]` → `chars().take(500)`** in the invalid-view-query
   warning — a real F4-class panic fix (multibyte UTF-8 at the byte
   boundary), with the `byte-slice-truncation-exemptions.txt` entry removed
   rather than left behind. Gate re-run green. (An unrelated pre-existing
   `&content[..remaining]` remains at explorer_ws.rs:1903 — byte-bounded by
   an ASCII-length budget invariant, outside this diff, present at base;
   not this task's regression.)

## Acceptance criteria — verified

- **Route** `WS /api/v1/repos/:repo_id/explorer` — `lib.rs:671-674` (outer
  router, outside ABAC-body middleware because WS upgrades cannot go through
  body-reading middleware; per-handler auth verified below). Route pattern
  matches spec §4 exactly.
- **Client messages** — `ExplorerClientMessage` (view_query.rs:1138-1163)
  carries `message`/`save_view`/`load_view`/`list_views` (+`delete_view`,
  `cancel` extensions); every arm dispatched in the session loop
  (explorer_ws.rs:579-1463). `CanvasState` carries all five specced fields
  (selected_node, zoom_level, visible_tree_groups, active_filter,
  active_query).
- **Server messages** — `ExplorerServerMessage` (view_query.rs:1167-1191):
  `text{content, done}` (streamed: `stream_text` sends first-clause
  `done:false` then ~60-char chunks, terminal chunk carries `done:true`;
  native path LlmPort returns whole responses so chunking is the documented
  approximation, SDK path streams real tokens), `view_query`, `views`,
  `status`.
- **Status progression** — thinking on message receipt (session loop :731)
  and agent start (:2380/:2398/:2817), refining in the dry-run self-check
  loop (:2967, capped at `MAX_REFINEMENT_TURNS = 3` per spec) and per tool
  turn (:3080), refining on forced synthesis (:3137), ready after the turn
  completes (:844). All nine sites use the spec constants.
- **View CRUD** — save_view validates (name/description limits, ViewQuery
  parse + `validate()`) then persists via `state.saved_views.create()`
  (port-backed, tenant-scoped); load_view enforces repo-or-workspace +
  tenant scope and sends `view_query`; list_views merges repo + workspace
  views filtered by tenant at SQL level and seeds system defaults under a
  UNIQUE-constraint race guard. Not in-memory stand-ins.
- **Auth** — `AuthenticatedAgent` extractor rejects missing tokens with 401
  (auth.rs:502-504), supports Bearer header / deprecated `?token=` /
  single-use `?ticket=`; the session body re-verifies repo→workspace→tenant
  and user membership or agent workspace binding (:271-429) — this covers
  the middleware-exempt route properly.
- **Tests** — 4 new lib tests (35→39, counted at both revisions) cover the
  constants, end-to-end Status serialization, and `normalize_status`
  mappings; new integration test `explorer_ws_status_progression` asserts
  every observed wire status ∈ {thinking, refining, ready} plus terminal
  ready, done=true text, and view_query — a test with teeth (it would have
  failed at base, where "Analyzing..." leaked onto the wire on the same
  code path).

## Probes (this sandbox, candidate 2f092bcd, 2026-10-10)

- `cargo test -p gyre-server --lib explorer_ws` → **39 passed, 0 failed**
  (includes all 4 new tests).
- `cargo test -p gyre-server --test explorer_ws_integration
  explorer_ws_status_progression` → fails at `WsCtx::new()` line 38 (first
  HTTP request, `IncompleteMessage`) — sandbox TCP `accept()` is
  seccomp-blocked (errno 95, per `/tmp/stage/capabilities.json`), the same
  boundary all 8 tests in the file hit, including the 7 pre-existing ones.
  Infrastructure restriction, not a code defect; implementer's claim
  independently reproduced. `cargo build -p gyre-server --tests` compiles
  clean (the test binary builds and runs to its listener boundary).
- Static gates all pass at candidate: `check-byte-slice-truncation.sh`,
  `check-task-commit-attribution.sh`, `check-abac-route-registry.sh`,
  `check-abac-exempt-handlers.sh`, `check-arch.sh`.
- Worktree restored clean after probes (cargo regenerated `web/dist/`;
  reverted — no production/script/spec edits made by this review).

### Host/GitHub-CI verification commands (transport-blocked here)

```
cargo test -p gyre-server --test explorer_ws_integration            # all 8
cargo test -p gyre-server --test explorer_ws_integration explorer_ws_status_progression --nocapture
```

## Repair-round changes (b666a474, 2f092bcd)

- Base merge: no task-069 files touched; all probes re-run at merged HEAD
  during this review confirm the implementer's claim.
- task-196 frontmatter attribution of base commit `05709c24` — the gate's
  prescribed remedy (frontmatter, not the frozen exemption file); gate
  passes at candidate.

## Verdict

**approved.** The audit findings are accurate, the single real defect found
is fixed at every send site with constants enforced end-to-end, the adjacent
UTF-8 panic is fixed with its exemption retired, the new tests fail on the
defect they target (verified by reasoning over the base code they now
reject), and view CRUD / auth / streaming all hit real port-backed storage
and real protocol paths. The one unverifiable-in-sandbox item is the WS
transport test, whose blocker is documented with exact host commands.
