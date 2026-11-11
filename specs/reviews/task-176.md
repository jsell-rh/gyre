# Review — task-176 (Meta-specs Preview Loop layout — editor split states and preview workflow)

Spec: `specs/system/ui-layout.md` §9 "Meta-specs Preview Loop Layout".
Candidate: `c41e24a76` (base `cba416b46`), round 6, retry policy
`generated-assets-owned-by-verifier-v1`.
Verdict: **approved** — no source defects; the sole prior-round finding class
(stale committed `web/dist`) is generated output owned by the deterministic
verifier under the retry policy and is recorded below as a non-blocking
observation.

## Round 6 verification (evidence: `/tmp/stage/review-evidence/task176-r6-*`)

`web/src` at `c41e24a76` is byte-identical to the round-4 candidate
`5b1d85405` (`git diff --shortstat 5b1d8540 c41e24a76 -- web/src` empty),
which was functionally vetted in rounds 1–4; this round re-ran the probes
independently:

- **Full frontend suite**: `npm ci` + `npm test` → 105 files, 2138 passed /
  41 skipped. Focused run of the three touched test files → 169/169 passed.
- **Mutation probe (iterate retention)**: dropping
  `|| Boolean(metaPreviewResult)` from `showResultTabs` in
  `EditorSplit.svelte` makes the §9 meta-spec Iterate test **fail**
  (1 failed / 49 skipped) — the round-1 defect's guard is a real test, not
  self-confirming. Source restored; working tree clean.
- **Server type check**: `cargo check -p gyre-server` → exit 0.
- **ABAC registry**: `scripts/check-abac-route-registry.sh` → OK; the new
  `POST /api/v1/workspaces/:id/meta-specs/assist` route is registered in the
  resolver as `meta_spec`/`generate` (no exemption-file growth).

## Verified against the acceptance criteria

- **EditorSplit layout**: MetaSpecs workspace view mounts the shared
  EditorSplit in `context="meta-spec"` with `workspaceId`, `specPath`
  (selected meta-spec id), `targetSpecs`, `baselineContent`, `onpublish`.
- **State 1**: target-spec checkbox selector (Select All/None) in the right
  pane; Preview/Publish actions; the LLM chat is usable without repo context
  via the workspace-scoped assist (round-1 finding closed by `f51cc2296`).
- **State 2**: editor locked (`locked-editor`, no textarea) showing the
  pending-changes LCS line diff against the published registry prompt
  (+/- markers, add/remove summary, verbatim fallback without a baseline);
  right pane shows per-spec ◐ running / ✓ complete from the real
  `previewPersonaStatus` poll plus aggregate progress and
  [Cancel Preview] (button + Esc). Publish refreshes the diff baseline.
- **State 3**: [Architecture] (default) / [Code Diff] tabs render the
  server-computed PreviewResponse fields (structural impact, blast radius,
  per-spec statuses).
- **Iterate**: returns to State 1 with the selector restored AND the last
  preview results retained (`showResultTabs`; mutation-proven).
- **Publish**: PUTs the draft to the meta-specs registry; the server resets
  approval to Pending on content change (`meta_specs.rs:1597`), entering the
  approval flow.
- **Server endpoint** `assist_meta_spec` (specs_assist.rs): real
  implementation — workspace lookup (404), per-user/workspace 10 req/60 s
  rate limit, 503 when LLM unconfigured, cross-workspace meta-spec rejected
  403, 400 when neither draft nor id given, SSE protocol shared with the
  repo-scoped assist via extracted `assist_sse_events`, budget accounting.
  Tests use a `RecordingLlmFactory` asserting on the actual system prompt
  (stored-prompt lookup, draft-over-stored precedence, kind/name injection).
- **Coverage**: ui-layout.md row 37 → `implemented` with accurate evidence;
  SUMMARY counts consistent.

## Non-blocking generated-output observation (verifier-owned)

The committed `web/dist/` at `c41e24a76` is byte-identical to the base's
task-175 bundle (rounds 1–5's only finding). Under the
`generated-assets-owned-by-verifier-v1` policy this is not a review finding:
the deterministic verifier rebuilds `web/dist` from this exact tree after
integration and binds publication to the tested result. Empirical support
that the authored source produces the §9 UI: build.rs regenerated the dist
during `cargo check`, and the fresh `index-C1lqdB9v.js` contains
`locked-diff` / `metaSpecsAssist` / `pending_changes` (4 matches) while the
committed bundle contains none. Working tree was restored to the committed
state after the probe.

## Tests the verifier must run (large builds, out of review budget)

- `cargo test -p gyre-server specs_assist::tests::assist_meta_spec` (8 tests)
- `cargo test -p gyre-server`
- `cd web && npm ci && npm test`
- `bash scripts/check-abac-route-registry.sh`
