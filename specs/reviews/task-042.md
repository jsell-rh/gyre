# Review: TASK-042 — Evaluative Node Detail Tab

**Reviewer:** Verifier  
**Round:** R1  
**Verdict:** needs-revision

---

## Findings

- [-] [process-revision-complete] **F1 — `onSpanSelect` is a no-op stub; canvas highlighting never occurs.**  
  Acceptance criterion: "Clicking a span row highlights it on the canvas."  
  `ExplorerView.svelte:1486` passes `onSpanSelect={(span) => { /* highlight span on canvas via detailNode metadata */ }}` — an empty function body with only a comment. When the user clicks a span row in the evaluative tab, `onSpanSelect` fires (the component correctly calls it at `NodeDetailPanel.svelte:1797`), but nothing happens on the canvas. The acceptance criterion is explicitly stated and the implementation does not satisfy it. The callback must do something observable — e.g., set a reactive variable that the canvas reads to highlight the corresponding particle/node, or update `detailNode` metadata that the canvas already watches.

- [-] [process-revision-complete] **F2 — No test for expanded span detail (attributes, input/output summary).**  
  Spec `explorer-canvas.md` §Evaluative Interaction: "Click a particle → detail panel shows the span: operation name, duration, status, **attributes, input/output summary**."  
  The implementation correctly renders expanded detail when a span row is clicked (attributes at `NodeDetailPanel.svelte:1835-1847`, input/output summary at `NodeDetailPanel.svelte:1849-1860`), but no test verifies this. The test `calls onSpanSelect when a span row is clicked` only asserts the callback was invoked — it does not assert that the expanded content (`.eval-span-detail`, `.eval-attributes`, `.eval-detail-mono`) is rendered after the click. Test data `TRACE_SPANS[0]` (span s1) has `attributes: { 'db.system': 'sqlite' }`, `input_summary: 'id=42'`, and `output_summary: '{"name":"Alice"}'` — ideal for asserting expanded content. A test should click s1's row, then assert the expanded detail contains the attribute key/value, input summary, and output summary text.

- [-] [process-revision-complete] **F3 — Dead code in test `shows no evaluative tab when no spans match the node`.**  
  `NodeDetailPanel.test.js:659-662`: The first `render` call creates a component with `INTERFACE_NODE` (id `n2`) and `[TRACE_SPANS[3]]` (span s4, which matches `n2`). The returned `container` is never asserted on. The implementer's own comment acknowledges the mistake: "Actually n2 does have s4, let's test with no matching spans." The second render (lines 665-670) is the actual test. The first render is dead code that should be removed — it confuses readers and adds execution overhead. Additionally, the first render would actually show the evaluative tab (since s4 matches n2), which contradicts the test's stated intent.
