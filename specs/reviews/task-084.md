# Review: TASK-084 — HSI Policies ↔ Trust Level Integration UI

**Candidate:** `3346276803a3917f6c68df4a217f9bd4b8ee7eb6` (base `a884329d71182b59aba3868a9291f2ee2f4282a0`)
**Production commit:** `7271313c9bbec615e6ec5baad90715eab261a0a4`
**Scope:** `web/src/components/WorkspaceSettings.svelte`, `web/src/__tests__/WorkspaceSettings.test.js`, `web/src/locales/en.json`, coverage matrix + task record updates.

## Verdict

**Approved.** Independent evidence supports the task contract; no blocking findings.

## What was verified

1. **Diff inspection** (base → candidate): the only production change is the web component + locales, plus the task/coverage records. No backend, script, or unrelated-spec changes.

2. **Domain-mirror accuracy.** The UI's `TRUST_PRESET_POLICIES` mirror matches `gyre-domain/src/policy.rs::trust_policies_for_level` exactly (Supervised → `trust:require-human-mr-review` Deny, actions `merge` on `mr` for subject.type `system` — the UI's "Deny / merge by system" row is a faithful human summary; Guided/Autonomous/Custom → empty). The Policies tab separately shows the *actual* set from `GET /api/v1/policies?scope=Workspace&scope_id=…`, so the two views cannot silently diverge for persisted state.

3. **API-shape fidelity.** `list_policies` returns `Vec<Policy>` with snake_case `name`/`effect`/`actions`/`resource_types`/`built_in`/`immutable` — exactly the fields the component reads. `PUT /workspaces/:id` returns `WorkspaceResponse` including `trust_level`, so recording `persistedTrustLevel` from the save response is sound. `create_policy` rejects reserved `trust:`/`builtin:` prefixes server-side, matching the UI's lock model.

4. **Focused tests** (`/tmp/stage/bin/npx vitest run src/__tests__/WorkspaceSettings.test.js`): 68/68 passed, exit 0. Evidence: `/tmp/stage/review-evidence/task-084-focused-vitest.txt`.

5. **Full web suite** (`npm ci` then `npm test`): 64 files, 1644 passed, 41 skipped, exit 0 — matching the shipped claim exactly. Evidence: `/tmp/stage/review-evidence/task-084-full-npm-test.txt`.

6. **Mutation probes** (both restored; `git status` clean after):
   - `isCustomTrust` forced to `false` → 2 tests fail (Custom banner, trust-delete-enabled-under-Custom). Tests are not self-confirming on the banner/lock behavior.
   - `trust:` grouping branch disabled → 5 tests fail (grouping, badge, lock, delete states). Grouping is genuinely asserted.

7. **Acceptance criteria** — all 11 checked against the rendered DOM in tests: read-only implied-policy summary below the radio grid (DOM-order asserted via `compareDocumentPosition`); name/effect/target per row; "View all policies →" in-place tab switch; builtin:/trust:/user grouping with counts; "From: {level}" badge; preset-managed and Custom banner strings verbatim per spec; immutable rows greyed (`.policy-locked`) with the exact "This policy cannot be removed or overridden." tooltip and disabled delete; "← Change trust level" back-link; no router navigation on either link.

8. **No dangling references** to the removed `abac_title`/`abac_loading`/`abac_empty` locale keys outside tests.

## Non-blocking observation

- **Cosmetic:** `presetDiff` is a non-null `{add: [], remove: []}` object when switching between two presets that both imply no trust: policies (Guided↔Autonomous, any→Custom), so an empty dashed diff container renders with no lines. Verified by a temporary probe test (deleted after; evidence `/tmp/stage/review-evidence/task-084-diff-probe.txt`). No acceptance criterion depends on it; trivially fixable later by returning `null` when both arrays are empty.

## Probes for the verifier (none blocked)

All probes ran in this sandbox; no transport restrictions encountered. `npm ci` used before frontend probes; focused vitest + full `npm test` suffice — no Rust or server probes needed for this web-only change.
