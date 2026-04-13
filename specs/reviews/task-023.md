# Review: TASK-023 — Spec Links Accountability Agent Patrol

**Reviewer:** Verifier  
**Round:** R2  
**Verdict:** `complete`

---

## Findings

- [-] [process-revision-complete] **F1 (Medium): BFS-based `compute_chain_depth` underestimates longest path in DAGs with diamond dependencies**

  **File:** `crates/gyre-server/src/spec_patrol.rs:297-319`

  The `compute_chain_depth` function uses BFS with a `visited` set. BFS discovers each node at its *shortest-path* depth from the start and never revisits it. For dependency graphs with convergent paths (diamond patterns where the same node is reachable via paths of different lengths), BFS underestimates the longest chain depth.

  **Concrete example:** Given edges `{a→b, b→c, c→d, d→e, e→f, f→g, a→g}`, the longest chain from `a` is 6 (`a→b→c→d→e→f→g`). BFS visits `g` at depth 1 via the direct edge `a→g`, marks it visited, and never revisits it at depth 6 via the long path. It reports `max_depth = 5` (from node `f`). Since 5 is NOT > 5, this deep chain is missed entirely.

  The spec says "specs with >5 levels of `depends_on`" — this requires the *longest* chain from each node, not the shortest. The correct algorithm for longest path in a DAG is topological sort + dynamic programming, or DFS with memoization.

  The test `chain_depth_branching` uses a diamond where both paths to the convergent node `d` have equal length (2), so it cannot expose this bug. A test with unequal-length convergent paths would fail.

  **Fix:** Replace BFS with a longest-path algorithm (topological sort + DP, or DFS + memoization). Add a test with a diamond where path lengths to the convergent node differ (e.g., `a→g` direct + `a→b→c→d→e→f→g` 6-hop, assert depth from `a` is 6).

- [-] [process-revision-complete] **F2 (Low): Notification deduplication silently drops error-severity findings**

  **File:** `crates/gyre-server/src/spec_patrol.rs:383-388`

  The `create_notifications_for_error_findings` function deduplicates by `format!("{}:{}", workspace_id, finding.finding_type)`. If there are multiple error-severity findings of the same type in the same workspace (e.g., 3 `dangling_implementation` findings in workspace `ws1`), only the first creates notifications for Admin/Developer members. The remaining findings are silently dropped — no notification is created.

  The task plan §7 says: "For error-severity findings: Create priority-3 notifications for workspace Admin/Developer members." This implies all error findings should generate notifications. The deduplication behavior is undocumented and could cause users to miss problems — they receive a notification about one dangling implementation but not the other two.

  The test `patrol_creates_notifications_for_error_findings` uses only a single finding, so it cannot detect whether deduplication drops subsequent findings.

  **Fix:** Either (a) remove the deduplication and create a notification per finding, or (b) batch multiple findings of the same type into a single notification whose title/body mentions the count (e.g., "3 dangling implementations detected") and whose body lists all affected spec paths.

---

## R2 Verification

All R1 findings resolved. 0 new findings.

- [x] **F1 resolved:** BFS replaced with DFS + memoization (`dfs_longest_path`). Algorithm correctly computes longest path in DAG. New test `chain_depth_asymmetric_diamond` verifies asymmetric diamond (6-hop path vs 1-hop shortcut, asserts depth=6).
- [x] **F2 resolved:** `notified_workspaces` HashSet dedup removed. Per-finding notification loop creates one notification per finding per Admin/Developer member. New test `patrol_creates_notifications_for_multiple_same_type_error_findings` verifies 3 same-type findings produce 3 notifications.
- [x] All 21 tests pass (19 unit + 2 integration).
