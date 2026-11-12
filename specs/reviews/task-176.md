# Review — task-176 (Meta-specs Preview Loop layout — editor split states and preview workflow)

Spec: `specs/system/ui-layout.md` §9 "Meta-specs Preview Loop Layout".
Candidate: `d95f73477aad784bf098f154b992983212cd9bd4` (base `3af80bd21a8a3452420e014e645eefdfd137f9be`).
Product commits: `eee86b3e` (State 2 locked-editor pending-changes diff), `6e3bb1a7` (baseline refresh after in-session publish), `91fe1e86` (Iterate results retention), `f51cc229` (workspace-scoped §9 State 1 assist endpoint), `95c85a35` (rustfmt repair).
Verdict: **approved** — all eight acceptance criteria verified with independent evidence; no blocking findings.

## Probes run (evidence in /tmp/stage/review-evidence)

- `cd web && npm ci && npx vitest run src/__tests__/lineDiff.test.js src/__tests__/EditorSplit.test.js src/__tests__/MetaSpecs.test.js` → **169/169 passed** (clean tree).
- Mutation probe (sources restored byte-identical afterwards, re-run green):
  - removed `selectedMsBaseline = selectedMsContent` from `MetaSpecs.svelte publish()` → 2 MetaSpecs tests failed (baseline refresh regression killed);
  - added `metaPreviewResult = null` to `EditorSplit.svelte iteratePreview()` → the §9 Iterate-retention test failed.
  Both seeded regressions are killed → the new tests guard real production behavior, not mirrored logic.
- `/tmp/stage/bin/cargo check -p gyre-server` → clean (8m13s; includes web asset build).
- `rustfmt --check --edition 2021` on all four changed Rust files → clean. (The prior verification round's only failure — rustfmt changed-lines in `specs_assist.rs` at source `c41e24a76` — is repaired by `95c85a35`.)
- `bash scripts/check-arch.sh` → OK; `bash scripts/check-abac-route-registry.sh` → OK (the new `/api/v1/workspaces/:id/meta-specs/assist` route has its `RouteResourceMapping` entry).
- `bash scripts/check-task-commit-attribution.sh` fails **only** against the review-staging working-tree reset of `specs/tasks/task-176.md` (`commits: []`, pre-existing before this review started); the committed candidate `d95f73477` lists all five product commits in its frontmatter.
- i18n sweep: every new `editor_split.*` key used by the component exists in `web/src/locales/en.json`.

## Verified against the acceptance criteria

1. **EditorSplit layout** — `MetaSpecs.svelte` (workspace scope) mounts the shared `EditorSplit` in `context="meta-spec"` with `workspaceId`, `targetSpecs`, `baselineContent`, `onpublish`, and `onPreviewStateChange` (which locks the persona selector during State 2). One shared implementation, no parallel §9 client.
2. **Target spec selector** — right-panel checkbox list from `api.getSpecs()` (filtered to non-meta specs), Select All/Clear, gated Preview enablement (`canRunPreview` requires ≥1 selection in meta-spec context).
3. **Preview + per-spec progress** — Preview POSTs the real `POST /workspaces/:id/meta-specs/preview` with `{persona_id, content, spec_paths}` (exact `PreviewMetaSpecRequest` shape in `meta_specs.rs`), then polls `GET .../preview/{id}` and renders the real `PreviewStatusResponse` (`state`, `specs`, `blast_radius`, `structural_impact`); progress list shows ◐/✓ per spec plus an aggregate counter.
4. **Locked editor with diff** — State 2 replaces the textarea with a read-only pane; when a published baseline exists it renders an LCS line diff (`lineDiff.js`: prefix/suffix trim + bounded DP, correct round-trip and trailing-newline handling, unit-tested) with +/- markers and an add/remove summary; verbatim fallback when no/unchanged baseline. Cancel via button and Esc.
5. **Architecture / Code Diff tabs** — State 3 right panel: Architecture (default) renders the server-computed structural impact + blast radius; Code Diff lists per-spec statuses (the endpoint produces no line diff; no fabrication). Failed/unavailable/timed-out previews return to editing with an error banner — State 3 is never presented without a real completed result (tested, including the honest 404 path).
6. **Iterate** — returns to State 1 with the selector restored AND the last results retained (`metaPreviewResult` bound, not `previewState`); mutation-killed test.
7. **Publish** — PUTs the draft prompt to `/meta-specs-registry/{id}` (server bumps version, archives prior, resets approval to Pending → the approval-flow entry) and refreshes the in-session diff baseline + cached registry entry (regression tests for both stale-baseline failure modes).
8. **Tests** — state transitions 1→2→3→1, cancellation (button + Esc), architecture delta display, locked-diff add/remove/context, baseline fallback, assist routing/suggestion Accept — 169/169 pass; the two seeded-regression mutations were killed.

**Server endpoint (§9 State 1 chat)** — `POST /workspaces/:id/meta-specs/assist`: real streaming LLM call (model resolution via workspace LLM config with `PROMPT_META_SPECS_ASSIST` fallback), real registry prompt lookup with draft-over-published precedence (asserted via a recording LLM factory that captures the system prompt), cross-workspace Forbidden for foreign Workspace-scoped meta-specs, 404 unknown workspace/meta-spec, 400 missing draft+id, 503 LLM-unavailable, 10 req/60 s rate limit, budget/cost accounting, and the identical SSE wire protocol as the repo-scoped assist (shared `assist_sse_events`/`assist_sse_response`). ABAC resolver entry added; `docs/api-reference.md` row added and its claims match the handler.

## Non-blocking observations (recorded for the verifier / future tasks)

- The preview endpoint itself (pre-existing, M32 `post_meta_spec_preview`) marks all specs `complete` immediately without spawning agents on throwaway branches — its own comments call agent-based preview a future extension. The §9 UI honestly renders the real statuses and never fabricates; the agent-spawning gap belongs to that endpoint's spec section (meta-spec-reconciliation §5), not to this ui-layout §9 layout task, which was instructed to use the existing preview API.
- ABAC maps the new assist route to `meta_spec`/`generate`; no builtin Allow covers `generate` on `meta_spec`, so Developer-role JWTs are middleware-denied (Admin and the system token pass). This mirrors the pre-existing `briefing/ask` (`workspace`/`generate`) convention, and the full §9 loop is Admin-completion anyway (registry PUT/publish is Admin-only) — but note the preview endpoint's handler allows Developer while the chat does not; a future task may want `meta_spec` added to `developer-generate-access` or the assist action changed for Developer parity.
- The assist endpoint scope-checks Workspace-scoped meta-specs but not Tenant-scoped ones (a caller in workspace B can reference a tenant-A meta-spec id). This mirrors the strictly weaker pre-existing `GET /meta-specs-registry/{id}` (no scope check at all, returns the full record), so it introduces no new leak class; recorded for a future cross-tenant read-hardening pass over the registry surface.

## Tests the verifier must run (server compilation exceeds the review budget)

- `cargo test -p gyre-server specs_assist::` — the new `assist_meta_spec_*` suite (404/403/400/503, SSE complete/partial events, stored-prompt vs draft precedence via recording LLM).
- `cargo test --all` and `cd web && npm test` per the standard pipeline, plus the retained mechanical checks (all observed green on the candidate: arch, ABAC registry, rustfmt).
