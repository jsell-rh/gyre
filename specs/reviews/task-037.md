# Review: TASK-037 — Frontend ViewQuery TypeScript Types & Validation

**Reviewer:** Verifier  
**Round:** R1  
**Verdict:** `needs-revision`  
**Commit reviewed:** `e9b13077`

---

## Summary

The implementation is well-executed overall. TypeScript interfaces accurately mirror the Rust types field-by-field (correct optionality, discriminated unions, serde-compatible shapes). The validator covers the critical Rust checks that prevent rendering errors, and the 53 unit tests are comprehensive. Two findings:

## Findings

- [-] [process-revision-complete] **F1 — Pre-check guard bypasses validator for scope-less queries (acceptance criterion 5 violation)**

  `ExplorerChat.svelte:350` has a pre-existing guard:

  ```javascript
  if (!query || typeof query !== 'object' || !query.scope) break;
  ```

  The `!query.scope` condition silently drops view queries that lack a `scope` field — the most straightforward form of invalid query. The newly added validator at lines 353–366 correctly handles this case by returning a user-facing error message (`"Missing or invalid 'scope' field"`), but the guard short-circuits before the validator runs.

  This violates **acceptance criterion 5**: *"Invalid queries show an error message instead of crashing."* A query without `scope` is silently dropped (no crash, but no error message either).

  The guard is entirely redundant with the validator's own input checks (`validateViewQuery` handles `null`, non-objects, and missing `scope` — lines 42–52 of `view-query-validator.js`). Removing the guard is safe because the validator's error path `break`s before any downstream code (like `onViewQuery(query)` on line 387) can access the invalid query.

  **Fix:** Remove the entire guard on line 350, or at minimum remove the `!query.scope` condition. The validator covers all three cases.

- [-] [process-revision-complete] **F2 — Validation error path does not finalize in-flight streaming text**

  When validation fails (lines 353–366), the handler `break`s without finalizing any accumulated `streamingText`. On the non-error path (lines 369–371), streaming text is finalized into a visible message before the view query is applied. On the error path, any LLM explanation text accumulated during the `thinking` phase is orphaned — it stays in `streamingText` and will be incorrectly prepended to the next assistant response.

  This is a minor UX bug but can produce confusing output: the LLM's reasoning about a query that was rejected appears as part of a completely unrelated future response.

  **Fix:** Add the streaming text finalization block before the validation error message:

  ```javascript
  if (streamingText.trim()) {
    messages = capMessages([...messages, { id: nextMsgId++, role: 'assistant', content: streamingText, timestamp: Date.now() }]);
    streamingText = '';
  }
  ```

---

## R2 — `complete`

**Round:** R2
**Commit reviewed:** `35d45dac`

### R1 finding verification

- [x] **F1 — Pre-check guard bypasses validator**: Fixed in `35d45dac`. The redundant guard `if (!query || typeof query !== 'object' || !query.scope) break;` was removed. The validator now runs for all incoming view queries, including null, non-objects, and scope-less queries — all of which produce user-facing error messages as acceptance criterion 5 requires.

- [x] **F2 — Validation error path does not finalize in-flight streaming text**: Fixed in `35d45dac`. The streaming text finalization block (`if (streamingText.trim()) { ... streamingText = ''; }`) was added before the error message on the validation failure path, matching the success path's behavior. Status is set to `'ready'` and `scrollToBottom()` is called on both paths — state symmetry is correct.

### R2 verification

- TypeScript interfaces match Rust types field-by-field (all 11 structs, discriminated union for Scope, correct optionality for `#[serde(default)]` and `Option<T>` fields, `#[serde(untagged)]` Zoom union).
- Validator covers all critical rendering-error checks from Rust's `ViewQuery::validate()`.
- 53 unit tests pass covering valid queries, structural errors, scope-specific errors, edge types, emphasis, zoom, colors, narrative, and multi-error reporting.
- ExplorerCanvas `activeQuery` prop typed via JSDoc `import('./types/view-query.ts').ViewQuery | null`.
- No fix-introduced regressions: guard removal is covered by existing validator tests; streaming text finalization on error path is correct by code inspection (state symmetry with success path confirmed).

**Verdict:** `complete` — all acceptance criteria met, both R1 findings resolved.
