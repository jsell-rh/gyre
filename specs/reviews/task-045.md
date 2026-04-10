# Review: TASK-045 — Spec Links — Interactive DAG Visualization

**Reviewer:** Verifier
**Round:** R2
**Verdict:** `complete`

---

## Findings

- [-] [process-revision-complete] **F1 (resolved R2): Missing strikethrough visual on supersedes edge target node**

  **Spec source:** Task Requirements §3 (task-045.md line 43):
  > `supersedes` → dotted arrow with strikethrough target

  **What's wrong:** The `edgeStyle` function in `SpecGraphDAG.svelte:68-74` correctly returns a dotted dash pattern (`'3 3'`) and purple color for `supersedes` edges, but neither the edge rendering block (lines 280-324) nor the node rendering block (lines 327-377) applies any visual strikethrough effect to the _target_ node of a `supersedes` relationship.

  The spec's approval gates table (spec-links.md §Approval Gates) explains the semantic: "When source is approved, target is automatically set to `deprecated`." The strikethrough visual communicates that the target is being superseded — a user looking at the DAG should immediately see which specs are supersession targets without having to read edge labels.

  **Evidence:** `grep -n 'strikethrough\|text-decoration\|line-through' web/src/components/SpecGraphDAG.svelte` returns zero hits. The supersedes test at line 335-352 of `SpecGraphDAG.test.js` only verifies the dotted dash pattern and purple color — it does not assert any target-node visual treatment.

  **Fix:** When rendering nodes (lines 328-377), check whether any `supersedes` edge targets this node. If so, apply a visual strikethrough — e.g., a `<line>` element across the node label text, a `text-decoration: line-through` CSS class, or reduced opacity with a cross-out overlay. The test should include a supersedes edge and assert the target node has the strikethrough decoration.
