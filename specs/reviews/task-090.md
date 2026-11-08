# Review — task-090 (HSI Explorer Three Lenses: Structural, Evaluative, Observable)

Spec: `specs/system/human-system-interface.md` §3 "Three Lenses" (+ `specs/system/explorer-canvas.md` §3/§4 lens definitions).
Candidate under review: `d00990bc2` (assignment base `c5deca801`, branch `pipeline/task-090/622a4d2ada5a4161a9271168b0760fbe-1`).
Verdict: **approved**.

This is the repair round after a prior review of candidate `64598aca` found one defect: the committed `web/dist` bundle equaled the pre-task-090 base bundle (checkpoint `86b6f3006` had reverted the verified assets). This review re-examines the exact assigned candidate with independent probes; evidence retained at `/tmp/stage/review-evidence/task-090-final` (controller-side).

## What was verified

**Commit attribution.** All 8 commits in the task's `commits:` frontmatter (`86b6f3006`, `9ebea6a42`, `54c0b32ab`, `00d6497aa`, `47fca1998`, `f51574568`, `7bb23bca6`, `0766a1679`) are ancestors of `d00990bc2` (`git merge-base --is-ancestor`, exit 0 each).

**Source parity with the reviewed candidate.** `web/src` and `crates/gyre-server/src/api/gates.rs` are byte-identical between `64598aca` (the candidate whose lens source was reviewed in the prior round) and `d00990bc2`. The only non-`crates` differences are documentation and other tasks' records arriving via the merged assignment base (`docs/`, `specs/tasks/task-105.md`, `specs/coverage/observability.md`) plus the task-090 repair-round disposition note. No production-code drift since the reviewed state.

**The prior dist finding — resolved as a generated-asset disposition, verified independently.** The committed `web/dist` at the candidate equals the assignment base (empty `git diff base..candidate -- web/dist`), exactly as the disposition documents ("no dist files were committed in this assignment; final integration rebuilds and publishes those assets remotely from source"). Independent verification:

- `crates/gyre-server/build.rs` runs `npm ci` (when needed) and `npm run build` on every build unless `SKIP_WEB_BUILD=1`; `spa.rs` embeds `web/dist` via rust-embed. The standard build path — including CI's `cargo build --release` in `e2e.yml`, which runs before the e2e server starts — therefore always embeds a dist freshly built from `web/src`, never the stale committed one. The committed-dist staleness affects only the opt-in `SKIP_WEB_BUILD=1` Rust-only dev mode, which is the controller's accepted trade-off for generated assets on parallel task branches.
- A fresh `vite build` of the candidate's HEAD source emits `index-QXRGsaeH.js` with sha256 `4c80cc80…`, byte-identical to the verified task-090 feature bundle from `54c0b32ab`, and the bundle contains `gateStatusBySpec`, `specApprovals`, the lens URL-persistence code, and the evaluative legend labels. The source genuinely builds to the shipped feature set.

**Focused tests.** After `npm ci`: `npx vitest run lens-rendering.test.js ExplorerCanvas.test.js ExplorerViewLensUrl.test.js` → 191/191 passed, exit 0.

**Mutation probe — the tests kill real defects.** With the evaluative decoration block in `drawLeafNode` and the per-spec gate edge-coloring block in `drawEdges` temporarily disabled (`if (false && …)`), the ExplorerCanvas suite fails 4 tests: coverage badge, gate ring/edge colors, the path-geometry-isolated edge-stroke test, and the approval icons. The lens tests are differential draw-path tests (canvas ctx spies, baseline-vs-data comparisons, edge strokes discriminated from rings by path geometry), not self-confirming. File restored byte-identically afterwards.

**Full frontend suite (acceptance criterion).** `npm test` in `web/` → 85 files, 1899 passed / 41 skipped, exit 0. (The shipped notes' 80 files / 1865 tests predate the merged assignment base's extra test files — not a candidate discrepancy.)

**Server-side ledger change.** `cargo test -p gyre-server --lib api::gates` → 7 passed, including `list_spec_approvals_includes_rejection_fields`: `SpecApprovalResponse` now serializes `rejected_at`/`rejected_reason` (skip-when-none), with an HTTP round-trip test through `api_router`. The JS approval inference (`!active && revoked_at == null` ⟹ rejected) was checked against the domain invariant `is_active() = revoked_at.is_none() && rejected_at.is_none()` (`gyre-domain/src/spec_approval.rs:57-59`) — exactly equivalent, so the UI's ✓/⏳/✗ mapping is sound.

**E2E baseline repair.** `toolbar-evaluative-linux.png` regenerated at `9ebea6a42` is 919×133, matching the CI-recorded actual render (the task-065-era 919×103 baseline predated the evaluative legend, which legitimately wraps to an extra `flex-wrap` row) and matching the source-built bundle served in CI. `toolbar-default.png` unchanged. No assertion weakened (same locator, same `maxDiffPixelRatio: 0.02`). The full e2e run was not repeated locally (cold release build avoided per review constraints); CI is the remote-verification stage's scope.

## Acceptance criteria

- Lens selector in toolbar with Structural (default) / Evaluative / Observable (`aria-disabled`, "requires production telemetry integration" tooltip) — verified in source and tests.
- Evaluative overlays: coverage badge (green ≥70% / yellow ≥30% / red below, from `GraphNode.test_coverage`), gate status ring + edge colors (green pass / red fail, most-recent-MR-per-spec join), approval icons (✓/⏳/✗ from the ledger) — mutation-verified.
- Lenses compose; switching lenses does not re-layout (label-position identity test).
- `?lens=evaluative` URL persistence (deep-link restore; structural default drops the param) — behavioral tests against real DOM/history.
- Spec assertion pass/fail badges: pre-existing `assertionBadges` (correctly not duplicated).
- `npm test` passes in `web/` — verified (exit 0).
- Coverage matrix row 23 (Three Lenses) marked `implemented` with an accurate REAL description — justified by the above.

## Notes (non-blocking)

- The `rejected_at`/`rejected_reason` serialization is technically redundant for the UI (the active+revoked_at pair already determines the tri-state under the domain invariant), but it makes the ledger API self-describing for other consumers and is tested; harmless and additive.
- `loadEvaluativeData` fetches MR gate results sequentially (bounded at 10 MRs, best-effort); acceptable for an overlay load.
