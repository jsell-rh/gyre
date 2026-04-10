# Review: TASK-026 — Merge Dependencies: Topological Sort and Priority Ordering

**Reviewer:** Verifier  
**Round:** R1  
**Commit:** `fc7db38e`  
**Verdict:** needs-revision

---

## Findings

- [x] [process-revision-complete] **F1: Selection loop only checks deps — gates and atomic group readiness are not evaluated during candidate selection (spec step 4b/4c violation).** The spec's processing algorithm step 4 iterates entries in topological order and checks three conditions per entry: (a) deps merged, (b) gates passed, (c) atomic group ready — skipping to the next entry when any condition fails. The implementation's selection loop (`merge_processor.rs:453-467`) only evaluates condition (a). Conditions (b) and (c) are checked *after* selecting a single candidate (gates at line 795, atomic group at line 521). If either check fails, the entry is requeued and `process_next` returns without trying subsequent entries in topological order. **Concrete scenario:** entries `[A(priority=100, gates pending), B(priority=50, gates passed)]`, both independent. Spec: skip A (gates not ready), merge B. Implementation: select A, check gates at line 795, requeue A, return — B is never attempted this cycle. The fix is to move the gate check and atomic-group-readiness check into the selection loop so entries failing those conditions are skipped and the next topological candidate is tried.

- [x] [process-revision-complete] **F2: Dead code — unused `in_degree` HashMap in `topological_sort_with_priority` (lines 248-261).** The function computes `in_degree: HashMap<String, usize>` (line 248) which is populated but never read. It is immediately shadowed by `in_deg` (line 262), which performs the actual in-degree computation used by Kahn's algorithm. Line 254 contains `+= 0`, a literal no-op. These ~14 lines are dead code that should be removed.

## Verification Summary

- Kahn's algorithm for topological sort: correct (verified with diamond, chain, and atomic group topologies)
- Priority within tiers: correct — `pb.cmp(&pa).then(ta.cmp(&tb))` sorts by priority descending then enqueue time ascending
- Chain depth computation: correct — uses topological sort + DP for longest path in DAG (not BFS which finds shortest paths)
- Asymmetric diamond test (`chain_depth_diamond_asymmetric_uses_longest_path`): correctly verifies longest-path semantics
- `DependencyChainTooDeep` notification: priority 7 matches spec, body contains `chain_depth`, `threshold`, `workspace_id`
- Atomic group implied ordering: correct — earlier-enqueued members become dependencies of later-enqueued members
- Out-of-queue dependency handling: correct — deps outside the queue are excluded from graph edges, checked at merge time by `dependencies_satisfied()`
- `cargo test -p gyre-server -- merge_processor::tests`: 22 tests pass
- Notification type added to `gyre-common`: variant, `as_str`, `parse`, `default_priority(7)`, and test coverage all present

---

**Round:** R2  
**Commit:** `66d68539`  
**Verdict:** needs-revision

## R1 Fix Verification

- F1: **Fixed.** Gates (step 4b) and atomic group readiness (step 4c) are now checked inside the selection loop (`merge_processor.rs:451-516`). When a candidate fails either check, `continue` skips to the next topologically-ordered candidate. Dead code `in_degree` HashMap removed. Two new tests (`selection_skips_gates_pending_tries_next_candidate`, `selection_fails_gates_failed_entry_tries_next`) verify the gate-skip behavior through `process_next`.
- F2: **Fixed.** Dead `in_degree` HashMap removed. Only `in_deg` remains, computing in-degree correctly.

## R2 Findings

- [ ] **F3: Fix-round code change without test coverage — atomic group readiness skip path (step 4c) has no dedicated test.** The R1 fix moved the atomic group readiness check into the selection loop (lines 496-512). When `atomic_group_ready` returns `Ok(false)`, the selection loop `continue`s to the next candidate (line 506). This changed behavior: before the fix, a not-ready atomic group caused the entry to be requeued and `process_next` to return without trying subsequent candidates. After the fix, the next candidate is tried. The fix added tests for the equivalent gate-check paths (step 4b `Ok(false)` and `Err`), but no test exercises the atomic group skip through `process_next`. **Concrete scenario:** entries `[A(in atomic group "x", high priority, group not ready), B(no group, low priority, all gates passed)]`. Expected: A skipped (group not ready), B selected and merged. Without a test, if someone accidentally removes the `continue` at line 506, the regression is undetected. Test pattern: mirror `selection_skips_gates_pending_tries_next_candidate` — enqueue A with an atomic group where another member has pending gates, enqueue B with no group, call `process_next`, assert B is merged and A remains Queued.

## R2 Verification Summary

- 24 merge processor tests pass
- Topological sort algorithm: correct (Kahn's with priority-aware tier ordering)
- Chain depth algorithm: correct (longest-path via topological DP, not BFS)
- Selection loop correctly checks all three spec conditions (deps, gates, atomic group) with skip semantics
- `DependencyChainTooDeep` notification: priority 7, body JSON verified
- All acceptance criteria met (topological ordering, priority within tiers, depth warning, existing behavior preserved)
